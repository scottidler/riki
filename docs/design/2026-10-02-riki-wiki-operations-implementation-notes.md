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
