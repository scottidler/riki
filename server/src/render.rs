//! Pure HTML assembly for the shell: templates filled with already-rendered pieces. No axum, no
//! IO; every value that reaches a template is escaped here or comes from comrak's safe mode.

use riki_core::index::PageNode;
use riki_core::render::{TocEntry, encode_path, escape_html};
use riki_core::wiki::{Rejected, Unreachable};
use tracing::debug;

const PAGE_TEMPLATE: &str = include_str!("../templates/page.html");
const ERROR_TEMPLATE: &str = include_str!("../templates/error.html");

/// CSP for rendered pages: same-origin script (the editor bundle at `/_riki/assets/`), same-origin
/// stylesheets plus the template's inline style, images from this origin, `https:` or `data:`.
/// Nothing else loads.
pub const CSP_PAGE: &str = "default-src 'none'; script-src 'self'; connect-src 'self'; \
img-src 'self' https: data:; style-src 'self' 'unsafe-inline'; frame-ancestors 'self'";

/// CSP for `/_riki/raw/` responses: marquee's `CSP_ASSET`. An SVG served from here cannot run
/// script against the save API.
pub const CSP_ASSET: &str = "default-src 'none'; frame-ancestors 'self'";

/// Single-pass `__TOKEN__` substitution: a value placed for one token is never re-scanned, so
/// page content containing a literal `__BODY__` cannot smuggle itself into a later token. Tokens
/// match in list order; unrecognized `__...__` runs pass through verbatim (marquee
/// `server/src/render.rs` precedent).
pub fn fill(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(pos) = rest.find("__") {
        out.push_str(&rest[..pos]);
        let after = &rest[pos..];
        let hit = vars.iter().find_map(|(token, value)| {
            debug_assert!(!token.is_empty(), "fill(): empty token");
            after.strip_prefix(token).map(|tail| (*value, tail))
        });
        match hit {
            Some((value, tail)) => {
                out.push_str(value);
                rest = tail;
            }
            None => {
                out.push_str("__");
                rest = &after[2..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// What the Edit slot of a page shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action<'a> {
    /// An existing page: the Edit button for the file at this repo path, with the file's page on
    /// GitHub when the content remote is a GitHub repo (the editor links it when it refuses).
    Edit { file: &'a str, source: Option<&'a str> },
    /// Nothing to act on: a path riki's own routes own.
    None,
    /// A missing page: offer to create the file at this repo path.
    Create { file: &'a str },
}

/// Everything a full page shows. Every `_html` field is already safe HTML (comrak's safe mode or
/// built by this module); plain-text fields are escaped here.
#[derive(Debug, Clone, Copy)]
pub struct PageView<'a> {
    /// The page's own title (its first `# heading`, else its URL segment).
    pub title: &'a str,
    /// The wiki's title: the home page's first `# heading`, else `riki`.
    pub site_title: &'a str,
    pub body_html: &'a str,
    pub toc: &'a [TocEntry],
    pub sidebar_html: &'a str,
    pub breadcrumbs_html: &'a str,
    pub banners_html: &'a str,
    pub action: Action<'a>,
}

/// A full page: header, sidebar, breadcrumbs, the rendered body, and the "On this page" list.
pub fn page(view: &PageView<'_>) -> String {
    debug!("render::page: title={:?} action={:?}", view.title, view.action);
    let action_html = match view.action {
        Action::Edit { file, source } => {
            let source = source.map_or_else(String::new, |url| format!(r#" data-source="{}""#, escape_html(url)));
            format!(
                r#"<button id="riki-edit" type="button" data-path="{}"{source}>Edit</button>"#,
                escape_html(file)
            )
        }
        Action::None => String::new(),
        Action::Create { file } => format!(
            r##"<a id="riki-create" href="#" data-path="{}">Create this page</a>"##,
            escape_html(file)
        ),
    };
    fill(
        PAGE_TEMPLATE,
        &[
            ("__TITLE__", &escape_html(view.title)),
            ("__SITE__", &escape_html(view.site_title)),
            ("__ACTION__", &action_html),
            ("__BANNERS__", view.banners_html),
            ("__SIDEBAR__", view.sidebar_html),
            ("__BREADCRUMBS__", view.breadcrumbs_html),
            ("__BODY__", view.body_html),
            ("__TOC__", &toc(view.toc)),
        ],
    )
}

/// The "On this page" list: every h2 and h3, linked by anchor id. Always present (empty when the
/// page has no h2 / h3) so the layout does not shift between pages.
pub fn toc(entries: &[TocEntry]) -> String {
    if entries.is_empty() {
        return r#"<aside class="riki-toc" aria-label="On this page"></aside>"#.to_string();
    }
    let mut out = String::from(
        r#"<aside class="riki-toc" aria-label="On this page"><p class="riki-toc-title">On this page</p><ul>"#,
    );
    for entry in entries {
        out.push_str(&format!(
            r##"<li class="riki-toc-h{}"><a href="#{}">{}</a></li>"##,
            entry.level,
            escape_html(&encode_fragment(&entry.id)),
            escape_html(&entry.text)
        ));
    }
    out.push_str("</ul></aside>");
    out
}

/// An anchor id as a URL fragment: ids keep Unicode letters, so percent-encode anything outside
/// the unreserved set.
fn encode_fragment(id: &str) -> String {
    encode_path(id)
}

/// The self-contained error page: inline style, no scripts, no sidebar, no store access, so it
/// renders even when the store is broken.
pub fn error_page(status: &str, message: &str) -> String {
    debug!("render::error_page: status={status:?}");
    fill(
        ERROR_TEMPLATE,
        &[
            ("__STATUS__", &escape_html(status)),
            ("__MESSAGE__", &escape_html(message)),
        ],
    )
}

/// The banners every page shows: an index error on the newest tip, upstream unreachable.
pub fn banners(rejected: Option<&Rejected>, unreachable: Option<&Unreachable>) -> String {
    let mut out = String::new();
    if let Some(rejected) = rejected {
        out.push_str(&format!(
            "<div class=\"banner error\" role=\"alert\">Index error on commit {}: {}. Serving the last good version.</div>\n",
            rejected.commit,
            escape_html(&rejected.errors)
        ));
    }
    if let Some(down) = unreachable {
        out.push_str(&format!(
            "<div class=\"banner\" role=\"status\">Upstream unreachable since {}. Serving the last good version.</div>\n",
            escape_html(&down.since_text())
        ));
    }
    out
}

/// The label a node shows: its page title, else its URL segment (`Home` for the root).
pub fn label<'a>(node: &'a PageNode, segment: &'a str) -> &'a str {
    match &node.title {
        Some(title) => title,
        None if node.url.is_empty() => "Home",
        None => segment,
    }
}

/// The sidebar: the home page and top-level pages as links, then one collapsible section per
/// directory. A directory with a `README.md` heads its section with a link to it; one without is
/// a plain label. `current` is the URL path of the page being shown (no leading `/`).
pub fn sidebar(root: &PageNode, current: &str) -> String {
    let mut out = String::from(r#"<ul class="riki-nav-list">"#);
    if root.file.is_some() {
        out.push_str(&format!("<li>{}</li>", link(root, "Home", current)));
    }
    for (segment, child) in root.children.iter().filter(|(_, child)| child.children.is_empty()) {
        out.push_str(&format!("<li>{}</li>", link(child, label(child, segment), current)));
    }
    out.push_str("</ul>");
    for (segment, child) in root.children.iter().filter(|(_, child)| !child.children.is_empty()) {
        out.push_str(&section(segment, child, current, true));
    }
    out
}

fn contains(node: &PageNode, current: &str) -> bool {
    current == node.url || current.starts_with(&format!("{}/", node.url))
}

/// A directory as `<details>`: open at the top level and along the path to the current page.
fn section(segment: &str, node: &PageNode, current: &str, top: bool) -> String {
    let open = if top || contains(node, current) { " open" } else { "" };
    let head = if node.file.is_some() {
        link(node, label(node, segment), current)
    } else {
        format!(r#"<span class="riki-nav-label">{}</span>"#, escape_html(segment))
    };
    let mut out =
        format!(r#"<details class="riki-nav-section"{open}><summary>{head}</summary><ul class="riki-nav-list">"#);
    for (child_segment, child) in &node.children {
        if child.children.is_empty() {
            out.push_str(&format!(
                "<li>{}</li>",
                link(child, label(child, child_segment), current)
            ));
        } else {
            out.push_str(&format!("<li>{}</li>", section(child_segment, child, current, false)));
        }
    }
    out.push_str("</ul></details>");
    out
}

fn link(node: &PageNode, label: &str, current: &str) -> String {
    let here = if node.url == current {
        r#" class="current" aria-current="page""#
    } else {
        ""
    };
    format!(
        "<a href=\"/{}\"{here}>{}</a>",
        escape_html(&encode_path(&node.url)),
        escape_html(label)
    )
}

/// Breadcrumbs for the page at `path`: Home, then every ancestor directory (a link when it has a
/// page), then the page itself. Empty on the home page.
pub fn breadcrumbs(root: &PageNode, path: &str, title: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    let mut out = String::from(r#"<nav class="riki-breadcrumbs" aria-label="Breadcrumb"><ol>"#);
    out.push_str(r#"<li><a href="/">Home</a></li>"#);
    let segments: Vec<&str> = path.split('/').collect();
    let mut node = Some(root);
    for (i, segment) in segments.iter().enumerate() {
        node = node.and_then(|n| n.children.get(*segment));
        if i + 1 == segments.len() {
            break;
        }
        match node {
            Some(dir) if dir.file.is_some() => out.push_str(&format!(
                r#"<li><a href="/{}">{}</a></li>"#,
                escape_html(&encode_path(&dir.url)),
                escape_html(label(dir, segment))
            )),
            Some(dir) => out.push_str(&format!("<li>{}</li>", escape_html(label(dir, segment)))),
            None => out.push_str(&format!("<li>{}</li>", escape_html(segment))),
        }
    }
    out.push_str(&format!(r#"<li aria-current="page">{}</li>"#, escape_html(title)));
    out.push_str("</ol></nav>");
    out
}

#[cfg(test)]
mod tests;
