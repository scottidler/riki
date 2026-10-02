# Keystatic learnings for riki

**Source:** `Thinkmill/keystatic`, branch `main`, commit `b8ec9650`, cloned at `~/repos/Thinkmill/keystatic`. All paths below are relative to that repo. Paths marked "riki" are relative to `~/repos/scottidler/riki`.
**Date:** 2026-10-02

## Framing

- Keystatic is a schema-driven CMS, not a wiki. Content lives in git: the local filesystem, GitHub, or Keystatic Cloud (`packages/keystatic/src/config.tsx:81`, `:102`, `:121`). That part matches riki.
- The write path is the reverse of riki's. In GitHub mode the browser commits straight to GitHub with GraphQL `createCommitOnBranch` (`packages/keystatic/src/app/updating.tsx:213-227`, mutation at `:346-364`). riki's browser never talks to git: the server fetches, checks, builds the commit, and pushes (riki `core/src/write.rs:114-193`).
- The editor is a hand-built ProseMirror schema on raw `prosemirror-*` packages (`packages/keystatic/package.json:156-163`), not Milkdown. ProseMirror to Markdown goes through hand-written mappers to and from mdast. Those mappers are called by `mdast-util-from-markdown` / `mdast-util-to-markdown` with the GFM table, strikethrough, and autolink extensions, for the `mdx` field (`packages/keystatic/src/form/fields/markdoc/ui.tsx:76-120`). The `markdoc` field goes through Markdoc's own parser and formatter instead (`packages/keystatic/src/form/fields/markdoc/ui.tsx:39-74`).
- Keystatic accepts normalization on save. It has no round-trip guard, so whatever its serializer writes is what gets committed. riki refuses to edit any page it can't reproduce byte for byte (riki `docs/design/2026-10-01-riki.md:148`). Most of the fidelity lessons below are about what Keystatic gives up and riki's guard would catch.
- What transfers is the editor UX and its tests, the test cases for the serializer, and the way conflicts are shown. The architecture does not transfer.

## What transfers

### Slash menu ("insert menu")

riki shipped its slash menu in the wiki-operations work (riki `docs/design/2026-10-02-riki-wiki-operations.md:182`, riki `editor/src/slashmenu.ts:1-5`).

- Each entry is offered only if its command would run at the cursor. The list goes through `matchSorter` on the label, then `.filter(option => option.command(editorState))`, a dry run with no `dispatch` (`packages/keystatic/src/form/fields/markdoc/editor/autocomplete/insert-menu.tsx:97-103`). riki filters by label only (riki `editor/src/slashmenu.ts:43-46`), and handles the places where the menu is wrong by turning the whole menu off: it only opens in a paragraph outside a table (riki `editor/src/slashmenu.ts:52-61`). A dry-run filter would drop single entries that can't apply in a given spot (inside an alert, inside a list item) without riki listing those places by hand.
- Space with exactly one match runs it. Space with no matches closes the menu and leaves the typed text alone (`packages/keystatic/src/form/fields/markdoc/editor/autocomplete/insert-menu.tsx:105-115`).
- The typed `/query` is a decorated range in the document. Choosing an entry deletes that range first, then runs the command against the state without it (`packages/keystatic/src/form/fields/markdoc/editor/autocomplete/insert-menu.tsx:70-79`, delete at `packages/keystatic/src/form/fields/markdoc/editor/autocomplete/decoration.tsx:131-137`). riki does the same delete-then-run (riki `editor/src/slashmenu.ts:199-209`).
- The menu closes itself when:
  - the selection leaves the range
  - the trigger character is gone
  - a non-text node lands inside the range (checked with a `\u{fffd}` placeholder)
  - a paste or drop happens

  (`packages/keystatic/src/form/fields/markdoc/editor/autocomplete/decoration.tsx:166-191`, paste and drop at `:215-220`).
- Menu entries are declared on the node specs (`insertMenu` on `table`, `packages/keystatic/src/form/fields/markdoc/editor/schema.tsx:269-276`) and collected into one sorted list when the schema is built (`packages/keystatic/src/form/fields/markdoc/editor/schema.tsx:673-702`). The same list feeds the toolbar (`forToolbar`, `packages/keystatic/src/form/fields/markdoc/editor/autocomplete/insert-menu.tsx:26-32`). That stops the slash menu, the toolbar and the block handle's Turn into from drifting apart. riki builds its slash list by hand from `TURN_INTO` plus inserts (riki `editor/src/slashmenu.ts:30-40`).

### Typing shortcuts

riki's typing shortcuts are the Milkdown preset rules plus a new alert rule (riki `docs/design/2026-10-02-riki-wiki-operations.md:185`).

- **Link shortcut.** Typing `[text](href)` turns it into a link mark when the `)` is typed (`packages/keystatic/src/form/fields/markdoc/editor/inputrules/rules.ts:104-119`). The `(?:^|[^!])` prefix keeps it from firing on image syntax (`:107`), and an `allowsMarkType` check stops it in places a link can't go (`:110`, helper at `:138-151`). riki's list doesn't include this rule (riki `docs/design/2026-10-02-riki-wiki-operations.md:185`). If it's added, it needs collision tests against the task `[ ]` rule and riki's `[!NOTE]` alert rule.
  - Borrowable tests (`packages/keystatic/src/form/fields/markdoc/editor/tests/markdown-link-shortcut.test.tsx`):
    - basic (`:7-36`)
    - text before the link, with and without a space directly before it (`:38-104`)
    - bold inside the link text (`:171-223`)
    - typing after the link stays outside it (`:137-169`)
    - text after the cursor is not pulled into the link (`:267-300`)
- **Undo after a shortcut.** Keystatic forks `prosemirror-inputrules` so a rule's result is its own history step: `closeHistory(tr)` (`packages/keystatic/src/form/fields/markdoc/editor/inputrules/inputrules.ts:1-2`, `:97-104`). Undo then gives back the literal typed text. Tests:
  - heading: undo returns `# ` as text, redo reapplies (`packages/keystatic/src/form/fields/markdoc/editor/tests/heading.test.tsx:40-57`)
  - link: undo returns `[content](https://...)` (`packages/keystatic/src/form/fields/markdoc/editor/tests/markdown-link-shortcut.test.tsx:225-265`)

  riki gets Backspace-undo from Milkdown, which binds `undoInputRule` first in its Backspace chain (riki dependency `editor/node_modules/.pnpm/@milkdown+core@7.22.2/node_modules/@milkdown/core/lib/index.js:239`). No riki editor test checks undo or Backspace after a shortcut (only riki `editor/test/page-actions.test.ts` mentions undo, and that's page delete). These tests port directly.
- **Enter rule for fences.** Typing ```` ```lang ```` then Enter makes a code block, as well as ```` ```lang ```` then Space (`packages/keystatic/src/form/fields/markdoc/editor/inputrules/rules.ts:44-51`, Enter rules at `:125-136`, Enter handling at `packages/keystatic/src/form/fields/markdoc/editor/inputrules/inputrules.ts:46-51`).
- **Rules are skipped inside code blocks** (`packages/keystatic/src/form/fields/markdoc/editor/inputrules/inputrules.ts:70`). riki should pin this with a test once its alert and table rules are in.
- **Don't copy the typography shortcuts.** `...` -> `…`, `--` -> `–`, `->` -> `→`, `(c)` -> `©` and others fire on the following whitespace (`packages/keystatic/src/form/fields/markdoc/editor/inputrules/shortcuts.ts:1-11`, rules at `packages/keystatic/src/form/fields/markdoc/editor/inputrules/rules.ts:14-19`, tests in `packages/keystatic/src/form/fields/markdoc/editor/tests/shortcuts.test.tsx`). In a technical wiki they would rewrite prose about CLI flags and arrows.

### Editing an existing link

- When the selection sits inside one link, a link popover opens with Edit, Open, and Unlink (`packages/keystatic/src/form/fields/markdoc/editor/popovers/index.tsx:558-582`, toolbar at `packages/keystatic/src/form/fields/markdoc/editor/popovers/link-toolbar.tsx:21-74`).
- `markAround` widens a cursor to the whole run of text carrying that mark (`packages/keystatic/src/form/fields/markdoc/editor/popovers/index.tsx:242-266`). Edit and Unlink then remove or replace the mark over that whole range (`:274-310`).
- riki's Ctrl+K link box applies a choice to the range kept when the box opened. With a collapsed cursor it inserts new linked text (riki `editor/src/linkbox.ts:71-77`, insert at `:75`). So Ctrl+K with the cursor inside an existing link inserts a second link rather than changing the target of the first. Running `markAround` before opening would make Ctrl+K inside a link mean "change its target".

### Paste

riki's `makeEditor` registers no paste or clipboard plugin of its own (riki `editor/src/setup.ts:79-107`).

- **Paste a URL over selected text** and it becomes a link on that text. This happens only for `http(s)`/`mailto`/`tel`, only when the selection is inside one block, and only when it holds no link yet (`packages/keystatic/src/form/fields/markdoc/editor/links.tsx:57-83`). A bare URL pasted on its own becomes a linked URL (`packages/keystatic/src/form/fields/markdoc/editor/links.tsx:32-56`). Tests are in `packages/keystatic/src/form/fields/markdoc/editor/tests/pasting/links.test.tsx`.
- **`transformPasted` is the hook for riki's parked pasted-table bug.** riki parks "Pasted HTML table writes `| :--- |`" because paste doesn't go through `createAndFill` (riki `docs/design/2026-10-02-riki-wiki-operations.md:60`). Keystatic's `transformPasted` rewrites the pasted slice before it lands (`packages/keystatic/src/form/fields/markdoc/editor/links.tsx:32-56`). A riki `transformPasted` that sets `alignment: null` on every pasted `table_cell` / `table_header` would close that gap with the same intent as riki's schema-default fix (riki `docs/design/2026-10-02-riki-wiki-operations.md:184`).
- **VS Code copies arrive as HTML** (a monospace `div` of `div`/`span` lines). Keystatic detects that shape and pastes the plain text instead (`packages/keystatic/src/form/fields/markdoc/editor/markdoc/clipboard.tsx:56-70`, detection at `:75-97`). Without this, code copied from an editor pastes as styled paragraphs.
- Copy writes Markdown to `text/plain` through the document serializer, with a plain-text fallback (`packages/keystatic/src/form/fields/markdoc/editor/markdoc/clipboard.tsx:22-38`).

### Fidelity choices riki's guard would refuse

These are Keystatic's mdast mapping choices. Each one is a silent rewrite in Keystatic, and in riki it would be a failing fixture. They're useful as a checklist of fixtures for riki's `rewritten/` set.

- **Code fence meta with spaces is cut off.** On load, `lang` and `meta` are joined into one `language` attr with a space (`packages/keystatic/src/form/fields/markdoc/editor/mdx/parse.ts:272-281`). On save they're split with `lang.split(' ', 2)` (`packages/keystatic/src/form/fields/markdoc/editor/mdx/serialize.ts:231-246`). A limit of 2 drops everything after the second space: `'js title="a b"'.split(' ', 2)` is `["js", "title=\"a"]` (checked in node). The only test uses a one-word meta, `js blah` (`packages/keystatic/src/form/fields/markdoc/editor/tests/mdx.test.tsx:377-408`). riki has parked code-fence titles, and the planned fix is a separate `meta` attr (riki `docs/design/2026-10-01-riki.md:63`). Keystatic shows why it should be separate, with a test for a meta that has spaces in it.
- **Link titles are dropped on load.** The link mark is created from `node.url` only (`packages/keystatic/src/form/fields/markdoc/editor/mdx/parse.ts:287-293`). The serializer writes `mark.attrs.title` (`packages/keystatic/src/form/fields/markdoc/editor/mdx/serialize.ts:147-153`), but that value is always the default.
- **Reference links are inlined.** Definitions are collected first, keeping the first one for each id as CommonMark does (`packages/keystatic/src/form/fields/markdoc/editor/mdx/parse.ts:133-143`). Each `linkReference` becomes a plain link (`:294-305`), and the definition node is dropped (`:447`). The test writes `[a](https://keystatic.com)` for `[a][b]` (`packages/keystatic/src/form/fields/markdoc/editor/tests/mdx.test.tsx:629-656`). riki's design notes Milkdown does the same (riki `docs/design/2026-10-01-riki.md:771`), and riki's `editor/fixtures/rewritten/reference-links--definition.md` covers it.
- **Table alignment is dropped.** No table node or cell has an alignment attr (`packages/keystatic/src/form/fields/markdoc/editor/schema.tsx:269-334`). The parser passes `{}` for the table (`packages/keystatic/src/form/fields/markdoc/editor/mdx/parse.ts:329-331`), and the serializer writes no `align` (`packages/keystatic/src/form/fields/markdoc/editor/mdx/serialize.ts:208-223`).
- **List style is normalized:**
  - `-` comes back as `*` (`packages/keystatic/src/form/fields/markdoc/editor/tests/mdx.test.tsx:1014-1025`)
  - `1.`/`1.`/`1.` is renumbered `1.`/`2.`/`3.` (`:1027-1038`)
  - a start of `5` is kept and the rest renumbered (`:1058-1069`)
  - a two-space hard break becomes `\` (`:579-602`)

  riki's `bullet: '-'` option (riki `editor/src/setup.ts:56-63`) and its `rewritten/` fixtures (`ordered-list--repeated-one.md`, `bullet-char--star.md`, `hard-break--two-spaces` in riki `editor/test/fixtures.test.ts:74-78`) already cover these.

### Serializer tests riki can borrow

- **Whitespace moved out of marks.** A bold run of `"  content "` serializes as `start  **content** something`, because `**content **` would not parse back as strong (`packages/keystatic/src/form/fields/markdoc/editor/tests/mdx.test.tsx:929-946`). The same check runs for bold, italic and strikethrough (`:957-976`). Keystatic's logic is in `packages/keystatic/src/form/fields/markdoc/editor/serialize-inline.ts:44-74`. Milkdown does the same: its serializer closes every mark with `#closeNodeAndPush(true)`, which runs `#moveSpaces` (riki dependency `editor/node_modules/.pnpm/@milkdown+transformer@7.22.2/node_modules/@milkdown/transformer/lib/index.js:283-311`, call at `:347`). No riki test pins this. A selection-toolbar Bold over a selection that ends in a space is exactly the case, and the test is cheap.
- **Mark nesting is kept minimal.** `**a *b* c**` round-trips as is, not as `**a ***b*** c**` (`packages/keystatic/src/form/fields/markdoc/editor/tests/mdx.test.tsx:837-867`). Overlapping link and emphasis, `**a [ sd *a*sf a](https://b) c**`, also round-trips (`:869-927`). Keystatic reorders marks that are already open so the open delimiters stay open (`packages/keystatic/src/form/fields/markdoc/editor/serialize-inline.ts:76-99`, adapted from prosemirror-markdown per `:1`). These two inputs are good canonical fixtures for riki, provided they serialize unchanged under riki's stringify set.
- **Fail-closed load tests.** An unknown component throws `Missing component definition` (`packages/keystatic/src/form/fields/markdoc/editor/tests/mdx.test.tsx:820-826`). An unhandled mdast type throws with the offending source re-serialized into the message (`:828-835`, built at `packages/keystatic/src/form/fields/markdoc/editor/mdx/parse.ts:448-458`). Errors are collected and thrown together after the walk (`packages/keystatic/src/form/fields/markdoc/editor/mdx/parse.ts:38-40`, `:146-148`). riki's guard reports `first-diff-line` (riki `editor/src/api.ts:72-76`). Naming the node type the way Keystatic does would tell the author which construct blocked editing, not just which line.
- **Test harness shape.** Tests build documents as JSX with `<cursor />`, `<anchor />`/`<head />` and `<node_selection>` markers (`packages/keystatic/src/form/fields/markdoc/editor/tests/utils.tsx:125-135`). They drive a mounted editor through `@testing-library/user-event` keystrokes (`packages/keystatic/src/form/fields/markdoc/editor/tests/utils.tsx:517-560`, `undo`/`redo` helpers at `:507-513`) and snapshot the resulting document with the cursor shown. riki's typing tests call `handleTextInput` per character and assert on Markdown output only (riki `editor/test/typing.test.ts:8-17`). Cursor-in-snapshot assertions would make block-handle and slash tests say where the cursor ended up, which Markdown output can't show.

### Atomic commits and how conflicts are shown

- **One commit per save, many files.** `serializeEntryToFiles` turns one entry into several files: the data file, front matter joined to the body, and extra files such as images (`packages/keystatic/src/app/updating.tsx:56-107`, front matter join at `:37-54`). They all go into one `createCommitOnBranch` as `additions` and `deletions` (`:213-227`). Files the entry had when it was opened but no longer writes become deletions in the same commit (`:181-196`). That's how a slug rename is one commit. riki's `TreeOp` list is the same idea, built server-side (riki `core/src/store.rs:81-83`, `commit_tree` at `:238`).
- **Unchanged files are left out.** Keystatic computes each addition's git blob sha locally and drops any addition whose sha already matches the tree (`packages/keystatic/src/app/updating.tsx:169-192`). riki answers `Unchanged` with no commit (riki `core/src/save.rs:134-136`).
- **Optimistic concurrency, two levels.**
  1. `expectedHeadOid` is the commit the editor loaded (`packages/keystatic/src/app/updating.tsx:228`). Any commit to the branch since then makes GitHub answer `STALE_DATA`.
  2. On `STALE_DATA`, Keystatic fetches the new head outside its cache (`:244-264`) and computes a "tree key": the joined tree shas of every directory the entry owns (`packages/keystatic/src/app/tree-key.tsx:67-86`). If that key matches the one taken when the entry was opened, it retries once against the new head (`packages/keystatic/src/app/updating.tsx:274-277`). If not, it gives up with "This entry has been updated since it was opened" (`:278-285`).

  That's the per-entry check riki makes per file (`blob(path) == base-oid` at the tip, riki `core/src/save.rs:120-133`).
- **How a conflict reaches the author.** `needs-new-branch` opens a dialog. It creates a branch at the *loaded* commit and saves the author's edit there (`packages/keystatic/src/app/ItemPage.tsx:297-317`, `createRef` with `oid: props.branchOid` at `:796`, button "Create branch and save" at `:829`). The edit is never lost, and merging is left to a PR. A protected-branch rejection (`BRANCH_PROTECTION_RULE_VIOLATION` or "repository rule violations") goes to the same dialog (`packages/keystatic/src/app/updating.tsx:229-242`). The "app not installed" `FORBIDDEN` error is reworded into a sentence that says what to fix (`:289-300`). Any other error is shown as a critical notice above the form (`packages/keystatic/src/app/ItemPage.tsx:266-271`).
- **What riki can take from this.** riki serves one branch, so a branch-and-PR fallback doesn't fit as is. Two smaller pieces do fit:
  - riki's server already sends the current page body with every 409 (`current-body`, riki `server/src/api.rs:375-381`, asserted in riki `server/src/api/save_tests.rs:234`). The editor drops it: `savePage` keeps only the error message (riki `editor/src/api.ts:116`), and the conflict UI offers only "Load latest", which discards the edit after a confirm (riki `editor/src/session.ts:225-236`). Showing the current body next to the author's text, or a copy-my-text button, would keep the edit the way Keystatic's branch dialog does.
  - The push-rejection path maps to 502 with the server's words (riki `docs/design/2026-10-02-riki-wiki-operations.md:151`). Recognizing a branch-protection rejection there and saying so in plain words is the Keystatic `FORBIDDEN` rewording applied to riki.
- **Not every op got the retry.** Delete sends `expectedHeadOid: baseCommit` with no `STALE_DATA` handling (`packages/keystatic/src/app/updating.tsx:420-447`). Any unrelated commit on the branch makes delete fail with a raw error. riki runs every op through one driver for exactly this reason (riki `docs/design/2026-10-02-riki-wiki-operations.md:330`).

### Local drafts keyed to what was loaded

- Every change to an entry writes its serialized files to IndexedDB (`idb-keyval`, store `keystatic`/`items`, `packages/keystatic/src/app/persistence.tsx:66-71`). The draft carries the tree key it was based on and a `savedAt` time (`packages/keystatic/src/app/ItemPage.tsx:437-457`).
- On reopen, the draft is restored with a toast. If the tree key changed since the draft was saved, the toast warns that the entry has moved on (`packages/keystatic/src/app/utils.ts:235-238`, toast at `packages/keystatic/src/app/persistence.tsx:47-63`).
- riki excludes a *server-side* draft store ("A pending store, publish queue, or draft branch", riki `docs/design/2026-10-02-riki-wiki-operations.md:43`). A browser-local draft keyed by `(path, base-oid)` doesn't conflict with that: git stays the database. It would also cover the case where "Load latest" or a closed tab throws an edit away.

### Image upload in the page's commit

riki parks this ("Image upload from the editor", riki `docs/design/2026-10-01-riki.md:57`).

- Dropping or pasting an image file creates an image node that holds the bytes (`src: Uint8Array`) and a filename (`packages/keystatic/src/form/fields/markdoc/editor/images.tsx:27-95`).
- On save, the serializer moves those bytes into `extraFiles` or a configured directory and writes the Markdown `url` with the filename (`packages/keystatic/src/form/fields/markdoc/editor/mdx/serialize.ts:64-90`). Those files then join the same commit as the page (`packages/keystatic/src/app/updating.tsx:96-105`, `:220-225`).
- With riki's `commit_tree`, that's a page `Upsert` plus an image `Upsert` in one commit, through the same check and push path.

## What it confirms riki already has right

- **A byte-identical guard beats normalize-and-hope.**
  - Keystatic's Markdoc save runs `format(parse(format(node)))` (`packages/keystatic/src/form/fields/markdoc/ui.tsx:68-71`) to reach a stable output in production.
  - Its only corpus test compares that normalized form, after deleting the `marker` attribute from every `em` and `strong` (`packages/keystatic/src/form/fields/markdoc/editor/tests/docs.test.ts:31-52`).

  Neither can notice when an author's file is rewritten. riki checks idempotence in tests (every rewritten fixture's output must round-trip unchanged, riki `editor/test/fixtures.test.ts:110-116`) and refuses edits on pages that aren't byte-identical.
- **Fail closed on what the editor can't represent.** Keystatic throws on load for unknown nodes (`packages/keystatic/src/form/fields/markdoc/editor/mdx/parse.ts:146-148`, `:448-458`). That's the same policy as riki's guard. riki applies it to every byte, not only to node types.
- **Move as a pure rename.** A Keystatic slug change writes the new files and deletes the old ones in the same commit as the edit (`packages/keystatic/src/app/updating.tsx:181-200`). Rename detection then has to work from similarity. riki keeps a move to a pure `R100` rename with no edit, so exact-rename redirects work (riki `docs/design/2026-10-02-riki-wiki-operations.md:338-340`).
- **The check belongs on the server, for every op.** Keystatic's per-entry tree-key retry lives only in the upsert path, and delete lacks it (see above). riki runs fetch, check at tip, build, validate, push, retry for every op in one driver (riki `core/src/write.rs:114-193`).
- **A narrow slash trigger.** Keystatic opens the menu on `/` after any whitespace (`packages/keystatic/src/form/fields/markdoc/editor/autocomplete/insert-menu.tsx:47-58`), so prose like `see /etc/hosts` pops it. riki opens only on `/` at the start of an otherwise empty paragraph (riki `editor/src/slashmenu.ts:48`, `:65-70`), which suits a wiki full of paths.
- **`rule: '-'`.** Keystatic's MDX serializer picks the same thematic break as riki (`packages/keystatic/src/form/fields/markdoc/ui.tsx:108-115`, riki `editor/src/setup.ts:60`).
