//! Title -> new page path. The slug is advisory: the check runs at the good tip, and a page that
//! appears before Save is the save's own 409.

use thiserror::Error;
use tracing::debug;

use crate::index::{NavIndex, RESERVED};
use crate::path::{self, PathError};

const UNTITLED: &str = "untitled";

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SlugError {
    #[error("invalid folder: {0}")]
    BadFolder(#[from] PathError),
    #[error("folder {0:?} starts with the reserved name /{1}")]
    ReservedFolder(String, String),
}

/// Lowercase; keep Unicode letters and digits; every other run becomes one `-`; trim `-`; empty
/// becomes `untitled`.
pub fn slugify(title: &str) -> String {
    let mut slug = String::new();
    let mut pending_dash = false;
    for ch in title.chars().flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            slug.push(ch);
        } else {
            pending_dash = true;
        }
    }
    if slug.is_empty() { UNTITLED.to_string() } else { slug }
}

/// The path a new page titled `title` gets in `folder` (`""` is the root), given the nav index of
/// the good tip. Taken when the tip has `<folder>/<slug>.md` or `<folder>/<slug>/README.md` (both
/// map to one URL) or the URL is reserved at the top level; then `-2`, `-3`, ... The folder need
/// not exist yet.
pub fn new_page_path(index: &NavIndex, folder: &str, title: &str) -> Result<String, SlugError> {
    debug!("new_page_path: folder={folder:?} title={title:?}");
    if !folder.is_empty() {
        path::validate(folder)?;
        let top = folder.split('/').next().unwrap_or_default();
        if RESERVED.contains(&top) {
            return Err(SlugError::ReservedFolder(folder.to_string(), top.to_string()));
        }
    }
    let base = slugify(title);
    let prefix = if folder.is_empty() {
        String::new()
    } else {
        format!("{folder}/")
    };
    let mut n = 1u32;
    loop {
        let slug = if n == 1 { base.clone() } else { format!("{base}-{n}") };
        let url = format!("{prefix}{slug}");
        let top = url.split('/').next().unwrap_or_default();
        let taken = index.file_for_url(&url).is_some() || (folder.is_empty() && RESERVED.contains(&top));
        if !taken {
            return Ok(format!("{url}.md"));
        }
        n += 1;
    }
}

#[cfg(test)]
mod tests {
    use git2::Oid;

    use super::*;

    fn index(paths: &[&str]) -> NavIndex {
        NavIndex::build(Oid::ZERO_SHA1, paths.iter().copied())
    }

    #[test]
    fn slugify_follows_the_rule() {
        assert_eq!(slugify("Getting Started"), "getting-started");
        assert_eq!(slugify("  --Hello,   World!--  "), "hello-world");
        assert_eq!(slugify("Café notes"), "café-notes");
        assert_eq!(slugify("AC4 probe"), "ac4-probe");
        assert_eq!(slugify("a/b\\c"), "a-b-c");
    }

    #[test]
    fn slugify_of_nothing_is_untitled() {
        assert_eq!(slugify(""), "untitled");
        assert_eq!(slugify(" !?- "), "untitled");
    }

    #[test]
    fn a_free_slug_is_used() {
        let idx = index(&["README.md", "guide/intro.md"]);
        assert_eq!(
            new_page_path(&idx, "guide", "Getting Started").unwrap(),
            "guide/getting-started.md"
        );
        assert_eq!(new_page_path(&idx, "", "Hello").unwrap(), "hello.md");
    }

    #[test]
    fn a_page_file_takes_the_slug() {
        let idx = index(&["guide/getting-started.md"]);
        assert_eq!(
            new_page_path(&idx, "guide", "Getting Started").unwrap(),
            "guide/getting-started-2.md"
        );
    }

    #[test]
    fn a_folder_readme_takes_the_slug() {
        let idx = index(&["guide/getting-started/README.md"]);
        assert_eq!(
            new_page_path(&idx, "guide", "Getting Started").unwrap(),
            "guide/getting-started-2.md"
        );
    }

    #[test]
    fn suffixes_climb_past_every_taken_one() {
        let idx = index(&["a.md", "a-2.md", "a-3/README.md"]);
        assert_eq!(new_page_path(&idx, "", "A").unwrap(), "a-4.md");
    }

    #[test]
    fn a_reserved_top_level_url_is_taken() {
        let idx = index(&["README.md"]);
        assert_eq!(new_page_path(&idx, "", "Status").unwrap(), "status-2.md");
        assert_eq!(new_page_path(&idx, "", "_riki").unwrap(), "riki.md");
        assert_eq!(new_page_path(&idx, "docs", "Status").unwrap(), "docs/status.md");
    }

    #[test]
    fn a_new_folder_is_allowed_and_a_bad_one_is_not() {
        let idx = index(&["README.md"]);
        assert_eq!(new_page_path(&idx, "brand/new", "X").unwrap(), "brand/new/x.md");
        assert!(matches!(new_page_path(&idx, "../x", "X"), Err(SlugError::BadFolder(_))));
        assert!(matches!(new_page_path(&idx, ".git", "X"), Err(SlugError::BadFolder(_))));
        assert!(matches!(new_page_path(&idx, "a//b", "X"), Err(SlugError::BadFolder(_))));
        assert!(matches!(
            new_page_path(&idx, "status/sub", "X"),
            Err(SlugError::ReservedFolder(..))
        ));
    }
}
