//! Page file rules: the front matter split, the static rules, the trailing-newline rule, and the
//! round-trip comparison.
//!
//! The front matter boundary is whatever comrak parses as front matter: riki parses the file and
//! takes the `NodeValue::FrontMatter` text as the prefix (comrak's own splitter is private), so
//! the editor's body and the renderer agree on where the body starts.

use std::fmt;

use comrak::nodes::NodeValue;
use comrak::{Arena, Options, parse_document};
use thiserror::Error;
use tracing::debug;

use crate::index::{RESERVED, url_for_file};
use crate::path::{self, PathError};

/// The delimiter the renderer also uses (`render::render_markdown`).
pub const FRONT_MATTER_DELIMITER: &str = "---";

/// What a new page ends with.
pub const NEW_PAGE_TRAILING: &str = "\n";

const BOM: &str = "\u{feff}";

/// A page file cut at the front matter boundary: `front_matter + body` is the whole file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Split<'a> {
    pub front_matter: &'a str,
    pub body: &'a str,
}

/// Split `text` where comrak's parsed front matter ends. No front matter: the prefix is empty.
pub fn split_front_matter(text: &str) -> Split<'_> {
    let mut options = Options::default();
    options.extension.front_matter_delimiter = Some(FRONT_MATTER_DELIMITER.to_string());
    let arena = Arena::new();
    let root = parse_document(&arena, text, &options);
    let parsed = root.first_child().and_then(|node| match &node.data().value {
        NodeValue::FrontMatter(front_matter) => Some(front_matter.len()),
        _ => None,
    });
    // comrak skips a leading BOM before matching the opener, so the parsed text starts after it.
    let offset = if text.starts_with(BOM) { BOM.len() } else { 0 };
    let end = parsed.map_or(0, |len| offset + len);
    debug!("split_front_matter: bytes={} front_matter={end}", text.len());
    let (front_matter, body) = text.split_at(end);
    Split { front_matter, body }
}

/// Why a path cannot be a page a save writes.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PagePathError {
    #[error(transparent)]
    Path(#[from] PathError),
    #[error("{0:?} is not a `.md` file")]
    NotMarkdown(String),
    #[error("{path:?} claims the reserved name /{name}")]
    Reserved { path: String, name: String },
}

/// A page path a save may write: a valid path, a `.md` file, and not under a reserved name (the
/// same rule the nav index applies).
pub fn validate_page_path(path: &str) -> Result<(), PagePathError> {
    path::validate(path)?;
    let url = url_for_file(path).ok_or_else(|| PagePathError::NotMarkdown(path.to_string()))?;
    let top = url.split('/').next().unwrap_or_default();
    if RESERVED.contains(&top) {
        return Err(PagePathError::Reserved {
            path: path.to_string(),
            name: top.to_string(),
        });
    }
    Ok(())
}

/// A rule that makes a file never editable in the browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticRule {
    NotUtf8,
    LeadingBom,
    CarriageReturn,
}

impl fmt::Display for StaticRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotUtf8 => "the file is not valid UTF-8",
            Self::LeadingBom => "the file starts with a byte-order mark",
            Self::CarriageReturn => "the file contains a carriage return (\\r)",
        })
    }
}

/// The page text when `bytes` passes every static rule, else the first rule it breaks.
pub fn check_static_rules(bytes: &[u8]) -> Result<&str, StaticRule> {
    let text = std::str::from_utf8(bytes).map_err(|_| StaticRule::NotUtf8)?;
    if text.starts_with(BOM) {
        return Err(StaticRule::LeadingBom);
    }
    if text.contains('\r') {
        return Err(StaticRule::CarriageReturn);
    }
    Ok(text)
}

/// The run of `\n` that ends `text` (empty when it does not end with a newline).
pub fn trailing_newlines(text: &str) -> &str {
    &text[text.trim_end_matches('\n').len()..]
}

/// The bytes a save writes: `front_matter`, then `body` with its own trailing newlines replaced by
/// `trailing` (the original file's trailing-newline state, or [`NEW_PAGE_TRAILING`]). An empty
/// body under front matter writes the front matter alone; a front matter block that ends without
/// a newline gets one before a non-empty body.
pub fn compose(front_matter: &str, body: &str, trailing: &str) -> String {
    let body = body.trim_end_matches('\n');
    if body.is_empty() && !front_matter.is_empty() {
        return front_matter.to_string();
    }
    let separator = if !front_matter.is_empty() && !front_matter.ends_with('\n') {
        "\n"
    } else {
        ""
    };
    format!("{front_matter}{separator}{body}{trailing}")
}

/// The first line (1-based) where `serialized` differs from `stored`, trailing newlines aside, or
/// `None` when they are identical. Trailing newlines are aside because a save replaces them with
/// the file's own state (see [`compose`]).
pub fn first_diff_line(stored: &str, serialized: &str) -> Option<usize> {
    let stored = stored.trim_end_matches('\n');
    let serialized = serialized.trim_end_matches('\n');
    if stored == serialized {
        return None;
    }
    let mut left = stored.split('\n');
    let mut right = serialized.split('\n');
    let mut line = 1;
    loop {
        match (left.next(), right.next()) {
            (Some(a), Some(b)) if a == b => line += 1,
            _ => return Some(line),
        }
    }
}

#[cfg(test)]
mod tests;
