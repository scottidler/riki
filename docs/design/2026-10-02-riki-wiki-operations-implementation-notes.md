# Implementation notes: riki wiki operations

## Phase 1: Multi-op tree commit
### Design decisions
- `TreeOp = Upsert{path, blob: Oid} | Remove{path}` with a `path()` accessor: core/src/store.rs. One place to validate and dedupe paths.
- `StoreError::DuplicateTreeOp(path)` is the typed error; checked (with `path::validate` on every op) before the builder runs and before any object is written: core/src/store.rs:commit_tree. The test counts object files to prove no write.
- `commit_file` validates the path, writes the blob in one blocking task, then delegates to `commit_tree` with a single `Upsert`: core/src/store.rs:commit_file. `commit_tree` takes blob oids, so blob creation stays with the file-contents caller.
- New tests (core/src/store/tests.rs): remove+upsert move (old path absent, same blob oid, emptied directory `a` gone), duplicate-path rejection (upsert+upsert and remove+upsert, no objects written), bad path in any op.
### Deviations
- `commit_tree` takes `author` and `committer` separately, not one `signer`, because `commit_file` already distinguishes them (author is the browser identity, committer is riki). Same effect, correct seam.
### Tradeoffs
- Dedup is a `HashSet` over exact path strings vs. also rejecting a path that is a prefix of another (file vs directory): the latter is a tree-state conflict that the Phase 3/4 op checks turn into a 409, and libgit2 reports it itself.
### Open questions
- None.

## Phase 2: Write driver
### Design decisions
- `write::run<O: WriteOp>` owns steps 3, 7, 8 under one hold of the repo mutex, including the non-fast-forward retry loop, which re-fetches and re-runs the op's check: core/src/write.rs:run. The op is a trait (`WriteOp::check_and_build`, an `impl Future + Send` so axum handlers stay `Send`) rather than a closure, so each later op (`Delete`, `Restore`, `Move`) is a named type with its own outcome.
- The op answers each fetched tip with `Check<T>`: `Committed(oid)` (driver validates and pushes), `NoCommit(T)` (a 200 without a commit: driver publishes the tip first, index conflict if it won't publish), `Conflict(T)` (driver publishes the tip, answers `T` regardless, as save's 409 did), `Refused(T)` (nothing published, as save's 422 did): core/src/write.rs:Check. These four are exactly the publish behaviors `save()` had, now named once.
- Step 6 (building the commit) is the op's, not the driver's: the op gets `Fetched {store, tip, author, committer}` and returns the commit oid. `Edit` keeps calling `commit_file` unchanged; later ops call `commit_tree`. Matches the doc's "op's own check-and-build for steps 4-6".
- `WriteOutcome<T>` carries the driver's own endings (`Pushed`, `IndexConflict`, `RetriesExhausted`, `FetchFailed`, `NoTip`, `PushTimedOut`, `PushFailed`) plus `Op(T)`; `save()` maps it onto the unchanged `SaveOutcome`, so `server/src/api.rs` and every save test are untouched: core/src/save.rs:save.
- `publish_tip` moved from save.rs into write.rs unchanged.
- Driver tests (core/src/write/tests.rs) use a `TestOp` that answers one `Check` variant; they cover push + publish, step-7 refusal, a raced non-fast-forward that re-runs check-and-build on the new tip, retries exhausted, each no-commit variant's publish behavior (including on a tip that refuses publish), and a failed fetch never calling the op.
### Deviations
- Log lines emitted by the driver say `write:` instead of `save:`, and save's "already holds the content" INFO is logged before the tip is published rather than after. Log text only; no outcome changes.
### Tradeoffs
- The driver takes `&SaveSettings` (committer + push retries) vs. a new `WriteSettings`: `SaveSettings` is imported by name in `server/src/api/save_tests.rs`, which this phase must leave unmodified, and adding a second struct with the same fields would be a shim. A rename can ride a later phase that touches the server wiring.
### Open questions
- Should `SaveSettings` be renamed `WriteSettings` (and moved to write.rs) when Phase 3 adds the first non-edit op, given it then configures every write, not just save?

## Phase 3: Delete and restore
### Design decisions
- `SaveSettings` stays as is, not renamed `WriteSettings` (Phase 2 open question, decided by the orchestrator): `server/src/api/save_tests.rs` imports it by name and must stay unmodified. Delete and restore take the same `&SaveSettings` through `write::run`.
- Both ops live in one module, `core/src/delete.rs` (`delete()`, `restore()`, the `Delete` / `Restore` `WriteOp`s): restore is only ever the undo of a delete, and D recognition belongs beside the commit it recognizes.
- One shared answer type for path ops, `write::OpAnswer { ContentPresent, Conflict(String), BadRequest(String) }`, and the ops return `WriteOutcome<OpAnswer>` directly instead of a per-op outcome enum mapped field by field. Front-door refusals (bad path, root README, multi-line message) come back as `WriteOutcome::Op(OpAnswer::BadRequest)` without running the driver. Phase 4's move can reuse it: core/src/write.rs:OpAnswer.
- "Present" / "absent" use a new `GitStore::entry_at` (`Tree::get_path`, any entry kind), per the Architecture bullet; a file that became a directory is a 409 for both ops (tests `delete_of_a_path_that_became_a_directory_is_a_conflict`, `restore_where_a_directory_now_sits_is_a_conflict`). Delete compares the entry oid to `base-oid` directly: git oids hash the object type, so a matching oid is that blob: core/src/delete.rs:Delete::check_and_build.
- D recognition, run on every fetched tip before either branch, in this order: D is a commit (`GitStore::commit_info`, `None` for a missing oid or a non-commit object), D reaches from the tip (`GitStore::reaches`: D == tip or `graph_descendant_of`), exactly one parent, `path` is a **file** in the parent (`blob_at`), nothing at `path` in D (`entry_at`). Any failure is `Check::Refused(BadRequest)` -> 400, so nothing is published: core/src/delete.rs:Restore::recognize.
- Root README guard is a 400 (the doc says "refused" without a status); it joins the other never-succeeds requests rather than inventing a new status: core/src/delete.rs:delete.
- The one-line message rule moved from `save::commit_message` to `write::one_line_message(message, default)` so delete shares it; `save::commit_message` keeps its signature and test and delegates.
- HTTP: handlers in `server/src/api/ops.rs` (child of `api`, so it uses `api`'s private `error` / `parse_oid` helpers), routes registered in `api::router`'s JSON-guarded POST group. The driver endings' responses (`index_conflict`, `retries_exhausted`, `fetch_failed`, `no_tip`, `push_timed_out`) were extracted from `save_response` into functions both `save_response` and `ops::op_response` call, so the status mapping exists once. 200 body is `{commit, content-present}` plus `url` for restore; `commit: null` with `content-present: true` on the idempotent branches.
- `parse_oid` takes the field name so a bad restore `commit` says `commit "..." is not an object id`, not `base-oid`.
- Tests: core `delete::tests` (10, every op check and idempotent branch, each D-recognition refusal), store tests for `entry_at` / `commit_info` / `reaches`, server `api::ops_tests` (8, the Phase 3 criteria end to end plus 400s, plain-503 fetch failure, and an index-conflict restore), and the 401/415 guard tests now loop over all four POST routes. Mutation checks: dropping delete's base-oid comparison fails 2 tests; dropping restore's same-bytes branch and the ancestry check each fail a test.
### Deviations
- Restore's message quotes D's actual subject, `Revert "<D summary>"` + blank line + `This reverts commit <D>.`, instead of the literal `Revert "riki: delete <path>"`. Identical for a default delete; when the user gave the delete a message, git's own revert format (which the doc cites) quotes that message, and a literal would quote a subject that never existed.
- D recognition requires `path` to be a file (blob) in D's parent, not just any entry: restore re-upserts a blob, so a directory there is unrestorable and is a 400 like the other non-delete commits.
- The `api/tests.rs` guard tests changed (route list extended, one 401 test added); save_tests.rs is untouched (`git diff --stat a8021a8 -- server/src/api/save_tests.rs` empty).
### Tradeoffs
- `WriteOutcome<OpAnswer>` returned from core vs. a `DeleteOutcome` / `RestoreOutcome` enum like `SaveOutcome`: the per-op enums would restate every driver variant only to be matched once in the server; `SaveOutcome` keeps its own enum because save_tests and the server already depend on it.
- D recognition per fetched tip (inside `check_and_build`) vs. once before the driver: per tip is what lets a restore on replica 2 recognize a delete made on replica 1 (D arrives with the fetch), and a non-fast-forward retry re-checks ancestry against the new tip.
### Open questions
- None.

## Phase 4: Move and redirects
### Design decisions
- The move op lives in `core/src/move_page.rs` (`move_page()`, `MoveRequest`, the `Move` `WriteOp`), returning `WriteOutcome<OpAnswer>` like delete and restore, so the server reuses `ops::op_response` unchanged. The commit is `[Remove{from}, Upsert{to, blob: base-oid}]` through `commit_tree`, which git reads as `R100`.
- Op check follows the table literally: proceed when `blob(from) == base-oid` (`blob_at`, so a directory at `from` never proceeds and a tree oid is never upserted as a file) and nothing at `to` (`entry_at`); `ContentPresent` when nothing at `from` and `blob(to) == base-oid`; otherwise a 409 whose message says which (`to` exists, `from` gone, `from` changed): core/src/move_page.rs:Move::check_and_build.
- Two extra 409s checked in the op, before the builder runs, so they never reach git2 or step 7 as opaque errors: a folder of `to` that is a file at the tip (`y.md/x.md` with `y.md` a file), and `to`'s twin (`a/b.md` <-> `a/b/README.md`) already present, which is the README collision: core/src/move_page.rs:Move::blocked. The twin check ignores the twin when it is `from`, so `a.md` -> `a/README.md` is allowed.
- Front-door 400s: either path fails `validate_page_path`, `from` is any `README.md` (folder move), `to == from`, multi-line message: core/src/move_page.rs:refusal.
- Rename detection is a store method, `GitStore::first_parent_renames(commit, since)`: one `spawn_blocking` that revwalks `simplify_first_parent` from `commit` until it meets `since`, then diffs each walked commit against its first parent with `find_similar(renames, exact_match_only)`, oldest first. It reports whether it met `since`; when it did not, the commits it walked are the full history. `redirect.rs` holds only the map logic, so the git work stays behind the store seam: core/src/store.rs:first_parent_renames.
- `Redirects::build(store, previous, commit)`: incremental from `previous` when the walk met its `walked` commit, otherwise a full rebuild from an empty map; logs mode, commits walked, renames, and elapsed time at INFO: core/src/redirect.rs. Each `.md` -> `.md` rename inserts `url_for_file(from) -> url_for_file(to)`, a later rename of the same URL replaces the earlier one, and a rename that keeps the URL (`a.md` -> `a/README.md`) adds nothing (it would be a self-loop).
- `Published { nav: Arc<NavIndex>, redirects: Redirects }` with public fields, behind `RwLock<Option<Arc<Published>>>`; `Wiki::good()` returns it, and `Published::commit()` keeps every `good().commit()` caller unchanged. Search adds a field here in Phase 8: core/src/wiki.rs:Published.
- `publish` builds the redirects after the nav index passes and before `set_good`, then swaps the whole snapshot; a store error during the walk leaves `refs/riki/good` and the served snapshot both on the old commit. The first publish (`Wiki::open`, and every restart) has no previous snapshot, so it does the full walk, awaited: core/src/wiki.rs:publish.
- 404 path: only when the good tip has no page at the URL and the request has no `new` query parameter, `redirects.resolve(path, is_page)` follows the chain, stops at the first hop that is a page at the good tip, and 301s there; `resolve` gives up after map-size hops (a cycle of dead URLs) and the request falls through to the existing 404: server/src/pages.rs:page_at, core/src/redirect.rs:resolve. `/x.md` 301s to `/x` first and is redirected on its next request, as today.
- Tests: core `redirect::tests` (resolve and apply units, full walk over laptop `git mv` history including a move-plus-edit that gets no redirect, incremental == cold, force-pushed history rewalked in full, redirects present in the first snapshot after a restart, poll extends the map), core `move_page::tests` (12), server `api::ops_tests` (6 move tests: the Phase 4 criteria end to end plus 400s), and the route-guard tests now cover `/_riki/api/move`. Mutation checks: removing the `?new=` skip fails `new_skips_the_redirect_and_offers_create_this_page`; returning the first hop regardless of liveness fails 3 `resolve` tests; disabling the move's idempotent branch fails `a_retried_move_is_content_present_and_commits_nothing`.
### Deviations
- Phase 4 says the startup walk happens "before the server binds". `server/src/lib.rs` binds the listener first (so a taken port fails fast) and calls `axum::serve` only after `Wiki::open` returns, so the redirects are in the first snapshot before any request is served; nothing changed there. Same effect, the bind order stays.
- "Rewalks fully if that one is no longer an ancestor" is implemented as "the first-parent walk back from the new tip did not meet the last walked commit". That covers a rewritten history and also a last walked commit that is an ancestor only through a merge's second parent, where an ancestor-based incremental walk would not equal a cold first-parent walk. One walk either way: a miss has already visited the full history.
- The op checks two 409 cases the doc does not list (a folder of `to` is a file; `to`'s README twin exists), per the Architecture rule that a file-vs-directory collision is a 409, never a git2 builder error.
- `pages.rs` reads `?new=` only to skip the redirect lookup; the editor prefill with `# <title>` stays Phase 7.
### Tradeoffs
- libgit2's default rename limit (200 candidate sources per target, `diff_tform.c`) vs. raising it: riki's own moves are one rename per commit, and a laptop commit moving more than 200 files at once is the parked folder move. Raising it makes every large laptop commit an O(sources x targets) oid compare.
- `Redirects` stored by value inside the `Arc<Published>` vs. its own `Arc`: the incremental build clones the previous map either way, and the snapshot is already shared.
- Rename detection in `GitStore` vs. a `git2::Repository` inside `redirect.rs`: the store is the only module that opens the repo, and `redirect.rs` stays testable without git for `apply` and `resolve`.
### Open questions
- None.

## Phase 5: Sidebar order
### Design decisions
- `_order` is read in `Wiki::index` (the per-oid nav build), beside the titles: `index::order_files(paths)` picks the visible `_order` blobs, `read_blobs` fetches them, `NavIndex::with_orders` attaches them. The result is structure only (`PageNode.order: Vec<String>`), so the per-oid cache stays text-free: core/src/index.rs, core/src/wiki.rs:index.
- `with_orders` keeps only entries that name a child of the folder (page stem or subfolder = the child's URL segment), once each, in file order. Unknown entries are dropped there, so the renderer never sees them. Blank lines, `#` lines, and surrounding whitespace are skipped: core/src/index.rs:with_orders.
- Problems come back as data, `OrderWarning { UnknownEntry, NotUtf8, NoPages }`, and `Wiki::index` logs one WARN per warning. They are never `IndexError`s, so publish goes on. Because the index is cached per commit the WARN fires once per commit, not per request or poll.
- `nav_items` / `group` share `listed_first(folder, rest)`: listed children first, then `rest` in today's order (root: pages then groups, each byte order; inside a group: all children interleaved in byte order). Without `_order` the iteration is exactly the old one: server/src/render.rs.
- `sidebar()` walks `nav_items` once. A run of pages shares one `<ul>`; a group closes it and renders its `riki-nav-group` block; a page after a group opens a new `<ul>`. The first `<ul>` is always emitted and a trailing one only when open, so the no-`_order` HTML is byte-for-byte what main emitted.
- The folder's `README.md` is the group itself; `README` as an `_order` entry names no child, so it warns like any unknown name. The root README is pushed first outside `_order`.
- Tests: index units (parse, dedupe, unknown, nested folder, unusable files, `order_files`, `_order` never a page), render tests (root order puts folder `c` before `b` before `a` with the pager's Previous/Next agreeing, new `<ul>` after a group, unlisted keep today's order, group order interleaves pages and folders, root README first), and a wiki test that publishes `a.md b.md c/x.md _order=c,b,gone`, asserts publish succeeded and exactly one WARN naming `gone`.
### Deviations
- A fourth warning, `NoPages` (an `_order` in a folder with no children, or non-UTF-8 content), is added beyond the doc's "entry naming nothing". Same effect: skipped with a WARN, publish goes on.
- The WARN capture needs a `tracing-subscriber` dev-dependency (fmt feature only) in `core`; the crate was already in the lockfile via the server.
### Tradeoffs
- Dropping unknown entries inside `NavIndex::with_orders` vs. leaving them in `PageNode.order` for the renderer to skip: the former keeps the renderer free of a case and the WARN next to the one place that knows the tree.
- Warning in `Wiki::index` (cached, once per commit) vs. in `publish`: `index` also runs for step-7 candidate commits that never publish, so a bad `_order` on a candidate also warns; that is useful signal and does not repeat.
### Open questions
- None.

## Phase 6: Tree and new-page routes, slug
### Design decisions
- `core/src/slug.rs` has `slugify(title)` (the rule, pure) and `new_page_path(&NavIndex, folder, title)`. "Taken" is `NavIndex::file_for_url(<folder>/<slug>).is_some()`: `<folder>/<slug>.md` and `<folder>/<slug>/README.md` map to one URL (`index.rs:url_for_file`), so one lookup covers both files from the good tip's nav index, with no extra git read. Top-level reserved names are checked against `index::RESERVED`.
- Folder validation is `path::validate` (400 on `..`, leading `.`, empty segment, NUL) plus a reserved-first-segment check (`status/x.md` would be an index error, so `folder=status` is a 400, `SlugError::ReservedFolder`). The folder need not exist: the move dialog creates folders by naming one.
- `GET /_riki/api/tree` reads the published `nav`: `folders` is every directory that directly holds a page file, plus `""` always (the root is always a valid destination, and the doc's example shows it); `pages` is `{path, url, title}` with `url` leading-slash (`/guide`, `/` for home) and title from `render::label`, the same label the sidebar shows (front matter or H1, else prettified segment, `Home` at root). 503 when nothing is published yet.
- `GET /_riki/api/new-page` returns `{path, url}` with a leading-slash url; missing `title` is a 400 (a missing `folder` means the root).
- Tests: core `slug::tests` (7: rule, untitled, free, `.md` taken, `README.md` taken, climbing suffixes, reserved at root only, new/invalid/reserved folder) and server `api::tree_tests` (4, via the router, including the three Phase 6 criteria cases: `guide/getting-started.md`, `-2` via either file, `status-2.md`).
### Deviations
- None.
### Tradeoffs
- Nav-index lookup vs. `entry_at` tree reads for the collision check: the doc says the check runs at the good tip and the slug is advisory; the nav index is that tip, already in memory. A bare directory with no page in it (e.g. `img/`) does not take a slug, which is right: no URL collides. A non-page file with the same stem (`x.png`) is not a collision either.
- Leading-slash `url` in both routes vs. riki's internal slash-less URLs: the client navigates with it directly, as the move/restore responses already do.
### Open questions
- None.

## Phase 7: Page actions UI
### Design decisions
- Server renders the controls, the page script wires them. `render::page` puts `data-path` and `data-base-oid` on the `<article>` (existing pages only; a missing page's article carries neither) and a ⋯ menu (`#riki-more`, `.riki-menu`, items `data-riki-action="move|delete"`) after Edit; `sidebar()` emits a "+" (`data-riki-new="<folder>"`) on a root row (`Pages`) and on each top-level group header: server/src/render.rs:page_menu, add_button, sidebar. `Action::Edit` gained `base_oid`, taken from `blob_at` in `pages.rs` (the same blob the page was rendered from, so Move/Delete check against what the reader saw).
- The menu is omitted per rule, not by JS: Move is not offered for any `README.md` (folder move, parked), Delete not for the root `README.md`; a page with neither gets no menu. This mirrors the server's 400s so the UI never offers a refused action.
- The "+" button is a sibling after the group's `<p class="riki-nav-heading">`, positioned over it by CSS, so the heading markup the existing sidebar tests pin is byte-identical.
- `?new=` prefill is the editor bundle's, not the server's: `editor/src/main.ts` auto-starts the session on `#riki-create` when the URL has a non-blank `new` param; `Session` uses `PageTarget.prefill` as the starting text only when the page does not exist (`base-oid` null and empty body). After Save, `rerender` drops `?new=` with `history.replaceState`. `?new` on an existing page has no `#riki-create`, so it is ignored.
- Title -> H1 text is escaped (`\ ` * _ [ ] < > ~ | &`) and whitespace collapsed in `editor/src/newpage.ts`, shared by both bundles, so a title like `A *b*` is literal text in the H1.
- Page script split: `page/ops.ts` (route calls returning `Reply<T>`, pure path helpers), `page/dialog.ts` (a plain-element modal, no `<dialog>`, so it runs under jsdom), `page/actions.ts` (the three flows), `page/main.ts` only delegates clicks. Delete is confirmed in the same modal, not `window.confirm`.
- Move dialog: folder field filters `tree.folders` as you type; a typed name that is not a folder is offered as "Create folder x" (choosing it only fills the field); file name prefilled with the stem, `.md` added when missing. Move to the same path is refused client-side. The tree fetch failing degrades to a typed folder with the reason shown.
- Delete: 200 replaces the article in place with "Deleted <path>." and Undo, empties the header actions, removes the pager, and swaps in the sidebar from a fresh GET of the same URL. `commit: null` (idempotent retry) shows no Undo. Undo posts `{path, commit}` from the delete response and navigates to the restore `url`.
- Tests: vitest `test/page-actions.test.ts` (13: body escaping, path helpers, route bodies and error mapping, each dialog flow incl. no-Undo and refused delete); server render tests (article data, menu rules, plus placement/escaping) and a pages test for the oid; Playwright `e2e/actions.spec.ts` (the three Phase 7 criteria plus the root "+").
### Deviations
- None.
### Tradeoffs
- Cursor lands at the end of the prefilled H1, not in a new empty paragraph below it: the parsed `# T\n\n` is just a heading, and inserting an empty paragraph risks serializing an empty block if the author saves without typing. The author presses Enter to start the body.
- Sidebar "+" only on the root and top-level group headers (the static `riki-nav-heading` rows), not on nested chevron groups: the doc says "root and each group header"; nested groups can be reached by typing the folder in the move dialog, and a "+" on every chevron row crowds the sidebar.
- `data-path` on both the Edit button and the article vs. reading only the article: the editor bundle already reads the button, and the doc asks for the article's attribute for the page script.
### Open questions
- None.

## Phase 8: Search index and route
### Design decisions
- `core/src/search.rs` is pure: `SearchIndex::build(pages: impl IntoIterator<Item = PageSource>) -> (SearchIndex, Vec<Skipped>)` takes owned `{path, url, title, bytes}` and touches no git. `Wiki::search` (core/src/wiki.rs) reads every page blob with one `read_blobs`, builds in `spawn_blocking`, logs one WARN per `Skipped` and one INFO with pages / sections / terms / skipped / elapsed. `publish` calls it after the redirects and before `set_good`, so a store error leaves the good ref and the snapshot on the old commit, as redirects already do.
- `Published` gained `pub search: SearchIndex`; `NavIndex` and the per-oid `indexes` cache are unchanged. `index::tests::nav_index_has_no_text_field` exhaustively destructures `NavIndex` and `PageNode`, so adding a field to either fails to compile until the test is edited.
- Sections come from one walk of the AST the renderer parses (`render::options`, now `pub(crate)`): text, inline code, code blocks and math are collected; front matter, raw HTML blocks and inline raw HTML are skipped; block nodes and soft/hard breaks add a space, and whitespace is collapsed. Every `Heading` (any level, any depth) starts a section and takes its anchor from one `comrak::Anchorizer` fed `collect_text()` in document order, exactly what comrak's `render_heading` does, so `## Setup` twice gives `setup` / `setup-1` (test `anchors_match_the_ids_the_renderer_gives_duplicate_headings` checks the ids against `render_markdown`'s HTML). An empty unheaded leading section is dropped; an empty page keeps one empty section so its title still matches.
- Title postings go on every section of the page, so `guide installer` finds the Install section of the page titled Guide (AND across title and body). Score per section = sum over query terms of the best field that term prefix-matched (title 3, heading 2, body 1). One hit per page: highest-scoring section, earliest section on a tie; pages ordered by score desc then path.
- Postings dedupe per `(section, field)`: a section's field is posted in one pass, so a repeat is always the vec's last entry.
- Snippet: 160 chars (Unicode scalars) of the section's collapsed text, the first match centered; when only the title or heading matched, the section's start with no marks. `marks` are the matched **prefix** of each token in the window (`tabl` in `table` marks `tabl`), in UTF-16 units; the prefix end is found by summing each char's lowercase length, so a case change that alters byte length (`É`) still lands on a char boundary.
- The hit's `title` is the sidebar label: `render::label` moved from the server to `riki_core::index::label` (server render, pages, and the tree route now import it) so search, tree, and sidebar can never disagree. A core test pins it; the server render test still covers it.
- Route `GET /_riki/api/search?q=&limit=` in `server/src/api.rs`: `url` gets the leading slash like tree/new-page; `limit` defaults to 20 and is clamped to 50; missing `q` or a non-integer `limit` is a 400; empty `q` is 200 with no hits; nothing published is a 503.
- Tests: core `search::tests` (13: terms, prefix + heading + anchor + snippet, no Markdown syntax / code included / front matter and raw HTML excluded, AND and field weights, title in every section, one hit per page + tie + limit, renderer-matching duplicate anchors, unheaded section, empty page by title, 160-char centering, UTF-16 marks with an emoji and case-changing chars, title-only snippet, static-rule skips), core `wiki::tests` (3: built at publish and rebuilt by a poll while a held snapshot keeps its own, present in the first publish after a restart, CRLF page WARNs once and publish goes on), core `index::tests` (2: no text field, label), server `api::search_tests` (4: the Phase 8 criterion end to end through the router and a poll, home hit `url: "/"` with null heading/anchor, limit default/cap, 400s/503).
### Deviations
- `Hit` in core carries `score` (used by the ordering and asserted in tests); the server's response leaves it out to match the API table.
- Moved `label` from `server/src/render.rs` to `core/src/index.rs` (not in the phase's file list): search in core needs the same title the sidebar shows. Same function, correct seam; no shim left in the server.
### Tradeoffs
- Reading page blobs again in `publish` vs. caching text from `Wiki::index`'s title read: `index` is the per-oid cache that must stay text-free, and it also runs for step-7 candidates that never publish. One extra `read_blobs` per publish is the doc's "one AST walk per page per publish".
- Marking the matched prefix vs. the whole token: the prefix shows the user why the hit matched; the client wraps exactly those ranges.
- No ellipsis or word-boundary snapping on the snippet: either would shift or complicate the UTF-16 offsets; the palette (Phase 9) can add a visual ellipsis outside the text node when the snippet is shorter than the section.
- `limit` above 50 is clamped rather than a 400: the doc states a max, not an error, and a palette asking for more still gets a useful answer.
### Open questions
- None.

## Phase 9: Search palette
### Design decisions
- The palette is `editor/src/page/search.ts`, in the page script (`riki.js`, no Milkdown). `openPalette(navigate, fetch)` builds a modal with a search input and a listbox, reusing the `riki-dialog-backdrop` / `riki-dialog` look. Pure helpers are exported and tested alone: `hitUrl` (`url#anchor`, plain `url` when `anchor` is null), `markedNodes(text, marks)`, `isPaletteShortcut(event)`, `fetchSearch`.
- `markedNodes` slices the snippet with JS string indices (UTF-16 code units, the unit the route reports) and builds text nodes plus `<mark>` elements whose `textContent` is the slice. Titles, headings, paths and snippets are all set via `textContent` or text nodes; there is no `innerHTML` in the file. Marks that overlap, run backwards or fall outside the snippet are skipped rather than trusted.
- Keys: the palette registers a capture-phase `keydown` on the document while open. Down/Up move (wrapping), Enter navigates to the selected hit's `url#anchor`, Esc closes. The first hit is selected as results arrive, so Enter alone opens the best hit and Down then Enter opens the second. Selection uses `aria-selected` and `aria-activedescendant`.
- Query handling: 150ms debounce (`DEBOUNCE_MS`); a ticket counter drops responses that arrive after a newer query or after close; a blank query clears the list and sends nothing; a server error or an empty result shows a status line.
- Shortcut: `main.ts` calls `isPaletteShortcut` (Ctrl or Cmd, plain K, event target not inside `.riki-editor-root` / `.ProseMirror`) and then `preventDefault()` and `openPalette`. Events from the editor are left untouched for the Phase 10 link box.
- Header button: `server/templates/page.html` has a `data-riki-search` button (icon, "Search", "Ctrl K" hint) first in `.riki-header-end`; `main.ts` opens the palette from the delegated click. Styles in `editor/src/theme/riki.css`. Bundles rebuilt and staged.
- Tests: vitest `test/search-palette.test.ts` (17: url and anchor, mark wrapping incl. emoji offset and hostile text, bad-range skipping, shortcut rules incl. editor target, fetch error mapping, Down+Enter to the second hit's `url#anchor`, Up wrap, Esc, field rendering, HTML in fields stays text, debounce to one request, blank query, server error, single instance and click); Playwright `e2e/search.spec.ts` (Ctrl+K, type, Enter lands on the hit URL with its anchor; header button and Esc; Ctrl+K inside the editor opens no palette).
### Deviations
- None.
### Tradeoffs
- The Playwright landing test types a body word (`pipe`) rather than `tabl`: Phase 8 gives a title-only match a snippet with no marks, so the page's top hit for `tabl` has nothing to highlight. A body match proves the marks, the heading anchor and the landing together.
- Ctrl+K is handled on `keydown` at the document in the bubble phase, not capture: the editor's own handlers (Phase 10) see the key first, and `isPaletteShortcut` already stands down for editor targets. The palette's own keys use capture so a dialog opened over it cannot swallow them.
### Open questions
- None.

## Phase 10: Editor link box
### Design decisions
- `editor/src/linkbox.ts` holds the whole feature: `linkBoxPlugin(sourceFile)` (a `$prose` plugin, registered in `makeEditor` right after `imageView(sourceFile)`, so the fixture suite loads it), and pure helpers `relativeHref`, `isAbsoluteUrl`, `urlChoice`, `matchPages`, `applyLink`, with `openLinkBox` as the DOM piece. The plugin's `handleKeyDown` takes Ctrl/Cmd+K (no shift/alt), returns true so ProseMirror `preventDefault`s the browser's own shortcut: editor/src/linkbox.ts.
- Rows are the tree's pages filtered client-side (every word must be a substring of title or path, tree order), with the search route's hits appended for matches the title/path filter missed (deduped by path, 150ms debounce shared with the palette). Both read through the Phase 7/9 helpers (`fetchTree`, `fetchSearch`); a tree failure shows the reason and the box still takes a URL and still searches.
- `relativeHref(sourceFile, target)`: drop the shared leading directories, one `..` per remaining source directory, then the rest of the target (`a/b.md` -> `c/d.md` is `../c/d.md`; same directory is the bare name, no `./`). The href is the repo path of the `.md`, matching how `core/src/render.rs:resolve` reads links.
- Input matching `^scheme:\S+$` and `URL.canParse` is inserted as typed (first row, selected); text is the selection, or the URL itself when nothing is selected. A page row with no selection inserts the page title as the link text.
- The selection range is captured when the box opens and applied on Enter (`addMark` over a range, or a text node carrying the link mark at the cursor), then focus goes back to the editor. Escape or a backdrop click closes with no change. The plugin declines (returns false) when the editor is read-only or the cursor's parent cannot hold a link mark (code block).
- Palette hand-off: `isPaletteShortcut` now also stands down for targets inside `.riki-linkbox`, and the box swallows Ctrl+K while open, so Ctrl+K in the box never opens the page palette: editor/src/page/search.ts.
- The box reuses the palette's classes (`riki-dialog`, `riki-search*`), so no CSS changed; bundles rebuilt and staged.
- Tests: vitest `test/linkbox.test.ts` (12: relative paths, URL detection, page filtering, the Phase 10 criterion end to end, with `a/b.md` choosing `c/d.md` giving `[here](../c/d.md)` that round-trips byte-identically; no-selection title text; URL passthrough; arrows plus a search-only hit; Escape/Ctrl+K inside; Ctrl+K through the real keymap and its refusal in a code block; tree failure). Fixture suite untouched and green.
### Deviations
- The Phase 9 Playwright test `inside the editor Ctrl+K does not open the palette` (asserting no `.riki-search` at all) is inverted by name to `inside the editor Ctrl+K opens the link box, not the palette, and links relative to the file`, which saves and checks `[Home](../README.md)` in the committed file. The old assertion pinned "the editor does nothing on Ctrl+K", which this phase changes (e2e/search.spec.ts).
### Tradeoffs
- A hand-built box on the palette's styling vs. Milkdown's link-tooltip input: the tooltip edits an href by hand and has no page picker; the doc asks for the shared search.
- Linking only the selection's marks via `addMark` vs. replacing the text: keeps the author's other marks (bold, code) inside the link.
- Cursor already inside an existing link: Mod-k adds a link mark over the new range rather than editing the old href (the fixed toolbar's link button still does that). Editing an existing link's target is not in the Phase 10 bullet.
### Open questions
- None.

## Phase 11: Selection toolbar
### Design decisions
- `editor/src/selectiontoolbar.ts` holds the feature: `tooltipFactory('RIKI_SELECTION')` from `@milkdown/kit/plugin/tooltip`, configured by `configureSelectionToolbar(sourceFile)` and registered in `makeEditor` (editor/src/setup.ts) right after `linkBoxPlugin`, so the fixture suite loads it. It adds no node, mark or schema change, so serialization is untouched (all 66 canonical fixtures still byte-identical).
- Controls: a Turn into button opening a menu (Text, Heading 1-3, Bulleted list, Numbered list, Task list, Quote; `TURN_INTO`), then bold, italic, strikethrough, inline code, and link. No underline (not GFM). Commands are the same kit commands the fixed toolbar uses, called through `commandsCtx`.
- Link opens the Phase 10 `openLinkBox` over the selection (declines in a node that cannot hold a link mark), after hiding the bar.
- `shouldShowSelectionToolbar(state)`: a non-empty `TextSelection` whose parent is not a code block; the provider's `shouldShow` adds editable and focus (editor or the bar). Controls `preventDefault` on mousedown so the selection survives a click.
- The task-list helper in `toolbar.ts` became `makeTaskList(ctx)` (the old `taskList(editor)` wraps it) so both toolbars share one implementation.
- Bar controls carry `data-control="selection-<id>"`, not the fixed toolbar's ids: the existing e2e selectors (`[data-control="bold"]`, `"link"`) would otherwise match two elements and fail Playwright strict mode. Styles in `editor/src/editor.css` (`.riki-selection-toolbar`, `.riki-selection-menu`), reusing the fixed toolbar's button look. Bundles rebuilt and staged.
- Tests: vitest `test/selectiontoolbar.test.ts` (8: mounts inside the root with the listed controls and no underline, Turn into list, Turn into H2 on a paragraph serializes `## pick this`, bullet/task/quote/text, bold/italic/strike/code, menu open/close, link opens the link box, show rules for range/cursor/code block). Playwright `e2e/editor.spec.ts`: selecting a line shows the bar, Turn into Heading 2 then bold saves `## **A canonical page.**`, and the fixed toolbar is still visible.
### Deviations
- None.
### Tradeoffs
- A custom menu of buttons vs. a `<select>` for Turn into: a select takes focus and the native popup is unstyleable and flaky under Playwright; buttons with mousedown-prevent keep the editor selection.
- The bar mounts on the first editor update (the provider's behavior), not at editor creation: tests dispatch a selection before looking for it.
### Open questions
- None.

## Phase 12: Block handle
### Design decisions
- `editor/src/blockhandle.ts` holds the feature: Milkdown's `block` plugin (`@milkdown/kit/plugin/block`) with a `BlockProvider` view, configured by `configureBlockHandle` and registered in `makeEditor` (editor/src/setup.ts) right after the selection toolbar, so the fixture suite loads it. It adds no node, mark or schema change; all 66 canonical fixtures stay byte-identical.
- The handle (`.riki-block-handle`) holds two buttons, `data-control="block-add"` (+) and `data-control="block-handle"` (⋮⋮). Drag is the plugin's own: the provider makes the handle element draggable, and the block service turns mousedown into a NodeSelection of the hovered block and dragstart into ProseMirror's `view.dragging` move, so ProseMirror's drop does the reorder. No drag code in riki.
- The menu (`.riki-block-menu`: Turn into, Duplicate, Delete; `BLOCK_MENU`) is a sibling of the handle in the editor root, not inside it: the handle element is draggable and its mousedown re-selects the hovered block, which a click on a menu entry must not do. The hovered block (`{pos, node}`) is captured when the menu opens; every action first checks `blockIsCurrent` (the doc still holds that exact node at that position) and does nothing otherwise, and a doc change closes the menu.
- Turn into reuses Phase 11's `TURN_INTO` entries: `turnBlockInto` sets a text selection spanning the block (`TextSelection.between` over its bounds), runs the same entry the selection toolbar runs, then collapses to a cursor so the selection toolbar does not pop up. It is disabled for blocks with no text of their own (divider, table) via `canTurnInto`.
- `duplicateBlock` inserts the same node right after the block. `deleteBlock` removes it, widening to the parent while the block is its parent's only child (the only item of a list removes the list), so no empty container is left. `addBlockBelow` (the +) puts the cursor in a new empty paragraph below the block, or in the block itself when it is already an empty paragraph.
- Gutter: `.riki-editor-root` reaches 3.5rem left of the text column (`margin-left: -3.5rem; padding-left: 3.5rem`), so the text sits where the article had it and the handle (placement `left-start`, 4px offset) falls inside the root. Below 1024px, where `.riki-main` pads only 16px, the negative margin is dropped and the text moves right by the gutter instead.
- The handle hides and the menu closes whenever the view is not editable (guard failed, saving), checked in the plugin view's `update`.
- Tests: vitest `test/blockhandle.test.ts` (10). jsdom has no layout, so the hover test stubs `posAtCoords` and `document.elementFromPoint` and dispatches a real `pointermove`, which drives the plugin's own show path and the menu through DOM clicks. Criterion: Duplicate on the second block of `other--mixed-page` and `blank-line-inserted--blocks-separated` serializes to the fixture with that block written twice. Also Delete, Turn into H2 on the whole block, Turn into disabled for divider/table, menu labels and Escape, + below, stale-block refusal, only-list-item delete, read-only hides the handle. Playwright `e2e/editor.spec.ts`: hovering "A canonical page." shows ⋮⋮ inside the root and left of the text column, level with the paragraph; Delete then Save writes `# Guide\n\n- one\n- two\n`. A second test drags ⋮⋮ onto the H1 and saves `A canonical page.\n\n# Guide\n\n- one\n- two\n` (native drag through Chromium). Bundles rebuilt and staged.
### Deviations
- The gutter + inserts an empty paragraph below and focuses it; it does not open the slash menu, which does not exist until Phase 13. Phase 13 adds the slash-menu open to `addBlockBelow`'s caller.
### Tradeoffs
- Menu entries as buttons with a nested Turn into list vs. a second floating submenu: one box, no second positioning pass, and the same mousedown-prevent pattern as the selection toolbar.
- Capturing the block at menu-open time vs. reading the provider's live active block at click time: the pointer crosses other blocks on the way to the menu, which would retarget the action.
- Deleting the parent of an only child vs. deleting just the node: ProseMirror would otherwise refill the list with an empty item, leaving a stray `-` in the saved file.
### Open questions
- None.
