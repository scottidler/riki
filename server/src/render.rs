//! Pure HTML assembly for the shell: templates filled with already-rendered pieces. No axum, no
//! IO; every value that reaches a template is escaped here or comes from comrak's safe mode.

use riki_core::index::PageNode;
use riki_core::render::{encode_path, escape_html};
use riki_core::wiki::{Rejected, Unreachable};
use tracing::debug;

const PAGE_TEMPLATE: &str = include_str!("../templates/page.html");
const ERROR_TEMPLATE: &str = include_str!("../templates/error.html");

/// CSP for rendered pages: same-origin script (the editor bundle, Phase 6), inline style from the
/// template, images from this origin, `https:` or `data:`. Nothing else loads.
pub const CSP_PAGE: &str = "default-src 'none'; script-src 'self'; connect-src 'self'; \
img-src 'self' https: data:; style-src 'unsafe-inline'; frame-ancestors 'self'";

/// CSP for `/_riki/raw/` responses: marquee's `CSP_ASSET`. An SVG served from here cannot run
/// script against the save API.
pub const CSP_ASSET: &str = "default-src 'none'; frame-ancestors 'self'";

/// The Edit button. Phase 6 wires it; until then it is disabled.
const EDIT_BUTTON: &str = r#"<button id="riki-edit" type="button" disabled>Edit</button>"#;

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
    Edit,
    /// Nothing to act on: a path riki's own routes own.
    None,
    /// A missing page: offer to create the file at this repo path (Phase 6 wires it).
    Create {
        file: &'a str,
    },
}

/// A full page: sidebar, banners, and the rendered body (already safe HTML).
pub fn page(title: &str, body_html: &str, sidebar_html: &str, banners_html: &str, action: Action<'_>) -> String {
    debug!("render::page: title={title:?} action={action:?}");
    let action_html = match action {
        Action::Edit => EDIT_BUTTON.to_string(),
        Action::None => String::new(),
        Action::Create { file } => format!(
            r##"<a id="riki-create" href="#" data-path="{}">Create this page</a>"##,
            escape_html(file)
        ),
    };
    fill(
        PAGE_TEMPLATE,
        &[
            ("__TITLE__", &escape_html(title)),
            ("__ACTION__", &action_html),
            ("__BANNERS__", banners_html),
            ("__SIDEBAR__", sidebar_html),
            ("__BODY__", body_html),
        ],
    )
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

/// The sidebar: the page tree as nested lists. A directory with no `README.md` is a label, not a
/// link. `current` is the URL path of the page being shown (no leading `/`).
pub fn sidebar(root: &PageNode, current: &str) -> String {
    let mut out = String::from("<ul>");
    out.push_str(&item("Home", root, current));
    for (segment, child) in &root.children {
        out.push_str(&item(segment, child, current));
    }
    out.push_str("</ul>");
    out
}

fn item(label: &str, node: &PageNode, current: &str) -> String {
    let mut out = String::from("<li>");
    let label = escape_html(label);
    if node.file.is_some() {
        let class = if node.url == current { " class=\"current\"" } else { "" };
        out.push_str(&format!(
            "<a href=\"/{}\"{class}>{label}</a>",
            escape_html(&encode_path(&node.url))
        ));
    } else {
        out.push_str(&label);
    }
    if !node.children.is_empty() {
        out.push_str("<ul>");
        for (segment, child) in &node.children {
            out.push_str(&item(segment, child, current));
        }
        out.push_str("</ul>");
    }
    out.push_str("</li>");
    out
}

#[cfg(test)]
mod tests;
