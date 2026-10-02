//! The nav index: one walk over a commit's blob paths into the URL -> file map and a sorted page
//! tree, plus every index error the commit carries. A commit with any index error is never
//! published (see [`crate::wiki`]).
//!
//! URL mapping follows GitHub's conventions: `README.md` is `/`, `a/b.md` is `/a/b`, and
//! `a/b/README.md` is `/a/b` when there is no `a/b.md`.

use std::collections::BTreeMap;
use std::fmt;

use git2::Oid;
use thiserror::Error;
use tracing::debug;

use crate::path::{self, PathError};

/// Top-level URL segments riki's own routes own. `status`, `deployed`, and `version` are probed at
/// the root by `sdv`; content may not claim any of these.
pub const RESERVED: &[&str] = &["_riki", "health", "ready", "status", "deployed", "version"];

const PAGE_SUFFIX: &str = ".md";
const DIR_PAGE: &str = "README";

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum IndexError {
    #[error("{file} and {other} both map to /{url}")]
    Collision { url: String, file: String, other: String },
    #[error("{file} claims the reserved name /{name}")]
    Reserved { file: String, name: String },
    #[error("bad path: {0}")]
    BadPath(#[from] PathError),
}

/// One node of the page tree. A directory with no `README.md` is a node with no `file`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageNode {
    /// URL path without the leading `/` (`""` for the root).
    pub url: String,
    /// The `.md` file served at `url`, if any.
    pub file: Option<String>,
    /// The page's first `# heading`, once [`NavIndex::with_titles`] has read the blobs.
    pub title: Option<String>,
    /// Children keyed by URL segment, so iteration is sorted.
    pub children: BTreeMap<String, PageNode>,
}

/// The index for one commit. `errors` empty means the commit is publishable.
#[derive(Debug, Clone)]
pub struct NavIndex {
    commit: Oid,
    pages: BTreeMap<String, String>,
    tree: PageNode,
    errors: Vec<IndexError>,
}

impl NavIndex {
    /// Build the index for `commit` from its blob paths (raw tree-entry bytes, `/`-separated).
    /// Paths with a segment starting with `.` (`.github/`, `.gitignore`) are not content and are
    /// skipped; every other non-`.md` blob is an asset, not a page.
    pub fn build<I, P>(commit: Oid, blob_paths: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: AsRef<[u8]>,
    {
        let mut pages: BTreeMap<String, String> = BTreeMap::new();
        let mut errors = Vec::new();
        for raw in blob_paths {
            let raw = raw.as_ref();
            if is_hidden(raw) {
                continue;
            }
            let file = match path::validate_bytes(raw) {
                Ok(file) => file,
                Err(err) => {
                    errors.push(IndexError::BadPath(err));
                    continue;
                }
            };
            let Some(url) = url_for_file(file) else {
                continue;
            };
            let top = url.split('/').next().unwrap_or_default();
            if RESERVED.contains(&top) {
                errors.push(IndexError::Reserved {
                    file: file.to_string(),
                    name: top.to_string(),
                });
                continue;
            }
            if let Some(other) = pages.get(&url) {
                errors.push(IndexError::Collision {
                    url: url.clone(),
                    file: other.clone(),
                    other: file.to_string(),
                });
                continue;
            }
            pages.insert(url, file.to_string());
        }
        let tree = build_tree(&pages);
        debug!(
            "NavIndex::build: commit={commit} pages={} errors={}",
            pages.len(),
            errors.len()
        );
        Self {
            commit,
            pages,
            tree,
            errors,
        }
    }

    pub fn commit(&self) -> Oid {
        self.commit
    }

    pub fn errors(&self) -> &[IndexError] {
        &self.errors
    }

    pub fn is_publishable(&self) -> bool {
        self.errors.is_empty()
    }

    /// The `.md` file served at `url` (no leading `/`; `""` is the root).
    pub fn file_for_url(&self, url: &str) -> Option<&str> {
        self.pages.get(url).map(String::as_str)
    }

    /// Every page as `(url, file)`, sorted by URL.
    pub fn pages(&self) -> impl Iterator<Item = (&str, &str)> {
        self.pages.iter().map(|(u, f)| (u.as_str(), f.as_str()))
    }

    pub fn tree(&self) -> &PageNode {
        &self.tree
    }

    /// Attach page titles: `title_of(file)` for every page in the tree.
    pub fn with_titles(mut self, title_of: impl Fn(&str) -> Option<String>) -> Self {
        set_titles(&mut self.tree, &title_of);
        self
    }

    /// The node at `url` (no leading `/`; `""` is the root).
    pub fn node(&self, url: &str) -> Option<&PageNode> {
        let mut node = &self.tree;
        if url.is_empty() {
            return Some(node);
        }
        for segment in url.split('/') {
            node = node.children.get(segment)?;
        }
        Some(node)
    }
}

fn set_titles(node: &mut PageNode, title_of: &impl Fn(&str) -> Option<String>) {
    node.title = node.file.as_deref().and_then(title_of);
    for child in node.children.values_mut() {
        set_titles(child, title_of);
    }
}

/// Every index error on one line, for the banner and `/status`.
pub struct ErrorList<'a>(pub &'a [IndexError]);

impl fmt::Display for ErrorList<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, err) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str("; ")?;
            }
            write!(f, "{err}")?;
        }
        Ok(())
    }
}

/// The URL a `.md` file is served at, or `None` when the file is not a page.
pub fn url_for_file(file: &str) -> Option<String> {
    let stem = file.strip_suffix(PAGE_SUFFIX)?;
    let url = match stem.rsplit_once('/') {
        Some((dir, DIR_PAGE)) => dir,
        None if stem == DIR_PAGE => "",
        _ => stem,
    };
    Some(url.to_string())
}

/// A file or folder name as a label, sentence case: `-` and `_` become spaces (runs collapse), the
/// first letter is capitalized, and the rest is kept as written. `getting-started` ->
/// `Getting started`, `API_v2` -> `API v2`.
pub fn prettify(name: &str) -> String {
    let spaced = name
        .split(['-', '_', ' '])
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn is_hidden(raw: &[u8]) -> bool {
    raw.split(|b| *b == b'/').any(|segment| segment.first() == Some(&b'.'))
}

fn build_tree(pages: &BTreeMap<String, String>) -> PageNode {
    let mut root = PageNode::default();
    for (url, file) in pages {
        let mut node = &mut root;
        if !url.is_empty() {
            let mut prefix = String::new();
            for segment in url.split('/') {
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(segment);
                node = node.children.entry(segment.to_string()).or_insert_with(|| PageNode {
                    url: prefix.clone(),
                    ..PageNode::default()
                });
            }
        }
        node.file = Some(file.clone());
    }
    root
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oid() -> Oid {
        Oid::ZERO_SHA1
    }

    #[test]
    fn maps_files_to_urls_github_style() {
        let index = NavIndex::build(oid(), ["README.md", "a/b.md", "c/README.md", "img.png", "c/d/e.md"]);
        assert!(index.is_publishable(), "{:?}", index.errors());
        assert_eq!(index.file_for_url(""), Some("README.md"));
        assert_eq!(index.file_for_url("a/b"), Some("a/b.md"));
        assert_eq!(index.file_for_url("c"), Some("c/README.md"));
        assert_eq!(index.file_for_url("c/d/e"), Some("c/d/e.md"));
        assert_eq!(index.file_for_url("img"), None);
        assert_eq!(index.file_for_url("a"), None);
    }

    #[test]
    fn page_tree_is_sorted_and_nested() {
        let index = NavIndex::build(oid(), ["z.md", "a/b.md", "README.md", "a/README.md"]);
        let root = index.tree();
        assert_eq!(root.file.as_deref(), Some("README.md"));
        let names: Vec<&str> = root.children.keys().map(String::as_str).collect();
        assert_eq!(names, ["a", "z"]);
        let a = &root.children["a"];
        assert_eq!(a.file.as_deref(), Some("a/README.md"));
        assert_eq!(a.children["b"].url, "a/b");
        assert_eq!(a.children["b"].file.as_deref(), Some("a/b.md"));
    }

    #[test]
    fn titles_attach_to_pages_and_nodes_resolve_by_url() {
        let index = NavIndex::build(oid(), ["README.md", "a/b.md", "c/x.md"])
            .with_titles(|file| (file != "c/x.md").then(|| format!("T {file}")));
        assert_eq!(index.tree().title.as_deref(), Some("T README.md"));
        assert_eq!(index.node("a/b").and_then(|n| n.title.as_deref()), Some("T a/b.md"));
        assert_eq!(
            index.node("c").map(|n| n.title.is_none()),
            Some(true),
            "a directory has no title"
        );
        assert_eq!(index.node("c/x").map(|n| n.title.is_none()), Some(true));
        assert!(index.node("nope/deeper").is_none());
        assert_eq!(index.node("").map(|n| n.url.as_str()), Some(""));
    }

    #[test]
    fn file_and_dir_readme_collide() {
        let index = NavIndex::build(oid(), ["a/b.md", "a/b/README.md"]);
        assert!(!index.is_publishable());
        assert!(matches!(&index.errors()[0], IndexError::Collision { url, .. } if url == "a/b"));
    }

    #[test]
    fn reserved_top_level_names_are_errors() {
        for name in RESERVED {
            let file = format!("{name}.md");
            let index = NavIndex::build(oid(), [file.as_str()]);
            assert!(
                matches!(index.errors(), [IndexError::Reserved { name: n, .. }] if n == name),
                "{name}: {:?}",
                index.errors()
            );
        }
        let nested = NavIndex::build(oid(), ["status/README.md", "_riki/x.md"]);
        assert_eq!(nested.errors().len(), 2);
        let deep = NavIndex::build(oid(), ["docs/status.md"]);
        assert!(deep.is_publishable());
    }

    #[test]
    fn hidden_paths_are_skipped_not_indexed() {
        let index = NavIndex::build(oid(), [".github/PULL_REQUEST_TEMPLATE.md", ".gitignore", "a/.draft.md"]);
        assert!(index.is_publishable());
        assert_eq!(index.pages().count(), 0);
    }

    #[test]
    fn non_utf8_path_is_an_index_error() {
        let index = NavIndex::build(oid(), [b"a/\xff.md".as_slice()]);
        assert!(matches!(index.errors(), [IndexError::BadPath(PathError::NotUtf8(_))]));
    }

    #[test]
    fn error_list_joins_on_one_line() {
        let index = NavIndex::build(oid(), ["status.md", "a.md", "a/README.md"]);
        let line = ErrorList(index.errors()).to_string();
        assert!(line.contains("reserved name /status"), "{line}");
        assert!(line.contains("; "), "{line}");
        assert!(line.contains("map to /a"), "{line}");
    }

    #[test]
    fn prettify_turns_names_into_sentence_case_labels() {
        assert_eq!(prettify("reference"), "Reference");
        assert_eq!(prettify("getting-started"), "Getting started");
        assert_eq!(prettify("release_notes"), "Release notes");
        assert_eq!(prettify("API_v2"), "API v2");
        assert_eq!(prettify("a--b__c"), "A b c");
        assert_eq!(prettify("été"), "Été");
    }

    #[test]
    fn prettify_of_nothing_is_empty() {
        assert_eq!(prettify(""), "");
        assert_eq!(prettify("--"), "");
    }

    #[test]
    fn url_for_file_ignores_non_pages() {
        assert_eq!(url_for_file("x.png"), None);
        assert_eq!(url_for_file("README.md").as_deref(), Some(""));
        assert_eq!(url_for_file("a/README.md").as_deref(), Some("a"));
        assert_eq!(url_for_file("READMEx.md").as_deref(), Some("READMEx"));
    }
}
