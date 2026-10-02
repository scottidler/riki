//! Full-text search over the good tip (design doc, Data Model "Search index"): an in-memory
//! inverted index built by `publish` and held in the `Published` snapshot beside the nav, never in
//! the per-oid nav cache.
//!
//! The unit is a **section**: a page's text from one heading to the next heading of any level
//! (text before the first heading is the page's unheaded section). Text comes from the comrak AST,
//! so no Markdown syntax reaches the index. A query is AND over prefix-matched terms, scored per
//! term by the best field it hit (title 3, heading 2, body 1), one hit per page.

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use comrak::nodes::{AstNode, NodeValue};
use comrak::{Anchorizer, Arena, parse_document};
use tracing::debug;

use crate::page::{StaticRule, check_static_rules};

/// The most characters a snippet carries.
pub const SNIPPET_CHARS: usize = 160;

/// Where a term occurs in a section. Ordered by weight, so the best field is the max.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Field {
    Body,
    Heading,
    Title,
}

impl Field {
    pub fn weight(self) -> u32 {
        match self {
            Self::Body => 1,
            Self::Heading => 2,
            Self::Title => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Posting {
    section: u32,
    field: Field,
}

/// One page as publish hands it to [`SearchIndex::build`]: its file, its URL (no leading `/`),
/// the label the sidebar shows, and the blob.
#[derive(Debug, Clone)]
pub struct PageSource {
    pub path: String,
    pub url: String,
    pub title: String,
    pub bytes: Vec<u8>,
}

/// A page left out of the index because it breaks a static rule. Never an error: publish goes on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub path: String,
    pub rule: StaticRule,
}

impl fmt::Display for Skipped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} is not searchable: {}", self.path, self.rule)
    }
}

#[derive(Debug, Clone)]
struct Page {
    path: String,
    url: String,
    title: String,
}

#[derive(Debug, Clone)]
struct Section {
    page: u32,
    heading: Option<String>,
    /// The id the reader gives `heading`, from the same anchorizer run the renderer does.
    anchor: Option<String>,
    text: String,
}

/// One search result: the best section of a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub path: String,
    /// No leading `/`; `""` is the root.
    pub url: String,
    pub title: String,
    pub heading: Option<String>,
    pub anchor: Option<String>,
    pub snippet: String,
    /// `[start, end)` of each match in `snippet`, in UTF-16 code units.
    pub marks: Vec<[usize; 2]>,
    pub score: u32,
}

#[derive(Debug, Clone, Default)]
pub struct SearchIndex {
    pages: Vec<Page>,
    sections: Vec<Section>,
    postings: BTreeMap<String, Vec<Posting>>,
}

impl SearchIndex {
    /// Index `pages`. A page breaking a static rule (non-UTF-8, BOM, CR) is left out and reported.
    pub fn build(pages: impl IntoIterator<Item = PageSource>) -> (Self, Vec<Skipped>) {
        let mut index = Self::default();
        let mut skipped = Vec::new();
        for source in pages {
            let text = match check_static_rules(&source.bytes) {
                Ok(text) => text,
                Err(rule) => {
                    skipped.push(Skipped {
                        path: source.path,
                        rule,
                    });
                    continue;
                }
            };
            index.add(&source.path, &source.url, &source.title, text);
        }
        debug!(
            "SearchIndex::build: pages={} sections={} terms={} skipped={}",
            index.pages.len(),
            index.sections.len(),
            index.postings.len(),
            skipped.len()
        );
        (index, skipped)
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    pub fn section_count(&self) -> usize {
        self.sections.len()
    }

    pub fn term_count(&self) -> usize {
        self.postings.len()
    }

    fn add(&mut self, path: &str, url: &str, title: &str, markdown: &str) {
        let page = u32::try_from(self.pages.len()).expect("SearchIndex: more pages than u32");
        self.pages.push(Page {
            path: path.to_string(),
            url: url.to_string(),
            title: title.to_string(),
        });
        let title_terms = terms(title);
        for raw in sections(markdown) {
            let section = u32::try_from(self.sections.len()).expect("SearchIndex: more sections than u32");
            self.post(section, Field::Title, &title_terms);
            if let Some(heading) = &raw.heading {
                self.post(section, Field::Heading, &terms(heading));
            }
            self.post(section, Field::Body, &terms(&raw.text));
            self.sections.push(Section {
                page,
                heading: raw.heading,
                anchor: raw.anchor,
                text: raw.text,
            });
        }
    }

    fn post(&mut self, section: u32, field: Field, terms: &[String]) {
        let posting = Posting { section, field };
        for term in terms {
            let list = self.postings.entry(term.clone()).or_default();
            // One section's field is posted in one go, so a repeat is always the last entry.
            if list.last() != Some(&posting) {
                list.push(posting);
            }
        }
    }

    /// The best section of each page matching every term of `query` by prefix, highest score
    /// first, ties by path; at most `limit` hits. An empty query matches nothing.
    pub fn query(&self, query: &str, limit: usize) -> Vec<Hit> {
        let mut wanted = terms(query);
        wanted.sort();
        wanted.dedup();
        if wanted.is_empty() {
            return Vec::new();
        }
        let mut scores: Option<HashMap<u32, u32>> = None;
        for term in &wanted {
            let best = self.best_fields(term);
            scores = Some(match scores {
                None => best,
                Some(previous) => previous
                    .into_iter()
                    .filter_map(|(section, score)| best.get(&section).map(|weight| (section, score + weight)))
                    .collect(),
            });
        }
        let mut per_page: HashMap<u32, (u32, u32)> = HashMap::new();
        for (section, score) in scores.unwrap_or_default() {
            let page = self.sections[section as usize].page;
            let entry = per_page.entry(page).or_insert((score, section));
            if score > entry.0 || (score == entry.0 && section < entry.1) {
                *entry = (score, section);
            }
        }
        let mut ranked: Vec<(u32, u32, u32)> = per_page
            .into_iter()
            .map(|(page, (score, section))| (score, page, section))
            .collect();
        ranked.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| self.pages[a.1 as usize].path.cmp(&self.pages[b.1 as usize].path))
        });
        ranked.truncate(limit);
        debug!("SearchIndex::query: terms={wanted:?} hits={}", ranked.len());
        ranked
            .into_iter()
            .map(|(score, page, section)| self.hit(score, page, section, &wanted))
            .collect()
    }

    /// Section -> the best field `term` prefix-matches in it.
    fn best_fields(&self, term: &str) -> HashMap<u32, u32> {
        let mut best: HashMap<u32, u32> = HashMap::new();
        let matching = self
            .postings
            .range(term.to_string()..)
            .take_while(|(key, _)| key.starts_with(term));
        for (_, postings) in matching {
            for posting in postings {
                let weight = best.entry(posting.section).or_insert(0);
                *weight = (*weight).max(posting.field.weight());
            }
        }
        best
    }

    fn hit(&self, score: u32, page: u32, section: u32, terms: &[String]) -> Hit {
        let page = &self.pages[page as usize];
        let section = &self.sections[section as usize];
        let (snippet, marks) = snippet(&section.text, terms);
        Hit {
            path: page.path.clone(),
            url: page.url.clone(),
            title: page.title.clone(),
            heading: section.heading.clone(),
            anchor: section.anchor.clone(),
            snippet,
            marks,
            score,
        }
    }
}

/// Lowercase terms: runs of Unicode letters and digits, the slug rule's alphabet.
pub fn terms(text: &str) -> Vec<String> {
    tokens(text)
        .into_iter()
        .map(|token| token.text.to_lowercase())
        .collect()
}

/// A run of letters and digits in a text, with its char offsets.
struct Token<'a> {
    start: usize,
    text: &'a str,
}

fn tokens(text: &str) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    let mut run: Option<(usize, usize)> = None;
    for (chars, (byte, ch)) in text.char_indices().enumerate() {
        match (ch.is_alphanumeric(), run) {
            (true, None) => run = Some((chars, byte)),
            (false, Some((start, from))) => {
                out.push(Token {
                    start,
                    text: &text[from..byte],
                });
                run = None;
            }
            _ => {}
        }
    }
    if let Some((start, from)) = run {
        out.push(Token {
            start,
            text: &text[from..],
        });
    }
    out
}

/// How many chars of `token` the longest of `terms` prefix-matches, case-insensitively.
fn matched_chars(token: &str, terms: &[String]) -> Option<usize> {
    let lowered = token.to_lowercase();
    let term = terms
        .iter()
        .filter(|term| lowered.starts_with(term.as_str()))
        .max_by_key(|term| term.len())?;
    let mut length = 0;
    for (count, ch) in token.chars().enumerate() {
        length += ch.to_lowercase().map(char::len_utf8).sum::<usize>();
        if length >= term.len() {
            return Some(count + 1);
        }
    }
    Some(token.chars().count())
}

/// Up to [`SNIPPET_CHARS`] of `text` centered on the first match of any term, with every match in
/// it as UTF-16 ranges. No match in the text (the hit was the title or heading): its start.
fn snippet(text: &str, terms: &[String]) -> (String, Vec<[usize; 2]>) {
    let matches: Vec<(usize, usize)> = tokens(text)
        .iter()
        .filter_map(|token| matched_chars(token.text, terms).map(|len| (token.start, token.start + len)))
        .collect();
    let chars: Vec<char> = text.chars().collect();
    let total = chars.len();
    let start = match matches.first() {
        Some(&(first, last)) => {
            let lead = SNIPPET_CHARS.saturating_sub(last - first) / 2;
            let start = first.saturating_sub(lead);
            (start + SNIPPET_CHARS).min(total).saturating_sub(SNIPPET_CHARS)
        }
        None => 0,
    };
    let end = (start + SNIPPET_CHARS).min(total);
    let window = &chars[start..end];
    let mut utf16 = Vec::with_capacity(window.len() + 1);
    let mut units = 0;
    for ch in window {
        utf16.push(units);
        units += ch.len_utf16();
    }
    utf16.push(units);
    let marks = matches
        .into_iter()
        .filter(|&(from, to)| from >= start && to <= end)
        .map(|(from, to)| [utf16[from - start], utf16[to - start]])
        .collect();
    (window.iter().collect(), marks)
}

/// One section as the AST walk produces it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RawSection {
    heading: Option<String>,
    anchor: Option<String>,
    text: String,
}

/// The page's sections, from the comrak AST the renderer parses (front matter and raw HTML
/// excluded, code included). Every heading takes its anchor from one [`Anchorizer`] in document
/// order, as the renderer's heading ids do, so duplicate headings get the same `-1` suffixes. The
/// unheaded leading section is dropped when empty; a page with no text keeps one empty section so
/// its title still matches.
fn sections(markdown: &str) -> Vec<RawSection> {
    let options = crate::render::options("");
    let arena = Arena::new();
    let root = parse_document(&arena, markdown, &options);
    let mut walk = Walk {
        anchorizer: Anchorizer::new(),
        sections: vec![RawSection {
            heading: None,
            anchor: None,
            text: String::new(),
        }],
    };
    walk.node(root);
    let mut sections: Vec<RawSection> = walk
        .sections
        .into_iter()
        .map(|section| RawSection {
            text: collapse(&section.text),
            ..section
        })
        .collect();
    if sections.len() > 1 && sections[0].text.is_empty() {
        sections.remove(0);
    }
    sections
}

struct Walk {
    anchorizer: Anchorizer,
    sections: Vec<RawSection>,
}

impl Walk {
    fn node<'a>(&mut self, node: &'a AstNode<'a>) {
        let value = &node.data().value;
        match value {
            NodeValue::FrontMatter(_) | NodeValue::HtmlBlock(_) | NodeValue::HtmlInline(_) | NodeValue::Raw(_) => {}
            NodeValue::Heading(_) => {
                let text = node.collect_text();
                let anchor = self.anchorizer.anchorize(&text);
                self.sections.push(RawSection {
                    heading: Some(collapse(&text)),
                    anchor: Some(anchor),
                    text: String::new(),
                });
            }
            NodeValue::Text(literal) => self.push(literal),
            NodeValue::Code(code) => self.push(&code.literal),
            NodeValue::Math(math) => self.push(&math.literal),
            NodeValue::CodeBlock(block) => {
                self.push(&block.literal);
                self.push(" ");
            }
            NodeValue::SoftBreak | NodeValue::LineBreak => self.push(" "),
            _ => {
                for child in node.children() {
                    self.node(child);
                }
                if value.block() {
                    self.push(" ");
                }
            }
        }
    }

    fn push(&mut self, text: &str) {
        if let Some(section) = self.sections.last_mut() {
            section.text.push_str(text);
        }
    }
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests;
