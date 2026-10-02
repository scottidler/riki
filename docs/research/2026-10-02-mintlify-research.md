# Mintlify research for riki

**Date:** 2026-10-01/02
**Method:** Playwright measurements of live Mintlify-hosted docs sites (desktop 1280x900 and mobile 390x844, light and dark), Mintlify's public docs and `docs.json` schema, and a guarded tour of Mintlify's web editor on Tatari's evaluation site (`app.mintlify.com/tatari-tv`, backing repo `escote-tatari/mintlify-poc`). Nothing was published during the tour; the one scratch page was deleted.

Part 1 fed riki's Phase 8 theme (shipped in v0.1.1). Part 2 is input for Phase 9 (wiki operations), which still needs its own design doc.

## Part 1: reading-side styling (17 sites)

Sites verified as Mintlify (all send `x-mintlify-client-version`): Anaconda, Baseten, Browserbase, Cognition (Devin), Coinbase CDP, Dub, HubSpot, Kalshi, Lovable, OpenRouter, Perplexity, Polymarket, Replit, Resend, Together AI, World, X, Zapier. Anthropic's docs (platform.claude.com) are no longer Mintlify (`x-powered-by: Next.js`).

### Answers to the three theme questions

- **Header site title:** always a logo image (light + dark variants). The configured `name` appears only as screen-reader text, `og:site_name`, and the `<title>` suffix (`Page - Name`). It is never the home page H1.
- **Sidebar group names:** configured strings in `docs.json` (`navigation.groups[].group`), not folder names (e.g. "Get started" over `/overview`; "Admin & Management" over `/admin`). Page labels come from frontmatter `title`/`sidebarTitle`, else a prettified filename. Top-level groups are static headers; only nested groups collapse.
- **Mobile copy button:** always visible at `top: 12px; right: 16px` (26x26 buttons). A `pointer-events:none` fade overlay sits under it when the block overflows; `<code>` gets `padding-right` equal to the button cluster width. Titled or tabbed blocks put the buttons in a header bar above the code instead. Code never wraps.

### Measured defaults (Mintlify's default theme; most sites match)

| Element | Value |
|---|---|
| Body font | Inter (variable), 16/24; prose 16/28; first paragraph 18/28 |
| h1 | 36/40, weight 600, -0.9px (mobile 30/36) |
| h2 | 24/32, weight 600, -0.6px, margins 36 top / 18 bottom |
| h3 | 20/28, weight 600, -0.5px, margins 32 / 14 |
| Code | paperMono 14/24 (Mintlify-licensed; riki uses JetBrains Mono) |
| Inline code | 12-14px, radius 6px, padding 2px 8px, gray-100 at 50% |
| Sidebar | 288-304px, shown at >= 1024px; rows 14px; active item 12px-radius pill at primary/10% |
| Content | max ~576px |
| TOC | 304px, shown at >= 1280px, "On this page" |
| Tables | no outer border/zebra, 1px gray-100 row rule, th 600, cell padding 8px 8px 8px 0 |
| Code blocks | radius 16px, 1px border (black/10%, dark white/10%), padding 14px 16px, no language label |
| Callouts | radius 16px, 1px border, tinted bg, 16px line icon, no title row |
| Links | no text-decoration, 1px primary border-bottom, weight 500-600 |
| Dark mode | `html.dark` + `data-theme-preference=system\|light\|dark`, localStorage `isDarkMode`, 3-way toggle |
| Tokens | `:root` RGB triplets: `--primary`, `--primary-light`, `--primary-dark`, `--background-light/-dark`, `--gray-50..950` |
| Prev/next | bordered cards, 12px radius, "Previous"/"Next" label |
| Heading anchors | 24px chip 40px left of heading, hover/focus only |

`docs.json` model: required `name`, `theme`, `colors.primary`, `navigation`. Nine built-in themes (mint, maple, palm, willow, linden, almond, aspen, sequoia, luma). Highlighting is Shiki. Callouts are MDX components (Note, Tip, Info, Warning, Check, Danger); GitHub `> [!NOTE]` syntax is not supported.

## Part 2: the web editor (input for Phase 9)

### Save and publish, in git terms

- Typing on `main` autosaves into a **pending store on Mintlify's servers**; no commit. **Publish** opens a dialog of pending files (Added / Modified / Deleted) with Discard all / Publish, then commits + pushes to `main` and deploys. One publish per branch at a time; invalid MDX disables it.
- On a feature branch, edits auto-commit and push about 15s after typing stops; the first commit opens a draft PR, then Request review / Merge and publish.
- "Edited 1d ago" opens version history: editor snapshots, not git commits.
- External pushes are three-way merged into open pages; conflicts keep both versions highlighted.

### Page operations

- **Add:** "New page" (end of nav), or **+** on a nav group or on Files. No title/path prompt: a blank page creates pending `untitled-page-N.mdx`, and typing the title renames the path. A page whose title is never typed stays `untitled-page.mdx`.
- **Rename vs move are separate:** Rename (right-click or ⋯) changes only the title. Page settings > Path moves the file and creates folders. "Move to…" (⋯ or Ctrl+Shift+P) is a searchable folder picker with a Revert toast. Empty folders disappear. No automatic redirect; Path settings has a separate redirect field.
- **Delete:** ⋯ > Move to trash, Undo toast; pending "Deleted" until publish. No trash view.
- **Search:** Ctrl/Cmd+K palette, full text with prefix matching, highlighted snippet, section heading + page path per hit, keyboard navigation. Ctrl+K on selected text opens "Paste a link or search pages".
- **Sidebar order:** drag in the Navigation tree, saved into `docs.json`. "Move to files" / "Move to navigation" add or remove a page from the sidebar.
- **Files vs Navigation:** Navigation is the `docs.json` tree; Files is everything else in the repo. A separate Workspace tab holds pages that never reach git.

### Editing feel (TipTap/ProseMirror)

- No fixed toolbar. A **selection toolbar**: block type (Text, H1-H4, lists), bold, italic, underline, strike, code, link, comment, suggest, More (blockquote, sub/superscript, kbd).
- **Gutter on hover:** **+** to add a block, ⋮⋮ handle with Turn into / Duplicate / Delete.
- **Slash menu:** basic blocks, lists and tables, media, six callout kinds, layout (cards, columns, tabs, steps, accordion), code (code block, group, Mermaid, math), API reference, components.
- **Markdown typing shortcuts** convert live (`##`, `-`, `1.`, `[ ]`, `>`, triple backtick, `**`, backtick, `*`). `@` links a page; `[[` does nothing.
- **Tables:** `/table` gives 3x3 with header; **+** bars add rows/columns; column menu has insert, move, align, delete.
- **Links** render as inline page chips; stored as `[text](/path)`.
- **Images:** upload or pick from repo; stored under `images/`, 20 MB cap, no SVG.
- **Source mode** (Ctrl+Shift+S) shows the file; output is clean GFM apart from MDX component tags.

### Fit with riki (one URL, git is the database, plain GFM)

| Mintlify behavior | riki fit |
|---|---|
| New page with title-derived path | Fits; write nothing until first Save |
| Title change vs path change | Fits; a path change is one `git mv` commit, but the old URL needs a 301 (open decision) |
| Folder picker for moves | Fits; git has no empty folders either |
| Delete with Undo | Fits as `git rm` + revert commit; git history is the trash |
| Ctrl+K full-text search + link box | Fits; index derived from the git tree at the good tip, rebuilt per poll |
| Selection toolbar, block handles, slash menu, table handles, shortcuts | Fits riki's Milkdown editor; callouts stay GitHub alerts |
| Sidebar order in `docs.json` | Partial; needs an order file per folder or a naming convention (open decision) |
| Pending store, publish queue, separate editor URL | Does not fit; contradicts git-as-database and one URL |
