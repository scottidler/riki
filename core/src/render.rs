//! Markdown to HTML: comrak configured per the design, with link and image rewriting.
//!
//! Relative links and images resolve against the **source file's** directory, not the browser
//! URL: `/a/b` may be served from `a/b/README.md`, so `../x.md` there means `a/x.md`. Resolved
//! `.md` links become riki URLs (`x.md` -> `/x`); resolved images become `/_riki/raw/<path>`.
//! Absolute and external URLs pass through untouched.

use std::sync::Arc;

use comrak::{Options, markdown_to_html};
use tracing::debug;

use crate::index::url_for_file;

/// Where image blobs are served from.
pub const RAW_PREFIX: &str = "/_riki/raw/";

/// Render `markdown` (the body of `source_file`, a repo-relative path) to an HTML fragment.
/// Safe mode: raw HTML is omitted and `javascript:` hrefs are blanked.
pub fn render_markdown(source_file: &str, markdown: &str) -> String {
    debug!("render_markdown: source_file={source_file} bytes={}", markdown.len());
    let mut options = Options::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.tasklist = true;
    options.extension.autolink = true;
    options.extension.alerts = true;
    options.extension.front_matter_delimiter = Some("---".to_string());
    options.extension.header_id_prefix = Some(String::new());
    let for_links = source_file.to_string();
    options.extension.link_url_rewriter = Some(Arc::new(move |url: &str| rewrite_link(&for_links, url)));
    let for_images = source_file.to_string();
    options.extension.image_url_rewriter = Some(Arc::new(move |url: &str| rewrite_image(&for_images, url)));
    markdown_to_html(markdown, &options)
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
