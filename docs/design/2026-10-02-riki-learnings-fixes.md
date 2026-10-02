# Design Document: riki learnings fixes

**Author:** Scott Idler
**Date:** 2026-10-02
**Status:** In Review
**Review Passes Completed:** 5/5

## Summary

Eight git-backed wikis and editors were cloned and read on 2026-10-02 (`docs/*-learnings.md`). Read against riki v0.1.2, they surfaced six bugs in riki itself, five hardening gaps, and a set of test cases riki's fixture corpus is missing. This doc fixes the bugs, closes the gaps, and adds the tests. Every feature idea from the same reading is parked below with its source and a revisit condition.

## Problem Statement

### Background

- riki v0.1.2 (`2098d71`) shipped wiki operations: move, delete + undo, new page, Ctrl+K search, block editor (`docs/design/2026-10-02-riki-wiki-operations.md`, Status: Implemented)
- 2026-10-02: Scott had Wiki.js and seven more projects cloned and evaluated for what riki can learn (SilverBullet, Gollum, Otter Wiki, Gitea wiki, Keystatic, TinaCMS, llm-wiki). One learnings doc per project, citations at the cloned commits, committed on this branch (`3aaf57c`)
- A design-research pass then checked every candidate against riki's code and reproduced each bug in a scratch harness (vitest against `editor/test/serialize.ts`; a scratch crate against `riki-core`; a tokio program mirroring `store.rs:run_output`)
- Scott, same day: "/create-design-doc to apply these fixes, add tests, etc and to commit all of these findings from these other wikis"

### Problem

Bugs, each reproduced on this branch:

- **B1 conflict drops the author's edit.** The server sends `current-body` with the page-changed 409 (body struct `server/src/api.rs:318-323`, builder `:375-381`), the editor discards it (`editor/src/api.ts:116`). A retried Save re-sends the stale base oid and 409s forever (`core/src/save.rs:122`). The only ways out: copy the text by hand, or "Load latest", which discards the edit (`editor/src/session.ts:225-237`). Every 409 also maps to the same kind, so index-conflict and retries-exhausted 409s (`api.rs:397-406`) offer "Load latest" where it means nothing
- **B2 git's stderr reaches the browser.** A failed push returns the whole stderr as the 502 body (`api.rs:389`, `server/src/api/ops.rs:186`). A failed fetch puts `StoreError` Display, which carries stderr (`core/src/store.rs:43`), in the 503 body (`api.rs:408-413`). `internal()` echoes git2 errors into 500 bodies (`api.rs:99-102`). `/status` serves the same fetch stderr (`server/src/routes.rs:148` -> `core/src/wiki.rs:136-141`). Spike below: push stderr is mostly git's own noise (`error: failed to push some refs to '<url>'`, `hint:` lines) around the one line a person needs (`remote: error: GH006 ...`)
- **B3 Ctrl+K inside a link splits it.** Cursor inside `[abcd](/x)`, pick a page: `[ab](/x)[Why](y.md)[cd](/x)` (`applyLink`, `editor/src/linkbox.ts:71-75`: a collapsed range takes the insert branch at `:75`)
- **B4 a mark on a lone space corrupts the page.** Bold on the space in "Some word more" saves `Some**** word more`; reload reads it as text and serializes `Some\*\*\*\* word more`; the guard refuses the page from then on. Same for italic (`Some** word`) and strike (`Some~~~~ word`). Reachable from the selection toolbar (`editor/src/selectiontoolbar.ts:140-145`), the fixed toolbar, and Mod-b
- **B5 a hard break in an H3+ heading is lost.** Shift-Enter inside `### one two` saves the break as a space
- **B6 a hard break in an H1/H2 turns it setext.** Same keystroke saves `one\` / `two` / `===`; byte-stable, but riki's canonical heading form is ATX

Hardening gaps:

- **H1 git timeout orphans grandchildren.** `run_output` uses `kill_on_drop` only (`core/src/store.rs:544-551`); the timeout kills `git`, not the `ssh` it spawned. Reproduced: `sleep` survives with ppid 1. `~/repos/.claude/rules/rust.md` (Subprocess hygiene) already says kill the process group
- **H2 git output is matched in the host locale.** riki parses the porcelain `!` line (`store.rs:583-612`) and B2 adds `remote:` parsing; `run_output` sets no locale
- **H3 case-only twins are accepted.** Move (`core/src/move_page.rs:95-96, 111`), new page (`core/src/slug.rs:63`) and the index (`core/src/index.rs:108`) compare exact names. Measured: `new_page_path` with `Setup.md` present returns `setup.md`; folder `docs` beside `Docs/` is accepted. A macOS or Windows checkout can hold only one of each pair
- **H4 hidden and bidirectional control characters render invisibly.** comrak passes U+202E, U+2066-2069, U+200B, U+2060 and U+FEFF raw into text, code, link text and headings; the static rules (`core/src/page.rs:96-105`) check only UTF-8, a leading BOM and `\r`
- **H5 no page size cap.** Every page view reads, renders and highlights the whole blob (`server/src/pages.rs:114`); so do the editor load, the guard and search (`api.rs:137, 460`, `core/src/wiki.rs:160, 227`)

Test gaps:

- **T1** 15 inputs from TinaCMS's and Keystatic's suites round-trip byte-identical through riki's editor today and have no canonical fixture; 9 more are rewritten and have no rewritten fixture (lists in Phase 1)
- **T2** behavior that passes today with nothing pinning it: whitespace hoisted out of marks, Backspace right after an input rule, slug round trip over arbitrary titles, comrak's `javascript:` scrubbing behind entities and invisible characters

### Goals

Each traces to Scott's ask above; the item ids are the Problem list.

1. Fix B1-B6, each with a regression test that fails on this branch
2. Close H1-H5
3. Add T1 fixtures and T2 pins
4. Commit the eight learnings docs (done, `3aaf57c`)

### Non-Goals

Excluded:

- Any change to the round-trip guard's definition (byte identity stays)
- Any new database, store or service

Parked, with revisit condition (source = the learnings doc that raised it):

| parked | meanwhile | revisit when | source |
|---|---|---|---|
| Three-way merge on save (word-level diff3) | B1 keeps the author's text | a 409 on a shared page is a complaint | silverbullet:14-48, gollum:52 |
| Diff view of mine vs theirs on a 409 | B1's read-only panel | the panel is not enough | silverbullet:14-37 |
| "Overwrite with mine" on a 409 (needs `current-oid` in the 409) | copy, Load latest, paste | Scott asks | gollum:59 |
| Committed conflict-marker detection | none (riki never writes markers) | a laptop merge commits markers | silverbullet:50-57 |
| Link rewriting on move | redirects + link box | broken-link reports | silverbullet:59-77, otterwiki:17-37, wiki-js:14-25 |
| Page history that follows moves | GitHub history | history UI is designed | silverbullet:84-90, gitea:24-33, otterwiki:56-66, gollum:104-110 |
| Folder moves | page at a time | a folder move is asked for | otterwiki:39-45 |
| Revert any commit | git on a laptop | history UI is designed | otterwiki:47-54, gollum:61-71 |
| Push webhook | poll | poll staleness is a complaint | otterwiki:68-77 |
| Roles from proxy headers, permission-aware search | edge auth only | a multi-team deploy | otterwiki:79-86, wiki-js:44-51 |
| Broken-link scan, "most wanted", page-problems scan, render-time `absent` links | none | broken-link reports | otterwiki:88-95, wiki-js:27-36, gollum:90-102 |
| Browser-local drafts keyed by (path, base-oid) | none | an edit is lost to a closed tab | keystatic:105-109, otterwiki:97-103 |
| Image upload in the page commit | images via git | already parked in the v0.1 doc | keystatic:111-117 |
| Slash menu: dry-run filter, Space runs a single match, entries declared on node specs | label filter | the slash menu is reworked | keystatic:20-30 |
| `[text](href)` input rule, Enter after ```` ```lang ````, paste URL over selection, `transformPasted` for pasted-table alignment, VS Code HTML paste, copy as Markdown | toolbar + link box | editor UX round | keystatic:36-65 |
| Strike input rule: single tilde strikes, `~~s~~` + Backspace double-fires | pinned as-is (Phase 12) | a strike typing complaint | design-research, `@milkdown/preset-gfm` `lib/index.js:65` |
| Code-fence `meta` attr | page opens read-only (already parked) | titled fences in pages | tinacms:60, keystatic:71 |
| Fuzzy search fallback, stopwords, quoted phrases, path segments as a field, incremental reindex | current search | search quality or rebuild time is a complaint | tinacms:75-80, gollum:76-78, llm-wiki:42-59 |
| Backlink graph, single shared link resolver | none | link repair is designed | llm-wiki:61-72, wiki-js:38-42 |
| Natural sort fallback, "recently changed" list | byte order | Scott asks | gitea:59-60 |
| Case-insensitive URL fallback | 404 | a case-mistyped URL is a complaint | gollum:84 |
| Wiki in a subfolder of a code repo, per-folder `_template.md` | whole repo is the wiki | a Tatari pilot on a code repo | gollum:86-87 |
| Plain-words message for branch-protection rejections | B2's `remote:` lines | the GH006 text confuses someone | keystatic:102 |
| Default-author privacy (`alwaysUseDefaultAuthor`) | author is the editor | a public mirror or the work deploy | wiki-js:53-57 |
| Unicode normalization (NFC vs NFD) as a collision | H3 covers case only | an APFS checkout trips on it | design-research |

## Proposed Solution

### Overview

Twelve small phases, one commit each, deterministic first: tests that pin today's behavior, then server and git hardening, then editor fixes, then the two judgment-heavy changes (conflict UX, case collisions). No new crate enters `Cargo.lock`; `libc` becomes a direct dependency of `riki-core` at the version already locked transitively (0.2.189).

N=1 vs N=2+: nothing here is shared between replicas. Every fix is per process (subprocess hygiene, response bodies), per request (collision checks), or per browser (editor). Inert across the boundary.

### Spike results (run during design)

Push into a local upstream whose `pre-receive` hook exits 1 (git 2.53.0, `~/tmp/spike-push`):

```
remote: error: GH006: Protected branch update failed for refs/heads/main.<8 trailing spaces>
error: failed to push some refs to '../up.git'
To ../up.git
!	HEAD:refs/heads/main	[remote rejected] (pre-receive hook declined)
Done
```

- The `error:` line names the remote URL; for riki's deploy that is `git@github.com:scottidler/riki-content.git`, plus `Permission denied (publickey)`-style ssh text on auth failures (`docs/design/2026-10-01-riki.md:517`)
- `remote:` lines carry trailing sideband padding (8 spaces after the period, shown as `<8 trailing spaces>` above because the repo's whitespace lint strips them): the sanitizer trims them
- No git translations are installed on desk (`/usr/share/locale/*/LC_MESSAGES/git.mo`: 0 files), so locale cannot be measured here; `LC_ALL=C` makes the question moot on any host

### Architecture

**core**

- `store.rs` `run_output`: `.env("LC_ALL", "C")`; `.process_group(0)`; the child is held in a `ProcessGroup` guard whose `Drop` calls `libc::killpg(pgid, SIGKILL)` only while the group leader (git) is unreaped. To make "unreaped" span the whole read, `run_output` drains stdout and stderr to EOF while holding the `Child`, and only then calls `wait`; tokio's `wait_with_output` reaps the leader while the pipe reads are still pending (`tokio-1.53.1/src/process/mod.rs:1463`), which would free the pid and pgid before the guard's last chance to kill. With the leader unreaped its pid cannot be reused, so `killpg` cannot hit a stranger. In the `rules/rust.md:72` shape (git exits, a grandchild holds stdout) EOF never comes, the timeout or a drop fires, and the guard kills the group, after a positive-pid check (otto precedent, `~/repos/otto-rs/otto/src/executor/scheduler.rs:658-690`)
  - Drop, not only the timeout branch: nothing in the write path spawns a task (`core/src/write.rs`, `server/src/api.rs`), so a browser that disconnects mid-push drops the handler future and today orphans `ssh` the same way a timeout does
  - `process_group(0)` also takes git out of riki's terminal group, so a terminal Ctrl-C reaches git only through riki's exit, which drops the guard; under systemd the unit's cgroup kill covers it either way
- `store.rs`: `pub fn browser_message(stderr: &str) -> String` next to `classify_push`. Reverses the shipped decision "any other rejection (auth, protected branch): 502 with git's stderr" (`docs/design/2026-10-01-riki.md:217`). Primary reason is readability: the author sees GitHub's sentence, not git's hints. Secondary: the remote URL, ssh errors and local paths stay in the server log; readers are edge-authenticated, so this is tidiness, not a security boundary: keep the porcelain `!` line and `remote: ` lines (trimmed), drop everything else; empty result -> `"push rejected; see the server log"`. Full stderr still logged (`write.rs:187`)
- `move_page.rs`, `slug.rs`, `delete.rs` (restore, `core/src/delete.rs:157`), `save.rs` (create path, `base_oid == None`, which includes the missing-URL "Create this page" flow): a case-folded (`str::to_lowercase`) comparison of every path segment against the tree at the tip; a case-only twin is a 409 (move, create) or the next `-2` (slug). A move's own source path is not a twin of its destination, so `setup.md` -> `Setup.md` is allowed
- `index.rs`: a case-only twin already in the tree is a WARN at publish plus a banner on both pages ("`Setup.md` and `setup.md` differ only in case; macOS and Windows checkouts can hold only one"), beside the index-error banner (`server/src/render.rs:263-275`). Not an index error: it is not ambiguous for riki's routing, and an index error stops the whole wiki publishing over one laptop push
- `page.rs`: `hidden_controls(text) -> Option<HiddenControls { count, first_line }>` over bidi controls (U+202A-202E, U+2066-2069, U+200E, U+200F, U+061C), U+200B, U+2060 and non-leading U+FEFF. Excludes ZWJ U+200D and ZWNJ U+200C (emoji sequences, Persian and Indic text). A content warning, not a `StaticRule`: a static rule blocks the browser edit that would remove the characters
- `page.rs`: `StaticRule::TooLarge { bytes, limit }` checked first in `check_static_rules`, which gains the limit as a parameter; render and search skip a too-large page with a WARN; the nav title falls back to the file name instead of reading the blob (`core/src/wiki.rs:160`)
- save and create refuse with a 400, after the fetch (`core/src/write.rs:127`) and before any commit or push, when the composed file (front matter + body, `page::compose`, `core/src/save.rs:111-114`) is over the limit
- `store.rs`: every blob read checks the size in the object header (`Odb::read_header`, git2 0.21.0) before loading: `read_blobs` (`:425-445`, nav and search), `blob_at` (`:322`, page render `server/src/pages.rs:114`, editor load `api.rs:137`, save step 4 `core/src/save.rs:109`) and `blob` (`:449`, the round-trip guard `api.rs:460`). An oversized blob comes back as a size-only result with no bytes; callers map it to `StaticRule::TooLarge`. No path loads an oversized blob

**server**

- `api.rs`, `api/ops.rs`: `PushFailed(stderr)` -> 502 with `browser_message(stderr)`; `fetch_failed` -> `"upstream unreachable; see the server log"`; `internal()` -> `"{context}; see the server log"`. The `error!` lines keep the detail
- `wiki.rs` `status_error`: the unreachable part becomes `upstream unreachable since <time>; see the server log` (`core/src/wiki.rs:136-141`, served by `routes.rs:148`)
- `api.rs`, `api/ops.rs`: every 409 body carries `conflict: page-changed | index | retries-exhausted | path` (kebab-case, the existing `error` and `current-body` fields unchanged). `path` means the target path is taken or collides. Five builders:
  - `api.rs:375-381` page-changed; `:397-399` index; `:401-406` retries-exhausted (today's four CONFLICT sites are `api.rs:380, :398, :403`, `api/ops.rs:179`)
  - `api/ops.rs:179` path (`OpAnswer::Conflict`: move, delete, restore, and Phase 9's move and restore twins)
  - new: `SaveOutcome::PathTaken(message)` -> path, for Phase 9's create-path twin (create runs through `save.rs`, whose only 409 today is `Conflict { current_body }`)
- `pages.rs`: banner "N hidden or bidirectional control characters, first at line L" beside the existing banners (`server/src/pages.rs:101`); "This page is too large to render (N bytes, limit L)" stub with the GitHub link
- `config.rs`: `content.max-page-bytes`, default 2 MiB (2,097,152); `riki.example.yml` documents it
  - Why not Gitea's 5 MB: riki sets no `DefaultBodyLimit`, so axum 0.8's 2 MiB default already caps every browser save; a 5 MiB render cap would let laptop pushes render pages the editor can never save
  - the router's `DefaultBodyLimit` is derived from it (`6 * max-page-bytes + 64 KiB`), so the two never diverge. 6x is JSON's worst case: a C0 control escapes to `\u00XX`, 6 bytes; non-ASCII is not escaped (panel round 1 measured 2 MiB of U+0001 -> 12,582,923 bytes of JSON; 1 MiB of `é` -> 1,048,587)

**editor**

- `setup.ts` `stringifyOptions`: a `root` handler that prunes `strong`, `emphasis` and `delete` nodes whose children are all empty text, then calls `defaultHandlers.root` (`mdast-util-to-markdown`, already a direct pinned dep). Milkdown's serializer does not run remark transformers (`@milkdown/transformer@7.22.2 lib/index.js:375`), so the prune lives in the stringify handler
- `setup.ts` `makeEditor`: `ctx.update(hardbreakFilterNodes.key, prev => [...prev, 'heading'])` so Shift-Enter in a heading is a no-op
- `setup.ts`: an `appendTransaction` plugin that replaces each hard break inside a heading with a space, scoped to headings that overlap the transaction's changed ranges. One chokepoint for every way a break lands in a heading: paste, turning a paragraph that holds a break into a heading (toolbar, slash menu, Turn into, `## ` input rule). Scoped to changed ranges so an edit elsewhere never rewrites an existing setext-with-break heading (those files stay canonical)
- `linkbox.ts` `openLinkBox` (which captures the range `applyLink` later uses, `:97-98`): a collapsed cursor with link marks on both sides (`$from.nodeBefore` and `$from.nodeAfter` both carry the same link mark) widens the range to the mark's extent, then takes the existing `addMark` branch. At a link's edge the cursor is outside: inserting a new link there is the expected result
- `api.ts`: `SaveResult` gains `{ kind: 'conflict', message, currentBody }` (page changed), `{ kind: 'index-conflict', message }`, `{ kind: 'retries-exhausted', message }`, chosen by the 409's `conflict` field, never by message text
- `session.ts`:
  - `conflict`: the status offers "Copy my text" (Clipboard API; the panel's text stays selectable where the API is unavailable) and "Load latest". "Load latest" keeps the author's serialized Markdown, loads the current page, and shows the author's text in a read-only panel with a copy button until dismissed
  - `index-conflict`: the message, Save stays available (the author fixes the page and saves again)
  - `retries-exhausted`: the message plus "Save again" (upstream moved; the next fetch usually lands)

### Data Model

- Config: `content.max-page-bytes: u64` (kebab-case, `deny_unknown_fields` as today); default 2,097,152
- `StaticRule::TooLarge { bytes: u64, limit: u64 }`
- `HiddenControls { count: usize, first_line: usize }` (1-based line)
- Editor `SaveResult` kinds above; `currentBody: string | null`

### API Design

No route changes. Response bodies change (the path ops in `api/ops.rs` share these builders):

| response | before | after |
|---|---|---|
| 502 push failed | full git stderr | `!` line + trimmed `remote:` lines, or `push rejected; see the server log` |
| 503 fetch failed | `upstream unreachable: <stderr>` | `upstream unreachable; see the server log` |
| 500 internal | `<context>: <git2 error>` | `<context>; see the server log` |
| 409 page changed | `{error, current-body}` | `{error, current-body, conflict: page-changed}` |
| 409 index | `{error}` | `{error, conflict: index}` |
| 409 retries exhausted | `{error}` | `{error, conflict: retries-exhausted}` |
| 409 path op (move, delete, restore) | `{error}` | `{error, conflict: path}` |
| 409 create onto a case twin | (accepted today) | `{error, conflict: path}` |
| `/status` while upstream is down | `... since <t>: <stderr>` | `... since <t>; see the server log` |

### Implementation Plan

Every phase: one commit, `otto ci` green (lint, bloat, check, test, core-guard, editor). Editor-source phases commit the rebuilt `server/assets/editor.js` (the `editor` task fails on a stale bundle). Every new test is shown to fail with the fix reverted before the commit.

#### Phase 1: Fixtures
**Model:** sonnet
- Add the files below. Each value is the file's exact bytes as a JSON string; for rewritten fixtures the second value is what riki writes. Every input was run through `roundTrip` twice on this branch during design: canonical ones came back identical; rewritten ones came back as shown and were a fixed point on the second pass. The JSON blocks below were themselves parsed and re-run through `roundTrip` during design: 15/15 canonical identical, 9/9 rewritten match and are fixed points
- Canonical, under `editor/fixtures/canonical/` (15):

```json
{
  "escapes--escaped-block-starts.md": "\\# not a heading\n\n\\> not a quote\n\n\\- not a list\n",
  "escapes--escaped-backslash-line.md": "\\\\\n",
  "images--in-link.md": "[![alt](/i.png)](/x)\n",
  "hard-break--in-link.md": "[one\\\ntwo](/x)\n",
  "hard-break--in-blockquote.md": "> one\\\n> two\n",
  "hard-break--in-list-item.md": "- one\\\n  two\n",
  "hard-break--in-ordered-item.md": "1. one\\\n   two\n",
  "hard-break--before-html.md": "one\\\n<em>x</em>\n",
  "lists--tasks-mixed.md": "- [ ] todo\n- plain\n- [x] done\n",
  "lists--tasks-ordered.md": "1. [ ] one\n2. [x] two\n",
  "other--unicode.md": "Family: 👨‍👩‍👧‍👦 and 👍🏽 skin tone.\n\n中文文本，日本語のテキスト，한국어 텍스트.\n\nمرحبا بالعالم and שלום עולם.\n",
  "other--bold-emphasis.md": "***bold and emphasized***\n",
  "other--nested-marks.md": "**a *b* c**\n",
  "setext-heading--with-break.md": "one\\\ntwo\n===\n",
  "setext-heading--with-break-h2.md": "one\\\ntwo\n---\n"
}
```

- Rewritten, under `editor/fixtures/rewritten/` (9), `[file bytes, riki writes]`:

```json
{
  "escapes--backslash-before-space.md": [
    "a \\\\ b\n",
    "a \\ b\n"
  ],
  "escapes--escaped-pipe.md": [
    "a \\| b\n",
    "a | b\n"
  ],
  "tables--escaped-backslash.md": [
    "| a |\n| --- |\n| C:\\\\path |\n",
    "| a |\n| --- |\n| C:\\path |\n"
  ],
  "escapes--escaped-link.md": [
    "\\[not a link\\](https://example.com)\n",
    "\\[not a link]\\(<https://example.com>)\n"
  ],
  "links--strong-in-text.md": [
    "[**a**](/x)\n",
    "**[a](/x)**\n"
  ],
  "links--emphasis-in-text.md": [
    "[_b_](/y)\n",
    "_[b](/y)_\n"
  ],
  "lists--task-uppercase-x.md": [
    "- [X] done\n",
    "- [x] done\n"
  ],
  "code-fence-style--meta-with-spaces.md": [
    "```js title=\"a b\"\nx\n```\n",
    "```js\nx\n```\n"
  ],
  "other--marks-overlap-link.md": [
    "**a [ sd *a*sf a](https://b) c**\n",
    "**a  [sd *a*sf a](https://b) c**\n"
  ]
}
```

- Every name's prefix is in `CATEGORIES` (`editor/test/fixtures.test.ts:13-39`)
- **Success criteria:** `pnpm -C editor run test` passes; `ls editor/fixtures/canonical | wc -l` -> 81; `ls editor/fixtures/rewritten | wc -l` -> 35

#### Phase 2: Render safety pins
**Model:** sonnet
- Golden tests in `core/src/render/tests.rs` through `render_markdown`: `javascript:` in any case, `javascript&colon;`, `&#58;`, `vbscript:`, `data:text/html`, autolink form -> `href=""`; tab, newline, CR, ZWSP and leading control entities -> percent-encoded relative href; reference definitions and images the same; `data:image/png` allowed on images
- **Success criteria:** `cargo test -p riki-core render::tests::unsafe_scheme` passes; changing one expected `href=""` to the input makes it fail

#### Phase 3: git subprocess hygiene
**Model:** opus
- `run_output`: `LC_ALL=C`, `process_group(0)`, `killpg` on timeout; `cargo add libc` in `core`
- Tests in `core/src/store/tests.rs`, through `run_output`:
  - `sh -c 'sleep 30 & echo $! > pidfile; wait'` with a 200ms timeout -> `Timeout`, then `kill(pid, 0)` reports ESRCH within 2s
  - the same command with the `run_output` future dropped after 200ms (`tokio::time::timeout` around it, timeout config 30s) -> grandchild gone within 2s
  - `sh -c 'sleep 30 & echo $! > pidfile'` (sh exits at once, the grandchild holds stdout) with a 200ms timeout -> `Timeout` and the grandchild gone within 2s
  - `sh -c 'printf %s "$LC_ALL"'` -> stdout `C`
- **Success criteria:** `cargo test -p riki-core store::tests::timeout_kills_process_group`, `store::tests::drop_kills_process_group`, `store::tests::exited_parent_kills_process_group` and `store::tests::git_runs_in_c_locale` pass; the first three fail with the `killpg` line removed, the last with the env line removed; `cargo tree -p riki-core --depth 1 | rg libc` prints one line

#### Phase 4: Error bodies
**Model:** sonnet
- `browser_message`, the three response-body changes, the API Design table
- Rewrite `server/src/api/save_tests.rs:361-379` (`any_other_push_failure_is_502_with_gits_stderr`, which asserts local receive-pack stderr reaches the body): rename it to `any_other_push_failure_is_502_with_the_remote_lines`; assert the fallback text and that the upstream temp path is absent
- A `status_error` test: an unreachable fetch whose error carries a path yields a `/status` message without it
- Update the shipped doc statements this reverses: `docs/design/2026-10-01-riki.md:217`, `:807`, `docs/design/2026-10-02-riki-wiki-operations.md:151` (each gets a one-line "superseded by" pointer to this doc)
- Unit tests feeding the spike's stderr: URL, `hint:` and `error:` lines gone, GH006 line present and trimmed
- **Success criteria:** `cargo test -p riki-core browser_message` and `cargo test -p riki-server save_tests` pass; the rewritten `save_tests.rs` push-failure test asserts the body equals `push rejected; see the server log` and fails against the old `api.rs:389`

#### Phase 5: Empty marks
**Model:** sonnet
- The `root` stringify handler (B4)
- Tests in a new `editor/test/serialize.test.ts` (only the `serialize.ts` helper exists today): strong, emphasis and strike on a lone space -> `Some word more`; `word ` selected -> `Some **word** more`
- **Success criteria:** `pnpm -C editor run test` passes; the three lone-space tests fail with the handler removed

#### Phase 6: Heading hard breaks
**Model:** sonnet
- `hardbreakFilterNodes` + `heading`; the scoped `appendTransaction` strip (B5, B6)
- Tests:
  - Shift-Enter in h1-h6 leaves the doc unchanged; Shift-Enter in a paragraph still breaks
  - pasting `<h3>a<br>b</h3>` -> `### a b`
  - paragraph `one\` / `two` turned into h3 or h1: the heading node holds no `hardbreak` (asserted on the ProseMirror doc, since serialization already drops an h3 break and would pass without the fix), and h1 serializes `# one two` (without the fix it is setext)
  - in a doc holding `one\` / `two` / `===`, typing in a later paragraph leaves the heading bytes unchanged
  - the Phase 1 setext-with-break fixtures stay canonical
- **Success criteria:** `pnpm -C editor run test` passes; the Shift-Enter tests fail with the filter line removed, and the paste and conversion tests fail with the plugin removed

#### Phase 7: Link box inside a link
**Model:** sonnet
- Range widening in `openLinkBox` (B3)
- Tests in `editor/test/linkbox.test.ts`: cursor inside `[abcd](/x)` -> `[abcd](y.md)`; inside `[ab**cd**](/x)` -> `[ab**cd**](y.md)`; cursor at the link's end -> a second link after it; plain text insert unchanged
- **Success criteria:** `pnpm -C editor run test` passes; the inside-link test fails without the widening

#### Phase 8: Conflict keeps the author's text
**Model:** opus
- the `conflict` field on the five 409 builders; `SaveResult` kinds (`path` maps to the message only); the session panel (B1)
- Tests: one server test per `conflict` value (`page-changed`, `index`, `retries-exhausted`, `path` from a move onto an existing page)
- `editor/test/api.test.ts:81-83` flips to expect `currentBody: 'y'`; new tests for `index-conflict` and `retries-exhausted`; a session test that after "Load latest" the panel holds the author's Markdown
- Rewrite the e2e test `editor/e2e/editor.spec.ts:106-124` ("…which discards after a confirm") to assert the author's text is shown after Load latest
- **Success criteria:** `pnpm -C editor run test` passes; `otto e2e` passes; the api test fails against the old `api.ts:116`

#### Phase 9: Case-only collisions
**Model:** opus
- Case-folded segment checks in move, create and slug; publish WARN for twins already present (H3)
- Tests:
  - move `a.md` -> `Setup.md` when `setup.md` exists -> 409
  - move `setup.md` -> `Setup.md` (the twin is the page itself) -> 200, one `R100` commit
  - create `docs/x.md` when `Docs/` exists -> 409
  - `new_page_path(.., "Setup")` with `Setup.md` present -> `setup-2.md`
  - Undo of a delete of `Setup.md` after `setup.md` was created -> 409
  - an index holding `Setup.md` + `setup.md` stays publishable, logs one WARN, and both pages render the twin banner
- **Success criteria:** `cargo test -p riki-core case_collision` passes; each refusal test fails with its check removed

#### Phase 10: Hidden control characters
**Model:** sonnet
- `hidden_controls`, the banner, a WARN at search build (H4)
- Tests: a page with U+202E renders the banner naming the line; Arabic or Hebrew text alone, and an emoji ZWJ sequence, render no banner
- **Success criteria:** `cargo test -p riki-server hidden_controls` passes and fails with the detector returning `None`

#### Phase 11: Page size cap
**Model:** sonnet
- `content.max-page-bytes`, `StaticRule::TooLarge`, the stub, the search skip (H5)
- Tests: a page one byte over a test limit renders the stub, is not editable, and is absent from search; a page at the limit renders; a save whose composed file is one byte over gets 400 and no commit; a body of C0 controls at the limit gets riki's message (`the page would exceed max-page-bytes ...`), not axum's length-limit text that riki already wraps in a 400 (`JsonRejection` -> 400, `api.rs:332-335`), so the test asserts the message, not the status; `read_blobs` returns no bytes for the oversized blob; the config loads the default when the key is absent and rejects a non-integer
- **Success criteria:** `cargo test -p riki-server too_large` passes and fails with the size check removed; `riki.example.yml` contains `max-page-bytes`

#### Phase 12: Pins
**Model:** sonnet
- Backspace right after an input rule restores the literal text for `**bold**`, `*it*`, `` `code` ``, `> [!NOTE] `, `|2x2| `; headings downgrade (`## ` -> `#`) and that is pinned as today's behavior; strike rule quirks pinned as today's behavior (parked)
- Slug property test: xorshift over 20,000 random titles (ASCII, C0, Latin, General Punctuation incl. bidi, CJK, emoji, combining marks) across three folders: `new_page_path` passes `validate_page_path`; a one-page index is publishable; `file_for_url(url_for_file(p)) == p`; `slugify` idempotent. No new crate
- **Success criteria:** `pnpm -C editor run test` and `cargo test -p riki-core slug::tests::property` pass; the property test fails with `slugify` changed to keep `/`, and the Backspace pins fail with the input rules removed

### Blast radius and ship order

| repo | change | when |
|---|---|---|
| `scottidler/riki` | all code, docs | Phases 1-12 |
| `scottidler/dotfiles` | none: `max-page-bytes` defaults when absent | |
| `scottidler/riki-content`, `riki-content-test` | none | |

Ship: PR from `wiki-learnings-fixes`, `bump release` on the branch, `bump finish` after merge, restart the `riki` user unit, `sdv probe` / `curl 127.0.0.1:8737/version` until the new tag shows.

## Acceptance Criteria

Run on `wiki-learnings-fixes` at `3aaf57c` (= `main` `297e814` + docs) on 2026-10-02.

- [ ] `ls editor/fixtures/canonical | wc -l` prints `81` and `pnpm -C editor run test` passes (Phases 1, 5-8, 12)
  - Observed: `66`; `Test Files 15 passed (15)`, `Tests 267 passed (267)`
- [ ] `cargo test -p riki-core timeout_kills_process_group` runs 1 test and passes (Phase 3)
  - Observed: `running 0 tests` / `0 passed; ... 184 filtered out` (test absent until Phase 3)
- [ ] `cargo test -p riki-server save_tests` passes with no test asserting git stderr text in a 5xx body (Phase 4)
  - Observed: `15 passed; 0 failed` (lib), including `any_other_push_failure_is_502_with_gits_stderr`, which asserts stderr text in a 502 and is rewritten in Phase 4
- [ ] `otto e2e` passes, including a conflict test that finds the author's text on the page after Load latest (Phase 8)
  - Observed: `29 passed (56.2s)`; the conflict test asserts the edit is discarded after a confirm (`editor/e2e/editor.spec.ts:106`), rewritten in Phase 8
- [ ] `otto ci; git status --short` exits 0 and prints nothing (every phase)
  - Observed: `=== All CI checks passed! ===`; `git status --short` -> `?? docs/design/2026-10-02-riki-learnings-fixes.md` (this doc, uncommitted at the time)

## Resolved Decisions

- 2026-10-02: the eight learnings docs are committed on this branch ahead of any phase (Scott: "commit all of these findings")
- 2026-10-02: B3 "inside a link" means link marks on both sides of the cursor; at an edge the cursor is outside (author)
- 2026-10-02: B1 ships the author-text panel; diff view, merge and overwrite are parked (author)
- 2026-10-02: B2 covers push, fetch and internal 5xx bodies and `/status`; reverses `docs/design/2026-10-01-riki.md:217`; justified on readability first (author; panel round 1 found the `/status` leak and the unnamed reversal)
- 2026-10-02: B5/B6: no hard breaks in any heading; existing setext-with-break files still load canonical (author)
- 2026-10-02: H3: case-only twins refused for riki's own writes (move, create, restore, slug); twins already in the tree get a WARN plus a page banner, not an index error; Unicode normalization parked (author; panel round 1 added restore and the banner)
- 2026-10-02: H4: bidi controls, U+200B, U+2060, non-leading U+FEFF; ZWJ and ZWNJ excluded; banner only (author)
- 2026-10-02: H5: a config key, default 2 MiB to match the save limit axum already enforces; request body limit `6 * max + 64 KiB`; the check runs on the composed file; every blob read checks the header first (author; panel round 1 corrected the multiplier and added the composed-file and size-first read; round 2 extended the header check from `read_blobs` to `blob_at` and `blob`)
- 2026-10-02: the 409 `conflict` field is `page-changed | index | retries-exhausted | path`, five builders including the path-op builder (`api/ops.rs:179`) and a new `SaveOutcome::PathTaken` for create (author; panel rounds 1-2; `op` renamed `path` because the create-path twin is not an op)
- 2026-10-02: H6 (comrak `javascript:` behind entities or invisible characters) is not a bug: comrak 0.55 safe mode blanks or percent-encodes every case tried; pinned in Phase 2 (design-research)

## Alternatives Considered

### Fix B4 with a remark plugin
- **Why not chosen:** Milkdown's serializer calls `remark.stringify(this.build())` without running transformers (`@milkdown/transformer@7.22.2 lib/index.js:375`); a `$remark` plugin never sees the tree

### Fix B4 by refusing marks on whitespace-only selections in the toolbar
- **Why not chosen:** covers the toolbar but not Mod-b or paste; the serializer is the one chokepoint

### H5 as a const
- **Why not chosen:** a tunable belongs in config (`rules/taste.md`, "Config drives behavior")

### H3 twins as an index error
- **Why not chosen:** riki's routing is case-sensitive, so twins are not ambiguous; an index error stops the whole wiki publishing over one laptop push

## Technical Considerations

### Dependencies
- `libc` direct in `riki-core` (already `0.2.189` in `Cargo.lock`, same as otto); no new crate, no editor dependency change

### Performance
- `hidden_controls` and the size check are one pass over text already in memory
- Case-folded checks walk only the path's segments at the tip tree

### Security
- B2 stops git stderr (remote URL, ssh errors, local paths) reaching any browser; logs keep it
- H4 makes Trojan-Source text visible to readers
- H6 pins the `javascript:` scrubbing against comrak upgrades

### Testing Strategy
- Unit and fixture tests per phase; one e2e rewrite (Phase 8)
- Every new test shown to fail with its fix reverted (`rules/taste.md`, "Tests must bite")

### Rollout Plan
- Home deploy only; config default means no dotfiles change

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| The `root` handler changes serialization of an existing fixture | Low | High | 66 canonical fixtures run in every `otto ci` |
| `killpg` hits the wrong group | Low | High | positive-pid check before `killpg`, otto precedent |
| A GitHub `remote:` line carries something sensitive | Low | Low | it is the server's message to the pusher; GitHub writes it for humans |
| Paste path still puts a break in a heading | Med | Med | Phase 6 paste test |
| A page between the old 5 MB habit and 2 MiB stops rendering | Low | Low | riki-content's largest blob is 210 bytes (`gh api repos/scottidler/riki-content/git/trees/main?recursive=1`); the cap is config |
| The Drop guard kills a reused process group | Low | High | the group id is the child's own pid from `process_group(0)`; kill only while the child is unreaped |
| The case check refuses a move a user meant | Med | Low | the move's own source is excluded, so a case-only rename of the page itself passes (Phase 9 test) |

## Open Questions

(none)

## Review Log

- **Pass 1 (Draft):** shape from the design-research brief; spike run; open questions from the brief closed in Resolved Decisions
- **Pass 2 (Correctness):** the editor cannot tell the three 409s apart (index and retries-exhausted bodies carry only `error`, `api.rs:397-406`), so the 409 gains a `conflict` field; retries-exhausted offers Save again; the `LC_ALL` test runs through `run_output` instead of inspecting a command it never sees; Phase 9 tests rewritten (one was garbled) and a case-only rename of the page itself allowed; Phase 4 criterion made falsifiable
- **Pass 3 (Clarity):** Phase 1 carried "contents from the design-research brief", a scratch file outside the repo; the fixture contents now live in the doc as tables, re-run through `roundTrip` during design (the probe agrees with the brief over two learnings docs: `\\` on its own line is canonical, contradicting tinacms-learnings.md:55; the Keystatic overlap input is rewritten, contradicting keystatic-learnings.md:86); fixture contents are fenced JSON, since `|` and `\` inside a Markdown table cell are ambiguous
- **Pass 4 (Edge cases):** a disconnecting browser drops the write future with no spawned task in between, orphaning `ssh` exactly like a timeout, so the process-group kill moves into a Drop guard with its own test; H5's 5 MiB default contradicted axum's 2 MiB default body limit (riki sets none), so the default is 2 MiB, saves over it 400, the body limit derives from it, and the nav title stops reading oversized blobs; the missing-URL create flow named under H3; clipboard fallback for B1; fixture JSON re-verified from the doc text itself
- **Pass 5 (Excellence):** Summary said four hardening gaps (there are five); the size-cap risk row now cites the measured largest blob; lint pass for em-dashes and "real"
- **Panel round 1** (architect rc=0; staff-engineer timed out twice, findings taken from its trace and re-checked by the orchestrator; synthesis `/tmp/review-panel/7bDJwUgR/synthesis.md`):
  - Folded, must-fix: H5 multiplier 2x -> 6x (C0 escapes); the 409 `conflict` field covers `api/ops.rs:179` and `page-moved` -> `page-changed`, plus a path value; converting a paragraph that holds a break into a heading bypassed the filter, so a scoped `appendTransaction` strip replaces the paste-only `transformPasted`; `/status` leaked fetch stderr, now sanitized, and the reversal of `docs/design/2026-10-01-riki.md:217` is named
  - Folded, cheap-win: restore in the H3 check; H3 twins get a page banner; the size check runs on the composed file; size-first blob read; the Drop guard keys on `wait_with_output` returning, with a third test; "fails when reverted" on Phases 10-12; citations corrected (`pages.rs:101`, `api.rs:375-381`, `linkbox.ts:71-75`, new `serialize.test.ts`, renamed `save_tests` test); LC_ALL test named in the Phase 3 criterion
  - Pushed back (architect): B2 should be dropped under `rules/taste.md` "no privacy scaffolding": kept, re-justified on readability (the author sees GitHub's sentence, not git's hints), security framing demoted; H3 twins as an index error: kept non-blocking, an index error stops the whole wiki publishing over one laptop push, the new banner answers "fail loudly"; PID reuse races `killpg`: an unreaped child's pid and a live group's pgid cannot be reused; the H5 request-limit derivation is unrequested scope: without it riki's 400 is unreachable behind axum's 413
- **Panel round 2** (both seats rc=0; scoped to four disputes and the staff seat's missing check):
  - Disputes: B2 on readability, H3 non-blocking + banner, and the H5 request-limit derivation accepted by both seats; PID reuse: the staff engineer showed tokio's `wait_with_output` reaps git while pipe reads are pending (`tokio-1.53.1/src/process/mod.rs:1463`), so my condition was wrong; accepted, `run_output` now drains to EOF before `wait` and kills only while git is unreaped
  - Folded, must-fix: the header check covers `blob_at` and `blob`, not only `read_blobs`; the Phase 6 h3 conversion test asserts at the node level (the serialized string passes without the fix); the create-path case twin gets `SaveOutcome::PathTaken`, a fifth 409 builder, and the value `op` becomes `path`
  - Folded, cheap-win: Phase 8 names five builders and tests each `conflict` value; the H5 C0 test asserts riki's message since riki wraps `JsonRejection` in a 400 (`api.rs:332-335`); "before any git work" -> after the fetch, before commit or push

## References

- `docs/{wiki-js,silverbullet,gollum,otterwiki,gitea-wiki,keystatic,tinacms,llm-wiki}-learnings.md`
- `docs/design/2026-10-01-riki.md`, `docs/design/2026-10-02-riki-wiki-operations.md`
- `~/repos/.claude/rules/rust.md` (Subprocess hygiene); `~/repos/otto-rs/otto/src/executor/scheduler.rs:658-690`
