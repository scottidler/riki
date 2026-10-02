# SilverBullet learnings for riki

**Source:** `silverbulletmd/silverbullet`, branch `main`, commit `d9bfedde`, cloned at `~/repos/silverbulletmd/silverbullet`. All paths below are relative to that repo; riki paths are marked (riki).
**Date:** 2026-10-02

## Framing

- SilverBullet is a single-space personal notes app ("Personal Knowledge Database", `README.md:8`). Its Rust Axum server serves a directory of files on disk (`/.fs/{*path}` routes, `server/src/router.rs:258-263`); the browser client does the indexing, in plugs that run in a separate WASM sandbox (`plugs/index/conflict.ts:1-4`). Server-side rendering exists only so a public read-only space is crawlable (`server/src/ssr.rs:1-4`).
- Git is a history layer behind the working tree, not the store. Writes land on disk first; a debounced engine commits them later, grouped by author: 30 s quiet, at most every 5 min (`server/src/revisions/engine.rs:11-12`, `:265-325`). Remote sync is a fetch, `git merge`, push loop on a working tree (`server/src/revisions/sync.rs:212-357`), and the docs call it "primarily a **backup mechanism**, it should not be considered for any type of collaboration" (`docs/Revisions.md:31`).
- riki is the reverse: a bare clone, every save is one commit, and the GitHub push is the lock (riki `docs/design/2026-10-01-riki.md:90-92`). So SilverBullet's sync and conflict engine mostly shows what riki avoids. Its merge crate, its marker detection, and its rename refactor are the parts that transfer.

## What transfers

### Three-way merge on save instead of 409

riki answers 409 whenever the blob at the tip is not the editor's base oid (riki `core/src/save.rs:122-133`); co-editing is parked until "409s show up in practice" (riki `docs/design/2026-10-01-riki.md:54`). SilverBullet's reconcile endpoint is the middle step: merge on the server, and fall back to a conflict only when hunks overlap.

- **The endpoint:** `POST /.fs/{*path}` with an 8 MiB body limit (`server/src/router.rs:260-263`). The request carries `baseHash`, `baseText`, `proposedHash`, `proposedText` (`server-common/src/reconcile.rs:5-14`). The answer is one of `applied`, `merged`, `conflicted`, `retry`, each with the new revision and, except `retry`, the text that was written (`server-common/src/reconcile.rs:25-45`).
- **Input checks, in order** (`server/src/handlers/fs.rs:703-737`):
  - malformed JSON, or a hash that doesn't match its text: 400 (`:708-717`)
  - either text over `MERGE_SIZE_LIMIT` (1 MiB, `server-common/src/space/disk.rs:483-484`): 413 (`:719-724`)
  - a path that isn't text-like: 409 (`:726-730`, rule at `server-common/src/space/disk.rs:486-502`)
  - base or proposed already holding SilverBullet's own conflict markers: 409 (`:731-737`)
- **The decision ladder, under a per-path lock** (`server/src/handlers/fs.rs:739-746`, body at `:827-931`):
  - file gone: write the proposal (`:834-847`)
  - current == proposed: no write, `applied` (`:850-861`); this is riki's `content-present`
  - current == base: plain write (`:863-875`); this is riki's fast path
  - current not UTF-8, over the limit, or already holding markers: 409 (`:877-885`)
  - otherwise `merge(base, proposed, current)`; more than `CONFLICT_HUNK_LIMIT = 100` hunks is a 409, not a marker-laden file (`:887-893`, constant at `:43-45`)
  - before writing, the file is re-read; if it moved, the loop runs again, up to `MAX_RECONCILE_ATTEMPTS = 3`, then answers `retry` (`:896-920`, constant at `:47-50`)
- **How this maps onto riki's write driver** (riki `core/src/write.rs:114-193`):
  - The merge is a new branch in step 4, where today riki answers 409. The re-read loop is the push retry riki already has: a non-fast-forward goes back to the fetch and runs the check again (riki `core/src/write.rs:169-175`).
  - SilverBullet makes the client send `baseText` and checks its hash, because its server keeps no old versions to hand. riki already receives the base oid, and that blob is in the bare clone's object database, so the request doesn't change.
  - Merge whole files: the base blob, `compose(front matter of base, body, trailing newlines of base)` (riki `core/src/page.rs:116`), and the tip blob. Then front matter edits from a laptop merge like any other lines.
  - A clean merge is a text that no Milkdown session produced, so the round-trip guard never saw it. riki still runs its static rules (riki `core/src/page.rs:96-105`) and the step 7 nav index build before pushing. SilverBullet returns the merged `text` so the client can load it (`server-common/src/reconcile.rs:33-37`); riki should do the same, along with the new blob oid, so the editor reloads and runs the guard again on its next edit.
  - **Don't commit `conflicted`.** SilverBullet writes the marker text into the file (`server/src/handlers/fs.rs:922-927`), because the file is the author's own working copy. In riki that would be a commit on GitHub, served to every reader. riki should commit only `Clean`; on `Conflicted` it should keep today's 409, and it can add the marker text to the response for the editor to show.
  - Retried saves stay idempotent. When the push lands but times out (503 `retry-safe`) and the client retries, the tip already holds merge M. Each chunk then has one side equal to the other, the first rule in `classify_chunk` (`server-merge/src/diff3.rs:61-75`), so the merge gives back M, and riki's existing "current bytes == new" check answers `content-present`. This deserves its own test next to riki's existing timeout-retry tests.

### The merge library (`server-merge/`)

- **Size:** about 350 lines with two dependencies, `similar` and `sha2` (`server-merge/Cargo.toml:7-9`). The public API is `merge(base, a, b) -> MergeOutcome::{Clean, Conflicted{text, hunk_count}}` plus `contains_conflict_markers` (`server-merge/src/lib.rs:1-6`, `server-merge/src/diff3.rs:6-54`). Small enough for riki's `core` to vendor or reimplement without adding a git merge index.
- **Line-level diff3:** both sides are aligned to the base with Myers (`server-merge/src/diff3.rs:114-130`). Lines are split with `split_inclusive('\n')`, so line endings and a missing trailing newline survive byte for byte (`:132-134`). Each stretch between sync points resolves by `classify_chunk`: sides equal, take A; A unchanged, take B; B unchanged, take A; otherwise it's a conflict (`:61-75`).
- **Word-level refinement:** this part matters most for riki. A Markdown paragraph written without hard wraps is one line, so line-level diff3 conflicts whenever two people edit the same paragraph. Before declaring a hunk conflicted, `refine_chunk` runs the same diff3 over whitespace-attached word tokens (`server-merge/src/diff3.rs:158-170`, `server-merge/src/refine.rs:5-44`).
  - It bails out above `REFINE_CHUNK_LIMIT = 8192` bytes (`server-merge/src/refine.rs:3-11`).
  - Any token-level conflict aborts the refinement as a whole (`:34`), so it never produces a partial merge.
  - The tokenizer is lossless by test (`server-merge/src/refine.rs:86-104`).
  - Golden tests: edits at the start and end of one line merge; insertions at the same position stay conflicted (`server-merge/tests/golden.rs:209-286`).
- **Symmetric output:** conflict markers are labeled with each side's SHA-256, and the side with the lower hash goes first (`server-merge/src/markers.rs:29-51`). So `merge(base, a, b)` and `merge(base, b, a)` are byte-identical, which a test helper asserts (`server-merge/tests/golden.rs:11-27`). If riki ever shows marker text in a 409, this keeps what the user sees independent of which save arrived first.

### Conflict marker detection

A laptop or agent can push a page holding a committed merge conflict. riki serves it as is: in CommonMark, a text line followed by `=======` is a setext H1, so the conflict renders as a garbled heading instead of looking like a conflict.

- **Strict to declare a conflict:** the server requires a complete `<<<<<<<` / `=======` / `>>>>>>>` triple, in order, so a page that only quotes a marker line doesn't read as conflicted (`server/src/revisions/sync.rs:359-377`). Inside a merge, the marker size comes from git's own `conflict-marker-size` attribute, bounded to 3..=256 (`server/src/revisions/conflicts.rs:216-273`).
- **Loose to declare it resolved:** a conflict stays open while any line, after leading whitespace, starts with three or more of `<`, `=`, `>`, `|` (`server/src/revisions/conflicts.rs:275-281`, used at `:748`). A false positive leaves the space paused, which is the safe direction, and the comment at `sync.rs:359-362` says so.
- **Fence-aware:** the client-side detector skips git-form markers inside ``` or ~~~ fences, because "a conflict-markers example inside a ``` / ~~~ block is documentation" (`plugs/index/conflict.ts:65-92`, fence exemption at `:179-184`). riki's own docs about git would trip a fence-blind check. Entry point: `containsConflictMarkers` (`plugs/index/conflict.ts:271-273`).
- **Degraded indexing:** a page with markers is still indexed, for its text only. Front matter "may be half-merged garbage" and is skipped, and the page stays in the page picker (`plugs/index/indexer.ts:217-235`). For riki, the equivalent is a WARN at publish, a banner on the page, and the page staying searchable. Making it a new `StaticRule` would make the page non-editable (riki `core/src/page.rs:79-83`), which blocks fixing it in the browser.

### Rewriting links on move

riki parks this ("Rewriting links when a page moves", riki `docs/design/2026-10-02-riki-wiki-operations.md:54`) and keeps every move a pure `R100` rename (riki `docs/design/2026-10-02-riki-wiki-operations.md:340`).

- **Two kinds of links, treated differently** (`plugs/index/refactor.ts:212-305`):
  - The moved page's own relative links are rewritten only when the folder changes (`:218-219`), because a relative link only breaks then.
  - Inbound links on other pages are always rewritten (`:289`, `updateBacklinks` at `:470-533`).
  - The split matters for riki. Rewriting inbound links touches other files, so the moved blob stays byte-identical and the rename stays `R100`, which riki's exact-rename redirect detection depends on (riki `core/src/redirect.rs:1-4`). Rewriting the page's own links changes the moved blob, so the move falls out of exact-rename detection. That half stays parked unless redirects move to similarity scoring.
- **Range splices, not reformatting:** each link is replaced at the source byte range the index recorded for it (`plugs/index/refactor.ts:32-33`, `plugs/index/refactor_splice.ts:25-54`).
  - Splices apply in descending offset order so earlier edits don't shift later ranges (`plugs/index/refactor.ts:252-253`, `:505-507`).
  - A page where no splice changed anything is not written (`:525-529`).
  - This fits riki's byte-identical round-trip rule. riki's nav index holds no link data today (riki `core/src/index.rs:67-72`), so riki would need link source positions; comrak's sourcepos is the natural source.
- **Markdown link handling** (`plugs/index/refactor_splice.ts:92-129`):
  - A relative target is recomputed from the editing page's location (`:109-114`).
  - `#header` and similar suffixes are kept (`:106`, `:115`, `matchDetail` at `:131-136`).
  - `<...>` wrapping is kept, or added when the new path has a space (`:117-120`).
- **Stale-index guard:** a splice applies only if the text at the range still names the old target; otherwise the text is left alone (`plugs/index/refactor_splice.ts:78-86`).
- **A trap to avoid:** the Markdown-link regex is `^(\[[^\]]*\]\()(<)?([^)]*?)(>)?(\))$` (`plugs/index/refactor_splice.ts:101`). For `[t](old.md "Title")`, the title falls inside the captured URL and is replaced along with it (`:109-115`), and none of the Markdown-link tests cover a title (`plugs/index/refactor_splice.test.ts:101-165`). riki should take the destination's span from the parser, not from a regex over the whole link.
- **Not atomic:** the rename writes the new file, deletes the old one, then writes each linking page, all as separate client calls (`plugs/index/refactor.ts:269`, `:285-287`, `:528`). SilverBullet gets away with this because its debounced auto-commit folds them into one commit. riki can do better: `commit_tree` already takes a list of tree ops (riki `docs/design/2026-10-02-riki-wiki-operations.md:88`), so the move and every inbound fix can be one commit.

### Case-only collisions

- A rename is refused when the target differs from an existing path only in case, because the two paths can't coexist on a case-insensitive filesystem (`plugs/index/refactor_case.ts:1-19`, used at `plugs/index/refactor.ts:162-171`). The check compares against the file list, not a metadata probe, which on such a filesystem would resolve the other casing (`plugs/index/refactor.ts:149-151`).
- riki's move only requires `to` to be absent, matched exactly (riki `docs/design/2026-10-02-riki-wiki-operations.md:159`), and `core/src/move_page.rs` (riki) has no case folding. The bare clone on Linux accepts `Setup.md` next to `setup.md`, but a macOS laptop checkout of the content repo can't hold both. New-page slugs are lowercased (riki `core/src/slug.rs:25`); a move's file name is typed by the user and isn't.

### Page history that follows moves

riki parks a page history UI (riki `docs/design/2026-10-01-riki.md:56`), and its moves rename files.

- Per-file history runs `git log --follow` (`server/src/revisions/read.rs:189-191`, enabled for pages at `:246-263`).
- Reading a version from before a rename looks up the file's old name through the same rename chain (`path_at_rev`, `server/src/revisions/read.rs:309-378`; fallback in `file_at` at `:396-418`). Without it, "every read of those rows would 404" (`:310-312`).
- Restore puts the old version back into the editor as one undo step; it isn't a separate write path (`docs/Revisions.md:76`). In riki, that's loading old text into the editor and saving it through the normal base-oid save.

### Search: what SilverBullet learned

- Built-in full-text search was removed from the main distribution because "full text indexing is expensive and the search results were quite bad" (`docs/CHANGELOG.md:388`). It now ships as optional libraries (`docs/Full Text Search.md:1-8`).
- The `Ctrl-k` page picker searches page names only, and the docs point to it as faster for pages you can name (`docs/Full Text Search.md:12`).
- For riki: the hand-rolled in-memory index (riki `docs/design/2026-10-02-riki-wiki-operations.md:332`) ranks title above heading above body, which is the cheap half that SilverBullet kept. The palette should keep that title match fast and good, because that's what readers lean on.

### git subprocess hygiene

- **Process groups:** every git child gets its own process group, and a timeout kills the whole group (`server/src/revisions/git.rs:104-108`, `:127-137`). riki uses `kill_on_drop` (riki `core/src/store.rs:551`), which kills `git` itself but not the `ssh` child that git spawns. riki's deploy remote is SSH (riki `docs/design/2026-10-01-riki.md:156`), so an ssh child stuck on a timed-out fetch or push survives the timeout.
- **Locale:** SilverBullet pins `LC_ALL=C` and `LANG=C` because callers match git's stderr text (`server/src/revisions/git.rs:70-72`, classifier at `server/src/revisions/sync.rs:30-54`). It also sets `GIT_OPTIONAL_LOCKS=0` (`:69`). riki sets only `GIT_TERMINAL_PROMPT=0` (riki `core/src/store.rs:547`) and matches porcelain rejection reasons (riki `core/src/store.rs:575-589`). Pinning the locale costs nothing and takes translation off the table.

## What it confirms riki already has right

- **No working tree:** SilverBullet has to tell its own writes from everyone else's. It keeps a watcher, attributes unknown writes as "External" (`server/src/revisions/engine.rs:684-705`, `docs/Revisions.md:26`), and runs an hourly sweep for changes the watcher missed (`server/src/revisions/engine.rs:13-15`). riki's bare clone has no files to watch; a laptop push arrives as a commit with its own author.
- **No `MERGE_HEAD` state:** SilverBullet needs all of this to live with a merge in progress:
  - auto-commit paused while `MERGE_HEAD`, `REBASE_HEAD`, `CHERRY_PICK_HEAD` or `REVERT_HEAD` exists (`server/src/revisions/store.rs:51-66`, `:203-205`)
  - a reported state that survives restarts (`server/src/revisions/engine.rs:655-664`)
  - a forced 60 s re-tick, because nothing else can close the merge (`server/src/revisions/engine.rs:16-20`)
  - a provenance file keyed by `HEAD`+`MERGE_HEAD` (`server/src/revisions/conflicts.rs:153-183`)
  - re-running `git merge-file` to prove a conflict is plain text (`server/src/revisions/conflicts.rs:323-369`)
  - generation and content-revision checks on every resolve (`server/src/revisions/conflicts.rs:622-643`)
  - `Resolved-by:` trailers on the merge commit (`server/src/revisions/conflicts.rs:783-793`)

  riki's fast-forward-only push never creates any of that state. A server-side merge in step 4 (above) keeps it that way: the merge is text in memory, and the commit has one parent.
- **Retry after a rejected push:** SilverBullet retries a rejected push once, with a full fetch and merge (`server/src/revisions/sync.rs:203-209`). riki's driver does the same, bounded by `push-retries` (riki `core/src/write.rs:169-175`).
- **One link resolver:** SilverBullet's server keeps a Rust copy of the client's link resolver, "mirroring `plug-api/lib/resolve_path.ts`" (`server/src/link_resolve.rs:1-3`). Two copies that must agree is the same trap the Wiki.js notes flag. riki resolves links in one place, at render time in `core`, and should keep the editor's link box on that rule.
- **Rebuild on publish:** SilverBullet's incremental index has to work out which pages to re-index when a file appears or disappears: backlinks to the path, pages with links to a page that didn't exist yet, and so on (`plugs/index/invalidate.ts:19-60`). riki rebuilds search and redirects inside `publish` for the whole good tip (riki `docs/design/2026-10-02-riki-wiki-operations.md:92`), so it never faces that bookkeeping.
