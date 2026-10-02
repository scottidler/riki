# Handoff: riki Phase 9, wiki operations

**Branch:** `wiki-operations` (from `main` at `eed41cb`)
**Written:** 2026-10-02, by the session that built riki v0.1.0 to v0.1.1 and the home deploy

## Next action

Run `/create-design-doc` for a NEW design doc, `docs/design/2026-10-02-riki-wiki-operations.md`, scoped to the five items below. Do not append phases to `docs/design/2026-10-01-riki.md`; it is Status: Implemented. Run the Rule of Five and a review-panel round, and get Scott's calls on the two open decisions before the doc is ready to build.

## Scope (Scott approved items 1-5)

1. **New page:** a "+ New page here" control in the sidebar that asks only for a title and derives the path (kebab-case, `-2` if taken). Write nothing until the first Save. Keep the existing "missing URL -> Create this page" flow.
2. **Rename / move:** a title change is an ordinary edit; a path change is one `git mv` commit, chosen from a searchable folder picker that can create folders.
3. **Delete:** one `git rm` commit, with an Undo toast implemented as a revert commit. Git history is the trash.
4. **Search:** a Ctrl+K full-text palette (prefix matching, snippet, heading + path, keyboard nav), from an index derived from the git tree at the good tip and rebuilt on each poll. The same index drives a Ctrl+K "link to page" box in the editor that inserts a relative `.md` link.
5. **Milkdown editor upgrades:** selection toolbar, gutter **+** and ⋮⋮ block handle (Turn into / Duplicate / Delete), a slash menu limited to riki's GFM blocks, table row/column handles, Markdown typing shortcuts. Callouts stay GitHub alerts (`> [!NOTE]`). Every change must keep the round-trip guard byte-identical (vitest fixtures).

## Open decisions (need Scott, put them in the doc's Open Questions)

- **Redirects after a move:** riki's URL is the file path, so a moved page's old URL needs a 301. Options: a redirect map file committed in the content repo, or rename detection from git history.
- **Sidebar order:** today it's alphabetical from the tree. Options: an order file per folder (committed like any edit), or a number-prefix naming convention (no UI).

## Read first, in order

1. `docs/research/2026-10-02-mintlify-research.md`: Mintlify's editor tour (Part 2) and the styling measurements (Part 1).
2. `docs/design/2026-10-01-riki.md`: the shipped design. Non-Goals has the parked rows this phase revisits (full-text search; rename/move/delete in browser; page history), plus the parked editor gaps (fence titles, editor syntax colors).
3. `docs/design/2026-10-01-riki-implementation-notes.md`: per-phase decisions, especially Phase 5 (save algorithm), Phase 6 (editor), Phase 8/8b (theme), Phase 7 (deploy).
4. Code entry points: `core/src/save.rs` (save algorithm), `core/src/store.rs` (git store, push classification), `core/src/index.rs` (nav index), `server/src/api.rs` (API routes), `editor/src/` (Milkdown setup, toolbar, session).

## State at hand-off

- **Released:** v0.1.1 at `e4451b9`, tagged and pushed; CI green. `main` on GitHub = `e4451b9`.
- **Local only, not pushed:** `eed41cb` on `main` (Phase 7 results in the design doc and notes). This branch adds the research doc and this handoff on top.
- **Theme branch:** `riki-theme` merged into `main` (PR https://github.com/scottidler/riki/pull/1, CI passed). Its worktree at `/home/saidler/repos/scottidler/riki-theme` is no longer needed.
- **Home deploy (live):** systemd user unit `riki` on desk, `127.0.0.1:8737`, behind Caddy + Authelia at https://riki.escote.duckdns.org, content repo `scottidler/riki-content`. Probe: `systemctl --user is-active riki && curl -s 127.0.0.1:8737/status`.
- **Test content repo:** `scottidler/riki-content-test` (private) stays. It is in active use for testing; never propose deleting it.

## Unverified / blocked (re-test before believing)

- **Browser save as Scott:** Authelia user `saidler` got `email: scott.a.idler@gmail.com` (homelab `cbc668a`, branch `authelia`). Scott's retest after re-login was not confirmed. Probe: `gh api repos/scottidler/riki-content/commits/main --jq '[.commit.author.email, .commit.committer.email]'` should show his email and `riki@localhost` after he saves.
- **Phase 0c check 2 (forged `Remote-Email` overwritten by Authelia):** still needs Scott's credentials; the exact curl is in the implementation notes, Phase 7 section.
- **homelab:** Phase 7 changes are on branch `authelia`, 8+ commits ahead of `main`, no PR. Probe: `git -C ~/repos/scottidler/homelab log --oneline main..authelia`.

## Session-scoped (gone after this session)

- The session scratchpad (`/tmp/claude-1000/.../scratchpad/`) held screenshots, the Mintlify Playwright profile (Scott's logged-in Tatari Mintlify session), the archify spec, and test harnesses. Treat it as gone; everything needed is in the research doc.
- Published artifacts that survive: the theme screenshot page https://claude.ai/artifact/Aa27ksV8BsHhDoo5oWqQu4 and the architecture diagram https://marquee.internal.tatari.dev/p/~scott-idler/riki-architecture/.

## Known gotchas

- `bump release` on this repo: run from the main checkout on `main` with `--install "cargo install --path server --locked"` (the root is a virtual workspace, so bump skips install otherwise).
- CI only runs on `main` pushes and PRs; open a PR to get CI on a branch before merging. Hooks require PR titles to match the branch slug and a `Release:` line in the body.
- Rollback to v0.1.0 needs `site:` commented out of `~/.config/riki/riki.yml` (v0.1.0 rejects the unknown key).
- `riki.service` forces the deploy key with `ssh -F /dev/null ... -o IdentityAgent=none`; without it, pushes authenticate as Scott, not the deploy key.
- Nothing non-markdown under `docs/`.

## Suggested skills

- `/create-design-doc`: the next action.
- `review-panel` (via `Skill(review-panel)`): the design review round.
- `/how-to-execute-a-plan`: after the doc is ready to build.
- `rust-cli-coder`: Rust conventions for the server work.
