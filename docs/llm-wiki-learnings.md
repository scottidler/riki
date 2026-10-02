# llm-wiki learnings for riki

**Source:** `geronimo-iia/llm-wiki`, branch `main`, commit `9753aa8`, cloned at `~/repos/geronimo-iia/llm-wiki`. All paths below are relative to that repo; riki paths are marked (riki).
**Date:** 2026-10-02

## Framing

- llm-wiki is a headless wiki engine for agents: one Rust binary serving MCP and ACP tools, no web UI for humans (`README.md:1-8`). riki is a browser wiki for people.
- It works on a normal checkout with a working tree. It writes files to disk, stages with `index.add_all`, and commits locally (`src/git.rs:23-46`). A `notify` watcher picks up edits (`src/watch.rs:25-34`). riki keeps a bare clone, never has a working tree, and reads blobs at a commit (riki `core/src/store.rs:1-6`, `:425-446`).
- Its search index is tantivy on disk, with a `state.toml` beside it recording the commit it was built from (`src/index_manager.rs:138-155`). riki's is a hand-rolled inverted index in memory, rebuilt on every publish (riki `docs/design/2026-10-02-riki-wiki-operations.md:332-333`); tantivy was rejected (`:351-352`).
- Markdown goes through pulldown-cmark, not comrak.
- Crate versions (`Cargo.toml:56-74`, resolved in `Cargo.lock`): git2 0.21.0 with `vendored-libgit2` (libgit2-sys 0.18.7+1.9.6), tantivy 0.26.2, tantivy-jieba 0.20.0, pulldown-cmark 0.13.4, notify 8.2.0, petgraph 0.8.3, axum 0.8.9. riki resolves git2 0.21.0 too (libgit2-sys 0.18.8+1.9.7, not vendored, riki `core/Cargo.toml:15`) and comrak 0.55.0.
- What transfers is the incremental-update shape and the backlink layout, not the storage model.

## What transfers

### Diffing from the last indexed commit

- `changed_since_commit` parses the stored oid, takes that commit's tree and HEAD's tree, and runs `diff_tree_to_tree` between them (`src/git.rs:115-139`, diff at `:130`). A tree diff compares two trees, not a history, so it does not care whether the old commit is an ancestor. That suits riki: the base is the previous good tip, and `refs/riki/good` still points at it while publish builds the next snapshot, because `set_good` runs after the search build (riki `core/src/wiki.rs:195-198`).
- `collect_md_changes` walks the deltas and keeps a path when it is under the wiki prefix and ends in `.md`, taking `new_file().path()` and falling back to `old_file().path()` so a delete still has a path (`src/git.rs:141-163`, filter at `:145-148`).
- No rename detection is run: the diff call passes no options and `find_similar` is never called (`src/git.rs:130`). A moved page therefore comes out as one `Deleted` delta and one `Added` delta, which is exactly the pair of index operations a move needs.
  - The trap for riki: riki's redirect walk does run `find_similar` with `renames(true)` (riki `core/src/store.rs:474-477`). If search ever reuses that diff, a `Renamed` delta must remove the old path as well as add the new one. llm-wiki's filter keeps only the new path for any delta, so a `Renamed` delta there would leave the old page in the index.
- It merges two diffs: last-indexed commit to HEAD, then working tree to HEAD on top so uncommitted edits win (`src/git.rs:165-194`). riki has no working tree, so only the first half applies.
- The watcher's file events are only a trigger. The paths are counted for the log line and to pick which wiki to update (`src/watch.rs:144-173`, count at `:153-156`); `update` then derives the change list from git again (`src/index_manager.rs:568`). riki's poll has the same shape: the poll says "tip moved", the diff says what changed.

### Applying a diff to the index

- For every changed path, `update` deletes the old document by its slug term first, unconditionally, then re-adds it unless the delta is `Deleted` (`src/index_manager.rs:587-629`, delete at `:596-597`). Delete-then-add makes the update idempotent: applying the same diff twice leaves the same index.
- A changed file that is unreadable, or now excluded by the ingest rules, ends up deleted and not re-added (`src/index_manager.rs:602-606`). That matches riki's "skip with a WARN, never fail the build" rule for search (riki `docs/design/2026-10-02-riki-wiki-operations.md:130`), provided riki also counts it as skipped.
- `update` reads the new content from the filesystem (`src/index_manager.rs:602-603`). riki would read the blob at the new commit instead, with the existing `read_blobs` (riki `core/src/store.rs:425-446`), and only for the changed paths.
- Rebuild triggers are an enum (`src/index_manager.rs:123-134`), chosen by `staleness_kind` (`:726-770`):
  - no `state.toml` -> full rebuild (`:729-735`)
  - same commit and same schema hash -> nothing (`:741-743`)
  - commit moved, schema unchanged -> incremental `update` (`:745-747`)
  - per-type schema hashes changed -> re-index just those types (`:749-769`), with a full rebuild if the partial one fails (`src/engine.rs:249-267`)
  - the check itself failing -> full rebuild (`src/engine.rs:278-287`)
- Two gaps to avoid:
  - `update` never writes `state.toml`, on purpose (`src/index_manager.rs:633-637`). The stored commit stays at the last full rebuild, so every later update diffs from that old commit to HEAD and re-indexes everything changed since then, a list that only grows. riki should advance its base on every publish. It already does this for redirects through `walked` (riki `core/src/redirect.rs:15-20`, `:39`).
  - When the stored commit can't be found, `collect_changed_files` drops the error and goes on with the working-tree diff alone (`src/git.rs:178-184`), so the index can silently miss commits. riki's redirect walk does better here: when `since` isn't reached it starts from an empty map and walks the full history (riki `core/src/redirect.rs:33-37`). Search should fall back to a cold build in the same way.
- A full rebuild writes to a side directory, swaps it in with renames, and rolls back to the previous index if the reopen fails (`src/index_manager.rs:364-366`, swap at `:463-476`, rollback at `:510-531`).

### Incremental search for riki (the scaling question)

riki's own review names this as the first view to hurt at scale: re-tokenizing the whole corpus on every publish (riki `docs/sage-advice.md:9`). Today publish reads every page blob twice, once for nav titles (riki `core/src/wiki.rs:149-181`) and once for search (`:213-253`), then rebuilds the index from scratch (riki `core/src/search.rs:109-133`).

Using llm-wiki's approach, riki can keep the in-memory index and stop rebuilding it from scratch:

- **Base:** the previous snapshot's commit (riki `core/src/wiki.rs:59-63`, `previous` at `:195`). Nothing needs to be saved to disk: a restart does a cold build, as it does now and as the redirect walk does (riki `core/src/redirect.rs:22-37`). llm-wiki needs `state.toml` only because its index outlives the process (`src/index_manager.rs:334-344`).
- **Change list:** `diff_tree_to_tree(previous good, new tip)` filtered to `.md`, without `find_similar` (`src/git.rs:130`, `:141-163`).
- **Apply:** remove each changed path, then re-add the ones that still exist, reading only those blobs (`src/index_manager.rs:596-629`).
- **Fallback:** if the diff fails, do a cold build, never a partial update (the opposite of `src/git.rs:178-184`).

What riki's current structure makes harder:

- Postings point at dense `u32` section ids that index a `Vec` (riki `core/src/search.rs:40-44`, `:100-105`, ids assigned at `:148`, `:156`), so removing a page means tombstones or renumbering. tantivy hides this with deletes by term plus segment merges (`src/index_manager.rs:597`). A hand-rolled version needs each page to keep its own term list, so removal touches only those posting lists, plus a free list or compaction for ids.
- `Published` is swapped as one immutable unit (riki `core/src/wiki.rs:49-57`, `:199-204`), so an update has to build a new `SearchIndex` while requests read the old one. `SearchIndex` derives `Clone` (riki `core/src/search.rs:100`), so clone-then-patch works, the way `Redirects::build` clones the previous map (riki `core/src/redirect.rs:35`). The clone still copies the whole index, but the blob reads, comrak parses and AST walks then scale with the number of changed pages. tantivy gets cheap snapshots from immutable segments: a searcher is an `Arc` clone of the current segment set (`src/index_manager.rs:282-293`), refreshed by a manual reload after each commit (`:295-313`, `ReloadPolicy::Manual` at `:271`).
- Ranking uses no corpus-wide statistics: weights are per field (riki `core/src/search.rs:30-37`), and ties sort by path (`:214-217`). So an incremental index can't drift from a cold one the way BM25 document frequencies could. One order dependency remains: the best section within a page breaks ties by lower section id (`:205`), so a re-added page must get its section ids in document order.
- The same diff could feed nav titles, which today re-reads every page per new commit (riki `core/src/wiki.rs:154-167`).
- Test it the way riki tests redirects: incremental equals cold (riki `docs/design/2026-10-02-riki-wiki-operations-implementation-notes.md:64`). llm-wiki's tests cover update for add, delete, modify and no-op (`tests/index_manager.rs:191`, `:232`, `:256`, `:306`), but none exercises a rename.

### Backlink graph

- Outbound links are stored on the source page's own search document, as a multi-valued keyword field `body_links` (`src/index_manager.rs:928-933`, field at `src/index_schema.rs:289-298`).
- Backlinks for a target are a single term query on that field (`src/ops/content.rs:27-66`). The useful property: replacing a page's document replaces its outbound edges, so an incremental update keeps backlinks correct with no reverse table to patch. riki's in-memory version would hold `target -> set of source paths` and, per changed page, remove its old outbound set and add the new one, the same per-page bookkeeping incremental search needs.
- A deleted page's inbound links survive in the linking pages' documents, so its backlinks still answer. That is the raw material for the broken-link report riki parks (riki `docs/design/2026-10-02-riki-wiki-operations.md:54`).
- Extraction rules worth matching (`src/links.rs:133-203`):
  - fenced blocks and inline code are skipped (`:147-163`, tests at `:380-397`)
  - `#fragment` is cut off, and `http`, `https`, `mailto` and same-page `#` links are dropped (`:164-175`)
  - relative destinations are resolved against the source page's directory, with `.md` stripped (`:85-131`)
  - a bundle page (`dir/index.md`) resolves from its own directory, a flat page from its parent (`src/index_manager.rs:422-430`). riki has the same two files per URL (`README.md` and `<x>.md`, riki `core/src/index.rs:261-269`), so backlink targets should be normalized through `url_for_file` and riki's `render::resolve` (riki `core/src/render.rs:265-282`), the resolver the renderer already uses.
- A trap: all text outside code goes into one buffer with no separator before the `[[...]]` scan (`src/links.rs:161-163`, scan at `:189-202`), so a stray `[[` in one paragraph and `]]` in a later one join into one link.
- `tests/backlinks.rs` covers two pages linking to a target, an unrelated page, and a page with no inbound links (`tests/backlinks.rs:57-125`), always on a fresh full rebuild (`:41-55`). Nothing tests backlinks after an incremental update, a delete or a move, which is where riki's version would break.

### git2 usage patterns

- Resolving a stored oid string to a tree: `Oid::from_str`, then `find_commit(..)?.tree()?`, with `head().and_then(|h| h.peel_to_tree())` for the other side (`src/git.rs:123-129`).
- No empty commits: compare the new tree oid with the parent's `tree_id()` and return early (`src/git.rs:34-41`, again at `:63-69`).
- Signature fallback: `repo.signature()`, else a fixed engine identity (`src/git.rs:16-20`). riki takes the author from the browser identity instead.
- `diff.foreach` with a closure that returns `true` to continue (`src/git.rs:143-161`). Its result is discarded with `.ok()` (`:161`), so a diff error yields an empty change list. riki's `diff.deltas()` iterator (riki `core/src/store.rs:478-489`) is simpler and doesn't hide the error.
- Page history shells out to `git log --follow` because libgit2 has no follow (`src/git.rs:211-232`). riki already uses the git CLI for the network (riki `core/src/store.rs:3-4`); page history (parked, riki `docs/design/2026-10-01-riki.md:56`) could use the same split.

## What it confirms riki already has right

- **No index format to version:** llm-wiki hashes its type schemas and stores per-type hashes so it can tell when the on-disk index was built by an older layout (`src/index_manager.rs:140-155`, `:749-769`). riki's index lives only in memory and is rebuilt when the process starts, so a new binary never reads an index built by an old one.
- **No corruption recovery:** llm-wiki has to detect an unopenable index, delete it and rebuild (`src/index_manager.rs:249-267`), and report `openable` / `queryable` health (`:641-714`). riki has nothing on disk to corrupt.
- **Atomic swap:** llm-wiki gets atomic replacement from directory renames plus a two-step rollback (`src/index_manager.rs:463-531`). riki gets it by swapping one `Arc<Published>` (riki `core/src/wiki.rs:199-204`).
- **Serialized rebuilds:** llm-wiki adds a `rebuild_lock` and an `AtomicBool` so the watcher and an MCP call don't race the swap (`src/index_manager.rs:171-173`, `src/engine.rs:208-219`, `src/watch.rs:94-102`). riki's publish already requires the repo mutex (riki `core/src/wiki.rs:183-187`).
- **Fallback when the base is gone:** riki's redirect walk notices when `since` isn't on the chain and rewalks in full (riki `core/src/redirect.rs:33-37`). llm-wiki keeps going on a partial diff (`src/git.rs:178-184`).
- **One key for derived data:** llm-wiki needs a generation counter separate from the commit, because a same-commit schema rebuild must still invalidate its graph caches (`src/index_manager.rs:211-220`, `src/graph.rs:1544-1547`). Every riki view comes from the good-tip commit plus the running binary, so the snapshot's commit is a complete key (riki `core/src/wiki.rs:59-63`).
- **Paths can't climb out of the root:** llm-wiki's link normalizer ignores `..` past the root (`src/links.rs:109-114`). riki's `resolve` returns `None` there (riki `core/src/render.rs:265-277`).
- **Collisions block publish:** llm-wiki maps `x.md` and `x/index.md` to one slug (`src/slug.rs:22-38`) and deletes by slug (`src/index_manager.rs:597`), so with both files present, deleting one can drop the survivor from search until the next full rebuild. riki refuses to publish a tree where two files map to one URL (riki `core/src/index.rs:109`, `:140-142`), so per-path and per-URL keys never disagree at the good tip.
