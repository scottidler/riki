//! Pure HTML assembly for the shell: templates filled with already-rendered pieces. No axum, no
//! IO; every value that reaches a template is escaped here or comes from comrak's safe mode.

use riki_core::index::{PageNode, prettify};
use riki_core::render::{RAW_PREFIX, TocEntry, encode_path, escape_html};
use riki_core::wiki::{Rejected, Unreachable};
use tracing::debug;

const PAGE_TEMPLATE: &str = include_str!("../templates/page.html");
const ERROR_TEMPLATE: &str = include_str!("../templates/error.html");

/// CSP for rendered pages: same-origin script (the editor bundle at `/_riki/assets/`), same-origin
/// stylesheets plus the template's inline style, same-origin fonts (the bundled Inter and
/// JetBrains Mono), images from this origin, `https:` or `data:`. Nothing else loads.
pub const CSP_PAGE: &str = "default-src 'none'; script-src 'self'; connect-src 'self'; \
img-src 'self' https: data:; style-src 'self' 'unsafe-inline'; font-src 'self'; frame-ancestors 'self'";

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

/// The wiki's identity: the name in the header and the `<title>` suffix, and optionally a logo
/// per theme that the header shows instead of the name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    pub name: String,
    pub logo: Option<Logo>,
}

/// Repo-relative image files, served through `/_riki/raw/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Logo {
    pub light: String,
    pub dark: String,
}

impl Default for Site {
    fn default() -> Self {
        Self {
            name: "riki".to_string(),
            logo: None,
        }
    }
}

/// The header's home link: both logo variants (the stylesheet shows the one for the current
/// theme) with the name for screen readers, or the name as text when there is no logo.
pub fn brand(site: &Site) -> String {
    let name = escape_html(&site.name);
    match &site.logo {
        Some(logo) => format!(
            concat!(
                r#"<a class="riki-brand" href="/">"#,
                r#"<img class="riki-logo riki-logo-light" src="{}" alt="">"#,
                r#"<img class="riki-logo riki-logo-dark" src="{}" alt="">"#,
                r#"<span class="riki-sr-only">{}</span></a>"#
            ),
            escape_html(&raw_url(&logo.light)),
            escape_html(&raw_url(&logo.dark)),
            name
        ),
        None => format!(r#"<a class="riki-brand" href="/"><span class="riki-brand-name">{name}</span></a>"#),
    }
}

fn raw_url(file: &str) -> String {
    format!("{RAW_PREFIX}{}", encode_path(file))
}

/// Everything a full page shows. Every `_html` field is already safe HTML (comrak's safe mode or
/// built by this module); plain-text fields are escaped here.
#[derive(Debug, Clone, Copy)]
pub struct PageView<'a> {
    /// The page's own title ([`label`] of its node).
    pub title: &'a str,
    pub site: &'a Site,
    pub body_html: &'a str,
    pub toc: &'a [TocEntry],
    pub sidebar_html: &'a str,
    pub breadcrumbs_html: &'a str,
    /// The narrow-screen bar under the header: the drawer button and "Section > Page".
    pub trail_html: &'a str,
    /// Previous / Next cards under the article.
    pub pager_html: &'a str,
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
            ("__SITE__", &escape_html(&view.site.name)),
            ("__BRAND__", &brand(view.site)),
            ("__ACTION__", &action_html),
            ("__TRAIL__", view.trail_html),
            ("__BANNERS__", view.banners_html),
            ("__SIDEBAR__", view.sidebar_html),
            ("__BREADCRUMBS__", view.breadcrumbs_html),
            ("__BODY__", view.body_html),
            ("__PAGER__", view.pager_html),
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

/// The label a node shows: its page title (front matter `title`, else first H1; for a directory,
/// its README's), else its prettified URL segment, else `Home` for the root.
pub fn label(node: &PageNode, segment: &str) -> String {
    match &node.title {
        Some(title) => title.clone(),
        None if node.url.is_empty() => "Home".to_string(),
        None => prettify(segment),
    }
}

/// One sidebar entry: a page link or a directory group. Built once, so the sidebar and the
/// Previous / Next order can never disagree.
enum NavItem<'a> {
    Page {
        node: &'a PageNode,
        label: String,
    },
    Group {
        node: &'a PageNode,
        label: String,
        items: Vec<NavItem<'a>>,
    },
}

/// The sidebar's entries: the home page (labelled `Home`), then the root's children. Without an
/// `_order` file that is top-level pages in name order, then one group per top-level directory;
/// the root's `_order` can put a group before a page. Inside a group, pages and sub-groups are
/// interleaved in name order unless the group's own `_order` says otherwise.
fn nav_items(root: &PageNode) -> Vec<NavItem<'_>> {
    let mut items = Vec::new();
    if root.file.is_some() {
        items.push(NavItem::Page {
            node: root,
            label: "Home".to_string(),
        });
    }
    let is_page = |child: &PageNode| child.children.is_empty();
    let (pages, groups): (Vec<_>, Vec<_>) = root.children.iter().partition(|(_, child)| is_page(child));
    let rest = pages.into_iter().chain(groups);
    items.extend(listed_first(root, rest).map(|(segment, child)| nav_child(segment, child)));
    items
}

fn group<'a>(segment: &str, node: &'a PageNode) -> NavItem<'a> {
    let items = listed_first(node, node.children.iter())
        .map(|(child_segment, child)| nav_child(child_segment, child))
        .collect();
    NavItem::Group {
        node,
        label: label(node, segment),
        items,
    }
}

fn nav_child<'a>(segment: &str, child: &'a PageNode) -> NavItem<'a> {
    if child.children.is_empty() {
        NavItem::Page {
            node: child,
            label: label(child, segment),
        }
    } else {
        group(segment, child)
    }
}

/// `folder`'s children with the `_order` entries first, in file order, then `rest` as given.
fn listed_first<'a>(
    folder: &'a PageNode,
    rest: impl Iterator<Item = (&'a String, &'a PageNode)>,
) -> impl Iterator<Item = (&'a String, &'a PageNode)> {
    let listed = folder
        .order
        .iter()
        .filter_map(|name| folder.children.get_key_value(name));
    let rest = rest.filter(|(name, _)| !folder.order.contains(name));
    listed.chain(rest)
}

/// The sidebar. Top-level directories are static group headers (a link when the directory has a
/// `README.md`); nested directories collapse behind a chevron button (`aria-expanded`), open
/// only along the path to the current page. `current` is the URL path being shown (no leading
/// `/`).
pub fn sidebar(root: &PageNode, current: &str) -> String {
    let mut out = String::from(r#"<ul class="riki-nav-list">"#);
    let mut list_open = true;
    for item in &nav_items(root) {
        match item {
            NavItem::Page { .. } => {
                if !list_open {
                    out.push_str(r#"<ul class="riki-nav-list">"#);
                    list_open = true;
                }
                out.push_str(&nav_item(item, current));
            }
            NavItem::Group { node, label, items } => {
                if list_open {
                    out.push_str("</ul>");
                    list_open = false;
                }
                let head = match node.file {
                    Some(_) => link(node, label, current),
                    None => escape_html(label),
                };
                out.push_str(&format!(
                    r#"<div class="riki-nav-group"><p class="riki-nav-heading">{head}</p><ul class="riki-nav-list">"#
                ));
                for child in items {
                    out.push_str(&nav_item(child, current));
                }
                out.push_str("</ul></div>");
            }
        }
    }
    if list_open {
        out.push_str("</ul>");
    }
    out
}

fn contains(node: &PageNode, current: &str) -> bool {
    current == node.url || current.starts_with(&format!("{}/", node.url))
}

/// One `<li>`: a page link, or a nested group with its chevron and (possibly hidden) list.
fn nav_item(item: &NavItem<'_>, current: &str) -> String {
    match item {
        NavItem::Page { node, label } => format!("<li>{}</li>", link(node, label, current)),
        NavItem::Group { node, label, items } => {
            let open = contains(node, current);
            let list_id = format!("riki-nav-{}", encode_path(&node.url));
            let toggle = |class: &str, text: &str| {
                format!(
                    r#"<button type="button" class="{class}" data-riki-toggle="group" aria-expanded="{open}" aria-controls="{}" aria-label="{}">{text}</button>"#,
                    escape_html(&list_id),
                    escape_html(label)
                )
            };
            let head = match node.file {
                Some(_) => format!("{}{}", link(node, label, current), toggle("riki-nav-chevron", "")),
                None => toggle(
                    "riki-nav-chevron riki-nav-sub-label",
                    &format!("<span>{}</span>", escape_html(label)),
                ),
            };
            let hidden = if open { "" } else { " hidden" };
            let mut out = format!(
                r#"<li class="riki-nav-sub"><div class="riki-nav-sub-head">{head}</div><ul class="riki-nav-list" id="{}"{hidden}>"#,
                escape_html(&list_id)
            );
            for child in items {
                out.push_str(&nav_item(child, current));
            }
            out.push_str("</ul></li>");
            out
        }
    }
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

/// Every page in sidebar order, with its sidebar label: the order Previous / Next walks.
pub fn reading_order(root: &PageNode) -> Vec<(&PageNode, String)> {
    fn walk<'a>(item: &NavItem<'a>, out: &mut Vec<(&'a PageNode, String)>) {
        match item {
            NavItem::Page { node, label } => out.push((node, label.clone())),
            NavItem::Group { node, label, items } => {
                if node.file.is_some() {
                    out.push((node, label.clone()));
                }
                for child in items {
                    walk(child, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    for item in &nav_items(root) {
        walk(item, &mut out);
    }
    out
}

/// Previous / Next cards for the page at `current`, in sidebar order. Empty when `current` is
/// not a page (a 404) or is the only one.
pub fn pager(root: &PageNode, current: &str) -> String {
    let order = reading_order(root);
    let Some(at) = order.iter().position(|(node, _)| node.url == current) else {
        return String::new();
    };
    let card = |(node, label): &(&PageNode, String), class: &str, word: &str| {
        format!(
            r#"<a class="riki-pager-card {class}" href="/{}"><span class="riki-pager-label">{word}</span><span class="riki-pager-title">{}</span></a>"#,
            escape_html(&encode_path(&node.url)),
            escape_html(label)
        )
    };
    let prev = at.checked_sub(1).and_then(|i| order.get(i));
    let next = order.get(at + 1);
    if prev.is_none() && next.is_none() {
        return String::new();
    }
    let mut out = String::from(r#"<nav class="riki-pager" aria-label="Previous and next pages">"#);
    if let Some(prev) = prev {
        out.push_str(&card(prev, "riki-pager-prev", "Previous"));
    }
    if let Some(next) = next {
        out.push_str(&card(next, "riki-pager-next", "Next"));
    }
    out.push_str("</nav>");
    out
}

/// The narrow-screen bar under the header: the drawer button, then the page's section (its
/// parent directory) and the page itself.
pub fn trail(root: &PageNode, path: &str, title: &str) -> String {
    let mut out = String::from(concat!(
        r#"<div class="riki-trail">"#,
        r#"<button type="button" class="riki-icon-button riki-nav-toggle" data-riki-toggle="nav" aria-label="Show pages" aria-controls="riki-nav" aria-expanded="false">"#,
        r#"<svg viewBox="0 0 20 20" width="20" height="20" aria-hidden="true"><path d="M3.5 5.5h13M3.5 10h13M3.5 14.5h13"/></svg></button>"#,
        r#"<ol class="riki-trail-path">"#
    ));
    if let Some((parent, _)) = path.rsplit_once('/') {
        let segment = parent.rsplit('/').next().unwrap_or(parent);
        let section = node_at(root, parent).map_or_else(|| prettify(segment), |dir| label(dir, segment));
        out.push_str(&format!("<li>{}</li>", escape_html(&section)));
    }
    out.push_str(&format!(
        r#"<li aria-current="page">{}</li></ol></div>"#,
        escape_html(title)
    ));
    out
}

fn node_at<'a>(root: &'a PageNode, url: &str) -> Option<&'a PageNode> {
    url.split('/')
        .try_fold(root, |node, segment| node.children.get(segment))
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
                escape_html(&label(dir, segment))
            )),
            Some(dir) => out.push_str(&format!("<li>{}</li>", escape_html(&label(dir, segment)))),
            None => out.push_str(&format!("<li>{}</li>", escape_html(&prettify(segment)))),
        }
    }
    out.push_str(&format!(r#"<li aria-current="page">{}</li>"#, escape_html(title)));
    out.push_str("</ol></nav>");
    out
}

#[cfg(test)]
mod tests;
