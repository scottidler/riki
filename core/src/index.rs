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
/// The per-folder sidebar order file. Not a dot name (`path::validate` refuses those) and not
/// `.md`, so it never enters the nav.
pub const ORDER_FILE: &str = "_order";
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
    /// The folder's `_order` entries that name a child, in file order, each once. Empty without
    /// an `_order` file. Unknown entries are dropped by [`NavIndex::with_orders`].
    pub order: Vec<String>,
}

/// A problem with an `_order` file. Never an index error: the sidebar skips what it can't use
/// and publish goes on.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum OrderWarning {
    #[error("{file} names \"{entry}\", which is not a page or folder there")]
    UnknownEntry { file: String, entry: String },
    #[error("{file} is not valid UTF-8, so it is ignored")]
    NotUtf8 { file: String },
    #[error("{file} is in a folder with no pages, so it is ignored")]
    NoPages { file: String },
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

    /// Attach each folder's sidebar order. `orders` is `(file, bytes)` for every `_order` file
    /// (see [`order_files`]); the folder is the file's directory. Returns what could not be used.
    pub fn with_orders(mut self, orders: impl IntoIterator<Item = (String, Vec<u8>)>) -> (Self, Vec<OrderWarning>) {
        let mut warnings = Vec::new();
        for (file, bytes) in orders {
            let folder = file.strip_suffix(ORDER_FILE).unwrap_or(&file).trim_end_matches('/');
            let Ok(text) = String::from_utf8(bytes) else {
                warnings.push(OrderWarning::NotUtf8 { file });
                continue;
            };
            let Some(node) = self.node_mut(folder).filter(|node| !node.children.is_empty()) else {
                warnings.push(OrderWarning::NoPages { file });
                continue;
            };
            node.order.clear();
            for entry in order_entries(&text) {
                if !node.children.contains_key(entry) {
                    warnings.push(OrderWarning::UnknownEntry {
                        file: file.clone(),
                        entry: entry.to_string(),
                    });
                } else if !node.order.iter().any(|seen| seen == entry) {
                    node.order.push(entry.to_string());
                }
            }
        }
        (self, warnings)
    }

    fn node_mut(&mut self, url: &str) -> Option<&mut PageNode> {
        let mut node = &mut self.tree;
        if url.is_empty() {
            return Some(node);
        }
        for segment in url.split('/') {
            node = node.children.get_mut(segment)?;
        }
        Some(node)
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

/// The `_order` files among a commit's blob paths: not hidden, valid UTF-8, named `_order`.
pub fn order_files<I, P>(blob_paths: I) -> Vec<String>
where
    I: IntoIterator<Item = P>,
    P: AsRef<[u8]>,
{
    blob_paths
        .into_iter()
        .filter(|raw| !is_hidden(raw.as_ref()))
        .filter_map(|raw| std::str::from_utf8(raw.as_ref()).ok().map(str::to_string))
        .filter(|file| file.rsplit('/').next() == Some(ORDER_FILE))
        .collect()
}

/// One name per line; blank lines and `#` lines skipped.
fn order_entries(text: &str) -> impl Iterator<Item = &str> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
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

/// The label a node shows: its page title (front matter `title`, else first H1; for a directory,
/// its README's), else its prettified URL segment, else `Home` for the root.
pub fn label(node: &PageNode, segment: &str) -> String {
    match &node.title {
        Some(title) => title.clone(),
        None if node.url.is_empty() => "Home".to_string(),
        None => prettify(segment),
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

    /// Search text lives only in the published snapshot (`wiki::Published::search`): the nav is
    /// cached per oid and never evicted. Adding a field to `NavIndex` breaks this destructure, so
    /// a text field cannot slip in unreviewed.
    #[test]
    fn nav_index_has_no_text_field() {
        let NavIndex {
            commit: _,
            pages: _,
            tree: _,
            errors: _,
        } = NavIndex::build(oid(), ["a.md"]);
        let PageNode {
            url: _,
            file: _,
            title: _,
            children: _,
            order: _,
        } = PageNode::default();
    }

    #[test]
    fn label_is_the_title_else_the_prettified_segment_else_home() {
        let titled = PageNode {
            title: Some("Guide".to_string()),
            url: "g".to_string(),
            ..PageNode::default()
        };
        assert_eq!(label(&titled, "g"), "Guide");
        let untitled = PageNode {
            url: "getting-started".to_string(),
            ..PageNode::default()
        };
        assert_eq!(label(&untitled, "getting-started"), "Getting started");
        assert_eq!(label(&PageNode::default(), ""), "Home");
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

    fn ordered(files: &[&str], orders: &[(&str, &str)]) -> (NavIndex, Vec<OrderWarning>) {
        NavIndex::build(oid(), files.iter().copied()).with_orders(
            orders
                .iter()
                .map(|(file, text)| (file.to_string(), text.as_bytes().to_vec())),
        )
    }

    #[test]
    fn order_keeps_known_entries_in_file_order_once_and_skips_comments_and_blanks() {
        let (index, warnings) = ordered(&["a.md", "b.md", "c/x.md"], &[("_order", "# first\n\n c \nb\nc\nb\n")]);
        assert_eq!(index.tree().order, ["c", "b"]);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn order_entry_naming_nothing_is_a_warning_and_dropped() {
        let (index, warnings) = ordered(&["a.md", "g/README.md", "g/x.md"], &[("_order", "gone\ng\nREADME\n")]);
        assert_eq!(index.tree().order, ["g"]);
        assert_eq!(
            warnings,
            [
                OrderWarning::UnknownEntry {
                    file: "_order".into(),
                    entry: "gone".into()
                },
                OrderWarning::UnknownEntry {
                    file: "_order".into(),
                    entry: "README".into()
                },
            ]
        );
    }

    #[test]
    fn order_in_a_nested_folder_attaches_to_that_folder() {
        let (index, warnings) = ordered(&["g/a.md", "g/b.md"], &[("g/_order", "b\na\n")]);
        assert_eq!(
            index.node("g").map(|n| n.order.clone()),
            Some(vec!["b".into(), "a".into()])
        );
        assert!(index.tree().order.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn unusable_order_files_warn_instead_of_failing() {
        let index = NavIndex::build(oid(), ["a.md", "leaf/README.md"]);
        let (index, warnings) = index.with_orders([
            ("_order".to_string(), vec![0xff, 0xfe]),
            ("leaf/_order".to_string(), b"x\n".to_vec()),
            ("nowhere/_order".to_string(), b"x\n".to_vec()),
        ]);
        assert!(index.tree().order.is_empty());
        assert_eq!(
            warnings,
            [
                OrderWarning::NotUtf8 { file: "_order".into() },
                OrderWarning::NoPages {
                    file: "leaf/_order".into()
                },
                OrderWarning::NoPages {
                    file: "nowhere/_order".into()
                },
            ]
        );
        assert!(index.is_publishable());
    }

    #[test]
    fn order_files_finds_visible_order_files_only() {
        let found = order_files([
            "_order",
            "a/_order",
            "a/b.md",
            ".git/_order",
            "a/.x/_order",
            "a/_order.md",
        ]);
        assert_eq!(found, ["_order", "a/_order"]);
    }

    #[test]
    fn an_order_file_is_never_a_page() {
        let index = NavIndex::build(oid(), ["_order", "a/_order"]);
        assert!(index.is_publishable());
        assert_eq!(index.pages().count(), 0);
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
