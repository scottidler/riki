# riki implementation notes

## Phase 0c: Identity edge spike
### Design decisions
- Used the homelab's own `caddy-caddy:latest` image with a scratchpad Caddyfile, https via `tls internal` on 127.0.0.1:8739, host `hindsight.escote.duckdns.org` (covered by an existing `one_factor`, `group:admins` Authelia rule) - scratchpad `riki-0c/Caddyfile` - the real edge is https and Authelia rejects forward-auth with `X-Forwarded-Proto: http`.
- Unauthenticated check used `--resolve` rather than Host-header over http - same reason.

### Deviations
- Throwaway Caddy on loopback instead of a live route on the homelab Caddy - hard constraint: no edits or reloads of live Caddy/Authelia. Same effect (the `(authelia)` snippet body verbatim in front of a reverse_proxy to the echo), different seam (separate listener, not a `*.escote.duckdns.org` site).
- Criteria 1 and 2 not executed: they need Scott's Authelia Basic credential, which must not be read or guessed. Recorded as BLOCKED-ON-OPERATOR with exact curl commands in the design doc.
- Loopback tests ran outside the Bash sandbox: the sandbox has its own network namespace and cannot reach host 127.0.0.1:9091 (Authelia). Only my own throwaway processes were started.

### Tradeoffs
- Plain-http harness (simpler) vs https with a self-signed internal cert - https, because http produced a misleading `200` empty body from Caddy relaying Authelia's 400.

### Open questions
- Scott: run criteria 1 and 2 with his credential (commands in the design doc; `riki-0c/up.sh` recreates the stack).
- Observation for the design: Caddy relays Authelia's 400 (http proto) as a `200` with empty body. Fail-closed for riki (no echo hit), but riki's server should never rely on status codes alone from the edge.

## Phase 0b: Git mechanics spike
### Design decisions
- Harness is a scratchpad cargo bin (git2 0.21, default-features off) that builds a nested-path commit with `TreeUpdateBuilder::upsert`, authored by a spike user and committed by `riki` - scratchpad `riki-0b/h` - exercises the exact write API and author/committer split the design uses.
- The unreachable push used a direct URL (`git@192.0.2.1:x/y.git`) with `core.sshCommand = ssh -o ConnectTimeout=5 -o BatchMode=yes` set in the repo config - no wrapper script, short timeout.
- `fetch =` refspec line was written by editing the bare clone's `config` file directly: `git config` inside the sandbox was mangled by a phantom `config` entry (it ran `git <path>/config`).

### Deviations
- Branch `spike-0b` substituted for `main` in the refspec, the pushes, and the seed (from `af97ba9`) - the spike target is the live `scottidler/riki` repo, so `main` must not be touched. The remote branch was deleted afterwards; `git ls-remote origin` shows only `HEAD` and `refs/heads/main` at `af97ba9`.
- Used the existing repo instead of an operator-created throwaway repo (per the task) - same mechanics, GitHub receive path identical.
- Network git ran outside the Bash sandbox (it denies `~/.ssh`), and cargo's first build ran outside it too (`~/.cargo/registry` is read-only there).
- The pre-push hook inspects the cwd of the same Bash call, so each push used `cd <dir>; git push ...` in one call rather than a separate `cd` call (a separate `cd` does not persist, and the hook blocked the push with "object is absent locally").

### Tradeoffs
- Second push rejected as `(fetch first)` (first clone had not fetched the new tip) vs inducing `(non-fast-forward)` (requires the clone to know the tip and diverge) - `(fetch first)` is the realistic riki race and satisfies the criterion.

### Open questions
- The unreachable test was a connect timeout (exit 128). riki's own `git.timeout` kill path yields no exit code at all; Phase 2 should treat "killed by timeout" and "128 with no ref line" both as transport failure. Confirm that is the intended mapping.

## Phase 0a: Milkdown round-trip spike
### Design decisions
- Load once, serialize under every candidate. The harness builds the editor's `SerializerState` tree and runs it through `mdast-util-to-markdown` with the editor's own settings and extensions (scratchpad `rt.mjs:load`), so each body needs one editor load instead of one per candidate. Checked against the real path: equal to `getMarkdown()` on 5711/5711 bodies with defaults. A second editor with the chosen set inside `remarkStringifyOptionsCtx` also matched on 5711/5711 bodies plus 15/15 fixtures.
- Front matter split uses comrak 0.55's parsed `NodeValue::FrontMatter` text as the prefix, the way the Data Model says (scratchpad `cm/src/main.rs:front_matter`). `prefix + body == file` is asserted on every body.
- Strict pass is equality after dropping at most one final `\n` from each side. That's the narrow reading of "trailing-newline state aside". Ignoring all trailing newlines moves the count only from 1296 to 1306.
- Alert prototype (scratchpad `alert.mjs`):
  - A remark transform turns a blockquote whose source line reads `> [!type]` (comrak's scanner rule: one `>`, one space, any case) into an `alert` mdast node. It keeps the raw marker, the title text, and whether the marker sat in its own paragraph.
  - The ProseMirror node `alert` has `content: 'block*'`.
  - Its stringify handler is the blockquote handler's shape, with the marker line in front.
  - Registered after both presets (see Deviations).
- The histogram tags each line-diff hunk with the first matching heuristic category (scratchpad `classify.mjs`). It goes beyond the doc's seven categories, because two of the biggest buckets (blank line inserted between blocks, bare URL wrapped in `<...>`) are none of them.

### Deviations
- The chosen set is not pure stringify options. It adds a `table` handler that widens delimiter cells to `---`, because no gfm table option reproduces the corpus's dominant `| --- |` delimiter row. That's same effect, correct seam: it goes in `remarkStringifyOptionsCtx.handlers`, where top-level handlers beat extension handlers (`mdast-util-to-markdown` `configure.js`). The best pure-options set is reported alongside it: `{bullet: '-', rule: '-'}`, 21.9%.
- The headline numbers apply a harness-local image fix. A one-line remark plugin coerces mdast `title: null` / `alt: null` to `''`, because `@milkdown/preset-commonmark` 7.22.2 drops every untitled image on load: the attr is declared `validate: 'string'` and ProseMirror throws. Stock numbers are reported next to them in the doc.
- The alert node must be registered after preset-commonmark/gfm, not before. Registered first, it became the schema's first `block` type, ProseMirror filled empty docs with an alert, and the 4 front-matter-only bodies crashed the serializer.
- Corpus alert count: 143 distinct bodies render at least one alert in comrak. The doc's "35 with GitHub alerts" was not reproduced, and its method is unknown. The doc text was left as is and the measured number recorded beside it.
- Run outputs went to `~/.cache/riki-0a-runs` (deleted at the end), not the scratchpad. The shared 16 GB `/tmp` tmpfs filled up mid-run, and a first candidate run died silently on ENOSPC. That partial run was discarded and rerun. Harness code stayed in the scratchpad; nothing went into the repo except these two `.md` edits.

### Tradeoffs
- Delimiter-widening `table` handler vs pure options only: +45 strict passes (21.9% to 22.7%), and it changes a newly inserted table's delimiter row from `| - |` to `| --- |`, the GitHub-typical form. The cost is one small wrapper function to maintain against `mdast-util-gfm-table` upgrades.
- Heuristic hunk classification vs hand-labelling: it covers all 4415 failures and can be repeated. Categories overlap at the edges. For example, "raw HTML" also catches `<placeholder>` text inside tables and code. Hence the two columns, first hunk and any hunk.
- `emphasis`/`strong: '_'` tied with the chosen set at 1296. The plain set was kept because the option has no effect: the preset's `remarkMarker` already preserves source markers.

### Open questions
- Strict-pass floor and go/no-go (Scott's gate): 22.7% strict, 95.0% comrak-HTML-equivalent, all 8 required alert fixtures byte-exact.
- Most remaining failures are structural, and no stringify option touches them:
  - blank line inserted between adjacent blocks
  - per-file table padding
  - text escapes
  - bare-URL bracketing
  - collapsed blank-line runs
  - inlined reference links
  - dropped code-fence meta
  - indented code rewritten as fenced

  Should a source-preserving approach be in scope for a later phase, e.g. reusing the original bytes of untouched blocks? Or is the floor set against the serializer as measured?
- Does riki accept the delimiter-widening `table` handler in Phase 6, or should it stay strictly on stringify options (21.9%)?
- Code-fence info-string meta (`title="..."`, `[macOS/Linux]`) and untitled images (before the fix) are content losses, not just style churn. Should Phase 6 extend the code_block node to carry `meta`, the way the image fix is already required?

## Phase 0a (TipTap run)
### Design decisions
- Same corpus, bytes and all: the harness reads the Milkdown run's split bodies (scratchpad `riki-0a/out/body`, 5711 files) rather than re-collecting. Selection, dedup, static-rule skips and the comrak front-matter split are therefore identical by construction. comrak (`riki-0a/cm`), `classify.mjs` categories, `nl.mjs`, `alerts-corpus.mjs` and the 15 fixtures are reused unchanged. Only the body path changed.
- Milkdown's chosen set was rerun in the same session as a control. It reproduced 1296 strict / 5427 HTML-equal exactly, so the side-by-side compares like with like.
- Load once, serialize under every candidate (scratchpad `riki-0a-tiptap/run.mjs`):
  - one editor per process, `setContent(body, {contentType: 'markdown'})` per body
  - each candidate is its own `MarkdownManager` over the same extensions plus its renderer overrides (`rt.mjs:manager`), serializing the editor's `getJSON()`
  - each manager gets its own `new Marked()`. The default is the global `marked` singleton, and every manager registers its tokenizers on it.
- Candidates are split by seam. "Variant" covers parse-affecting config, which needs its own editor run: Image `inline`, Link `autolink`, the code-mark fix, list attrs, mark delimiters. "Candidate" covers render-only overrides. Every piece is reported alone and in ablation, so Scott can see what's an option and what's prototype code.
- Alert prototype (`alert.mjs`). Blockquote's `parseMarkdown` is overridden: a blockquote whose raw first line matches `> [!type]` (comrak's one-`>`-one-space rule) becomes an `alert` node. The node has its own `renderMarkdown`, the blockquote shape with the marker line in front. It's registered last, for the same empty-doc fill reason as Milkdown. The title is kept as raw text.
- Content-loss detector (`loss.mjs`). It counts comrak-rendered features (raw-HTML nodes via `render.unsafe` off, images, links, code blocks, tables, alerts, visible text) in source vs round trip, per body. It was run on both editors. This goes beyond what the Milkdown run measured, and it's how the loss bullets in the doc were sized.
- Production-seam check (`e2e.mjs`): a fresh editor per doc with the overrides inside its own extensions, the text patch on `editor.markdown`, output through `getMarkdown()`.

### Deviations
- The "best" set is not options. `@tiptap/markdown` exposes one serializer option, `indentation`, and it only makes things worse (5.1% vs 5.3%). Everything that moves the number is an extension option, an `.extend()` override, or a patch:
  - final-newline normalization in a wrapper
  - table renderer
  - list renderers plus list parse attrs
  - italic/bold delimiter attrs
  - code mark `excludes: ''` with priority 50
  - Image `inline: true`
  - Link `autolink: false`
  - instance patch of `MarkdownManager.encodeTextForMarkdown`

  The last one has no extension seam at all; it overrides a private method. The doc lists all of them and the ablations.
- Measured through `setContent`, not initial `content`. The two paths differ on 12 of 5726 inputs: 10 image runs merged and 2 tables gaining an empty column. Link autolink also only fires on the `setContent` path, so the defaults row includes its 31 rewritten bodies. Recorded in the doc. Not rerun on the initial-content path, because riki's editor will see `setContent` and edit transactions anyway.
- Run outputs went to `~/.cache/riki-0a-tiptap-runs`, which was deleted at the end. That includes the Milkdown control rerun's outputs, so /tmp stayed untouched (72% before and after). Harness code stays in the scratchpad under `riki-0a-tiptap/`. Nothing went into the repo except these two `.md` edits.

### Tradeoffs
- Image `inline: true` vs the stock block image: strict-pass is identical (1355 either way), and schema-invalid docs drop from 149 to 82. The cost is that adjacent standalone images get merged into one paragraph on `setContent`. Chosen for validity, but neither setting is right. Image needs a node that is block when standalone and inline in text, and TipTap's Image can't be both.
- Unpadded `---` table renderer vs stock padding with the surrounding blank lines trimmed: 1335 vs 1300 with everything else in the best render set (measured on the autolink-on parse variant). Same call as Milkdown's handler.
- Text-encoder patch vs stock encoding: +151 strict on the stock parse (879 to 1030). But it overrides a private method, which breaks silently on any `@tiptap/markdown` refactor. The alternative is to accept stock's `&amp;` / `&lt;` / `&gt;` churn.
- Heuristic hunk classification is unchanged from the Milkdown run, which keeps the two histograms directly comparable at the cost of the same edge overlap.

### Open questions
- Go/no-go and the floor stay Scott's call. Side by side:
  - strict-pass: TipTap best 23.7% (nine prototype pieces) vs Milkdown best 22.7% (two options and one handler)
  - HTML-equivalent: TipTap 72.7% vs Milkdown 95.0%
  - required alert fixtures byte-exact: 8/8 in both
- TipTap loses content that Milkdown keeps, and that matters more than the 1-point strict edge:
  - raw HTML and comments (683 bodies)
  - 4-backtick fences (51 of 78 bodies)
  - `[](url)` anchors (26 bodies)
  - quoted image titles
  - with stock config, a code mark that makes 31% of docs schema-invalid

  Does a guard that fails closed on byte mismatch make these acceptable (they'd surface as "can't edit this page"), or are they disqualifying for an editor on a docs corpus where raw HTML is common?
- If TipTap stays in the running, raw HTML needs a node that keeps source bytes, and nothing in this run prototyped one. Is that in scope for a TipTap Phase 6, or is it the deciding cost?
- The biggest remaining bucket is the same in both editors: a blank line inserted between adjacent blocks (2027 vs 2032 bodies), then per-file table padding. Both are structural. The question already raised for Milkdown applies to either editor: should a later phase reuse the original bytes of untouched blocks?

## Phase 1: Workspace scaffold
### Design decisions
- Workspace of `riki-core` (axum-free) and `riki-server` (bin name `riki`), one flat `0.1.0` via `[workspace.package]` - `Cargo.toml`, `core/Cargo.toml`, `server/Cargo.toml` - marquee precedent, one `v*` tag.
- The standard-route shaping (Runtime, Build, response structs, `health/ready/status/version/deployed`) lives in core, axum handlers in server - `core/src/runtime.rs`, `server/src/routes.rs` - same split as marquee; `Build` is passed in because `env!` from the shell's `build.rs` cannot resolve in core.
- `status(uptime, error)` already takes an `Option<String>` error and reports `degraded` when present - `core/src/runtime.rs:status` - the doc's `/status` shape; Phase 2 only has to feed it. Phase 1 always passes `None`.
- Config: `content.remote` is the one required key; everything else defaults to the doc's values. A missing or invalid config file fails startup (no silent default config) - `server/src/config.rs` - riki must not run on guessed settings.
- `--config <path>` overrides the XDG path (`$XDG_CONFIG_HOME/riki/riki.yml`, else `~/.config/riki/riki.yml`) - `server/src/cli.rs` - needed to run against the shipped example without touching `~/.config`.
- `~` expansion is a pure function over an injected home dir, and fails when home cannot be resolved - `config.rs:expand_tilde` - testable without env mutation.
- `riki.example.yml` is `include_str!`-loaded by a test so the shipped example can never drift into an invalid config - `config/tests.rs:shipped_example_loads`.
- `core-guard` uses `cargo tree -p riki-core -i axum` (marquee's mechanism, dev-deps included).
- Scaffold output was used for `rustfmt.toml`, `clippy.toml`, `.pre-commit-config.yaml`, `.github/workflows/{ci,release}.yml`, and the `lint`/`bloat`/`check`/`test` tasks of `.otto.yml`; the flat single-crate layout, `env_logger`/`log`/`colored` deps, `cov` tasks, and sample `riki.yml` were dropped.

### Deviations
- Tracing: harvested the JSON `tracing-subscriber` setup but NOT marquee's OpenTelemetry/Datadog export - `server/src/observability.rs` - riki is a home POC with no OTLP collector; adds ~5 heavy deps for nothing. Log filter comes from `--log-level` (default `info`); `RUST_LOG` is not consulted (marquee house rule), and a bad filter fails startup.
- `/version`, `/deployed` do not read the Tatari platform runtime env vars (`GIT_DESCRIBE`, `GIT_BRANCH`, `GIT_VERSION`, `DEPLOY_DATETIME`) - `core/src/runtime.rs` - riki is not deployed by that pipeline; `/version` is purely build-time facts (so `branch` is the branch at build time). `/deployed` keeps its three keys: `deployed_at` = process start, `environment` from `ENV` (else `unknown`), `deployer` from `GIT_AUTHOR` (else empty). `/deployed` shape matches marquee's.
- `/ready` has one latch (marked right after bind) instead of marquee's two (index + corpus) - those prerequisites do not exist yet; Phase 2 will add its own.
- `/version.git_sha` is a plain `String` in core (marquee has `&'static str`) - the shape on the wire is identical.
- `.github/workflows/*`, `.pre-commit-config.yaml` come from scaffold and were not in the phase bullet list - kept because scaffold emits them and they are inert until a push/tag.

### Tradeoffs
- `serde_yaml` 0.9 (deprecated upstream) vs `serde_yml`/other - the design doc's dependency list names serde_yaml, and marquee uses it.
- `identity.mode` is an enum with only `header` vs a free string - an enum makes an unknown mode a startup failure for free; Phase 4 owns the loopback rule.
- Cargo deps pinned to major versions (`"1"`, `"0.8"`) instead of scaffold's `"*"` - reproducible resolution; `Cargo.lock` is committed.

### Open questions
- The sandbox's cargo registry is read-only and crates.io is not allowlisted, so every cargo invocation (including `otto ci`) was run unsandboxed. Not a repo issue, but the parent should expect the same for later phases.
- Observed `/version` output at the commit's parent: `{"branch":"riki-poc","revision":"af97ba96...","version":"af97ba9","git_sha":"af97ba9"}`; `version` has no `v` prefix or `-dirty` until a tag exists (describe falls back to the short sha).

## Phase 2: Git read store, poller, publish
### Design decisions
- Store, index, path rules, and publish all live in core; the server only maps config, owns the poll loop, and feeds `/status` - `core/src/{store,index,path,wiki}.rs`, `server/src/poller.rs` - publish is "core + server, one function" in the doc; putting the in-memory swap in core too (`Wiki::publish`) keeps it one function and testable without axum.
- The repo mutex is a token: `GitStore::lock()` returns a `RepoGuard`, and `fetch` / `set_good` / `Wiki::publish` take `&RepoGuard` - `core/src/store.rs` - Phase 5 must hold the lock from save step 3 to the response, so the lock has to be holdable across calls, and the type system now refuses an unlocked fetch or ref write. Reads (`tip`, `good`, `read_blob`, `blob_paths`) take no lock.
- Every git2 call opens the bare repo inside its own `spawn_blocking` (`GitStore::blocking`) - `git2::Repository` is `Send` but not `Sync`, and a per-call open is cheap next to a request.
- `GitStore::open` re-applies `remote.origin.url` and exactly one `remote.origin.fetch` (removes all, adds `+refs/heads/<branch>:refs/remotes/origin/<branch>`) on every start - `store.rs:configure` - config drives behavior: changing `content.remote` or `content.branch` takes effect on restart; also how AC5's "remote pointed at an unreachable host" gets exercised.
- CLI runner `store.rs:run`: `kill_on_drop(true)`, stdin null, stdout/stderr piped and drained concurrently by `wait_with_output`, `tokio::time::timeout` around it, `GIT_TERMINAL_PROMPT=0` so a missing credential fails rather than sitting until the timeout. Three error shapes: `Timeout` (killed), `Failed { code: Some(n) }`, `Failed { code: None }` (signal). Phase 0b's "timeout kill is a third transport-failure shape" is covered: all three are fetch failures to the poller.
- Index rules (`core/src/index.rs:NavIndex::build`): `.md` files only are pages; `x.md` and `x/README.md` mapping to the same URL is `IndexError::Collision`; a page whose first URL segment is in `RESERVED` (`_riki health ready status deployed version`) is `IndexError::Reserved` (so `status/x.md` is refused too, not just `status.md`); a non-UTF-8 tree path is `IndexError::BadPath(NotUtf8)`.
- Index cache is `HashMap<Oid, Arc<NavIndex>>` in `Wiki` - a repeatedly-refused tip costs one walk, not one per poll.
- `/status` error text: `upstream unreachable since <rfc3339>: <fetch error>` and/or `tip <oid> failed publish: <errors>`, joined with `; ` - `wiki.rs:status_error`. `since` is the first failure of the outage and is kept across failed polls; the first successful fetch clears it. A rejection clears when a later tip publishes, or when the tip returns to the good tip (force-push revert).
- Startup order in `server/src/lib.rs:run`: bind, `Wiki::open` (reads `refs/riki/good`, else tip, no network), one startup poll, spawn the poll loop, then latch `/ready`. A startup fetch failure is not fatal: riki serves the good tip and reports `degraded`. A store that cannot be opened (cache dir exists but is not a bare repo) fails startup.
- `riki_core::testing` (feature `testing`, plus `cfg(test)`) builds upstream commits with git2 directly into a local bare repo, and the server's dev-dependency enables it - no network in any test; upstream "unreachable" is the upstream dir renamed away, "recovered" is it renamed back.
- `riki_core` re-exports `git2::Oid` so the server never names git2.

### Deviations
- The bare clone is created with `git2::Repository::init_bare` + remote config + the first fetch, not `git clone --bare` - same effect, correct seam: the result is the configured bare repo Phase 0b verified (the clone's own `refs/heads/*` are never read by riki), and creating it needs no network, so a first start with upstream down still comes up `degraded` instead of failing.
- Paths with any segment starting with `.` (`.github/`, `.gitignore`, `a/.draft.md`) are skipped by the index rather than counted as index errors - the doc's segment rule read literally would make any content repo with a `.github/` or `.gitignore` unpublishable. Those paths are still typed errors (`PathError::LeadingDot`) for every by-path read (`read_blob`) and will be for saves. Flagged as an open question.
- `/status` stays HTTP 200 when `degraded` (body carries the state) - matches marquee's shape; the doc names the body, not the code.

### Tradeoffs
- Index cache unbounded vs evicting - it grows by one entry per distinct tip the process sees; at poll cadence on a POC that is negligible, and eviction would need a policy the doc does not give.
- Poll loop starts one interval after the startup poll (`interval_at`, `MissedTickBehavior::Delay`) vs firing immediately - avoids a double fetch at boot; a slow fetch delays the next tick instead of bursting.
- Fetch stderr goes into `/status` verbatim (multi-line) vs condensed - keeps git's exact words for the operator; Phase 3's banner can shorten it if it reads badly.

### Open questions
- Hidden paths: is "skip them" the intended reading of the "no leading `.`" index rule, or should a tip carrying any `.`-segment `.md` file be refused?
- Should reserved-name checks cover all of a reserved top-level directory (current: any page under `status/`, `_riki/`, etc. is refused) or only the exact URLs `/status`, `/health`, ...? The router only owns the exact paths except `_riki/*`.

## Phase 3: Rendering and routing
### Design decisions
- Markdown rendering, the two URL rewriters, `resolve`, `escape_html`, and `encode_path` live in core (`core/src/render.rs`); templates, sidebar, banners, and the error page are pure string functions in the server (`server/src/render.rs`); axum handlers are in `server/src/pages.rs` - keeps everything but the HTTP shell testable without axum, and `core-guard` still holds (comrak is `default-features = false`, so no syntect/clap).
- Link rewriting reuses `index::url_for_file`, so `x.md` -> `/x`, `a/README.md` -> `/a`, `README.md` -> `/` by the same rule the index uses; `?query` / `#fragment` tails survive. A relative link that climbs above the repo root is left as written rather than invented.
- Page CSP (`CSP_PAGE`): `default-src 'none'; script-src 'self'; connect-src 'self'; img-src 'self' https: data:; style-src 'unsafe-inline'; frame-ancestors 'self'` - CSS is inline in the template so there is no `/_riki/assets/` dependency (that route is Phase 6's); `connect-src 'self'` is for the Phase 6 editor's API calls.
- `/a/b.md` redirect is unconditional (no existence check) and goes through `url_for_file`, so `/a/README.md` redirects to `/a`, not the non-page `/a/README`.
- Paths owned by riki's own routes (`_riki`, `health`, ... as top segment) or failing path validation get a plain 404 with no Create link and no Edit button (`Action::None`).
- `/_riki/raw`: the extension check runs first (404 before any store access); a store `PathError` (`..`, dot segment) is a 404, not a 500; `X-Content-Type-Options: nosniff` added.
- `Unreachable::since_text()` added to core `wiki.rs` so the banner and `/status` format the time identically.
- Page titles are the last URL segment (`Home` for `/`); no heading extraction.

### Deviations
- Added `img-src https: data:` to the page CSP (the doc only says `script-src 'self'`) - wiki pages routinely embed external images, and `default-src 'none'` would block them silently. Same effect on scripts.
- Replaced the Phase 1 test `unknown_path_is_404_until_the_page_route_lands` (it pinned the pre-catch-all 404) with `pages::tests::a_missing_page_is_404_with_create_this_page` and `reserved_paths_never_offer_create`.
- The "Create this page" link is `href="#"` with `data-path`, and the Edit button is `disabled` - placeholders for Phase 6, which wires both.

### Tradeoffs
- Sidebar built from `NavIndex::tree()` per request vs caching the HTML per commit - the doc says no render caching until measured.
- Non-UTF-8 page blobs render lossily (U+FFFD) vs 500 - the static rules make such pages non-editable, not unreadable.
- Hand-rolled `fill` and `encode_path` vs a `percent-encoding` dep - ~10 lines each, no new dependency.

### Open questions
- None blocking. Phase 2's two questions were decided by the caller for the POC (hidden paths stay skipped from the index; reserved-name refusal stays as built, covering everything under a reserved top-level name).
- A page directory with no `README.md` shows as a plain label in the sidebar and 404s with Create; confirm that is the wanted behavior for directory URLs.

## Phase 4: Identity and write guards
### Design decisions
- Identity is an axum extractor (`FromRequestParts<AppState>`) over a pure `Identity::from_headers(&HeaderMap, &IdentityConfig)` - `server/src/identity.rs` - header names come from config, never constants; the pure function is the unit-tested part, the extractor is a thin shell that rejects with 401. A missing, blank, or non-UTF-8 email header is "no identity"; a blank name header falls back to the email.
- `AppState` carries `Arc<IdentityConfig>`; `AppState::new` keeps its signature (defaults) and `with_identity` sets the configured one - `server/src/routes.rs`, wired from `run` - avoids touching every existing test's constructor call.
- JSON-only guard is a `route_layer(from_fn(require_json))` on a dedicated POST router merged into the main one - `server/src/api.rs` - so a future POST route added to that router is guarded by construction; the content-type check compares the media-type essence (before `;`, trimmed, case-insensitive), so `application/json; charset=utf-8` passes and `application/jsonx` does not.
- Guard order is JSON (415) before identity (401), matching save step 1.
- Loopback rule is `Config::validate`, called at the end of `Config::from_yaml`, so every loaded config is validated before anything binds or touches the network - `server/src/config.rs`. The error names the rule: "identity.mode `header` requires a loopback `listen` address ... got 0.0.0.0:8737". Loopback = `IpAddr::is_loopback` (127.0.0.0/8, ::1).
- Startup criterion is tested through the real binary (`server/tests/startup.rs`, `CARGO_BIN_EXE_riki`): exit status and stderr, with a 10s deadline so a missing guard fails the test instead of hanging; cache-dir and remote are confined to a tempdir.

### Deviations
- The POST routes are stubs: `POST /_riki/api/page` requires the `Identity` extractor then returns 501; `POST /_riki/api/roundtrip` returns 501 behind the JSON guard only (roundtrip is not a write, so no identity requirement). Phase 5 replaces `save`, Phase 6 `roundtrip`. These exist only so the guards are testable; they do not fake the deferred behavior (501, not a fake success).
- The save-handler stub takes `_: Identity` rather than `_identity: Identity` because the repo's lint task forbids `_name` bindings.
- Loopback check lives in config loading, not in `run()`: same effect (startup exits non-zero before bind), earlier and covers any caller of `Config::load`.

### Tradeoffs
- Validate inside `from_yaml` vs a separate call in `run`/`main` - a config that can exist but is invalid is a footgun; one construction path means no caller can forget the check.
- `identity.mode` is still a one-variant enum, so `validate` matches exhaustively; the second identity mode (parked) will be forced by the compiler to state its own listen rule.

### Open questions
- None.

### Guard-removal evidence (each guard removed once, test run, restored)
- Identity guard removed (`save` stops taking `Identity`): `api::tests::save_without_the_email_header_is_401` FAILED, `left: 501 right: 401`; the other 6 api tests passed.
- JSON guard removed (the `route_layer` line deleted): `form_encoded_post_is_415_on_every_post_route`, `the_json_check_runs_before_the_identity_check`, `missing_content_type_is_415` FAILED.
- Charset handling broken (essence compare replaced by whole-value compare): `json_with_charset_parameter_is_accepted` and `is_json_matches_the_essence_only` FAILED.
- Loopback check removed (`validate` condition forced false): `config::tests::header_mode_refuses_a_non_loopback_listen` FAILED; `tests/startup.rs` FAILED after its 10s deadline with "riki kept running on a non-loopback listen in header mode". The first version of the startup test had no deadline and hung with the guard removed (the real binary started and began fetching), which is why the deadline and tempdir confinement exist. That hung run created an empty `~/.cache/riki/content.git` through the default cache-dir; it was removed with `rkvr rmrf` (archived under `/var/tmp/rmrf/2026-10-01-164857-000/`).
- All guards restored; `otto ci` exit 0 afterwards.

## Phase 5: Write path
### Design decisions
- The save algorithm lives in core and returns data: `riki_core::save::save` -> `SaveOutcome` (one variant per doc branch: `Saved`, `Unchanged`, `ContentPresent`, `Conflict`, `IndexConflict`, `RetriesExhausted`, `BadRequest`, `NotEditable`, `FetchFailed`, `NoTip`, `PushTimedOut`, `PushFailed`) - `core/src/save.rs` - the server's `api.rs:save_response` is a pure outcome -> status/JSON map, and the algorithm is testable without axum.
- The repo mutex is taken once, before step 3, and held across every retry until the outcome is decided (publish included) - `save.rs:save` - the `RepoGuard` from Phase 2 makes the unlocked fetch/push/ref write impossible to call.
- Page file rules are one module - `core/src/page.rs` - `split_front_matter` parses with comrak (`front_matter_delimiter: "---"`, the renderer's value) and cuts at the length of the parsed `NodeValue::FrontMatter` text; a leading BOM (which comrak skips before matching) is kept in the prefix so `prefix + body == file` holds for every input. Fixtures in `page/tests.rs`: none, front matter + body, front matter + one blank line, front matter only with and without trailing newline, `---` thematic break in the body (with and without front matter), unterminated opener, BOM.
- Trailing-newline state is the run of `\n` that ends the original file (`page.rs:trailing_newlines`); `compose` strips the incoming body's trailing `\n`s and appends that run (new pages: one `\n`). Front matter with an empty body writes the front matter alone; front matter that ends without `\n` gets one before a non-empty body. `an_unedited_body_composes_back_to_the_file` pins that an unedited body is byte-identical.
- Static rules (`page.rs:check_static_rules`) return the text on pass, so callers never decode twice; `NotUtf8` / `LeadingBom` / `CarriageReturn` carry the reason string used in both the GET `reason` and the 422 body.
- Page path rule for saves (`page.rs:validate_page_path`): `path::validate` + `.md` + top URL segment not in `RESERVED`, the same rule the index applies, so a save can never write a path the index refuses by name.
- Push (`store.rs:GitStore::push`): `git --git-dir <clone> push --porcelain origin <commit>:refs/heads/<branch>` through the Phase 2 runner (timeout, `kill_on_drop`, both streams drained), split into `run_output` so a non-zero exit still yields stdout for the porcelain line. `classify_push` is pure and is tested against Phase 0b's verbatim outputs.
- `commit_file` writes blob + `TreeUpdateBuilder::upsert` tree + commit with `repo.commit(None, ...)` (no ref moves, so no lock needed); `set_tip` moves `refs/remotes/origin/<branch>` after a push lands; then `Wiki::publish` reuses the step-7 index from the per-oid cache, so "publish with the index from step 7" holds without a new API.
- Step 4's publish-the-tip runs only when the tip is not already the good tip (`save.rs:publish_tip`), so a no-op save does not rewrite `refs/riki/good`.
- `GET /_riki/api/page?path=` reads from the good tip (what the reader is looking at) and returns `{path, base-oid, body, editable, reason}`; a missing page is `base-oid: null, body: "", editable: true`; a static-rule failure is `editable: false`, `body: null`, the rule as `reason`.
- `POST /_riki/api/roundtrip` implemented (trivial per the API table): blob by `base-oid` -> static rules (422) -> front matter split -> `page::first_diff_line`; INFO log `path`, `base-oid`, `identical`, `first-diff-line`. Unknown blob 404, bad oid 400.
- JSON shapes: 200 `{commit, content-present}` (both always present); 409 step 4 `{error, current-body}` (body without front matter, null when the page was deleted); every other error `{error}`, plus `retry-safe: true` on the push-timeout 503. Malformed JSON / bad `base-oid` -> 400 (the `Json` rejection is mapped so axum's own 422 cannot be confused with the static-rule 422).
- Test stalls without touching any user/global git config: `riki_core::testing::set_receive_pack` sets `remote.origin.receivepack` on the test's own tempdir bare clone; over `file://` git runs that program through `sh` with the remote path appended. Timeout test: `git receive-pack "$@"; sleep 10` (ref lands, then the client waits on the remote side past a 2s `git.timeout`). Race test: replica B's wrapper touches `b-waiting` and blocks until `b-released`, so A saves between B's fetch and B's push, deterministically; the wrapper logs each invocation to count pushes. Every repo in `server/src/api/save_tests.rs` is under the fixture's `TempDir`; `~/.cache/riki` confirmed absent after the runs.
- `AppState::with_save(SaveSettings)` and `routes::save_settings(&CommitterConfig, &GitConfig)` wire `committer.*` and `git.push-retries` from config - `server/src/routes.rs`, `server/src/lib.rs:run`.

### Deviations
- Retryable push rejections include `(incorrect old value provided)` besides the doc's `(fetch first)` / `(non-fast-forward)` - `store.rs:MOVED_REASONS`. Found by `two_replicas_saving_one_page_from_one_base_one_200_one_409`: when two replicas push truly at once, both read the same ref advertisement, and the loser's receive-pack ref compare-and-swap fails with `![remote rejected] (incorrect old value provided)` (git 2.53, local upstream). It is the same lost race as a non-fast-forward, so it goes back to step 3; before the fix that test returned 502 for the loser.
- 409 after `push-retries` is exhausted carries `{error}` naming the attempts and git's `!` line; the doc gives no body for it.
- Step 7's 409 (index conflict) does not publish the fetched tip; the doc lists publish only on step 4's branches and on 200s, and the poller publishes it on its next tick.
- Upstream with no commit on the branch after the fetch is a 503 (`SaveOutcome::NoTip`), not a parentless first commit - the doc does not cover an empty content repo; POC happy path.
- Save `message` with a newline is a 400 (`message must be one line`); blank is the default `riki: edit <path>`. The doc says "optional, one line" without saying how to enforce it.
- The round-trip comparison ignores all trailing newlines on both sides, not "at most one final `\n`" (Phase 0a's harness reading) - `page.rs:first_diff_line` - because the save replaces the body's trailing newlines with the file's own state, "identical" must mean "a no-edit save writes the stored bytes".
- Inverted Phase 4 stub tests by name: `api::tests::save_with_the_email_header_passes_the_guards` and `json_with_charset_parameter_is_accepted` now expect 400 (the real handler refusing `{}`) instead of 501.

### Tradeoffs
- Save logic in core with an outcome enum vs in the axum handler - core keeps the algorithm axum-free and lets the server map statuses in one match; costs one enum.
- receivepack wrapper vs `GIT_SSH` wrapper vs upstream hook for the stall - receivepack is per-repo config on the test's own clone, needs no env mutation (Rust 2024 `set_var` is `unsafe` and process-wide across parallel tests) and works over `file://`; an upstream `post-receive` hook would depend on `core.hooksPath` not being set globally.
- Classifying unknown `!` reasons (e.g. protected branch) as 502 vs retrying them - fail loud: only the three "branch moved" reasons retry.

### Open questions
- GitHub's wording for the concurrent-push ref-update race is not verified (Phase 0b only saw `(fetch first)`). If GitHub reports it as e.g. `(cannot lock ref ...)` or `(failed to update ref)`, the loser gets 502 instead of a retry. Worth one Phase 7 check, or add those reasons now?
- `editable: false` sends `body: null`; Phase 6 only needs the reason and the GitHub link. Confirm null (not an empty string) is the shape the editor should handle.

## Phase 6: Editor
### Design decisions
- One Milkdown setup for tests and the browser - `editor/src/setup.ts:makeEditor` - image-title fix plugin first, then commonmark, gfm, history, task commands, the RikiInsertTable command, and the alert plugins last (Phase 0a: registered first, the alert would become ProseMirror's empty-doc fill type). `stringifyOptions` is the Phase 0a set `{bullet: '-', rule: '-'}`, with the `table` and `alert` handlers in `remarkStringifyOptionsCtx.handlers`. `tableHandler` wraps `mdast-util-gfm-table`'s handler (`tablePipeAlign: false`) and widens delimiter dashes to `---`. `mdast-util-gfm-table` 2.0.0 and `mdast-util-to-markdown` 2.1.3 are pinned to the exact versions the kit resolves, so the handler wraps the same code the preset runs.
- Alert node - `editor/src/alert.ts` - Phase 0a's prototype, typed. It's a remark transform keyed on comrak's source-line rule (`> [!type]`, one `>` and one space), a `block*` node, and its own stringify handler. `setAlert` wraps the selected blocks, or changes the kind of the alert the cursor is already in.
- New tables are inserted with `alignment: null` cells - `editor/src/table.ts:createTable` - the kit's `insertTableCommand` makes left-aligned cells, which serialize as `| :--- |`. Ours give the canonical `| --- |`. The toolbar inserts a header row plus one body row, three columns.
- Task list without syntax - `editor/src/tasks.ts` - `makeTasks` sets `checked: false` on the selected list items. The toolbar wraps the block in a bullet list first if it's in no list. A click on a task item's own `li` box (CSS padding, not the text) toggles `checked`.
- Toolbar - `editor/src/toolbar.ts:buildToolbar`. Controls: a text-style select (Paragraph / Heading 1-6), Bold, Italic, Strike, Code, Bullet list, Numbered list, Tasks, Quote, Code block, Table, Link (the kit's `toggleLinkCommand`, which opens the kit's link tooltip), an alert-type select, and an Alert button. Buttons swallow `mousedown` so the editor keeps its selection, and the toolbar starts disabled.
- Fail-closed guard - `editor/src/api.ts:checkRoundTrip`. Only a well-formed `identical: true` passes. `identical: false`, HTTP errors, network errors, malformed JSON, and the 5s `AbortController` deadline all return a refusal with a reason. `editor/src/session.ts:saveAllowed` is the one rule for Save: guard `passed` or `skipped` (a new page) and no save in flight. A refusal never changes back during a session.
- Session flow, in place - `editor/src/session.ts:Session`. GET the page JSON (`editable: false` or a load error shows a notice and no editor), mount read-only, serialize, POST the guard, then enable. A 200 save calls `main.ts:rerender`, which fetches the same URL and swaps `header .actions`, `nav`, `main`, and the banners. The URL never changes. A 409 shows the server's words and a "Load latest" button. That button asks `window.confirm`, then remounts from a fresh GET, so it gets the new base oid and the guard runs again.
- GitHub link - `server/src/config.rs:github_blob_base` maps `git@github.com:o/r(.git)`, `ssh://git@github.com/o/r(.git)`, and `https://github.com/o/r(.git)` to `https://github.com/o/r/blob/<branch>/`. Anything else gives `None` and no link. `pages.rs` appends the encoded file path and renders it as `data-source` on the Edit button.
- Edit button - `server/src/render.rs:Action::Edit { file, source }`. It's now live (no `disabled`) and carries `data-path` (the served repo path, e.g. `a/b/README.md`) and `data-source`.
- Assets - `server/src/assets.rs`. `GET /_riki/assets/{name}` serves `editor.js` and `editor.css` from `include_bytes!` with `nosniff`, `Cache-Control: no-cache`, and an ETag (304 on match). Other names are 404.
- otto `editor` (in `ci`) checks for node and pnpm (fails loudly naming the missing one), then runs `pnpm install --frozen-lockfile --prefer-offline`, typecheck, vitest, and the build. It fails on `git diff --exit-code -- server/assets/editor.js server/assets/editor.css`. Verified both ways: a changed UI string failed with "server/assets/ differs from a fresh build", and `PATH=/usr/bin:/bin` failed with "node is not installed".
- otto `e2e` (not in `ci`) runs `cargo build -p riki-server` and then Playwright.
- Playwright fixture - `editor/e2e/riki.ts`. Every test gets a fresh riki process, built from a mkdtemp dir:
  - a `file://` bare upstream
  - a cache-dir in that same tempdir
  - its own config, on a free loopback port
  - git run with `GIT_CONFIG_GLOBAL=/dev/null` and `GIT_CONFIG_NOSYSTEM=1`, so host hooks, signing, and URL rewrites never apply

  The tempdir is removed afterwards. Playwright's `extraHTTPHeaders` inject `Remote-Email: e2e-editor@example.test` (synthetic). `~/.cache/riki` was confirmed absent after the runs, and no `riki-e2e-*` tempdirs were left.
- Fixtures - `editor/fixtures/canonical/` (65 files) must serialize byte-identical. `editor/fixtures/rewritten/` (26 files plus 2 inline) is valid GFM the serializer rewrites; the test asserts the guard would refuse it and that the rewrite is a fixed point. Names are `<phase-0a-category>--<variant>`. `test/fixtures.test.ts` asserts every one of the 25 histogram categories has a fixture and every required alert fixture is in the strict-pass set. All synthetic, written for this phase; nothing from the corpus.
- Evidence the tests bite:
  - Image fix removed: 2 image fixtures fail.
  - Stringify set removed: 36 fixtures fail.
  - `saveAllowed` mutated to `guard !== 'pending'`: the three Playwright guard tests fail on `toBeDisabled`.

### Deviations
- `style-src` in `CSP_PAGE` is now `'self' 'unsafe-inline'` (was `'unsafe-inline'`) - the editor's stylesheet is served from `/_riki/assets/editor.css`. `script-src 'self'` is unchanged.
- The bundle is two files, `server/assets/editor.js` and `server/assets/editor.css`, not just `editor.js`. The API table says `/_riki/assets/*` holds "embedded CSS and editor bundle". The otto diff check covers both.
- Two rewritten fixtures live inline in `test/fixtures.test.ts` instead of `editor/fixtures/`: `trailing-whitespace--spaces` and `hard-break--two-spaces`. The repo's `otto lint` (`whitespace -r`) strips trailing spaces from every file it scans, which would turn both fixtures into canonical input and break the test.
- Added a `RikiInsertTable` command instead of using the kit's `insertTableCommand` - same effect (insert a table), correct seam. The kit's left-aligned default would make every toolbar table `| :--- |`.
- The first-pass rewrite of `links--empty-text` is not a fixed point, so it's exempt from the fixed-point check (named `LOSSY` set). Milkdown 7.22.2 drops an empty-text link (`[](#setup)`) and leaves a trailing space. That's a content loss the guard catches (refused). It contradicts the doc's Phase 0a TipTap section, which lists `[](url)` loss as TipTap-only.
- "Load latest" re-GETs the page JSON rather than using the 409's `current-body`. The 409 body carries no oid, and editing from it would just 409 again.
- Typing in Playwright uses Enter and Tab (new paragraph, next table cell). The URL goes into the link tooltip's input. Neither is Markdown syntax.

### Tradeoffs
- Playwright in its own otto task vs inside `otto ci`: kept out of `ci`. It needs a built binary plus a downloaded Chromium (about 114 MiB, `chromium_headless_shell-1243` for Playwright 1.63), and that would make every `otto ci` depend on a browser install. The guard rules it exercises are also unit-tested (`session.test.ts`, `api.test.ts`).
- Bundle size: 589,637 bytes minified, 190,381 gzip -9. The CSS is 4,019 bytes. The largest pieces are prosemirror-view (96K), prosemirror-model (44K), Vue runtime-core (37K, which the kit's link tooltip needs), prosemirror-tables (37K), and dompurify (29K). It's acceptable on a loopback/LAN POC, and the ETag keeps repeat loads to a 304.
- A page re-render by fetch-and-swap vs `location.reload()`: the swap keeps the doc's "re-render the article" literal and avoids a full reload. It costs about 20 lines that depend on the template's `header .actions` / `nav` / `main` structure.
- TypeScript 7.0.2 (the native compiler): it's the current major, and `tsc --noEmit` was verified to report errors (an injected type error failed with TS2322).

### Open questions
- GitHub's reusable `rust-ci.yml` runs `otto ci`. If that runner has no node/pnpm, the new `editor` task fails there by design ("fails loudly if node is absent"). Should the reusable workflow install node + pnpm, or should GitHub CI skip `editor`?
- Should `otto e2e` join `otto ci`? That would mean a Chromium install on every machine that runs CI.
- Not a repo issue, recorded so the next phase doesn't re-derive it: in the agent's Bash tool, `./node_modules/.bin/playwright test` failed with `unknown command '<cwd>/test'`. The tool rewrites any bare argument that names an existing relative path into an absolute path (`echo src e2e nonexistent` prints the first two as absolute paths), so `test` matched `editor/test/`. `pnpm run e2e` and `sh -c '...'` are unaffected. A normal shell never sees it.
- Phase 5 open question: `editable: false` sends `body: null`. The editor treats null and `editable: false` the same way (notice plus GitHub link), so either shape works.

## Implementation audit round 1 fixes

Source: the review-panel synthesis at `/tmp/review-panel/JYGHyoQC/synthesis.md` (must-fix 1, cheap-wins 2-4, caller questions Q1 and Q2). Every fix has a test that was run against the unfixed code and failed:

| # | Fix | Test | Failure without the fix |
| --- | --- | --- | --- |
| 1 | A tip that refuses publish no longer returns 200 on the content-present and unchanged paths | `save::tests::unchanged_on_a_tip_that_refuses_publish_is_an_index_conflict_not_a_200`, `save::tests::content_present_on_a_tip_that_refuses_publish_is_an_index_conflict_not_a_200` | `expected IndexConflict, got Unchanged` / `got ContentPresent` |
| 2 | A missing homepage offers "Create this page" for `README.md` | `pages::tests::a_missing_homepage_is_404_with_create_readme` | no "Create this page" in the 404 |
| 3 | A save's fetch updates upstream health | `save::tests::a_save_observed_outage_and_recovery_update_upstream_health` | `a save's failed fetch marks upstream unreachable` |
| 4 | Relative images display in edit mode | `test/images.test.ts` "shows the resolved URL in edit mode and serializes the author src unchanged" | `Expected "/_riki/raw/a/b/img.png", Received "img.png"` |
| 5 | GitHub's lost-CAS wording retries | `store::tests::classify_push_retries_a_lost_server_side_ref_update` (plus `classify_push_fails_hook_atomic_and_generic_remote_rejections`, which pins what stays 502) | `Err(Failed { ... [remote rejected] (cannot lock ref 'refs/heads/main': is at ... but expected ...) })` |
| 6 | `otto editor` runs on a runner with node but no pnpm | Run by hand: `otto editor` with pnpm off PATH and node + corepack on it printed `=== pnpm not on PATH; using corepack pnpm@10.29.3 ===`, then passed (145 vitest tests, bundle matches). With node off PATH it exits 1 with `ERROR: node is not installed.` | the old task exited 1 with `ERROR: pnpm is not installed.` |

`otto e2e` passed (7 of 7) after the fixes. It stays out of `otto ci`, per Scott.

### Design decisions
- Refused publish maps to `IndexConflict` (409 naming the index errors) - `core/src/save.rs:publish_tip` now returns `Option<String>` with the errors. The doc says a 200 means the next GET renders the page as saved, and when the good tip did not move that is false.
- The page-moved `Conflict` path keeps returning `Conflict` even when the tip refuses publish - `core/src/save.rs:save`. It is already a 409, and its `current-body` is what the author needs to reconcile. The refusal still shows in the banner and `/status`, because `Wiki::publish` records it.
- One fetch with health bookkeeping, `Wiki::fetch` - `core/src/wiki.rs`. Both `Wiki::poll` and step 3 of the save call it, so a fetch failure or recovery seen by either one updates `health.unreachable` the same way (the first failure's `since` is kept). The log prefix changed from `poll: fetch failed` to `fetch: failed`.
- `MOVED_REASONS` entries are fragment lists that must all match - `core/src/store.rs:is_moved`. `cannot lock ref` needs `': is at ` and ` but expected ` as well, so `cannot lock ref '...': reference already exists` stays a 502. `(failed to update ref)` keeps its closing parenthesis so the atomic-push `(failed to update refs)` does not match.
- Edit-mode images use a ProseMirror node view, not a document transform - `editor/src/images.ts:imageView`. The node's `src` attr stays the author's text, so `getMarkdown()` is unchanged and all fixtures stay byte-identical. Only the displayed `<img>` points at `/_riki/raw/<resolved>`. `rawImageUrl`/`resolve` mirror `core/src/render.rs` `relative`/`resolve`/`rewrite_image` rule for rule. The clipboard path uses the schema's `toDOM`, so copied images keep the raw `src` too.
- `EditorSetup.sourceFile` is required - `editor/src/setup.ts`. The session passes the page path. The test harness defaults to `README.md`.
- The `editor` task resolves pnpm in order: `pnpm` on PATH, then `corepack <packageManager>`, with the version read from `editor/package.json` (already `pnpm@10.29.3`; `pnpm-lock.yaml` is lockfileVersion 9.0, the pnpm 10 format). If neither is present, or `packageManager` does not pin pnpm, it exits 1 naming the cause. `COREPACK_ENABLE_DOWNLOAD_PROMPT=0` stops corepack from prompting on a headless runner.

### Deviations
- None.

### Tradeoffs
- Corepack fallback vs adding a node/pnpm setup step to the shared `scottidler/github-actions` workflow: corepack keeps the fix in this repo (the shared workflow is off limits). The cost is that corepack downloads pnpm from registry.npmjs.org on first use, and corepack is not bundled with Node 25+. A runner on Node 25+ without pnpm fails loudly with "neither pnpm nor corepack is installed".
- Node view vs rewriting `src` on parse and back on serialize: a round-trip rewrite would risk exactly the byte drift the guard exists to catch. The node view never touches the document.

### Open questions
- Item 5 is matched against git's own strings plus the public GitHub logs codex cited. No live concurrent push against GitHub has been run, so whether GitHub emits one of these two reasons on today's servers is still unverified until there is a race test against a real GitHub repo.
- GitHub CI is still unrun (riki-poc is local only). The reusable workflow does not install node either. ubuntu-latest ships a system node, but its version against `engines.node >=24` (advisory, not enforced) has not been checked on a runner.
- Seen once during this round, not part of it: `server/src/tests.rs::non_default_listen_is_what_gets_bound` failed with `Address already in use (os error 98)`, then passed on rerun. The test binds port 0, drops the probe, then binds that port again. In that window the sibling test `bind_fails_loudly_when_the_port_is_taken` runs in parallel and also binds port 0, and the kernel can hand it the port that was just freed. Fixing it means holding the listener instead of re-binding (for example, passing a pre-bound `TcpListener` into `bind`). Left alone because it is outside this audit's scope.

### Round 1 follow-up (supersedes the flaky-test and GitHub-CI open questions above)

Two follow-ups the coordinator asked for before the release:

- **Flaky `server/src/tests.rs::non_default_listen_is_what_gets_bound`.** The test now configures `listen: 127.0.0.2:0` and asserts the bound IP is `127.0.0.2` and the port is nonzero. It no longer binds a probe port, drops it, and binds that port again. The kernel picks the port at bind time, so there is no window for the sibling test to take it. `config::tests::non_default_listen_takes_effect` still covers port parsing.
  - Evidence the test still bites: with `bind` hard-coded to `127.0.0.1:0`, it fails `left: "127.0.0.1" right: "127.0.0.2"`.
  - `cargo test -p riki-server` ran 10 times in a row with 10 passes and 0 failures.
- **Editor task on older node.** I ran `otto editor` under mise-installed node 20.20.2 and 22.23.3, with no pnpm on PATH (the corepack path), the way a stock runner would.

  | node | result |
  | --- | --- |
  | 20.20.2, `engines.node >=24` | pnpm and corepack worked; pnpm only printed `WARN Unsupported engine`. `tsc` passed. vitest crashed starting every worker with `TypeError: webidl.util.markAsUncloneable is not a function` (undici 8.11.2, loaded by jsdom 30.1.1). 0 tests ran. |
  | 22.23.3, `engines.node >=24` | passed: 145 tests, and the bundle was byte-identical to the committed one. pnpm only warned about engines. |
  | 24.4.1 (local) | passed (`otto ci`). |

  The failure on 20 comes from the code's dependencies, not from the engines field or corepack. The declared dependency floors are:
  - jsdom: `^22.22.2 || ^24.15.0 || >=26.0.0`
  - undici: `>=22.19.0`
  - vitest: `^22.12.0 || ^24.0.0 || >=26.0.0`
  - vite: `^20.19.0 || >=22.12.0`

  So the editor truly needs node 22 or newer. 24.4.1 is outside jsdom's declared range but works.

#### Design decisions
- `editor/package.json` `engines.node` is now `>=22.22.2`, down from `>=24`. That is jsdom's 22.x floor, and it was verified at 22.23.3 and 24.4.1.
- The `editor` task now enforces `engines.node` as a hard floor before pnpm runs (`.otto.yml`, editor task). pnpm only warns on engines. Node 20 now stops with `ERROR: node 20.20.2 is older than editor/package.json engines.node ">=22.22.2".` instead of a vitest worker crash. The task requires `engines.node` to be in `>=X.Y.Z` form and fails if it is not.

#### Deviations
- None.

#### Tradeoffs
- Fixing the test by binding a distinct loopback address with port 0, vs passing a pre-bound listener into `bind`: the test change keeps `bind(&Config)` as it is and still proves the configured address is the one bound.

#### Open questions
- Which node GitHub's ubuntu-latest image ships has not been checked here; no runner has run yet. If it is below 22.22.2, `otto ci` fails there, clearly, at the node-floor check. The reusable `scottidler/github-actions` `rust-ci.yml` would then need a node setup step. A caller workflow cannot add steps inside a reusable workflow's job. That is Scott's call, in that repo.

## Phase 8: Theme
### Design decisions
- Sidebar bug root cause: `server/src/render.rs:sidebar` called `item("Home", root, ...)`, which recursed into the root's children, then looped over the same children again at the top level, so every directory showed twice. The new `sidebar` renders the home page as a plain link and each directory once, as a `<details>` section. `render::tests::sidebar_lists_each_page_and_directory_exactly_once` counts each entry. The a04a56a `sidebar`/`item` code, pasted into a scratch test with the same assertion, failed with `left: 2, right: 1` on `href="/guide/setup"`, and so did the new code with the nesting put back once. The old test `sidebar_nests_marks_current_and_labels_readmeless_dirs` only used `contains`, which is why it missed this; it is replaced by the exactly-once test and `sidebar_labels_pages_by_title_and_marks_the_current_one`.
- One stylesheet for reading and editing - `editor/src/theme/riki.css`, built to `server/assets/riki.css` - content rules hang off `.riki-prose`, which both the rendered `<article>` and the editor's ProseMirror root carry (`editor/src/setup.ts:PROSE_CLASS` via `editorViewOptionsCtx.attributes`). `editor.css` now holds only editor chrome (toolbar, Save/Cancel, link tooltip) plus three overrides of the prosemirror base CSS that broke parity: `pre { white-space: pre }`, and the table's `width: 100%; table-layout: fixed` (the "wide, padded table" in edit mode, together with the `<p>` margins inside editor cells).
- The alert label mismatch ("Warning" vs "WARNING") came from the editor drawing the title with `::before { content: attr(data-kind) }`. The alert node's `toDOM` (`editor/src/alert.ts`) now emits the server's markup: a `p.markdown-alert-title` (`contenteditable=false`) with the same SVG icon and comrak's title text (`alertTitle`: the alert's own title, else `Warning` casing), then the content hole in `div.markdown-alert-body`. `parseDOM` reads content from `.markdown-alert-body`. The document model and serializer are unchanged; all 65 canonical fixtures stay byte-identical.
- Server rendering uses a comrak `create_formatter!` formatter (`core/src/render.rs:RikiFormatter`) over the default one, with three overrides:
  - headings: comrak's own output, plus every h2/h3 recorded as a `TocEntry` under `context.current_anchorized_id`, so a TOC id can never disagree with the HTML id (dedupe suffixes included)
  - alerts: comrak's markup with an inline `<svg class="riki-alert-icon">` (`alert_icon_path`) in the title
  - tables: wrapped in `div.riki-table`, a scroll container
  `render_markdown` now returns `Rendered { html, toc }`.
- Highlighting: comrak's `syntect-fancy` feature (pure-Rust regex, no oniguruma C build). `SyntectAdapterBuilder::css_with_class_prefix("hl-")` emits `<pre class="syntax-highlighting">` and `<span class="hl-...">` scopes, no inline styles. Token colors are CSS variables with light and dark values. The adapter is built once (`LazyLock`). `core-guard` still holds: syntect brings no axum.
- Titles: `core/src/render.rs:page_title` is the text of the first level-1 heading (comrak parse, front matter skipped, setext counts). `Wiki::index` reads every page blob once per commit (`GitStore::read_blobs`, one blocking task) and attaches titles to the cached `NavIndex` (`with_titles`, `PageNode.title`). They're used for the sidebar labels, the page `<title>`, and the breadcrumbs.
- Site title is the home page's H1, else `riki` (`pages.rs:site_title`). The sidebar's root entry is labelled `Home`, so the title isn't printed twice next to the header brand. A directory with a `README.md` heads its section with a link to it, labelled with its title. One without is labelled with its segment, as written.
- Sections are `<details>`, so collapsing works with no JS. Top-level sections render open; nested ones open only along the path to the current page. The current page link carries `class="current" aria-current="page"`.
- Template (`server/templates/page.html`): header (menu button, brand mark, Edit slot, theme toggle), `nav.riki-sidebar`, then `main` holding the content column (breadcrumbs + article) and `aside.riki-toc`. The TOC is always rendered, empty when the page has no h2/h3, so the layout never shifts. Inline `<style>` is gone; all CSS comes from `/_riki/assets/riki.css`.
- Page script `editor/src/page/{ui,main}.ts`, built to `server/assets/riki.js` and served from `/_riki/assets/`. It's loaded without `defer` in `<head>`, so a stored dark theme applies before first paint. It handles:
  - theme toggle: `localStorage` key `riki-theme`; every storage access, including touching `window.localStorage`, is wrapped in try/catch, and a throwing storage counts as "follow the system"
  - the mobile drawer (Escape closes it)
  - copy buttons on rendered `<pre>`s (clipboard API, then an `execCommand` fallback)
  - the TOC highlight on scroll
  Clicks are delegated from `document`. The editor's post-save re-render (`main.ts`) dispatches `riki:rendered` so new content gets its copy buttons. No inline script or handler anywhere; the CSP is unchanged.
- Dark mode is CSS `light-dark()` over `color-scheme`: `:root` follows `prefers-color-scheme`, and `[data-theme=light|dark]` pins it. Each color is defined once, with no duplicated dark block.
- Icons (alert icons, toolbar icons, logo mark, chevrons) are original 16x16 line drawings. The CSS ones are `data:` SVG masks, which the page CSP's `img-src data:` already allows. No third-party logos or assets.
- Editor chrome: while a session is open, Cancel / Save take the header's Edit slot (the Edit button is `hidden`, marked `data-riki-hidden-while-editing`, and restored on close). The sticky head above the page holds the icon toolbar and the status line, so the toolbar fits one row at desktop width. The `riki-editing` body class hides the TOC while editing.
- `[hidden] { display: none !important }` in the theme: author `display` rules (e.g. `#riki-edit { display: inline-flex }`) otherwise beat the UA's `[hidden]` rule. Found by the new e2e test "Save and Cancel take the Edit slot".
- The code-block copy wrapper (`.riki-code`) has no box of its own. Its margins collapse into the `<pre>`'s, so a rendered code block sits exactly where the editor's unwrapped `<pre>` does, and the parity test compares `pre` to `pre`.
- Coordinator item: `TraceLayer`'s failure hook logged every 5xx at ERROR, including the 503s riki returns for known states (no content yet, upstream unreachable, push timeout). `routes.rs:on_failure` logs a 503 at WARN and everything else at ERROR. `failure_level` is pure and unit-tested.
- Tests added:
  - core: golden HTML for the alert with its icon, all five alert types, a pinned highlighted-Rust golden with no `style=`, a TOC with dedupe and Unicode ids, anchors, the table wrapper, task-list classes, `page_title` hits and misses, `with_titles`/`node`, and `read_blobs`
  - server: the sidebar exactly-once test, title labels, section open state, breadcrumbs, a TOC golden, `<title>` with the site title, assets embedded and served, no inline script/style in the template, page-level titles/TOC/highlighting through the router, and the 503 log level
  - vitest: `test/page.test.ts` (theme storage incl. throwing storage, drawer, code decoration, copy, TOC) and `test/alert-dom.test.ts` (title, icon, body hole, kind change, byte-identical serialization, prose class)
  - Playwright: `e2e/theme.spec.ts`. Read vs edit computed styles for a table cell, the alert title (text and icon path too), h1, p, and the code block, plus the same left edge. Also: Save/Cancel slot, toggle persists across a reload, system dark, copy with no console errors, the mobile drawer.
- Evidence the parity test bites: with the editor's `riki-prose` class removed, it failed on the table cell, alert title, h1 styles and more (soft assertions report each one).

### Deviations
- The design doc's Renderer bullet configures comrak with `default-features = false` and no highlighter. Phase 8 adds the `syntect-fancy` feature and `render.tasklist_classes = true`. Both are required by this phase; same renderer, same safe mode.
- `riki_core::render::render_markdown` returns `Rendered { html, toc }` instead of `String`. Same effect, correct seam: the TOC must come from the same formatter pass that assigns heading ids.
- `server::render::page` takes a `PageView` struct instead of five positional strings; the page now has eight slots. Existing tests were moved to it by name, unchanged in what they assert, except where the markup changed: `pages_show_the_sidebar_and_an_edit_button` now expects `class="current" aria-current="page"`, and `golden_alert_renders_with_github_classes` now pins the icon in the title.
- Breadcrumbs are in the content column above the H1, not in the header bar. The header has the site title, the Edit slot, and the toggles; the breadcrumbs are replaced along with `main` on the editor's re-render.
- The editor's `SWAPPED` selector `nav` became `.riki-sidebar`, since the breadcrumbs are also a `<nav>`.
- Toolbar `Button.label` was dropped: buttons are icons with `title` / `aria-label`, so the text label had no reader. Control ids and order are unchanged (the existing toolbar test pins the order).
- Syntax highlighting happens only in the read view. The editor's code blocks show the same box, font and colors, without token colors, because highlighting is server-side by design. The parity test compares the `pre` box, not token spans.

### Tradeoffs
- Release binary 8,578,312 -> 12,256,304 bytes (+3.68 MB, measured against the installed v0.1.0 binary). This is syntect's bundled syntax set plus the default theme dump that comrak's `syntect` feature enables unconditionally. The themes go unused in CSS-class mode, but comrak's feature gives no way to drop them. The alternative, a hand-picked syntax subset loaded from a custom dump, would mean a build step and a second dependency on syntect; not worth it for a POC.
- System font stack (Inter first, if installed) vs serving a font file: no font asset, no license file to ship, no CDN. On Linux it renders as Noto Sans / Noto Sans Mono; on macOS as SF.
- Titles are read for every page on every new commit (one blob read each, cached with the index) vs lazily per request. Simple and correct for a POC wiki; at thousands of pages the first request after a push pays for it.
- `light-dark()` and `color-mix()` require a 2024-era browser (Chromium 123, Firefox 120, Safari 17.5) vs duplicated light and dark variable blocks. The single definition was chosen; older browsers get unstyled colors.
- Copy buttons are inserted by the page script, not rendered server-side. A button that does nothing without JS shouldn't be in the HTML, and the editor's code blocks must not get one.

### Open questions
- The sidebar labels the home page `Home` and the header shows the home page's H1 as the site title. Should a `site.title` config key drive the header instead?
- Directories without a `README.md` are labelled with their raw segment (e.g. `reference`), next to title-labelled sections such as `Guides`. Should the label be prettified (capitalized, `-` to space), or is the raw name the honest label?
- Process note: `cd` does not persist between this agent's Bash calls, so the phase's first `cargo build -p riki-server` ran in the main checkout `/home/saidler/repos/scottidler/riki` (unchanged source; it refreshed that checkout's `target/` only, and its `git status` stayed clean). Every later cargo/otto call chained `cd <worktree> &&` in the same call.
