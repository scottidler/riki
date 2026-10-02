# Wiki.js learnings for riki

**Source:** `requarks/wiki`, branch `scarlett` (3.x beta), commit `d71fff4e`, cloned at `~/repos/requarks/wiki`. All paths below are relative to that repo.
**Date:** 2026-10-02

## Framing

- Wiki.js stores content in Postgres, not git. Git is one of seven storage targets it syncs to on a schedule (`backend/modules/storage/`: azure, db, disk, gcs, git, s3, sftp). Its core architecture is the reverse of riki's.
- `scarlett` is the 3.x beta: its README says "NOT FOR PRODUCTION USE". The stable 2.x release lives in a separate repo, `requarks/wiki-v2`.
- What transfers is a set of subsystems, not the architecture.

## What transfers

### Link repair after a move

riki parks this ("Rewriting links when a page moves", `docs/design/2026-10-02-riki-wiki-operations.md` Non-Goals).

- A move leaves inbound links as they are, on purpose, so they can still be found as the links it broke. Fixing them is a separate, optional step offered in the rename dialog (`backend/models/pages.ts:2414-2421`).
- Each linking page is fixed through the normal `updatePage` save path, so every fix is an ordinary versioned edit that names the move as the reason (`backend/models/pages.ts:2422-2426`).
- The source is rewritten by regex on the syntax around the link, touching only the characters of the link target. No paragraph is reformatted (`backend/helpers/linkRewrite.ts:1-9`). That fits riki's byte-identical round-trip guard.
  - Patterns cover inline links and images, reference definitions, raw `href=` attributes, and full URLs in angle brackets or bare (`backend/helpers/linkRewrite.ts:43-60`).
  - Fenced blocks and inline code spans are skipped, because a link there is an example, not a link (`backend/helpers/linkRewrite.ts:16-17`, fence tracking at `:78-119`).
  - The entry point is `rewriteSourceLinks` (`backend/helpers/linkRewrite.ts:166`).
- If a link is in the render but can't be found in the source, it is left alone and reported, not forced (`backend/helpers/linkRewrite.ts:11-14`, `backend/models/pages.ts:2428-2430`).
- A page the mover isn't allowed to edit is skipped and reported (`backend/models/pages.ts:2433-2442`, check at `:2513`).

### Read-only "page problems" scan

This answers the gap where riki's good-tip check catches index-build failures but not link rot.

- The check list (`backend/models/pageProblems.ts:24-48`):
  - redirect target missing, redirect to itself, redirect loop, chain too long, chain present
  - tree entry missing, mismatched, orphaned, stale
  - search index missing, among others
- The scan never changes anything. Each check only reports, and fixing is a separate decision per check (`backend/models/pageProblems.ts:134-147`).
- The scan's hop limit `MAX_REDIRECT_HOPS = 5` must match the frontend's `MAX_HOPS`. Otherwise the scan reports a chain as fine that readers' browsers stop on (`backend/models/pageProblems.ts:90-95`). riki's redirect walk (Phase 4: "no live hop within map-size hops -> 404") faces the same agreement problem.

### Link graph kept as a derived table

- Before a link is stored, every way of writing the same page is resolved to one target: `../two`, `/en/one/two`, `/one/two.md` and a full URL can all be the same page (`backend/helpers/pageLinks.ts:5-12`).
- Its own comment admits this is the third copy of the URL-reading rules, after `helpers/common.ts` and `frontend/src/helpers/pagePaths.js`, and that all three must agree (`backend/helpers/pageLinks.ts:14-17`). That's a trap for riki to avoid: riki's link rewriting at render time (`core`) and the editor's link box should share one resolver.
- An href over `MAX_HREF_LENGTH = 2048` is simply not recorded, so derived data never fails the save (`backend/helpers/pageLinks.ts:97-104`, check at `:141`).

### Search hardening

These are the safeguards riki will need once permissions land.

- Title, description and body are weighted A/B/C in the tsvector (`backend/models/search.ts:495-497`). This matches riki's planned title > heading > body ranking.
- Permission filtering runs on the result rows after the query, because its access rules (regex, tags) can't be written as a `WHERE` clause (`backend/models/search.ts:124-129`).
- A password-protected page matches on its title and description only and comes back with no excerpt (`backend/models/search.ts:131-134`).
- Snippet highlight markers are the control characters `\u0002` / `\u0003`. The excerpt is HTML-escaped before they are turned into tags, so a page whose text contains `<script>` can't come back as markup (`backend/models/search.ts:139-147`, escape at `:462`).

### Author privacy in git history

- An `alwaysUseDefaultAuthor` setting keeps users' names and emails out of the history, for a public mirror or for users who didn't agree to have their address published with every edit (`backend/modules/storage/git/storage.ts:467-477`, applied at `:482`).
- The actor isn't looked up at all in that mode, so nothing can leak by mistake.
- This matters for riki's work deploy or any public mirror of the content repo, since riki writes the browser identity as the commit author.

## What it confirms riki already has right

- **Sync conflicts:** Wiki.js has to:
  - detect a working copy left mid-rebase (`backend/modules/storage/git/storage.ts:293-299`)
  - roll it back (`:317-326`)
  - retry the pull with the remote winning every file both sides changed (`:344-360`)
  - repair a second, unrelated history after a lost volume (`:399-431`)

  riki's bare clone with a push that can only fast-forward never gets into any of those states.
- **Serialized repo access:** Wiki.js queues operations per target because git's `index.lock` makes concurrent saves fail (`backend/modules/storage/git/storage.ts:215-220`). riki already does this with one `tokio::sync::Mutex` per repo.
- **No empty commits:** Wiki.js skips the commit when the stored form didn't change (`backend/modules/storage/git/storage.ts:489-494`). riki's save already answers `200 content-present` with no commit.
