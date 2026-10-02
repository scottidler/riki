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
