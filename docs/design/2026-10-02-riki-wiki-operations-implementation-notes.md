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
