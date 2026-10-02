# Design Document: riki wiki operations

**Author:** Scott Idler
**Date:** 2026-10-02
**Status:** Implemented
**Review Passes Completed:** 5/5

## Summary

riki v0.1.1 reads, edits, and creates pages, but everything else a wiki does (add a page from the nav, move it, delete it, find it) still happens through git on a laptop. This doc adds those operations in the browser, each one a single ordinary git commit, plus a Ctrl+K full-text search and the block-editing upgrades that make Milkdown feel like Mintlify's editor. Git stays the database: no pending store, no trash table, no search server.

## Problem Statement

### Background

- riki shipped v0.1.1 (`e4451b9`): request-time render, WYSIWYG edit, save as one commit, home deploy behind Caddy + Authelia (`docs/design/2026-10-01-riki.md`, Status: Implemented)
- That doc parked the operations this one picks up (Non-Goals table, `docs/design/2026-10-01-riki.md:42-66`):
  - Full-text search: "revisit when sidebar stops being enough to find pages"
  - Rename / move / delete in browser: "after first deploy"
- First deploy happened (Phase 7). Scott toured Mintlify's web editor on Tatari's eval site to set the bar: `docs/research/2026-10-02-mintlify-research.md`, Part 2
- Scott approved five items from that tour (handoff `docs/handoff/wiki-operations.md`, Scope)

### Problem

A riki reader who wants a new page has to type a URL that doesn't exist yet. Moving, renaming, or deleting a page needs a laptop and `git`. Finding a page means scanning the sidebar. The editor has a fixed toolbar and nothing else: no block handles, no slash menu, no table handles.

### Goals

Each traces to Scott's approved scope (handoff, items 1-5):

1. **New page:** a "+ New page" control in the sidebar asks only for a title, derives the path (kebab-case, `-2` if taken), and writes nothing until the first Save. The existing missing-URL "Create this page" flow stays
2. **Rename / move:** a title change is an ordinary edit; a path change is one `git mv` commit, chosen from a searchable folder picker that can create folders
3. **Delete:** one `git rm` commit, with an Undo toast implemented as a revert commit. Git history is the trash
4. **Search:** a Ctrl+K full-text palette (prefix matching, snippet, heading + path, keyboard nav) over an index derived from the git tree at the good tip. The same index drives a Ctrl+K "link to page" box in the editor that inserts a relative `.md` link
5. **Editor upgrades:** selection toolbar, gutter **+** and ⋮⋮ block handle (Turn into / Duplicate / Delete), a slash menu limited to riki's GFM blocks, table row/column handles, Markdown typing shortcuts. Callouts stay GitHub alerts. The round-trip guard stays byte-identical

Plus the two decisions the handoff left open, both settled by Scott on 2026-10-02 (Resolved Decisions): redirects after a move, sidebar order.

### Non-Goals

Excluded:

- A pending store, publish queue, or draft branch (Mintlify's model; contradicts git-as-database and one URL, research Part 2 "Fit with riki")
- A search server or database (Meilisearch, SQLite FTS): the index is derived in memory from the good tip
- MDX callouts or components; callouts stay `> [!NOTE]`

Parked, with revisit condition:

| parked | meanwhile | revisit when |
|---|---|---|
| Folder (subtree) moves | move pages one at a time; `git mv` on a laptop | a folder move is asked for |
| Undo for a move | move it back | a wrong move is a complaint |
| A trash view of deleted pages | `git log --diff-filter=D` | Scott asks |
| Rewriting links when a page moves (its own relative links, and inbound links) | redirects cover inbound URLs; the author fixes the page's own links with the link box | broken-link reports |
| Front matter `title:` editing in the browser | edit via git (already parked, `docs/design/2026-10-01-riki.md:59`) | unchanged |
| Drag-to-reorder in the sidebar | edit `_order` via git | Scott asks |
| Search ranking beyond title > heading > body | that order | ranking is a complaint |
| Mintlify's media, layout, API, Mermaid, math slash items | GFM blocks only | the block vocabulary grows |
| Search palette stale result: the ticket advances only when the debounced search fires (`editor/src/page/search.ts`), so an older in-flight response can render during the 150 ms window | accepted cost: Enter in that window can open the older query's hit (typed "new", navigated to "/old"); the fix is bumping the ticket on input | a wrong-page navigation from the palette is reported |
| Pasted HTML table writes `\| :--- \|` (same as v0.1.1; paste is not a `createAndFill` path) | accepted cost: a pasted table saves a non-canonical `:---` delimiter the round-trip guard cannot see | a pasted table shows up in a diff |

## Proposed Solution

### Overview

Every operation rides the save algorithm that already works (`core/src/save.rs:66-183`): fetch, check against the tip, build one commit, validate the nav index before pushing, push, retry on non-fast-forward, publish. What changes is the middle: today the commit is one upserted file; now it is a list of tree ops.

```
op           tree ops (one commit)                 check at tip
edit/create  upsert(path, blob)                    blob(path) == base-oid
move         remove(from) + upsert(to, same blob)  blob(from) == base-oid, to absent
delete       remove(path)                          blob(path) == base-oid
restore      upsert(path, blob from D's parent)    path absent, D removed path
```

N=1 vs N=2+: each replica builds its search index and redirect map from its own bare clone at its own good tip, so nothing new is shared between replicas; every op still serializes on the GitHub push. A restore on replica 2 of a delete made on replica 1 works because the op fetches first and D is in the pushed history.

Search is one more thing publish builds, next to the nav index. The editor work is all Milkdown plugins inside `makeEditor`, so the existing fixture suite covers them.

Terms (from the shipped doc): **tip** = `refs/remotes/origin/<branch>`; **good tip** = newest tip whose nav index built clean, persisted at `refs/riki/good`; **base oid** = the blob oid the client loaded. New here: **op** = one write request; **D** = a riki delete commit.

### Architecture

Unchanged crate split: `core` (no axum, `otto core-guard`), `server`, `editor/`.

**core**

- `store.rs`: `commit_tree(ops: &[TreeOp], parent, signer, message)` next to `commit_file` (`store.rs:176-197`); `commit_file` delegates. `TreeOp = Upsert{path, blob} | Remove{path}`. git2 `TreeUpdateBuilder` has both (`git2-0.21.0 src/build.rs:720,738`). Two ops on one path are a typed error before the builder runs (libgit2 rejects them with `duplicate entries given for update`, Phase 0b)
- `write.rs` (new): the op driver extracted from `save()`. Holds the repo mutex once, runs steps 3, 7, 8 for every op, calls the op's own check-and-build for steps 4-6. Every 200 without a commit publishes the fetched tip first and refuses with the index conflict if that tip won't publish, as `save()` does today (`core/src/save.rs:111-121, 125-129`), so a retried op never answers 200 while this replica still serves the old page. `save()` becomes the `Edit` op; its tests pass unchanged
- `slug.rs` (new): title -> path
- `search.rs` (new): in-memory inverted index, built from a commit, queried by prefix
- `wiki.rs`: `good` becomes one snapshot `Arc<Published {nav, search, redirects}>`, swapped as a unit in `publish`, so a request never sees the nav of commit N with the search of N-1. `publish` is the only writer: nothing else (startup included) swaps `good` The per-oid nav cache is unchanged and stays text-free
- `redirect.rs` (new): old URL -> new URL map from exact-rename detection over the branch history, part of the `Published` snapshot (Resolved Decisions)
- Tree lookups: "present" / "absent" in the op checks mean a tree entry of any kind at that path (`Tree::get_path`), not `blob_at`, which returns `None` for a directory (`core/src/store.rs:246-262`). A file-vs-directory collision is then a 409, never a git2 builder error
- `index.rs`: sidebar order read from each folder's `_order` file (Resolved Decisions)

**server**

- `api.rs`: new routes (API Design), same shape as `/_riki/api/page`: kebab-case JSON, `require_json` on POSTs, the `Identity` extractor for author, outcome enum in core mapped to HTTP in one `match`
- `pages.rs`: the 404 path consults redirects before rendering "Create this page", except with `?new=` (an explicit create; otherwise a page created at a moved-away URL, which the slug rule may pick, would 301 away before it exists); the page HTML carries `data-path` and `data-base-oid` on the article so the page script can move and delete without a round trip
- `render.rs`: "+" controls on the sidebar root and each group header; a ⋯ page-actions menu next to Edit

**editor (two bundles, as today)**

- `riki.js` (page script, every page, no Milkdown): Ctrl+K palette, new-page title dialog, ⋯ menu with Move (folder picker) and Delete (with Undo)
- `editor.js` (Milkdown): selection toolbar, block handle, slash menu, table handles, input rules, editor Ctrl+K link box. All registered inside `makeEditor` (`editor/src/setup.ts:73-93`) so `editor/test/fixtures.test.ts` exercises them. The fixed toolbar stays (`editor/e2e/editor.spec.ts:51` depends on it)

### Data Model

**Slug rule** (`core/src/slug.rs`)

- Lowercase (`char::to_lowercase`); keep `char::is_alphanumeric` (Unicode letters and digits stay, so "Café notes" -> `café-notes`); every other run -> one `-`; trim `-` both ends; empty -> `untitled`
- Candidate `<folder>/<slug>.md`; taken when the good tip has `<folder>/<slug>.md` or `<folder>/<slug>/README.md` (the two files that map to one URL, `core/src/index.rs:90-97`), or the URL is reserved at top level (`index.rs:19`). Then try `-2`, `-3`, ...
- The slug is advisory: the check runs at the good tip, and a page that appears before Save hits save step 4 (`current` != null base) -> 409, the editor keeps the text (existing behavior)

**Title**

- Unchanged rule: front matter `title:`, else first H1 (`core/src/render.rs:210-226`). A new page starts as `# <title>\n\n`
- Renaming a page = editing its H1, an ordinary save. Pages with a front matter `title:` keep that label until it's edited via git (front matter editing stays parked)

**Search index** (`core/src/search.rs`)

- Unit of indexing: a **section** = one page's text from a heading to the next heading of any level (text before the first heading is section 0 with no heading)
- Text: walk the comrak AST of the body (front matter excluded; comrak is already a dep) and collect text nodes, code included; no Markdown syntax reaches the index
- Terms: lowercase, split on non-alphanumeric, same alphabet rule as the slug
- Structure: `BTreeMap<String, Vec<Posting>>`, `Posting = {section: u32, field: Title | Heading | Body}`; prefix match = `range(prefix..)` while keys start with the prefix. No new crates
- Query: every query term must prefix-match (AND); score = per term, best field (title 3, heading 2, body 1); ties by path. Top `limit` sections, one hit per page by default (best section)
- Snippet: up to 160 chars of the section's text centered on the first match, with match ranges `[[start, end], ...]` in UTF-16 code units so the client can wrap `<mark>` via DOM text nodes (never `innerHTML`)
- Lifetime: part of the `Published` snapshot for the good tip. Not cached per oid: `indexes` (`wiki.rs:67`) is never evicted and also holds step-7 candidate commits that never publish (implementation notes, Phase 2 Tradeoffs), so text must not live there. Startup builds it through the `publish` that `Wiki::open` already runs (`wiki.rs:87-90`)
- Content problems never fail the build: a page failing static rules (non-UTF-8, BOM, CR) is skipped with a WARN. Store errors propagate exactly as `publish` already does (`wiki.rs:150-160`, `Result`)

**Delete commit (D) recognition, for restore**

- The client sends `{path, commit}` from the delete response
- Server accepts D when: D is the tip or an ancestor of it; D has exactly one parent; `path` is present in D's parent and absent in D. Anything else -> 400
- What this guarantees: restore only re-adds the bytes some commit in the branch's history removed at `path`. A move commit or an older delete passes too; the UI only ever sends the oid from its own delete response. Not a security check (any authenticated user can push anything via git)

### API Design

New routes, mounted before the page catch-all, next to the existing `/_riki/api/*`:

| route | request | 200 response |
|---|---|---|
| `GET /_riki/api/tree` | | `{folders: ["", "guide", ...], pages: [{path, url, title}]}` at the good tip; `folders` = every directory that holds at least one page |
| `GET /_riki/api/new-page?folder=&title=` | | `{path, url}` from the slug rule; 400 on an invalid folder |
| `GET /_riki/api/search?q=&limit=` | `limit` default 20, max 50 | `{hits: [{path, url, title, heading, anchor, snippet, marks}]}`; `anchor` = the id the reader page gives that heading, generated with `comrak::Anchorizer` (`comrak-0.55.0/src/lib.rs:91`) in document order so duplicate headings get the same `-1` suffixes the renderer emits (`core/src/render.rs:160`) |
| `POST /_riki/api/move` | `{from, base-oid, to, message?}` | `{commit, url}` |
| `POST /_riki/api/delete` | `{path, base-oid, message?}` | `{commit}` |
| `POST /_riki/api/restore` | `{path, commit}` | `{commit, url}` |

Write routes share the save route's status mapping (`server/src/api.rs:204-246`): 401 no identity, 415 not JSON, 400 invalid path, 409 conflict or index conflict or retries exhausted, 503 `retry-safe` on push timeout only, plain 503 on fetch failure (`server/src/api.rs:232-235`; `server/src/api/save_tests.rs:382-394` asserts no `retry-safe` there), 502 other push rejection.

Op checks (step 4 equivalent) and idempotent branches (a retried request after a 503 timeout must not error):

Every row's no-commit 200 publishes the fetched tip first (see `write.rs`). Restore validates D before either branch.

| op | proceed when | 200, no commit (`content-present: true`) when | 409 otherwise |
|---|---|---|---|
| move | `blob(from) == base-oid` and `to` absent | `from` absent and `blob(to) == base-oid` | yes |
| delete | `blob(path) == base-oid` | `path` absent (response has `commit: null`, so the client offers no Undo) | yes |
| restore | `path` absent | `blob(path)` == the blob D removed (same bytes re-created) | yes (different bytes at `path`) |

Extra guards:

- `move`: `to` passes `validate_page_path`; refused when `from` is any `README.md` (a folder's index page: moving it is a folder move, parked) or `to` equals `from`
- `delete`: refused for the root `README.md` (the site's home page)
- Commit messages: `riki: move <from> -> <to>`, `riki: delete <path>`, restore `Revert "riki: delete <path>"` + blank line + `This reverts commit <D>.` (git's own revert format). User `message` replaces the first line for move and delete
- Restore re-upserts the blob oid D removed, read from D's parent tree. It does not call `git2::Repository::revert_commit` (see Phase 0b)

**Page UI flows**

- **New page:** "+" on the sidebar root row and each group header -> a one-field dialog "Page title" -> `GET new-page` -> navigate to `<url>?new=<title>`. The missing-page view opens the editor with `# <title>\n\n` and a focused cursor. Nothing is written until Save; leaving the page writes nothing. The "Create this page" flow is the same code path without `?new`. `?new` on a URL that already has a page is ignored
- **Move:** ⋯ -> Move… -> dialog: folder picker (filter-as-you-type over `tree.folders`; typing a folder that doesn't exist offers "Create folder `<x>`", which only means the new path contains it) + file name (prefilled with the current stem) -> `POST move` -> navigate to the new `url`
- **Delete:** ⋯ -> Delete -> confirm -> `POST delete` -> the article is replaced in place by "Deleted `<path>`" with an **Undo** button; the sidebar re-renders without the page. Undo -> `POST restore` -> navigate to the restored URL. Leaving the page drops the Undo button; the commit stays in history
- **Search:** Ctrl+K (Cmd+K on macOS) anywhere outside the editor, plus a search button in the header -> palette; `preventDefault` so the browser's own Ctrl+K (address-bar search) doesn't fire; `riki.js` ignores the key when the event target is inside the editor, which owns Mod-k; 150ms debounce; ↑/↓ moves, Enter opens `url#anchor`, Esc closes; each hit shows title, heading, path, highlighted snippet
- **Link box (editor):** Ctrl+K inside the editor (no existing `Mod-k` binding, preset-commonmark keymaps checked) -> same search over `tree.pages` + search route; Enter inserts a link whose href is the target's path **relative to the edited file's directory** (`../c/d.md` from `a/b.md`), because riki resolves links against the source file (`core/src/render.rs:267-282`). With a selection, the selection becomes the link text; without, the target title. Input that parses as an absolute URL is inserted as-is

**Editor UI** (all inside `makeEditor`)

- **Selection toolbar** (`@milkdown/kit/plugin/tooltip`): Turn into (Text, H1, H2, H3, bullet, numbered, task, quote), bold, italic, strike, inline code, link (opens the link box). No underline (not GFM)
- **Block handle** (`@milkdown/kit/plugin/block`): on hover, gutter **+** (opens the slash menu below the block) and ⋮⋮ (drag to reorder; click opens Turn into / Duplicate / Delete)
- **Slash menu** (`@milkdown/kit/plugin/slash`), typed `/` at the start of an empty paragraph, filterable: Text, Heading 1-3, Bulleted list, Numbered list, Task list, Quote, Code block, Table, Divider, Note, Tip, Important, Warning, Caution. Same blocks as the toolbar plus heading, divider, and the five alert types
- **Table handles** (`@milkdown/kit/component/table-block`): row/column add, delete, drag, align. New cells get **no** alignment, so the delimiter row stays riki's canonical `| --- |`; an author-chosen alignment serializes as `:---` / `:---:` / `---:`
- **Alignment fix, one place:** extend the gfm `table_cell` and `table_header` schemas so `alignment` defaults to `null` (Milkdown `extendSchema`). That covers every `createAndFill()` path at once: table-block buttons, prosemirror-tables commands, the `|NxM| ` input rule, `insertTableCommand`. `RikiInsertTable`'s per-command workaround (`editor/src/table.ts:23-34`) then becomes redundant and is folded into it (Phase 0a: the default is the single cause)
- **Typing shortcuts:** the presets already convert `#`..`######`, `-`/`*`, `1.`, `[ ]`, `>`, ```` ``` ````, `---`, `**`, `*`, backtick, `~~`, `|NxM| ` (preset-commonmark `lib/index.js:1653-1665`, preset-gfm `:927-928`). Added: `[!NOTE] ` (and the other four types) typed at the start of a quote converts it to that alert. Fixed: `|NxM| ` builds unaligned cells, through the alignment fix above

### Implementation Plan

Phase 0 spikes write no repo code; harnesses lived in the session scratchpad and their results are pasted below. Every other phase: one commit, `otto ci` green (lint, bloat, check, test, core-guard, editor).

#### Phase 0a: Milkdown plugin round-trip spike (run during design)
**Model:** sonnet
- Mount block, slash, tooltip, table-block in a copy of `makeEditor`; run the 66 canonical + rewritten fixtures; check table alignment output; check Vue components under jsdom
- **Success criteria:** every canonical fixture byte-identical with the plugins loaded; alignment of new columns and `|3x3| ` recorded
- **Result** (copy of `editor/` with node_modules, `@milkdown/kit` 7.22.2, vitest 5.0.3 + jsdom; `.use(block).use(slashFactory(..)).use(tooltipFactory(..)).use(tableBlock)` in `makeEditor`, minimal `BlockProvider` / `SlashProvider` / `TooltipProvider` views):
  - full suite `Test Files 8 passed (8)`, `Tests 177 passed (177)` (169 + the spike file): all 66 canonical fixtures byte-identical, all 28 rewritten cases still fail the guard with a canonical rewrite
  - views mount under jsdom without warnings; the Vue table-block node view renders (`class="milkdown-table-block" data-v-app=""`, `data-role="col-drag-handle"`)
  - `|3x3| ` input rule and gfm `insertTableCommand()` defaults -> `| :--- | :--- | :--- |`; riki's `RikiInsertTable` -> `| --- | --- | --- |`
  - `addColumnAfter` (prosemirror-tables) and gfm `addColAfterCommand`, which table-block's add-column button calls (`components lib/table-block/index.js:627-631`), on `tables--basic` -> `| --- | :--- | --- |`: only the new column gets `:---`
  - cause: preset-gfm cell attr `alignment: { default: "left" }` (`preset-gfm lib/index.js:101`); every `createAndFill()` cell inherits it; parsed `| --- |` cells carry `null` (`:242`) and round-trip clean
  - no Mod-k binding in any `@milkdown/*` lib or in riki's `src/`; runtime Ctrl-k with the link tooltip loaded: not handled, doc unchanged (control: Ctrl-b handled)
  - not run: real-browser positioning and pointer interaction (Phases 11-14 Playwright)

#### Phase 0b: git mechanics spike (run during design)
**Model:** sonnet
- git2 0.21 on a bare repo: remove + upsert same oid; remove the last file in a directory; `revert_commit` on bare vs re-upsert; rename detection cost over a synthetic history
- **Success criteria:** `git diff -M --name-status` shows `R100`; no empty tree left; timings recorded for the redirect decision
- **Result** (git2 0.21.0 / libgit2-sys 0.18.8+1.9.7, `default-features = false`, bare repos, `create_updated` + `commit(None, ...)` as `commit_file` does):
  - remove `a/x.md` + upsert `c/x.md` (same blob) in one builder: `git diff -M --name-status` -> `R100	a/x.md	c/x.md`; `--no-renames` -> `D` + `A`
  - removing `b/z.md`, the only file in `b/`: tree `b` is gone from `ls-tree -r -t`; `git fsck --strict` exit 0
  - `revert_commit(&D, &tip, 0, None)` + `write_tree_to` works on a bare repo, no conflicts, tip = D and tip = D + one unrelated commit; its tree is byte-identical to re-upserting the deleted blob (`c2083fbd...` both, `55621e5c...` both)
  - two upserts of one path in one builder: `create_updated` fails `duplicate entries given for update` (code -1, class 14)
  - rename detection, full revwalk, `diff_tree_to_tree` + `find_similar(renames)` per commit, 200 files, 20 exact renames, release build, warm cache, loose objects: 1,000 commits 145 ms (exact only) / 141 ms (default similarity); 5,000 commits 586 / 584 ms (repeat runs 592-675 ms); all 20 renames found each time. Not measured: renames that also edit content, where similarity scoring does work

#### Phase 1: Multi-op tree commit
**Model:** sonnet
- `TreeOp`, `GitStore::commit_tree`; `commit_file` delegates
- **Success criteria:** store test: a remove + upsert commit has the old path absent and the same blob oid at the new path; two ops on one path -> the typed error, no commit; `cargo test -p riki-core store::` passes with every existing test unchanged
  - Observed on main (`a8021a8`): `test result: ok. 18 passed; 0 failed; ... 85 filtered out`

#### Phase 2: Write driver
**Model:** opus
- Extract steps 3, 7, 8 of `save()` into `core/src/write.rs`; `save()` becomes the `Edit` op; no behavior change
- **Success criteria:** `cargo test -p riki-server save_tests` passes with `server/src/api/save_tests.rs` unmodified (`git diff --stat a8021a8 -- server/src/api/save_tests.rs` empty)
  - Observed on main (`a8021a8`): `test result: ok. 15 passed; 0 failed; ... 110 filtered out`

#### Phase 3: Delete and restore
**Model:** opus
- `Delete` and `Restore` ops, D recognition, root-README guard, routes, status mapping
- **Success criteria:** tempfile test: delete then restore -> upstream gains exactly 2 commits and `file_at(path)` equals the original bytes; delete with a stale `base-oid` -> 409, nothing pushed; restore after the path was re-created with different bytes -> 409, with the same bytes -> 200 `content-present`, no commit; tempfile tests for delete and restore whose first push lands but times out (503 `retry-safe`), then a retry -> 200 `content-present` and an immediate GET renders the post-op state

#### Phase 4: Move and redirects
**Model:** opus
- `Move` op and route
- `redirect.rs`: revwalk the branch, first parent only, `diff_tree_to_tree` + `find_similar` with `renames(true)` and `exact_match_only(true)`; each `.md` -> `.md` rename adds `url_for_file(from) -> url_for_file(to)`. The first publish (the one `Wiki::open` awaits, `core/src/wiki.rs:75-89`) walks the full history in an awaited `spawn_blocking`, before the server binds, so redirects are in the first snapshot (586 ms at 5,000 commits, Phase 0b); each later publish walks only commits since the last walked one, or rewalks fully if that one is no longer an ancestor. The map is built inside `publish` and swapped with `good` as part of `Published`, never by a background task
- 404 path: follow `u -> ...` and stop at the **first** hop that is a page at the good tip, 301 there; no live hop within map-size hops -> 404. A page at `u` always wins; `?new=` skips the lookup
- **Success criteria:** tempfile test: a move -> one upstream commit whose `git diff -M --name-status` line is `R100 <from> <to>`, and `GET` of the old URL -> 301 to the new URL; a move onto an existing page or into a README collision -> 409, nothing pushed; move a->b then b->a, `GET /b` -> 301 `/a`; after foo->bar, `GET /foo?new=Foo` -> 404 "Create this page" (no 301); a move whose first push lands but times out (503 `retry-safe`), then a retry -> 200 `content-present` and an immediate `GET` of the old URL -> 301 to the new one; the map after an incremental publish equals the map from a cold full walk

#### Phase 5: Sidebar order
**Model:** sonnet
- `_order` file per folder: UTF-8, one name per line, a page stem (`setup` for `setup.md`) or a subfolder name; blank lines and `#` lines skipped. Listed entries first in file order, then the rest in today's order: at the root, root pages (byte order) then groups (byte order) (`nav_items`, `server/src/render.rs:250-267`); inside a group, pages and subfolders interleaved in byte order (`group`, `render.rs:270-289`). Without `_order` nothing moves; only a root `_order` can put a group before a page. The folder's own `README.md` is the group itself and never listed; the root README stays first. An entry naming nothing is skipped with a WARN, publish goes on
- `_order` is a non-dot name because `path::validate` refuses leading-`.` segments (`core/src/path.rs:40-45`); it isn't `.md`, so it never enters the nav (`index.rs:79-81`). Edited via git; riki's editor opens only `.md`
- Move and delete leave `_order` alone: a stale entry is skipped
- `nav_items` / `group` (`server/src/render.rs:250-290`) read the order, and `sidebar()` (`render.rs:296-320`) stops splitting root pages from root groups (today it renders all root pages, then all groups), so a folder can sort before a page. Previous/Next derive from `nav_items` (`reading_order`, `render.rs:375`) and a test checks they follow the same order
- **Success criteria:** test: a folder holding `a.md`, `b.md`, `c/x.md` and `_order` = `c\nb\ngone\n` renders the sidebar as `c`, `b`, `a` (folder `c` first) with Previous/Next in that order, logs one WARN for `gone`, and publish succeeds; with no `_order`, order is unchanged from main (existing sidebar tests pass unmodified)

#### Phase 6: Tree and new-page routes, slug
**Model:** sonnet
- `slug.rs`, `GET /_riki/api/tree`, `GET /_riki/api/new-page`
- **Success criteria:** unit test: "Getting Started" in `guide` -> `guide/getting-started.md`; `-2` when either `guide/getting-started.md` or `guide/getting-started/README.md` exists; title "Status" at root -> `status-2.md` (reserved)

#### Phase 7: Page actions UI
**Model:** sonnet
- `riki.js`: "+" controls and title dialog, `?new=` prefill, ⋯ menu, move dialog with folder picker, delete with in-place Undo; `data-base-oid` on the article
- **Success criteria:** Playwright: after foo->bar, opening `/foo?new=Foo` shows the editor prefilled with `# Foo` (no redirect); Playwright: "+" -> title -> the content repo has no new commit until Save, exactly one after; Playwright: delete then Undo -> the page renders at its URL again

#### Phase 8: Search index and route
**Model:** opus
- `search.rs`, build in `Wiki::publish`, `GET /_riki/api/search`
- **Success criteria:** test: query `tabl` finds `reference/tables.md` with its heading and a snippet; after a poll publishes a commit changing that text, the hit reflects it; `NavIndex` has no text field (search lives only beside `good`)

#### Phase 9: Search palette
**Model:** sonnet
- `riki.js` Ctrl+K palette and header button
- **Success criteria:** vitest: ↓ then Enter navigates to the second hit's `url#anchor`, Esc closes; Playwright: Ctrl+K, type, Enter lands on the hit's URL

#### Phase 10: Editor link box
**Model:** sonnet
- Mod-k in `editor.js`; relative-path computation; URL passthrough
- **Success criteria:** vitest: editing `a/b.md`, choosing `c/d.md` inserts `[…](../c/d.md)` and the doc round-trips through `editor/test/serialize.ts`

#### Phase 11: Selection toolbar
**Model:** sonnet
- **Success criteria:** `pnpm -C editor run test` green with the tooltip inside `makeEditor`; vitest: Turn into H2 on a paragraph serializes `## `

#### Phase 12: Block handle
**Model:** opus
- Gutter +, ⋮⋮ drag, Turn into / Duplicate / Delete
- **Success criteria:** fixture suite green; vitest: Duplicate on a canonical fixture's second block serializes to the fixture with exactly that block repeated; Playwright (`otto e2e`, not part of `otto ci`, `.otto.yml:136-140`): hovering a paragraph shows the ⋮⋮ handle inside the editor's left gutter, and choosing Delete from its menu removes that block from the saved file

#### Phase 13: Slash menu
**Model:** sonnet
- **Success criteria:** vitest: the menu lists exactly the 16 items above; choosing Warning on an empty paragraph serializes `> [!WARNING]`

#### Phase 14: Table handles
**Model:** opus
- Alignment default `null` via `extendSchema` (replaces `RikiInsertTable`'s workaround); table-block in `makeEditor`
- **Success criteria:** vitest: a column added through the handle serializes `| --- |`; Align center serializes `| :---: |`; Playwright (`otto e2e`): clicking a table's column handle and Add column after saves a file whose table has one more column

#### Phase 15: Typing shortcuts
**Model:** sonnet
- Alert input rule (`|NxM| ` is already unaligned after Phase 14)
- **Success criteria:** vitest: typing `> [!TIP] ` then text serializes `> [!TIP]\n> text`; typing `|2x2| ` serializes a `| --- | --- |` delimiter row

### Blast radius and ship order

| repo | change | when |
|---|---|---|
| `scottidler/riki` | all code | Phases 1-15 |
| `scottidler/riki-content-test` | test target for browser phases (stays; in active use for testing) | Phases 7, 9 |
| `scottidler/dotfiles` | `riki.yml` only if a phase adds a config key (none planned) | after the riki tag |
| `scottidler/homelab` | none | |

Ship: `bump release` from `main` with `--install "cargo install --path server --locked"`, restart the `riki` user unit, probe `/version` until the new tag shows.

## Acceptance Criteria

Run against the home deploy (`127.0.0.1:8737`, `/version` -> `v0.1.1`, `e4451b9`) and the `wiki-operations` checkout (`a8021a8` = `main` + docs) on 2026-10-02.

- [x] `curl -s '127.0.0.1:8737/_riki/api/search?q=lapt' | jq -r '.hits[0].url'` prints `/` (README's "Laptop push check" line) (Phase 8)
  - Observed on main: `curl -s -o /dev/null -w '%{http_code}' '127.0.0.1:8737/_riki/api/search?q=lapt'` -> `404` (route absent); README holds "Laptop push check: this line came from a git push" (`gh api repos/scottidler/riki-content/contents/README.md`)
  - Observed on v0.1.2 (`2098d71`, home deploy, 2026-10-02): `.hits[0]` -> `{"url":"/","title":"Wiki","snippet":"riki. Pages are Markdown files in this repo; ... Laptop push check: this line came from a git push, not the browser."}`
Criteria 2-4 write commits, so they run against a test riki pointed at `scottidler/riki-content-test`, never the home content repo.

- [x] A browser move of a page creates exactly one upstream commit whose `git diff -M --name-status HEAD~1 HEAD` line is `R100 <from> <to>`, and `curl -s -o /dev/null -w '%{http_code} %{redirect_url}' <old-url>` prints `301 <new-url>` (Phase 4, 7)
  - Observed on main: no move route (`GET /_riki/api/move` -> 404)
  - Observed on v0.1.2 (test riki on `127.0.0.1:8738` over `riki-content-test`, Playwright): move `notes/race.md` -> `archive/race.md` added 1 commit (`9e1a699`), `R100	notes/race.md	archive/race.md`; `GET /notes/race` -> `301 /archive/race`
- [x] Delete then Undo from the browser leaves upstream with exactly two new commits and the page's bytes identical to before (`git diff HEAD~2 HEAD` empty) (Phase 3, 7)
  - Observed on main: not runnable, no delete route
  - Observed on v0.1.2 (same test riki): delete then Undo on `notes/prose.md`, restore response `200`, 2 commits (`a5d76f2`, `dc3496f`), `git diff HEAD~2 HEAD` empty, bytes identical. A first attempt (`2dda448`) got no restore commit: the harness closed the browser about a second after clicking Undo, riki logged the delete and no restore, so the in-flight restore was dropped before it ran; that file was restored by API (`453090d`) and the run repeated waiting on the restore response
- [x] A "+" new page in `notes` with title "AC4 probe" creates no commit until Save, then exactly one adding `notes/ac4-probe.md` (Phase 6, 7)
  - Observed on main: not runnable, no "+" control; `notes/ac4-probe.md` absent (`gh api 'repos/scottidler/riki-content-test/git/trees/main?recursive=1' --jq '.tree[].path'` lists `notes/prose.md`, `notes/race.md` only)
  - Observed on v0.1.2 (same test riki): 0 commits after the title dialog, Save response `200`, then 1 commit (`1cc2b2d`) `A	notes/ac4-probe.md`
- [x] `pnpm -C editor run test` passes with block, slash, tooltip, and table-block registered in `makeEditor`, and `editor/fixtures/canonical/` still holds 66 files, unchanged (`git diff --stat a8021a8 -- editor/fixtures/canonical` empty) and all byte-identical (Phases 11-15)
  - Observed on main (`a8021a8`): `ls editor/fixtures/canonical | wc -l` -> `66`; `pnpm -C editor run test` -> `Test Files 7 passed (7)`, `Tests 169 passed (169)`, with none of the four plugins registered
  - Observed on v0.1.2 (`07fec8d` + bump): `pnpm -C editor run test` -> `Test Files 15 passed (15)`, `Tests 261 passed (261)`; `ls editor/fixtures/canonical | wc -l` -> `66`; `git diff --stat a8021a8 -- editor/fixtures/canonical` empty

## Resolved Decisions

- 2026-10-02: every op is one commit through one driver extracted from `save()` (author, from research)
- 2026-10-02: restore re-upserts the blob D removed, with git's revert message format, instead of calling `git2::Repository::revert_commit`. Phase 0b showed both give byte-identical trees on a bare repo; the upsert reuses `commit_tree` and needs no merge index (author)
- 2026-10-02: search is a hand-rolled in-memory inverted index in core, no new crates; SQLite FTS5 (clyde precedent) rejected (Alternatives) (author)
- 2026-10-02: search rebuilds on publish, not on every poll: a poll publishes only when the tip moves (`core/src/wiki.rs:203-206`) (author, correcting the handoff's "rebuilt on each poll")
- 2026-10-02: slugs keep Unicode letters; `path::validate` allows any UTF-8 and GitHub serves such names (author)
- 2026-10-02: the fixed toolbar stays beside the selection toolbar; removing it isn't in scope and an e2e test depends on it (author)
- 2026-10-02: README pages can't be moved (that's a folder move, parked); the root README can't be deleted (author)
- 2026-10-02: `scottidler/riki-content-test` stays; it is in active use for testing (Scott)
- 2026-10-02: **redirects by exact-rename detection from git history**, map held in memory beside `good`; covers riki moves and laptop `git mv`, no file, no path-rule exception. Phase 0b: 586 ms full walk at 5,000 commits, incremental after startup. Accepted costs: lost on history rewrite; a move that also edits the file in one commit gets no redirect. Rejected: redirect map file in the content repo (Scott, author's rec)
- 2026-10-02: **sidebar order from a per-folder `_order` file**; a reorder never changes a URL. Rejected: number-prefix file names, which change URLs (`index.rs:182-190`) and make every reorder a rename (Scott, author's rec)
- 2026-10-02: **a move leaves the page's own relative links as they are**; the move stays a pure `R100` rename and the author fixes links with the link box. Accepted cost (panel round 1): a page that fails the round-trip guard can't be edited in the browser (`editor/src/session.ts:92-93`), so its links get fixed on a laptop. Rejected: rewriting link destinations in the move commit (Scott, author's rec)
- 2026-10-02: under exact-rename detection, two byte-identical pages moved in one commit can pair ambiguously; accepted, pages carry an H1 so identical blobs are rare (author, panel round 1)

## Alternatives Considered

### SQLite FTS5 for search
- **Description:** clyde's shape (`~/repos/tatari-tv/clyde/sessions/src/db.rs:101-107`, `snippet()`, quoted tokens)
- **Pros:** proven ranking and snippets; in-house precedent
- **Cons:** new direct `rusqlite` dep with a bundled C build; a second store of page text to keep in sync with the good tip
- **Why not chosen:** the corpus is tiny (riki-content: 1 page; riki-content-test: 6) and the index is derivable on every publish; an in-memory map has no sync problem

### tantivy
- **Why not chosen:** a search engine's worth of deps for prefix matching over a few hundred pages; nothing in `Cargo.lock` today

### `git2::Repository::revert_commit` for Undo
- **Description:** libgit2's revert, then `write_tree_to` and commit
- **Pros:** works on a bare repo, no conflicts, tree identical to the upsert (Phase 0b)
- **Why not chosen:** same result for a single-file delete through a second code path (merge index) beside `commit_tree`

### Mintlify's untitled-page-then-rename flow
- **Description:** "+" creates `untitled-page-N` and the path follows the typed title (research Part 2)
- **Why not chosen:** riki writes nothing until Save, so the title has to come first; Scott's item 1 asks for the title prompt

### Pending store with a Publish dialog
- **Why not chosen:** contradicts git-as-database and one URL (research Part 2, "Does not fit")

## Technical Considerations

### Dependencies
- Rust: none new
- JS: none new; block, slash, tooltip, table-block ship inside `@milkdown/kit` 7.22.2. table-block is a Vue component; Vue is already in `editor.js` for the link tooltip

### Performance
- Search build: one AST walk per page per publish. Corpus today is 1 page (home) and 6 (test). Build time logged at INFO; no bound set until a corpus makes it a problem
- Search query: one BTreeMap range scan per term
- Bundle: `editor.js` grows by the four plugins; `riki.js` (4.8k, every page) grows by the palette and dialogs, still no Milkdown

### Security
- Write routes: same edge auth, `require_json`, `Identity` author as save
- Snippets and titles render through DOM text nodes; `marks` are offsets, never HTML
- Restore can only re-add bytes that a commit in the branch's history removed

### Testing Strategy
- Rust: tempfile bare repos (`riki_core::testing`), one test per op check and per idempotent branch, each broken once to prove it bites; the push race harness (`server/src/api/save_tests.rs:269`) reused for move and delete
- JS: vitest fixture suite with every new plugin inside `makeEditor`; Playwright against `scottidler/riki-content-test` for the browser flows

### Rollout Plan
- One release after Phase 15, or one after Phase 9 (operations + search) and one after Phase 15 (editor); Scott's call at bump time

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| A new Milkdown plugin changes serialization | Low (Phase 0a: 66/66 byte-identical with all four loaded) | High | plugins live inside `makeEditor`; fixture suite fails on any byte change |
| Table handles emit `:---` | Certain without the fix (Phase 0a) | Med | schema default `null`; Phase 14 criterion |
| A move breaks the page's own relative links | High when the folder changes | Med | accepted (Resolved Decisions); link box makes the fix quick; parked row |
| Two users act on one page at once | Low | Low | base-oid checks; 409 |
| A retried op after a 503 timeout errors instead of succeeding | Med | Med | idempotent branch per op, each publishing the tip first |
| A delete whose push lands but times out loses its Undo (the retry answers `commit: null`) | Low | Low | accepted; the page is in history (`git log --diff-filter=D`) and restorable via git |

## Open Questions

(none)

## Review Log

- **Pass 1 (Draft):** full template from the design-research brief
- **Pass 2 (Correctness):** search anchors via `comrak::Anchorizer` so duplicate-heading ids match the reader; `commit_tree` rejects two ops on one path (libgit2 `duplicate entries`, Phase 0b); restore rationale restated from Phase 0b (upsert and `revert_commit` trees identical); search rebuilds on publish, not per poll; acceptance criteria rerun and outputs recorded
- **Pass 3 (Clarity):** op table up front; terms `op` and `D` defined; UI flows written per operation; write criteria scoped to `riki-content-test`
- **Pass 4 (Edge cases):** idempotent branch per op for 503 retries; delete with `commit: null` offers no Undo; `?new` ignored on an existing page; Ctrl+K split between page script and editor plus `preventDefault`; README move and root delete refused; N=1 vs N=2+ stated; redirect hop limit and live-page-wins rule
- **Pass 5 (Excellence):** table alignment fixed once at the schema default instead of per command (Phase 0a root cause); Phase 0 results pasted; risks re-rated from measurements

- **Panel round 1** (synthesis `/tmp/review-panel/TvBKgH4c/synthesis.md`; seats reviewed the snapshot with Q1-3 open, findings checked against the live doc; 8 must-fix and 6 cheap-win, all folded in):
  - Must-fix: fetch failure is a plain 503, not `retry-safe`; every no-commit 200 publishes the fetched tip first; redirects stop at the first live hop (move-back test); `?new` skips redirects; `sidebar()` named for root `_order` interleave; slash count 16; restore same bytes -> 200 / different -> 409; "absent" = no tree entry
  - Cheap-win: diff bases pinned to `a8021a8`; AC2 `HEAD~1 HEAD`; AC4 uses an unused slug; Playwright criteria for Phases 12 and 14; nav + search + redirects in one `Published` snapshot; "never fails" restated; delete-timeout Undo loss in Risks; D guarantee restated; incremental == cold redirect test; 3B cost recorded
  - Pushed back (not folded): Architect's Q1 overturn and Q3 "A breaks the guard" (the guard checks Milkdown output, a server-side edit never passes through it); Architect's mutex-starvation concern (corpus is 1-6 pages, nav already reads every blob under that lock); Staff's drag-is-unrequested (drag is the ⋮⋮ handle's native behavior)

- **Panel round 2** (synthesis `/tmp/review-panel/TvBKgH4c/synthesis.md`, Round 2 section; scope D1-D3 + fold-in defects): both seats ACCEPT all three pushbacks; 3 must-fix, all folded in:
  - Redirect map built inside `publish`, the startup full walk awaited in `Wiki::open`'s first publish; `publish` is the only writer of `good`
  - Move timeout/retry test moved from Phase 3 to Phase 4; Phase 4 asserts the `?new=` 404 page, the editor prefill assert moved to Phase 7
  - Phase 5 fallback order stated: root pages then groups at the root, byte-order interleave inside groups; unchanged without `_order`

- **Panel implementation audit** (synthesis `/tmp/review-panel/pyquHtCj/synthesis.md`, probes `/tmp/review-panel/pyquHtCj/probes.md`; Mode 2 against `2de254d`): 2 must-fix and 3 cheap-win folded in, 2 deferred. The staff seat timed out (rc=124 on both attempts) and wrote no review; its findings came from its preserved traces, each re-run by the panel before inclusion
  - Must-fix: Turn into lifts list items and quotes out before applying the target (selection toolbar and the ⋮⋮ menu share `TURN_INTO`); restore under a file that took a folder name is a 409 (the move ancestor check, shared), not a git2 D/F 500
  - Cheap-win: tree, new-page, search, move and restore `url`s percent-encoded like the sidebar, and the link box writes encoded relative links; Save drops only `?new=`, keeping other params and the hash; the new-page H1 escapes `#` so a trailing ` #` stays in the title
  - Deferred (parked rows in Non-Goals): search palette stale result; pasted HTML table `:---`
  - Open questions answered: Q1 the empty alert keeps `> <br />` (Milkdown's universal empty-paragraph form; the marker line meets Phase 13); Q2 no "Add row after", the boundary `+` already meets row add; Q3 paste stays parked
- **v0.1.2 release and acceptance run** (2026-10-02): `bump release` from `main` -> `2098d71`, CI green, tag `v0.1.2`, `cargo install --path server --locked`, `riki` user unit restarted; `/version` -> `{"version":"v0.1.2","git_sha":"2098d71"}`, `/status` ok; startup log shows the redirect walk (3 commits, 2 ms) and search build before `riki listening`. AC1 on the home deploy; AC2-4 in Playwright against a test riki over `riki-content-test` (start `757d68a`, end `c246244`, tree identical to start after moving `race.md` back and deleting the AC4 page); AC5 local. All five pass
  - Found: closing the browser mid-operation drops the request before the op runs (first AC3 attempt); nothing is written, and the user gets no answer

## References

- Handoff: `docs/handoff/wiki-operations.md`
- Mintlify research: `docs/research/2026-10-02-mintlify-research.md`
- Shipped design: `docs/design/2026-10-01-riki.md`; implementation notes `docs/design/2026-10-01-riki-implementation-notes.md`
- marquee corpus snapshot pattern: `~/repos/tatari-tv/marquee/core/src/corpus.rs`
- clyde FTS5: `~/repos/tatari-tv/clyde/sessions/src/db.rs`
- git2 0.21 `src/build.rs:720,738`, `src/diff.rs:248`; Milkdown 7.22.2 kit plugins
