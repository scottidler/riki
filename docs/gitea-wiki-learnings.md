# Gitea wiki learnings for riki

**Source:** `go-gitea/gitea`, branch `main`, commit `e6ffbea888`, cloned at `~/repos/go-gitea/gitea`. All paths below are relative to that repo unless marked riki.
**Date:** 2026-10-02

## Framing

- Gitea's wiki is a second bare repo per project, on the same disk as the server (`<repo>.wiki`, pushed to as a local path: `modules/git/push.go:24-27`, `services/wiki/wiki.go:222-231`). riki's content repo lives on GitHub and riki works on a bare clone of it (riki: `core/src/store.rs:1-6`).
- Every write makes a throwaway clone, builds an index file, writes a tree, commits, and pushes back to the on-disk repo (`services/wiki/wiki.go:107-139`, `:174-238`). riki builds the tree in place with git2 and no index or temp dir (riki: `core/src/store.rs:234-272`).
- The page title is the file name. Title, web path and git path are three spellings of one string (`services/wiki/wiki_path.go:18-32`). In riki the title is the H1 or front matter and the path comes from a slug the title no longer controls after creation (riki: `docs/design/2026-10-02-riki-wiki-operations.md:110-119`).
- The namespace is flat. The routes don't support folders, so a `/` in a title is stored as `%2F` in the file name (`services/wiki/wiki_path.go:33-38`, `:146-147`, `:152-153`). riki has folders, a sidebar tree, and `README.md` folder pages.
- A write has no base revision. The edit form carries only title, content and message (`services/forms/repo_form.go:560-565`), and the commit's parent is whatever `HEAD` was when the temp clone was made (`services/wiki/wiki.go:212-214`). Two editors on one page: the later save wins with no warning. riki's base-oid check exists to stop exactly that (riki: `core/src/save.rs:122-132`).
- What transfers is mostly small: error hygiene, a test technique, and what a history view needs. Most of this list is warnings, not things to copy.

## What transfers

### Push error messages shown to the user

- Gitea splits push failures into two types: `ErrPushOutOfDate` when stderr contains `non-fast-forward`, `ErrPushRejected` on `! [remote rejected]` or `! [rejected]`, and a plain error otherwise (`modules/git/repo.go:259-268`, types at `modules/git/error.go:52-95`).
- For a rejection, `GenerateMessage` keeps only the `remote: ` lines from stderr, removing the `error: ` prefix, and falls back to "push is rejected" if there are none (`modules/git/error.go:96-125`). The reason is in its comment: "The local message contains sensitive information, so we only need the remote message" (`modules/git/error.go:105`).
- Gitea's own file editor shows that cleaned message to the user (`routers/web/repo/editor_error.go:70-75`).
- riki sends the whole push stderr to the browser in the 502 body, for save and for every path op (riki: `server/src/api.rs:389`, `server/src/api/ops.rs:186`; stderr assembled at `core/src/store.rs:562-573`). That text includes the remote URL and local git chatter. Gitea's split transfers directly: log the full stderr, and return only the porcelain `!` line plus the `remote:` lines. A GitHub protected-branch refusal arrives as `remote:` lines, so the user still sees why.

### Page history view

riki parks this ("Page history UI", riki: `docs/design/2026-10-01-riki.md:56`). Gitea's version is a cheap one and shows what the minimum is.

- The view page shows a revision count next to a history button, from `git rev-list --count` on the page's file (`routers/web/repo/wiki.go:309-311`, button at `templates/repo/wiki/view.tmpl:36`), and "last edited by X, time ago" from the last commit touching the path (`routers/web/repo/wiki.go:492-498`, `templates/repo/wiki/view.tmpl:39-41`).
- `?action=_revision` lists that file's commits, paged by `setting.Git.CommitsRangeSize` (`routers/web/repo/wiki.go:350-371`).
- Each row links to a commit diff served under the wiki prefix, `/wiki/commit/{sha}` and `.diff` / `.patch` (`templates/repo/commits_list.tmpl:23-24`, routes at `routers/web/web.go:1617-1618`).
- There is no "view the page as of this revision": the template skips those links for the wiki, with a comment saying so (`templates/repo/commits_list.tmpl:61-62`). A diff link per commit is enough for a first version.
- History stops at a rename. `CommitsByFileAndRange` supports `--follow` through `FollowRename` (`modules/git/repo_commit.go:244-254`, `:262-264`), but the web wiki doesn't set it (`routers/web/repo/wiki.go:354-359`) and neither does the API (`routers/api/v1/repo/wiki.go:425-430`). riki already knows every exact rename on the first-parent chain from its redirect walk (riki: `core/src/store.rs:469-490`, `core/src/redirect.rs:33-38`). A riki history view can join the history segments along the redirect chain, with no `--follow` and its guessing.
- When riki builds this, the per-commit links go to GitHub's commit pages, since riki serves no diffs today.

### Title, web path and git path mapping

riki's slug rule replaces most of this, but the hazards are worth knowing.

- Because the title is the file name, Gitea needs a reversible encoding. Spaces become `-` in the URL, so a title that contains a `-` gets a `.-` suffix, the "DashMarker", meaning "no dash/space conversion in this segment" (`services/wiki/wiki_path.go:26-27`, `:53-63`). A title ending in `.md` gets one too, so it can't look like a raw file path (`services/wiki/wiki_path.go:78-86`).
  - Examples from the tests: `wiki-name` -> `wiki-name.-`, `the wiki-name` -> `the+wiki-name.-`, `a/b` -> `a%2Fb`, `a%b` -> `a%25b`, empty or `..` -> `unnamed` (`services/wiki/wiki_test.go:32-50`).
  - In the git path, only spaces are left unescaped (`services/wiki/wiki_path.go:107-108`), so files on disk are named like `2000-01-02 meeting.-.md` (`services/wiki/wiki_path.go:29-32`).
  - A web path ending in `.md` skips all of this and is used directly as the git path, a way to reach a file the encoding can't name (`services/wiki/wiki_path.go:28`, `:97-100`).
- riki doesn't need any of it. The slug is one-way, lossy, and never read back as a title (riki: `core/src/slug.rs:20-37`). The URL is the file path minus `.md` or `/README.md` (riki: `core/src/index.rs:261-269`), and the title lives in the file. The DashMarker is what a wiki has to do when it lets the title name the file.
- **The test technique transfers.** `TestUserWebGitPathConsistency` generates 1,000 random printable-ASCII titles and checks title -> web path -> git path -> web path -> title returns the same title and the same paths at each step (`services/wiki/wiki_test.go:116-140`). riki's equivalents:
  - `slugify` over random Unicode titles always produces a path `validate_page_path` accepts (riki: `core/src/slug.rs:22-37`, `core/src/page.rs`).
  - `url_for_file` followed by the nav index's `file_for_url` returns the same file for random valid page paths, including the `a.md` / `a/README.md` pair (riki: `core/src/index.rs:145-147`, `:261-269`).
  - The percent-encoded URLs the API hands out decode back to the path they came from (the audit's encoding fix, riki: `docs/design/2026-10-02-riki-wiki-operations.md:424`).
- **Two spellings for one page.** On every write Gitea looks up two spellings of the file name, the name as given and its escaped form, prefers the first if it exists, and otherwise uses the escaped one, which its comment calls keeping backwards compatibility (`services/wiki/wiki.go:54-83`). Reads try the escaped path first, then the unescaped one (`routers/web/repo/wiki.go:80-96`). An edit removes and re-adds at the spelling it found, so a file keeps its spelling (`services/wiki/wiki.go:141`, `:155-183`). riki has the same shape with `a.md` and `a/README.md` (riki: `core/src/move_page.rs:70-80`, `core/src/slug.rs:39-42`). The lesson for any later slug rule change: resolve old spellings at read time, don't mass-rename files, which would also break history and flood the redirect map.
- Request paths are cleaned with `PathJoinRelX` before use, so `a/../b` becomes `b` (`services/wiki/wiki_path.go:144-149`, test at `services/wiki/wiki_test.go:325-327`). riki refuses `..` and leading-dot segments outright instead (riki: `core/src/path.rs:38-46`), which is stricter and needs nothing borrowed.

### Rendering safeguards

- Gitea passes rendered wiki HTML through `charset.EscapeControlReader` (`routers/web/repo/wiki.go:255-271`). When a page holds invisible or bidirectional control characters, the page gets "escape / unescape control characters" buttons (`templates/repo/wiki/view.tmpl:46-49`). That is the Trojan Source defense: text that reads one way and diffs another.
- riki's static rules check UTF-8, a leading BOM and `\r` only (riki: `core/src/page.rs:96-105`). Bidi overrides and zero-width characters pass through. A content-problem warning, like riki's non-UTF-8 skip, is a cheap place for them, and matters more once agents write pages.
- Gitea caps a wiki page read at 5 MB (`routers/web/repo/wiki.go:136`). riki reads whole blobs for render and search; a cap belongs next to the static rules if large files ever show up in a content repo.

### Pages list

- `?action=_pages` lists every page with its last-updated time, read in one pass with `GetCommitsInfo` (`routers/web/repo/wiki.go:558-591`), in natural sort order (`routers/web/repo/wiki.go:563`), so "Page 9" comes before "Page 10".
- riki's sidebar falls back to byte order where `_order` says nothing (riki: `docs/design/2026-10-02-riki-wiki-operations.md:241`). Natural order is a small fallback improvement that changes no URL. A "recently changed" list is a candidate for the same snapshot the search index lives in.

### Wiki search: still none

The 2021 "no wiki search" claim holds at `e6ffbea888`.

- The wiki routes are view, post, commit diff, blob excerpt, and raw. There is no search route (`routers/web/web.go:1610-1625`).
- `modules/indexer/` holds `code`, `issues` and `stats`, and none of it mentions a wiki (case-insensitive grep for `wiki` under `modules/indexer` finds nothing).
- The only "search" on a wiki page is an `<input name="search">` labeled "Filter page" inside the page-picker dropdown. It filters the page titles the server already put in the menu (`templates/repo/wiki/view.tmpl:8-27`; label `repo.wiki.filter_page` = "Filter page", `options/locale/locale_en-US.json:1974`).
- Nothing to borrow. riki's Ctrl+K full-text search over headings and body (riki: `core/src/search.rs`, route at `server/src/api.rs:271-297`) is already past it.

## What it confirms riki already has right

- **Optimistic writes with a base oid.** Gitea's wiki has none (see Framing), so concurrent edits are last-writer-wins. Gitea's own file editor does carry a commit ID and reports `commit_id_not_matching` and `file_changed_while_editing` (`routers/web/repo/editor_error.go:66-69`). The wiki is the part of Gitea that skipped it. riki's 409 on a moved base oid (riki: `core/src/save.rs:122-132`) is the right call.
- **A push the server cannot win by lock alone.** Gitea serializes wiki writes with a global lock per repo, `wiki_working_<id>`, held across clone, commit and push (`services/wiki/wiki.go:29-31`, `:95-99`, `:261-265`). The lock is in-process by default or Redis (`modules/globallock/globallock.go:19-26`), and it is taken only by these two functions. A `git push` to the wiki repo over SSH or HTTP doesn't take it. When that race happens, the error comes back typed (`services/wiki/wiki.go:234-236`), but the web handler maps everything except reserved-name and already-exists to a 500 (`routers/web/repo/wiki.go:641-649`), with no retry. riki holds its mutex for local ref safety only and lets GitHub's ref update decide, retrying from a fresh fetch on a moved branch (riki: `core/src/write.rs:122-125`, `:169-175`).
- **Classifying non-fast-forward by porcelain reason, not by stderr substring.** Gitea checks stderr for `non-fast-forward` first (`modules/git/repo.go:260-261`). A `! [rejected] ... (fetch first)` contains no such text and lands in `ErrPushRejected`, the "a hook refused it" bucket (`modules/git/repo.go:262-263`). riki reads `--porcelain` `!` lines and lists `(fetch first)` and the server-side compare-and-swap losses as "branch moved" (riki: `core/src/store.rs:575-590`, `:595-606`).
- **Rename never overwrites.** In Gitea, rename is the edit call with `oldWikiName != newWikiName`: old name from the URL, new name from the title field (`routers/web/repo/wiki.go:712-713`, `services/wiki/wiki.go:250-252`). The already-exists check runs only when `isNew` (`services/wiki/wiki.go:146-151`). On an edit it removes the old path and adds the content at the new one (`services/wiki/wiki.go:152-183`), so renaming page A to the title of an existing page B replaces B with no error. The rename test only renames to fresh names (`services/wiki/wiki_test.go:206-213`). riki's move requires `to` absent and its twin free, else 409 (riki: `core/src/move_page.rs:88-102`, `:109-116`).
- **Moves as pure renames.** A Gitea rename is always also a content save, so the commit can show as a delete and an add rather than a rename, and nothing redirects the old URL either way. riki keeps a move as its own `R100` commit (riki: `core/src/move_page.rs:117-128`) and builds redirects from exact rename detection, accepting that a move that also edits gets no redirect (riki: `docs/design/2026-10-02-riki-wiki-operations.md:338`).
- **Delete is idempotent and guarded.** Gitea's delete returns `util.ErrNotExist` when the page is already gone (`services/wiki/wiki.go:298-309`), and the handler turns any error into a 500 (`routers/web/repo/wiki.go:732-735`), so a double-click is a server error. The handler also defaults an empty name to `Home` (`routers/web/repo/wiki.go:730-731`), and nothing stops deleting the home page. There is no undo. riki answers an already-absent page with `200 content-present` (riki: `core/src/delete.rs:83-86`), refuses the root `README.md` (riki: `core/src/delete.rs:44-48`), and offers Undo as a revert commit.
- **No temp clone per write.** Gitea pays for a clone, a read of the whole tree into an index, and a `write-tree` on every save and delete (`services/wiki/wiki.go:107-139`, `:185-189`, `:271-296`). riki's `commit_tree` applies a list of upserts and removes to the parent tree directly (riki: `core/src/store.rs:234-272`).
- **Author and committer kept apart.** Gitea writes the user as author and either the user or the signing identity as committer, depending on the trust model (`services/wiki/wiki.go:195-216`). riki already separates the browser identity (author) from the configured committer (riki: `core/src/write.rs:16-22`, `:135-140`).
