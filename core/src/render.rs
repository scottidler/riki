//! Markdown to HTML: comrak configured per the design, with link and image rewriting.
//!
//! Relative links and images resolve against the **source file's** directory, not the browser
//! URL: `/a/b` may be served from `a/b/README.md`, so `../x.md` there means `a/x.md`. Resolved
//! `.md` links become riki URLs (`x.md` -> `/x`); resolved images become `/_riki/raw/<path>`.
//! Absolute and external URLs pass through untouched.

use std::fmt::Write as _;
use std::sync::{Arc, LazyLock};

use comrak::html::format_node_default;
use comrak::nodes::{AlertType, AstNode, NodeValue};
use comrak::options::Plugins;
use comrak::plugins::syntect::{SyntectAdapter, SyntectAdapterBuilder};
use comrak::{Arena, Options, create_formatter, parse_document};
use tracing::debug;

use crate::index::url_for_file;

/// Where image blobs are served from.
pub const RAW_PREFIX: &str = "/_riki/raw/";

/// Prefix on every syntax-highlighting class (`hl-keyword hl-control hl-rust`), so token classes
/// can never collide with the page's own CSS.
pub const HIGHLIGHT_CLASS_PREFIX: &str = "hl-";

/// One `h2` / `h3` of a page, for the "On this page" list. `id` is the heading's anchor id as the
/// rendered HTML carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocEntry {
    pub level: u8,
    pub id: String,
    pub text: String,
}

/// A rendered page body: the HTML fragment and its table of contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    pub html: String,
    pub toc: Vec<TocEntry>,
}

/// The icon in each alert's title: 16x16 line art, stroked with `currentColor` by the stylesheet.
/// The editor draws the same paths (`editor/src/alert.ts`), so an alert looks the same in both.
pub fn alert_icon_path(kind: AlertType) -> &'static str {
    match kind {
        AlertType::Note => "M8 14.25a6.25 6.25 0 1 0 0-12.5 6.25 6.25 0 0 0 0 12.5ZM8 7.25v4M8 4.75v.01",
        AlertType::Tip => {
            "M6 12.25h4M6.75 14.5h2.5M5.6 10.4a4.25 4.25 0 1 1 4.8 0c-.4.3-.65.75-.65 1.25v.6h-3.5v-.6c0-.5-.25-.95-.65-1.25Z"
        }
        AlertType::Important => {
            "M2.25 3.25c0-.55.45-1 1-1h9.5c.55 0 1 .45 1 1v7c0 .55-.45 1-1 1H8l-3.25 2.5v-2.5h-1.5c-.55 0-1-.45-1-1ZM8 4.75v2.75M8 9.5v.01"
        }
        AlertType::Warning => {
            "M7.13 2.5a1 1 0 0 1 1.74 0l5.4 9.5a1 1 0 0 1-.87 1.5H2.6a1 1 0 0 1-.87-1.5ZM8 6.25v3M8 11.25v.01"
        }
        AlertType::Caution => "M5.4 1.75h5.2l3.65 3.65v5.2l-3.65 3.65H5.4L1.75 10.6V5.4ZM8 4.75v3.75M8 10.75v.01",
    }
}

fn alert_kind(kind: AlertType) -> &'static str {
    match kind {
        AlertType::Note => "note",
        AlertType::Tip => "tip",
        AlertType::Important => "important",
        AlertType::Warning => "warning",
        AlertType::Caution => "caution",
    }
}

/// comrak's syntect adapter in CSS-class mode: tokens become `<span class="hl-...">`, colors live
/// in the stylesheet (light and dark), never in inline styles. Loading the syntax set is the
/// expensive part, so it happens once per process.
static HIGHLIGHTER: LazyLock<SyntectAdapter> = LazyLock::new(|| {
    SyntectAdapterBuilder::new()
        .css_with_class_prefix(HIGHLIGHT_CLASS_PREFIX)
        .build()
});

create_formatter!(RikiFormatter<Vec<TocEntry>>, {
    // comrak's heading (id + trailing anchor link), recording each h2 / h3 for the TOC under
    // the id comrak just assigned, so the two can never disagree.
    NodeValue::Heading(ref heading) => |context, node, entering| {
        let rendering = format_node_default(context, node, entering)?;
        if entering
            && matches!(heading.level, 2 | 3)
            && let Some(id) = context.current_anchorized_id.clone()
        {
            context.user.push(TocEntry { level: heading.level, id, text: heading_text(node) });
        }
        return Ok(rendering);
    },
    // comrak's alert markup, plus an inline SVG icon in the title.
    NodeValue::Alert(ref alert) => |context, node, entering| {
        if !entering {
            return format_node_default(context, node, entering);
        }
        let kind = alert.alert_type;
        context.cr()?;
        write!(
            context,
            "<div class=\"markdown-alert markdown-alert-{}\">\n<p class=\"markdown-alert-title\">\
             <svg class=\"riki-alert-icon\" viewBox=\"0 0 16 16\" width=\"16\" height=\"16\" aria-hidden=\"true\">\
             <path d=\"{}\"/></svg>",
            alert_kind(kind),
            alert_icon_path(kind)
        )?;
        match alert.title {
            Some(ref title) => context.escape(title)?,
            None => context.write_str(kind.default_title())?,
        }
        context.write_str("</p>")?;
        context.lf()?;
    },
    // Every code block sits in a `.riki-code` frame (the page script adds the copy button). A
    // fence that names a title (`title="x"`, or free text after the language) gets a header bar
    // carrying it; the language itself is never shown.
    NodeValue::CodeBlock(ref block) => |context, node, entering| {
        if !entering {
            return format_node_default(context, node, entering);
        }
        let title = code_title(&block.info);
        context.cr()?;
        match title {
            Some(ref title) => {
                context.write_str("<div class=\"riki-code riki-code-titled\"><div class=\"riki-code-head\"><span class=\"riki-code-title\">")?;
                context.escape(title)?;
                context.write_str("</span></div>")?;
            }
            None => context.write_str("<div class=\"riki-code\">")?,
        }
        format_node_default(context, node, entering)?;
        context.cr()?;
        context.write_str("</div>")?;
        context.lf()?;
    },
    // Tables sit in a scroll container, the same one the editor's table view has.
    NodeValue::Table(_) => |context, node, entering| {
        if entering {
            context.cr()?;
            context.write_str("<div class=\"riki-table\">")?;
            return format_node_default(context, node, entering);
        }
        format_node_default(context, node, entering)?;
        context.cr()?;
        context.write_str("</div>")?;
        context.lf()?;
    },
});

/// comrak configured per the design; link and image rewriting resolve against `source_file`.
fn options(source_file: &str) -> Options<'static> {
    let mut options = Options::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.tasklist = true;
    options.extension.autolink = true;
    options.extension.alerts = true;
    options.extension.front_matter_delimiter = Some("---".to_string());
    options.extension.header_id_prefix = Some(String::new());
    options.render.tasklist_classes = true;
    let for_links = source_file.to_string();
    options.extension.link_url_rewriter = Some(Arc::new(move |url: &str| rewrite_link(&for_links, url)));
    let for_images = source_file.to_string();
    options.extension.image_url_rewriter = Some(Arc::new(move |url: &str| rewrite_image(&for_images, url)));
    options
}

/// Render `markdown` (the body of `source_file`, a repo-relative path) to an HTML fragment plus
/// its h2 / h3 table of contents. Safe mode: raw HTML is omitted and `javascript:` hrefs are
/// blanked. Fenced code is highlighted server-side into `hl-` classes.
pub fn render_markdown(source_file: &str, markdown: &str) -> Rendered {
    debug!("render_markdown: source_file={source_file} bytes={}", markdown.len());
    let options = options(source_file);
    let arena = Arena::new();
    let root = parse_document(&arena, markdown, &options);
    let mut plugins = Plugins::default();
    plugins.render.codefence_syntax_highlighter = Some(&*HIGHLIGHTER);
    let mut html = String::new();
    let toc = RikiFormatter::format_document_with_plugins(root, &options, &mut html, &plugins, Vec::new())
        .expect("render_markdown: formatting into a String cannot fail");
    Rendered { html, toc }
}

/// The title a code fence names in its info string: `title="x"` (or `title='x'`) anywhere after
/// the language, else the free text after the language (```` ```rust src/main.rs ````). `None`
/// for a bare language, or meta that is only `key=value` / `{...}` attributes.
pub fn code_title(info: &str) -> Option<String> {
    let meta = info
        .trim()
        .split_once(char::is_whitespace)
        .map(|(_, meta)| meta.trim())?;
    if let Some(start) = meta.find("title=") {
        let value = &meta[start + "title=".len()..];
        let title = match value.chars().next() {
            Some(quote @ ('"' | '\'')) => value[1..].split(quote).next().unwrap_or_default(),
            _ => value.split_whitespace().next().unwrap_or_default(),
        };
        return (!title.is_empty()).then(|| title.to_string());
    }
    let attribute = |word: &str| word.contains('=') || word.starts_with('{');
    if meta.is_empty() || meta.split_whitespace().any(attribute) {
        return None;
    }
    Some(meta.to_string())
}

/// The page's title: the front matter's top-level `title:`, else the text of its first level-1
/// heading. `None` when the page has neither (or only empty ones).
pub fn page_title(markdown: &str) -> Option<String> {
    let options = options("");
    let arena = Arena::new();
    let root = parse_document(&arena, markdown, &options);
    let front_matter = root.children().find_map(|node| match node.data().value {
        NodeValue::FrontMatter(ref raw) => front_matter_title(raw),
        _ => None,
    });
    if front_matter.is_some() {
        return front_matter;
    }
    let heading = root
        .descendants()
        .find(|node| matches!(node.data().value, NodeValue::Heading(ref h) if h.level == 1))?;
    let text = heading_text(heading);
    (!text.is_empty()).then_some(text)
}

/// The scalar `title:` key at the top level of a YAML front matter block (`---` lines included).
/// Riki only needs this one key, so it reads the line rather than parsing YAML: an unquoted value
/// is trimmed, a quoted one has its quotes stripped. Nested (indented) `title:` keys are ignored.
fn front_matter_title(raw: &str) -> Option<String> {
    let value = raw.lines().find_map(|line| line.strip_prefix("title:"))?.trim();
    let unquoted = ['"', '\'']
        .iter()
        .find_map(|quote| value.strip_prefix(*quote).and_then(|rest| rest.strip_suffix(*quote)))
        .unwrap_or(value)
        .trim();
    (!unquoted.is_empty()).then(|| unquoted.to_string())
}

fn heading_text<'a>(node: &'a AstNode<'a>) -> String {
    node.collect_text().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A relative URL split into the path part and the `?query` / `#fragment` tail.
struct Relative<'a> {
    path: &'a str,
    tail: &'a str,
}

/// `None` for anything that is not a relative path: empty, `#frag`, `/abs`, `//host`, or a URL
/// with a scheme (`https:`, `mailto:`, `data:`).
fn relative(url: &str) -> Option<Relative<'_>> {
    if url.is_empty() || url.starts_with('#') || url.starts_with('/') {
        return None;
    }
    let split = url.find(['?', '#']).unwrap_or(url.len());
    let (path, tail) = url.split_at(split);
    if path.split('/').next().is_some_and(|first| first.contains(':')) {
        return None;
    }
    Some(Relative { path, tail })
}

/// Resolve `target` against the directory of `source_file`. `None` when `..` climbs out of the
/// repo root.
pub fn resolve(source_file: &str, target: &str) -> Option<String> {
    let mut segments: Vec<&str> = match source_file.rsplit_once('/') {
        Some((dir, _)) => dir.split('/').collect(),
        None => Vec::new(),
    };
    for segment in target.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            other => segments.push(other),
        }
    }
    Some(segments.join("/"))
}

/// The riki URL for a link in `source_file`.
pub fn rewrite_link(source_file: &str, url: &str) -> String {
    let Some(rel) = relative(url) else {
        return url.to_string();
    };
    let Some(resolved) = resolve(source_file, rel.path) else {
        return url.to_string();
    };
    let target = match url_for_file(&resolved) {
        Some(page_url) => page_url,
        None => resolved,
    };
    format!("/{target}{}", rel.tail)
}

/// The riki URL for an image in `source_file`.
pub fn rewrite_image(source_file: &str, url: &str) -> String {
    let Some(rel) = relative(url) else {
        return url.to_string();
    };
    let Some(resolved) = resolve(source_file, rel.path) else {
        return url.to_string();
    };
    format!("{RAW_PREFIX}{resolved}{}", rel.tail)
}

/// Escape text for HTML element content and double-quoted attributes.
pub fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            other => out.push(other),
        }
    }
    out
}

/// Percent-encode a `/`-separated URL path: everything but unreserved characters and `/`.
pub fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => out.push(char::from(byte)),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests;
