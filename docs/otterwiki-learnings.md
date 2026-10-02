# Otterwiki learnings for riki

**Source:** `redimp/otterwiki`, branch `main`, commit `cbb380f`, cloned at `~/repos/redimp/otterwiki`. All paths below are relative to that repo unless marked riki.
**Date:** 2026-10-02

## Framing

- Otterwiki is the closest architecture riki looked at: git-backed, rendered at request time. riki's shipped design turned it down because its editor edits Markdown source, and it's Python (riki `docs/design/2026-10-01-riki.md:746-750`).
- It keeps a working tree, not a bare clone. Every write goes through files on disk and the git index (`otterwiki/gitstorage.py:348-371`, `:402-423`), and reads of the current page come from the filesystem (`otterwiki/gitstorage.py:145-154`). riki reads blobs at the good tip from the object DB (riki `docs/design/2026-10-01-riki.md:718`).
- Its own repo is canonical. A remote is optional: it pushes after every commit if configured (`otterwiki/repomgmt.py:362-388`) and pulls on a webhook (`otterwiki/repomgmt.py:398-420`). For riki, GitHub is canonical and riki is one more git client (riki `docs/design/2026-10-01-riki.md:87-94`).
- Links are absolute wiki paths (`[x](/Page/Child)`) or WikiLinks (`[[Page]]`), not relative `.md` files. riki resolves relative links against the source file (riki `core/src/render.rs:265-297`). That difference shapes everything in the link repair section below.
- Accounts, preferences, drafts and a cache live in SQL tables beside the repo (`otterwiki/models.py:29-83`).
- What transfers is a set of subsystems, not the architecture.

## What transfers

### Link repair in the move commit

riki parks this ("Rewriting links when a page moves", riki `docs/design/2026-10-02-riki-wiki-operations.md:54`) and rejected rewriting link destinations in the move commit (riki `docs/design/2026-10-02-riki-wiki-operations.md:340`).

- Otterwiki rewrites inbound links in the same commit as the rename, so history never shows a moved page with broken links pointing at it (`otterwiki/wiki.py:1080-1092`). It's opt-in per rename: the "Update backlinks" checkbox is unchecked by default (`otterwiki/templates/rename.html:71-76`, read at `otterwiki/views.py:545`).
- Order matters. The rename runs first with the commit deferred, and links in other pages are rewritten only after it succeeds, so a failed rename (target exists) leaves no stray rewrites (`otterwiki/wiki.py:1039-1048`).
- On any failure it rolls back exactly the paths it touched, both sides of each rename plus every rewritten page (`otterwiki/wiki.py:1049`, `:1087-1088`, rollback at `:1093-1103`), and leaves other uncommitted changes alone (`otterwiki/gitstorage.py:450-477`). riki builds the commit as a tree in memory (riki `core/src/move_page.rs:117-127`), so there is nothing to roll back, and a multi-file move is just more `TreeOp`s in the same `commit_tree` call.
- Matching resolves each link to the file it names and compares that to the moved file, rather than comparing strings (`otterwiki/backlinks.py:79-81` for WikiLinks, `:164-171` for Markdown links). riki's equivalent compares `resolve(linking_file, target)` (riki `core/src/render.rs:267`) to the moved path. That reuses the renderer's own resolver, so the two can't disagree.
- The rewrite changes only the link target. Text, `#anchor` and `?query` are put back as found (`otterwiki/backlinks.py:121`, `:176-182`), and a page whose content didn't change is not written (`otterwiki/backlinks.py:244-245`). That fits riki's byte-identical guard, and the guard isn't in the way: a server-side edit never passes through Milkdown (riki `docs/design/2026-10-02-riki-wiki-operations.md:415`).
- The regexes (`otterwiki/backlinks.py:18-28`), applied in order WikiLink, link-wrapped image, image, link (`otterwiki/backlinks.py:213-223`):
  - `WIKILINK_RE = \[\[([^\]]+)\]\]`
  - `IMAGE_LINK_WRAPPER_RE` for `[![alt](src)](/href)`
  - `IMAGE_LINK_RE = !\[([^\]]*)\]\((/[^)]+)\)`
  - `MARKDOWN_LINK_RE = \[([^\]]+)\]\((/[^)]+)\)`
- Gaps to avoid copying:
  - Only `/`-prefixed Markdown links match (the `(/[^)]+)` group), and every parametrized test case is absolute (`tests/test_backlinks.py:129-186`). riki's links are relative, so riki would also have to rewrite the moved page's own outbound links, which Otterwiki never needs.
  - Code is not skipped. `_rewrite_content` runs the regexes over the whole file (`otterwiki/backlinks.py:213-223`), so a link inside a fenced block or code span gets rewritten too. Otterwiki's own broken-link scan strips fences and code spans first (`otterwiki/tools.py:162-203`, applied at `:231-238`): two scanners in one codebase that disagree on what counts as a link.
  - Reference definitions (`[id]: /Page`) and titled links (`[t](/Page "title")`) are not handled: the URL group stops at `)` and then fails to resolve, so the link is left alone (`otterwiki/backlinks.py:28`, `:169-171`). In riki this matters less, because Milkdown inlines reference links (riki `docs/design/2026-10-01-riki.md:771`) and every page is assumed canonical (riki `docs/design/2026-10-01-riki.md:729`).
  - A page it can't read is skipped with a WARN and the rename goes ahead (`otterwiki/backlinks.py:252-258`). That's silent at the UI. Wiki.js's pattern of reporting skipped pages is the better one (`docs/wiki-js-learnings.md`, riki).
- Interaction with riki's redirects: rewriting inbound links in *other* files keeps the moved blob unchanged, so the move still diffs as `R100` and the redirect map still sees it (riki `core/src/move_page.rs:117-123`, `core/src/store.rs:476`). Rewriting the moved page's *own* relative links changes its blob, and a move that also edits the file in one commit gets no redirect (riki `docs/design/2026-10-02-riki-wiki-operations.md:338`). If link repair is unparked, own-link fixes have to land in a second commit after the pure rename.
- A move that leaves a stale `_order` entry is skipped with a WARN (riki `docs/design/2026-10-02-riki-wiki-operations.md:243`). The same commit that rewrites inbound links could rename that entry too.

### Folder moves fall out of the page-directory convention

riki parks folder moves (riki `docs/design/2026-10-02-riki-wiki-operations.md:51`).

- A page's attachment directory is its filename minus `.md` (`otterwiki/helper.py:204-210`), which is also where its subpages live. Renaming a page `git mv`s that directory in the same commit (`otterwiki/wiki.py:1052-1068`), so a page with subpages moves as a subtree.
- Link repair covers part of that subtree by accident. Any Markdown URL under `/<old>/` is rewritten through the attachment branch (`otterwiki/backlinks.py:144-156`), which catches subpage links too. WikiLinks to subpages are not, because WikiLink matching compares the whole target to the moved page only (`otterwiki/backlinks.py:79-81`). If riki takes on folder moves, the subtree rewrite needs one explicit rule, not two that differ by link style.
- Delete has the same reach. With `recursive` checked it removes the page and its whole directory in one commit (`otterwiki/wiki.py:1187-1204`, flag from `otterwiki/views.py:558`). riki refuses moving a `README.md` because that's a folder move (riki `core/src/move_page.rs:54-58`).

### Revert any commit

riki's restore only undoes a delete commit D and re-upserts the blob it removed (riki `core/src/delete.rs:118-142`).

- Otterwiki reverts any commit from the changelog (`otterwiki/views.py:635-645`, `otterwiki/wiki.py:222-235`): `git revert <rev> --no-commit`, and on any failure `git revert --abort`, then raise (`otterwiki/gitstorage.py:479-489`). On success it commits the index under the user's author (`otterwiki/gitstorage.py:491-492`).
- The useful part is the shape: try to apply the inverse, and if it doesn't apply cleanly, leave nothing behind and say so. In riki that's `git2::Repository::revert_commit` against the tip, which the wiki-ops Phase 0b showed works on a bare repo (riki `docs/design/2026-10-02-riki-wiki-operations.md:211`). An index with conflicts would be a 409 through the write driver's `Check::Conflict` (riki `core/src/write.rs:149-152`). Nothing needs aborting, since nothing touched a working tree.
- riki already writes git's own revert message format (riki `core/src/delete.rs:191-193`). A general revert would use the same.
- The revision is checked against an allowlist (`HEAD` or 4-64 hex chars) before any git call, to stop argument injection, and a bad one is the same 404 as a missing one (`otterwiki/gitstorage.py:36-38`, `:64-78`). riki's restore takes the commit as a parsed `git2::Oid` (riki `core/src/delete.rs:27-31`), which gets the same guarantee from the type.

### History across renames

riki parks a page history UI (riki `docs/design/2026-10-01-riki.md:56`).

- Per-page log runs `git log --follow` so history continues past a rename (`otterwiki/gitstorage.py:302-316`).
- Viewing an old revision first tries the current filename, then asks `get_filename_at_revision` for the name the file had then (`otterwiki/wiki.py:413-435`). That runs `log --follow --name-only -z --format=%H` and takes the filename on the matching commit's entry (`otterwiki/gitstorage.py:639-673`). Any miss or git error quietly falls back to the current name (`otterwiki/gitstorage.py:675-678`).
- Gaps:
  - It only finds a name for commits that touched the file. A revision from elsewhere in history falls back to the current name.
  - Blame doesn't use it. It passes the current filename at the old revision (`otterwiki/wiki.py:889`), so blame of a pre-rename revision fails.
  - The page diff view diffs the whole repo between two revisions, not the page (`otterwiki/gitstorage.py:502-506`, called at `otterwiki/wiki.py:960`).
- For riki: the redirect map already walks first-parent history with exact-rename detection (riki `core/src/store.rs:390-394`, `:476`). A page history view can follow the same rename chain backwards from the map, so history and redirects agree on what counts as the same page, without a second `--follow` walk with different rules (git's `--follow` uses similarity, riki's map is exact-only).

### Push webhook

riki parks a GitHub push webhook (riki `docs/design/2026-10-01-riki.md:65`).

- Otterwiki's webhook carries no payload it trusts. A hit just starts the same pull the admin page runs (`otterwiki/views.py:759-796`, `otterwiki/repomgmt.py:398-420`). The fetch is the source of truth, and the webhook only says when to look.
- That is the right shape for riki: the handler calls the poller's `poll_once` (riki `server/src/poller.rs:13-18`) and returns. A forged hit costs one fetch, and publish still validates whatever arrives. So the endpoint needs rate limiting more than it needs payload signature checks.
- What not to copy:
  - The secret is in the URL: an HMAC of the remote URL keyed by `SECRET_KEY` (`otterwiki/util.py:477-484`). The legacy form is an unkeyed hash of the remote URL plus a fixed string (`otterwiki/util.py:487-488`), so anyone who knows the remote URL can compute it.
  - The comparison is `!=`, not constant-time (`otterwiki/views.py:782`).
  - It is `@csrf.exempt` and accepts `GET` (`otterwiki/views.py:759-760`).

### Roles from proxy headers

riki parks permissions finer than "authenticated at the edge" (riki `docs/design/2026-10-01-riki.md:60`).

- `PROXY_HEADER` mode reads name, email and a roles header from the proxy (`otterwiki/auth.py:681-714`; header names `x-otterwiki-name`, `-email`, `-permissions` at `otterwiki/server.py:37-39`).
- Config maps proxy role names to the four permissions READ / WRITE / UPLOAD / ADMIN (`otterwiki/auth.py:640-661`, defaults at `otterwiki/server.py:40-43`). The wiki never stores users in this mode (`otterwiki/auth.py:731-739`).
- Missing email is no user. Missing name falls back to the email. A missing roles header is an empty role set, not an error (`otterwiki/auth.py:682-704`).
- For riki: the homelab Caddy snippet already copies `Remote-Groups` from Authelia beside `Remote-Email` (riki `docs/design/2026-10-01-riki.md:544`). A "read for everyone, write for group X" split is a config map plus a check in the `Identity` extractor, with no user table.

### Broken-link scan and "most wanted"

riki's revisit trigger for link repair is "broken-link reports" (riki `docs/design/2026-10-02-riki-wiki-operations.md:54`), and nothing yet produces them.

- Housekeeping scans every page for WikiLinks to missing pages after stripping fenced code and inline code spans (`otterwiki/tools.py:206-240`, strip regexes at `:162-203`).
- The fence regex follows the renderer's closing rule: a closing fence of the same or greater length, and an unclosed fence runs to EOF (`otterwiki/tools.py:168-184`). A looser rule either swallows the links between two fences or reports links inside one.
- It reports per page and as "most wanted": missing targets ranked by how many pages link to them (`otterwiki/tools.py:300-327`). For riki those are the URLs that land on "Create this page".
- riki can do this without regexes: walk the comrak AST (search already does, riki `docs/design/2026-10-02-riki-wiki-operations.md:124`), resolve each link with `render::resolve`, and check the nav index. Code nodes are never links in an AST, so the fence problem disappears.

### Unsaved edits

Not on riki's roadmap. riki records that closing the browser mid-operation loses the request (riki `docs/design/2026-10-02-riki-wiki-operations.md:428`).

- Otterwiki autosaves editor content per user and page, with the revision the draft started from and the cursor position (`otterwiki/models.py:37-45`, `otterwiki/wiki.py:1401-1432`).
- Reopening the editor with a draft present shows draft and current side by side and asks to edit or discard (`otterwiki/wiki.py:758-795`). Save discards the draft (`otterwiki/wiki.py:864-865`).
- riki's non-goals rule out a server-side pending store (riki `docs/design/2026-10-02-riki-wiki-operations.md:43`). The same idea fits as a per-browser draft keyed by path and base oid. A draft whose base oid no longer matches is the 409 case riki already handles.

## What it confirms riki already has right

- **Optimistic writes:** Otterwiki's save has no base-revision check. The editor template carries the revision (`otterwiki/wiki.py:828-830`), but the save route never reads it (`otterwiki/views.py:582-597`), and `Page.save` writes and commits whatever arrives (`otterwiki/wiki.py:835-852`). Two editors on one page means the last writer wins, silently. riki's base oid check returns 409 (riki `docs/design/2026-10-01-riki.md:204-216`).
- **Byte fidelity:** every Otterwiki save runs `content.replace("\r\n", "\n").strip() + "\n"` (`otterwiki/views.py:590`), which drops leading whitespace and trailing blank lines on every page. riki writes the original file's trailing-newline state back and refuses what the editor can't round-trip (riki `core/src/page.rs:112-113`, `docs/design/2026-10-01-riki.md:234`).
- **Push and pull conflicts:** Otterwiki commits locally first, then pushes in a background thread (`otterwiki/gitstorage.py:388-398`, `otterwiki/repomgmt.py:380-384`).
  - A rejected push is an email to admins (`otterwiki/repomgmt.py:298-306`, `otterwiki/helper.py:474-512`). Local and remote have already diverged by then, and the user who saved was told it worked.
  - Pull is a plain `git pull` (`otterwiki/repomgmt.py:195`). A conflict is one more failure email.
  - Recovery is manual, from the admin page: force push (`otterwiki/preferences.py:403-429`), which drops the remote's commits, or fetch and `reset --hard FETCH_HEAD` (`otterwiki/repomgmt.py:247-262`), which drops the local ones.

  riki pushes inside the request, before answering, retries non-fast-forward from a fresh fetch, and answers 409 when retries run out (riki `core/src/write.rs:163-181`). No state needs a human to choose which side to lose.
- **One lock over every git mutation:** Otterwiki's `git_push_pull_Lock` covers push and pull only (`otterwiki/repomgmt.py:16`, `:121`, `:178`). Commits from saves don't take it (`otterwiki/gitstorage.py:373-423`), and `restore` notes a concurrent pull as a case it must not disturb (`otterwiki/gitstorage.py:456-457`). riki holds one repo mutex from fetch through push and publish (riki `core/src/write.rs:122-123`).
- **Change detection by tip, not by last commit:** after a pull or an HTTP push, Otterwiki tells plugins only the files in `HEAD`'s own commit (`otterwiki/gitstorage.py:86-92`). A pull that brings five commits reports one. riki rebuilds the nav, search and redirects for each new good tip (riki `docs/design/2026-10-02-riki-wiki-operations.md:92`).
- **Not being a git server:** smart-HTTP makes Otterwiki a second remote people push into, with `receive.denyCurrentBranch updateInstead` on a checked-out branch (`otterwiki/remote.py:17-24`), then it pushes on to the configured remote (`otterwiki/remote.py:89-106`).
  - Its subprocesses have no timeout (`otterwiki/remote.py:63-70`, `:108-116`).
  - It reads the whole request body into memory (`otterwiki/remote.py:116`).

  riki keeps GitHub as the only remote, and every git CLI call has a timeout and `kill_on_drop` (riki `docs/design/2026-10-01-riki.md:111-113`). The one part worth keeping in mind is the permission split: fetch needs READ, push needs UPLOAD (`otterwiki/remote.py:57-61`).
- **No working tree:** rename rollback needs `_known_paths` to work out which paths git knows, both in the index and in `HEAD`, because a staged rename drops the old path from the index (`otterwiki/gitstorage.py:425-448`). A bare clone with in-memory tree builds has no staged state to undo.
- **Sidebar order:** Otterwiki's `SidebarPageIndex` sorts by one global mode: name, case-folded name, or folders first (`otterwiki/sidebar.py:199-248`; modes in `otterwiki/server.py:63-66`). Anything else needs a plugin hook (`otterwiki/sidebar.py:244-248`). The hand-curated part is a JSON menu in config, separate from the tree (`otterwiki/sidebar.py:26-97`).
  - There is no per-folder order, and no order lives in the content repo.
  - It walks the working tree and reads the first 512 bytes of every page for its title on each request (`otterwiki/sidebar.py:161-184`, `:302-337`).

  riki's `_order` is versioned with the content, scoped per folder, and applied once per published tip (riki `core/src/index.rs:166-189`). Otterwiki's "focus" mode (load only the current page's branch, `otterwiki/sidebar.py:149-156`) is what riki's sidebar already does by opening nested groups only along the current path (riki `server/src/render.rs:349-353`).
- **Search:** Otterwiki's search is not a model to copy.
  - Each query lists every file and regex-scans the raw Markdown line by line (`otterwiki/wiki.py:1771-1818`), so syntax characters match and there's no index.
  - Users may pass their own regex. ReDoS is contained only by a 5 s timeout per call (`otterwiki/wiki.py:80`, `:1783-1787`).
  - Ranking is "name matched" then match count (`otterwiki/wiki.py:1930`).
  - History search is a TODO (`otterwiki/wiki.py:1822-1827`).
  - Highlighting HTML-escapes the line and then runs the regex over the escaped text (`otterwiki/wiki.py:1891-1904`). A query containing `&`, `<` or `>` then matches inside entities like `&amp;`, or misses text it matched before escaping.

  riki indexes AST text once per publish, prefix-matches with no user regex, and returns match ranges for the client to wrap through DOM text nodes (riki `core/src/search.rs:315-317`, `docs/design/2026-10-02-riki-wiki-operations.md:128`).
- **Redirects after a move:** Otterwiki has none. A renamed page's old URL is simply missing, and link repair is the only thing that keeps readers on it (`otterwiki/wiki.py:1148-1156` sends only the renamer to the new name). riki's rename-detected redirect map also covers bookmarks, external links, and laptop `git mv` (riki `core/src/redirect.rs:1-4`, `:64-77`).
