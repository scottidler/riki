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
