# Gollum learnings for riki

**Source:** `gollum/gollum` (the Sinatra app) and `gollum/gollum-lib` (the library), both branch `master`, commits `d00fefc8` (gollum) and `e4f2541f` (gollum-lib), cloned at `~/repos/gollum/gollum` and `~/repos/gollum/gollum-lib`. Paths below are prefixed with the repo name; riki paths are marked (riki).
**Date:** 2026-10-02

## Framing

- Gollum is the closest architecture riki considered: git-backed and rendered at request time. riki's shipped doc rejected it for its source editor and its language (riki `docs/design/2026-10-01-riki.md`, Alternatives, "Otterwiki / Gollum"). The editor is Ace over Markdown source (`gollum/lib/gollum/public/gollum/javascript/editor.js:1-12`).
- The local repo is the canonical copy. Gollum has no remote and never pushes. Syncing is left to a user `post_commit` hook, and its README warns that "it may not be possible to resolve all conflicts automatically" (`gollum-lib/README.md:230-233`). riki inverts this: GitHub is canonical and the push is the lock.
- gollum-lib reaches git through an adapter gem: `gollum-rugged_adapter` on MRI, `gollum-rjgit_adapter` on JRuby (`gollum-lib/gollum-lib.gemspec:5-9`). The adapter is not in these clones, so `index.commit`, `git.grep`, `revert_path` and `revert_commit` are described below by their call sites only.
- Gollum serves many markup formats (`gollum-lib/lib/gollum-lib/markups.rb:69-97`) and its URLs carry the file extension: a rename to `/C` lands on `/C.md` (`gollum/test/test_app.rb:184-190`). riki serves one format at extension-less URLs and 301s `/a/b.md` to `/a/b` (riki `server/src/pages.rs:85-87`).
- The app builds a new `Gollum::Wiki` for every request (`gollum/lib/gollum/app.rb:720-722`), so its commit and tree caches (`gollum-lib/lib/gollum-lib/git_access.rb:197-216`) live for one request.
- What transfers is a set of subsystem decisions and a few gaps to avoid, not the architecture.

## What transfers

### Committing onto a ref without a working tree

riki's equivalent is `commit_tree` over a list of `TreeOp`s (riki `core/src/store.rs:238-272`).

- Gollum's `Committer` stages into an in-memory git index seeded from the parent's tree, or from an explicit tree when one is given (`gollum-lib/lib/gollum-lib/committer.rb:39-49`). It then commits that index onto `@wiki.ref` in one call (`gollum-lib/lib/gollum-lib/committer.rb:127-128`). No checkout is involved.
- The working tree is an afterthought: after the commit, only a non-bare repo gets `git rm` or `git checkout <path> HEAD` for each touched path (`gollum-lib/lib/gollum-lib/committer.rb:107-122`). A bare repo is a first-class mode (`--bare`, `gollum/bin/gollum:66-67`).
- Batching is a convention, not a type: every write method takes an optional `:committer` and, when one is passed, stages into it and returns it instead of committing (`gollum-lib/lib/gollum-lib/wiki.rb:309-310`, `:321`; same shape in `update_page` `:354-355`, `:370` and `write` `:772-781`). The rename route uses this to put the move and the `.redirects.gollum` update in one commit (`gollum/lib/gollum/app.rb:317-332`). The edit route uses it for page, header, footer and sidebar together (`gollum/lib/gollum/app.rb:352-359`). riki's `&[TreeOp]` is the typed version of the same idea, and it already rejects two ops on one path before libgit2 sees them (riki `core/src/store.rs:246-251`).
- The "target must be absent" check lives inside the committer, so every write path gets it: `add_to_index` raises `DuplicatePageError` if the path exists in the parent tree, unless the same batch deletes it or the caller forces an overwrite (`gollum-lib/lib/gollum-lib/committer.rb:83-88`, deletion lookup `:157-170`). riki puts that check in each op's `check_and_build` instead (riki `core/src/move_page.rs:108-112`). Gollum's placement is worth noting if riki adds more ops: an absence check at the builder can't be forgotten by a new op.
- The parent is resolved lazily, as the ref's commit at the moment the index is first built (`gollum-lib/lib/gollum-lib/committer.rb:65-72`), not as the commit the client loaded. That gap matters for concurrency (see the 412 section).
- `after_commit` callbacks and a `:post_commit` hook run after every commit (`gollum-lib/lib/gollum-lib/committer.rb:129-133`, `gollum-lib/lib/gollum-lib/hook.rb:24-31`), and an optional `:note` is attached as a git note (`gollum-lib/lib/gollum-lib/committer.rb:132`, fed from the session at `gollum/lib/gollum/app.rb:731-737`). riki has no use for a hook today. A note would only leave the box if riki also pushed `refs/notes/*`; it pushes only `<commit>:refs/heads/<branch>` (riki `docs/design/2026-10-01-riki.md`, Save algorithm step 8).

### Redirects: an explicit file vs rename detection

riki chose redirects derived from exact-rename detection over first-parent history, and rejected a redirect map file in the content repo (riki `docs/design/2026-10-02-riki-wiki-operations.md`, Resolved Decisions). Gollum is that rejected alternative, shipped, so its failure modes test the decision.

How gollum does it:

- The map is a YAML hash in `.redirects.gollum` at the repo root (`gollum-lib/lib/gollum-lib/redirects.rb:3`, load `:19-31`, dump `:33-36`). Keys and values are file paths with extensions, e.g. `Home.old.md: Home.md` (`gollum-lib/test/test_wiki.rb:735-743`).
- Only the rename route adds an entry, old `url_path` to new path, staged into the rename's own commit (`gollum/lib/gollum/app.rb:327-332`, `gollum-lib/lib/gollum-lib/wiki.rb:566-569`). `remove_redirect` exists (`gollum-lib/lib/gollum-lib/wiki.rb:571-574`) but nothing in the app calls it.
- Lookup runs only after both the page and the file lookups miss, so a page at the old URL always wins (`gollum/lib/gollum/app.rb:648-667`). riki has the same rule (riki `server/src/pages.rs:92-100`).
- The redirect appends `?redirected_from=<old>` (`gollum/lib/gollum/app.rb:620-622`), so the landing page can say "redirected from". riki's 301 carries nothing (riki `server/src/pages.rs:170-173`). A query parameter would let riki show a "moved from" note without a lookup.
- The file is fenced off from the browser: no GET (`gollum/lib/gollum/app.rb:605-607`), no edit, rename, delete or revert (`gollum/lib/gollum/app.rb:222-232`), and it is on the protected-files list (`gollum-lib/lib/gollum-lib/file.rb:167-169`).

What that costs, each a point for riki's choice:

- **Renames outside the rename route get no redirect.** An edit that changes a page's format renames the file (delete + add, `gollum-lib/lib/gollum-lib/wiki.rb:351-362`) without touching the map, and a `git mv` from a laptop never touches it either. riki's detection covers laptop `git mv`. Its own gap is a move and an edit in one commit (riki `docs/design/2026-10-02-riki-wiki-operations.md`, Resolved Decisions, accepted cost).
- **One shared file is a write hot spot.** Every rename rewrites `.redirects.gollum`, so two unrelated renames always touch the same blob. Under riki's push-as-lock, that would turn two unrelated moves into a non-fast-forward retry, or a 409 if the check compared the file's blob. Derived redirects keep a move's commit a pure `R100` (riki `core/src/move_page.rs:117-123`).
- **A broken file silently drops every redirect.** A YAML error is rescued with `# TODO handle error` and the map comes back empty (`gollum-lib/lib/gollum-lib/redirects.rb:22-27`). riki has no file to break.
- **The cache key is the wrong ref.** Staleness compares `repo.head.commit.sha` (`gollum-lib/lib/gollum-lib/redirects.rb:10-12`, `:38-40`), while the file is read at the wiki's configured ref (`gollum-lib/lib/gollum-lib/redirects.rb:20`, default version `@ref` at `gollum-lib/lib/gollum-lib/wiki.rb:195-197`). When `--ref` names a branch other than HEAD (`gollum/bin/gollum:63-64`), the cache tracks one branch and the data comes from another. riki builds the map inside `publish` as part of the `Published` snapshot, keyed by the good tip (riki `core/src/redirect.rs:26-49`).
- **Chains are walked by the browser, one HTTP hop per entry, with no bound.** Each lookup is a single hash get (`gollum/lib/gollum/app.rb:666-667`). Reading the code: rename A to B, then B back to A, and the map holds `A.md: B.md` and `B.md: A.md`. Then delete A (delete never touches the map, `gollum/lib/gollum/app.rb:366-375`), and a GET of `A.md` redirects to `B.md`, which redirects back to `A.md`, until the browser gives up. riki walks the chain server side, stops at the first live hop, and caps the walk at the map's size (riki `core/src/redirect.rs:64-77`). Derived from the code paths above, not run.

### Edit conflicts: the 412 and what the client does

- The edit page embeds the page's blob sha as a hidden `etag` field (`gollum/lib/gollum/app.rb:245`, `gollum/lib/gollum/templates/editor.mustache:20`). This is riki's `base-oid`.
- On save, the route compares the field to the current blob sha. On mismatch it answers `412` with a JSON body `{etag, text_data}`, the latest sha and text (`gollum/lib/gollum/app.rb:339-349`). Tested at `gollum/test/test_app.rb:108-123`.
- The client throws that body away. On 412 it re-enables Save and shows an `alert` telling the user to copy their text out of the browser, reload, and reapply their changes (`gollum/lib/gollum/public/gollum/javascript/gollum.js.erb:614-617`). riki's 409 carries `current-body` (riki `core/src/save.rs:120-132`), and the editor keeps the author's text and offers "Load latest" behind a confirm (riki `editor/src/session.ts:225-237`). The gap gollum leaves open is the next step riki could take: the 409 already ships the latest body, so the editor could show a diff of the author's text against it instead of only discarding or keeping. That is the cheap step short of the parked co-editing row (riki `docs/design/2026-10-01-riki.md`, Non-Goals).
- The check is narrower than it looks:
  - Only the main page is checked. Header, footer and sidebar are written in the same commit with no sha check (`gollum/lib/gollum/app.rb:356-358`).
  - The check reads the page at the start of the request (`gollum/lib/gollum/app.rb:343`), but the parent is resolved later, when the committer first builds its index (`gollum-lib/lib/gollum-lib/committer.rb:65-72`). Nothing in gollum or gollum-lib holds a lock or passes an expected old ref value between the two. Whether the adapter's ref update compares and swaps can't be read from these clones.
  - Rename, delete and revert send no sha at all (`gollum/lib/gollum/app.rb:298-305`, `:366-375`, `:417-428`).

  riki checks every op against the tip it just fetched, under the repo mutex, and lets the push reject a stale parent (riki `core/src/write.rs:163-180`). That is the property gollum is missing, not the status code.
- `412 Precondition Failed` is the HTTP code for a failed `If-Match`. Gollum uses it for a form field, not a header. riki's `409` with a body is a fine choice. If riki ever exposes the page API to scripts, an `ETag` response header plus `If-Match` on the write would let `curl` users get the same guarantee with standard tooling.

### Revert and undo

- `revert_page` and `revert_commit` apply a reverse diff: with one sha the range is `sha^..sha`, with two it is `sha1..sha2` (`gollum-lib/lib/gollum-lib/wiki.rb:425-464`, range parsing `:719-727`). The adapter returns a tree, or nothing if the patch does not apply. The committer is then seeded from that tree with the current ref as parent (`gollum-lib/lib/gollum-lib/wiki.rb:729-745`, `gollum-lib/lib/gollum-lib/committer.rb:42-43`).
- When the reverse patch fails, the app shows the compare view with "The patch does not apply." (`gollum/lib/gollum/app.rb:428-436`). The commit message is `Revert commit <7-char sha>` (`gollum/lib/gollum/app.rb:427`).
- The app only exposes the per-page form. `revert_commit` has no route (its only caller is the library itself, `gollum-lib/lib/gollum-lib/wiki.rb:460-464`).
- `revert_page` returns `false` when the page does not exist at the ref (`gollum-lib/lib/gollum-lib/wiki.rb:441`), so a deleted page can't come back through this route. Gollum has no undo for delete. riki's restore covers exactly that case, by re-upserting the blob D removed (riki `core/src/delete.rs:148-187`).
- What transfers is the tradeoff, for when riki's parked page history UI lands (riki `docs/design/2026-10-01-riki.md`, Non-Goals, "Page history UI"):
  - A reverse patch keeps later, unrelated edits to the same page and fails cleanly when they overlap.
  - A blob restore (riki's restore shape) brings back exact bytes and overwrites anything after.

  "Undo this delete" wants the blob, as riki has it. "Revert this one edit from history" wants the reverse patch. git2's `Repository::revert_commit` already did the merge-index route on a bare repo in riki's Phase 0b spike (riki `docs/design/2026-10-02-riki-wiki-operations.md`, Phase 0b result), so it is the candidate for that second case.

### Search via git grep

- Every query is a fresh `git grep` over the configured ref and `page_file_dir`, with no index (`gollum-lib/lib/gollum-lib/wiki.rb:502-525`, called from `gollum/lib/gollum/app.rb:545-558`). That is the simplest correct shape for a git-backed wiki, and its cost grows with the repo on every query. Gollum's search is a submitted form; riki's palette queries on a 150 ms debounce as the user types (riki `docs/design/2026-10-02-riki-wiki-operations.md`, Page UI flows, Search), which would multiply that cost. riki's in-memory index, rebuilt on publish, puts that cost once per tip instead (riki `docs/design/2026-10-02-riki-wiki-operations.md`, Resolved Decisions).
- Quoted phrases are kept as one term: `query.scan(/"([^"]+)"|(\S+)/)` (`gollum-lib/lib/gollum-lib/wiki.rb:504`). riki splits every query on non-alphanumerics (riki `core/src/search.rs:260-285`), so `"round trip"` is two prefix terms, ANDed. A phrase filter over the section text riki already stores would be a small addition.
- Terms are ORed: the context regex is an alternation of all terms (`gollum-lib/lib/gollum-lib/wiki.rb:505-506`). riki ANDs (riki `core/src/search.rs:184-201`), which is the better default for a palette.
- Matches in the file name count toward the score next to body matches (`gollum-lib/lib/gollum-lib/wiki.rb:510`, combined at `:522`). riki indexes title, heading and body, not the path (riki `core/src/search.rs:24-28`, `:49-55`). A page whose title doesn't contain its file stem (`ops/runbook-db.md` titled "Database failover") can't be found by its stem. Indexing path segments as a fourth field is a direct transfer.
- Non-UTF-8 content is skipped per file instead of failing the search (`gollum-lib/lib/gollum-lib/wiki.rb:518-520`). riki already does the same at build time with a WARN (riki `docs/design/2026-10-02-riki-wiki-operations.md`, Search index, "Content problems never fail the build").

### Page, path and filename mapping

- **A URL without an extension matches any format.** A query without a registered extension is compared against the file name with its extension stripped (`gollum-lib/lib/gollum-lib/page.rb:36-40`), and the first blob in tree order that matches wins (`gollum-lib/lib/gollum-lib/file.rb:65-67`). `Foo.md` and `Foo.org` side by side resolve to whichever comes first, silently. riki makes the same ambiguity (`a/b.md` vs `a/b/README.md`) an index error that blocks publish (riki `core/src/index.rs:108-115`). Gollum shows why that strictness is worth keeping.
- **Opt-in loose matching.** `hyphened_tag_lookup` treats spaces as dashes and `case_insensitive_tag_lookup` compares case-insensitively (`gollum-lib/lib/gollum-lib/file.rb:28-37`, options at `gollum-lib/lib/gollum-lib/wiki.rb:143-144`). riki's slug rule lowercases and dashes new paths (riki `docs/design/2026-10-02-riki-wiki-operations.md`, Slug rule), but lookup stays exact. If hand-typed URLs become a complaint, a case-insensitive fallback before the redirect lookup is the gollum pattern. It needs the same ambiguity rule as above.
- **Traversal is closed by normalizing, then checking the prefix.** `canonical_path` joins under `/`, runs `cleanpath`, and strips the root (`gollum-lib/lib/gollum-lib/file.rb:17-23`). `merge_path_elements` raises `IllegalDirectoryPath` when the result leaves `page_file_dir` (`gollum-lib/lib/gollum-lib/wiki.rb:756-765`). riki refuses `..` segments outright instead (riki `core/src/path.rs:26-49`), which is stricter.
- **The wiki can live in a subdirectory of a code repo.** `page_file_dir` scopes every lookup, tree listing and search to one folder (`gollum-lib/lib/gollum-lib/wiki.rb:134-135`, `gollum-lib/lib/gollum-lib/git_access.rb:164`, `gollum-lib/lib/gollum-lib/wiki.rb:503`). riki serves a whole content repo. For a Tatari pilot, serving `docs/` out of a service repo is the same feature. riki's URL mapping would need one prefix strip on read and one prefix add on write.
- **Wiki-control files use a leading underscore.** `_Sidebar`, `_Header`, `_Footer` are found by walking up from the page's folder to the root (`gollum-lib/lib/gollum-lib/page.rb:319-355`). They are excluded from page listings (`gollum-lib/lib/gollum-lib/page.rb:59-62`), and every `_`-prefixed non-page file is left out of the file list (`gollum-lib/lib/gollum-lib/wiki.rb:651`). A per-folder `_Template` seeds new pages (`gollum/lib/gollum/app.rb:383`, `:699-702`). riki's `_order` is the same convention (riki `docs/design/2026-10-02-riki-wiki-operations.md`, Phase 5). A `_template.md` per folder would be the matching way to seed "+ New page" beyond `# <title>`. It would have to be kept out of the nav the way `_order` is.
- **Titles.** By default the title is the file name (`gollum-lib/lib/gollum-lib/page.rb:126-128`), overridable by a metadata `title` (`gollum-lib/lib/gollum-lib/page.rb:141-152`) or by the `h1_title` option (`gollum-lib/lib/gollum-lib/wiki.rb:68-70`). riki's front matter, then first H1, rule is the same ladder with a better default (riki `docs/design/2026-10-02-riki-wiki-operations.md`, Title).

### Link status at render time

This is relevant to riki's parked link rewriting on move (riki `docs/design/2026-10-02-riki-wiki-operations.md`, Non-Goals).

- Gollum resolves every internal `[[link]]` against the tree while rendering. A relative link is tried against the page's folder first (`gollum-lib/lib/gollum-lib/filter/tags.rb:250-260`). The href becomes the found page's canonical URL (`gollum-lib/lib/gollum-lib/filter/tags.rb:235-236`). A miss is rendered with `class="internal absent"` (`gollum-lib/lib/gollum-lib/filter/tags.rb:227`, `:293-294`).
- An optional global fallback finds a page by file name anywhere in the repo, marked "Slow!" in the source (`gollum-lib/lib/gollum-lib/filter/tags.rb:253-254`). That makes name-only links survive moves.
- riki's `rewrite_link` maps a relative `.md` target to a URL with no index lookup (riki `core/src/render.rs:285-297`), so a link to a moved or missing page renders like any other and costs the reader a 301 or a 404.

  Passing the published nav index and redirect map into the rewriter would allow two things, neither of which edits the source:
  - links to a moved page could point straight at the new URL
  - links to a missing page could get an `absent` class

  Both fit the byte-identical guard, because nothing in the file changes. They also cover the "broken-link reports" revisit condition at read time.

### History, compare and uploads (parked in riki)

These are on riki's parked list (riki `docs/design/2026-10-01-riki.md`, Non-Goals: "Page history UI", "Image upload from the editor").

- **History follows renames by default.** `follow_renames` defaults to `true` (`gollum/lib/gollum/app.rb:637-641`, `gollum-lib/lib/gollum-lib/page.rb:239-241`). riki already computes every exact rename for redirects (riki `core/src/store.rs:390-421`), so a history view can stitch a page's log across moves from the same walk instead of a per-request `--follow`.
- **Historical views have their own URL.** `/<path>/<40-hex sha>` renders the page at that commit (`gollum/lib/gollum/app.rb:583-603`). Historical pages are flagged so their links get `rel="nofollow"` (`gollum-lib/lib/gollum-lib/page.rb:282-289`). That fits riki's oid-keyed reads with no new storage.
- **Compare views.** Compare is a route over two shas for one path (`gollum/lib/gollum/app.rb:479-500`). A single commit has its own diff view (`gollum/lib/gollum/app.rb:527-543`). Latest changes are paginated and also served as RSS (`gollum/lib/gollum/app.rb:164-172`, `:471-477`).
- **Uploads.**
  - Uploads are off unless enabled (405, `gollum/lib/gollum/app.rb:257`).
  - They go to a shared `uploads/` folder or to a folder per page (`gollum/lib/gollum/app.rb:265`, `:739-744`).
  - Paths with `..` or absolute paths are refused (`gollum/lib/gollum/app.rb:267-268`).
  - Only page extensions are normalized, so binary bytes pass through untouched (`gollum/lib/gollum/app.rb:284`).
  - An existing file is a 409 rather than an overwrite (`gollum/lib/gollum/app.rb:292-293`).

  Each maps onto a riki op: an `Upsert` of a new blob with "to absent" as the check.

## What it confirms riki already has right

- **A remote that is the lock.** Gollum has no remote. Its README offers a pull hook and a push hook and admits conflicts may not resolve (`gollum-lib/README.md:230-233`). riki's bare clone plus fast-forward-only push never gets into a state that needs resolving.
- **Every op checks a version.** Gollum checks a sha on edit only (`gollum/lib/gollum/app.rb:346-349`). Rename, delete and revert take none (`gollum/lib/gollum/app.rb:298-305`, `:366-375`, `:417-428`). riki's move and delete both carry `base-oid`, and restore validates D (riki `docs/design/2026-10-02-riki-wiki-operations.md`, op check table).
- **A conflict response the client uses.** Gollum's 412 body is ignored by its own JavaScript (`gollum/lib/gollum/public/gollum/javascript/gollum.js.erb:614-617`). riki's 409 body feeds "Load latest" (riki `editor/src/session.ts:225-237`).
- **A move moves the same bytes.** Gollum's rename re-adds `page.raw_data` through `add_to_index` with normalization on (`gollum-lib/lib/gollum-lib/wiki.rb:312-313`, `gollum-lib/lib/gollum-lib/committer.rb:90-97`). Normalization strips `\r` (`gollum-lib/lib/gollum-lib/wiki.rb:626-628`), so renaming a CRLF page also rewrites it, and git no longer sees an exact rename. riki's move upserts the base blob oid itself (riki `core/src/move_page.rs:117-123`), so it is always `R100`, which its redirect detection depends on.
- **No empty commits.** Gollum's edit route skips staging when content and format are unchanged (`gollum/lib/gollum/app.rb:704-706`), but still calls `committer.commit` unconditionally (`gollum/lib/gollum/app.rb:359`). Whether the adapter then writes a commit with an unchanged tree isn't visible in these clones. riki answers `200` with `commit: null` and writes nothing (riki `docs/design/2026-10-01-riki.md`, Save algorithm step 5).
- **One snapshot per served commit.** Gollum rebuilds its `Wiki` per request (`gollum/lib/gollum/app.rb:720-722`) and keys the redirect cache on HEAD rather than the served ref (`gollum-lib/lib/gollum-lib/redirects.rb:10-12`). riki swaps nav, search and redirects together as one `Published` value for the good tip (riki `docs/design/2026-10-02-riki-wiki-operations.md`, Architecture, `wiki.rs`).
- **Ambiguous files are errors, not tree-order luck.** See the mapping section: gollum's first-match lookup (`gollum-lib/lib/gollum-lib/file.rb:65-67`) against riki's index collision error (riki `core/src/index.rs:108-115`).
