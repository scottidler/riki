use super::*;
use crate::render::render_markdown;

fn page(path: &str, title: &str, body: &str) -> PageSource {
    let url = crate::index::url_for_file(path).expect("page path");
    PageSource {
        path: path.to_string(),
        url,
        title: title.to_string(),
        bytes: body.as_bytes().to_vec(),
    }
}

fn index(pages: Vec<PageSource>) -> SearchIndex {
    let (index, skipped) = SearchIndex::build(pages);
    assert!(skipped.is_empty(), "{skipped:?}");
    index
}

/// The text a mark covers, decoding the UTF-16 offsets the client gets.
fn marked(hit: &Hit) -> Vec<String> {
    let units: Vec<u16> = hit.snippet.encode_utf16().collect();
    hit.marks
        .iter()
        .map(|[start, end]| String::from_utf16(&units[*start..*end]).expect("utf-16"))
        .collect()
}

#[test]
fn terms_lowercase_and_split_on_non_alphanumerics() {
    assert_eq!(terms("Café-notes, v2 (draft)"), ["café", "notes", "v2", "draft"]);
    assert!(terms("  --  ").is_empty());
}

#[test]
fn a_prefix_finds_the_page_with_its_heading_anchor_and_snippet() {
    let idx = index(vec![
        page("README.md", "Home", "# Home\n\nWelcome.\n"),
        page(
            "reference/tables.md",
            "Reference tables",
            "# Reference tables\n\nIntro.\n\n## Alignment\n\nColumns align with colons in a table row.\n",
        ),
    ]);
    let hits = idx.query("align", 20);
    assert_eq!(hits.len(), 1);
    let hit = &hits[0];
    assert_eq!(hit.path, "reference/tables.md");
    assert_eq!(hit.url, "reference/tables");
    assert_eq!(hit.title, "Reference tables");
    assert_eq!(hit.heading.as_deref(), Some("Alignment"));
    assert_eq!(hit.anchor.as_deref(), Some("alignment"));
    assert_eq!(hit.snippet, "Columns align with colons in a table row.");
    assert_eq!(marked(hit), ["align"]);
}

#[test]
fn no_markdown_syntax_reaches_the_text_and_code_is_included() {
    let idx = index(vec![page(
        "a.md",
        "A",
        "---\ntitle: Secret\n---\nSee **bold** [link](x.md) and `inline_code`.\n\n<div>rawhtml</div>\n\n```rust\nfn fenced() {}\n```\n\n| h1 | h2 |\n|---|---|\n| cell | other |\n",
    )]);
    let hit = &idx.query("bold", 1)[0];
    assert_eq!(
        hit.snippet,
        "See bold link and inline_code. fn fenced() {} h1 h2 cell other"
    );
    assert_eq!(idx.query("fenced", 1).len(), 1, "code blocks are searchable");
    assert_eq!(idx.query("inline", 1).len(), 1, "inline code is searchable");
    assert!(idx.query("secret", 1).is_empty(), "front matter is not text");
    assert!(idx.query("rawhtml", 1).is_empty(), "raw HTML is not text");
    assert!(idx.query("x", 1).is_empty(), "link destinations are not text");
}

#[test]
fn every_term_must_match_and_the_best_field_scores() {
    let idx = index(vec![
        page("a.md", "Tables", "# Tables\n\nPlain text.\n"),
        page("b.md", "Other", "# Other\n\n## Tables here\n\nwords\n"),
        page("c.md", "Third", "# Third\n\ntables are mentioned in passing\n"),
    ]);
    let hits = idx.query("tab", 20);
    let order: Vec<(&str, u32)> = hits.iter().map(|h| (h.path.as_str(), h.score)).collect();
    assert_eq!(order, [("a.md", 3), ("b.md", 2), ("c.md", 1)], "title > heading > body");
    let both = idx.query("tables passing", 20);
    assert_eq!(both.len(), 1, "AND: only c.md has both");
    assert_eq!(both[0].path, "c.md");
    assert!(idx.query("tables nowhere", 20).is_empty());
    assert!(idx.query("", 20).is_empty(), "an empty query matches nothing");
    assert!(idx.query("  ,, ", 20).is_empty());
}

#[test]
fn a_title_term_counts_in_every_section_of_its_page() {
    let idx = index(vec![page(
        "guide.md",
        "Guide",
        "# Guide\n\nTop.\n\n## Install\n\nRun the installer.\n",
    )]);
    let hits = idx.query("guide installer", 20);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].heading.as_deref(), Some("Install"));
    assert_eq!(hits[0].score, 4);
}

#[test]
fn one_hit_per_page_ties_by_path_and_limit_caps() {
    let idx = index(vec![
        page("b.md", "B", "# B\n\n## One\n\nzebra\n\n## Two\n\nzebra\n"),
        page("a.md", "A", "# A\n\nzebra\n"),
        page("c.md", "C", "# C\n\nzebra\n"),
    ]);
    let hits = idx.query("zebra", 20);
    let paths: Vec<&str> = hits.iter().map(|h| h.path.as_str()).collect();
    assert_eq!(paths, ["a.md", "b.md", "c.md"]);
    assert_eq!(
        hits[1].heading.as_deref(),
        Some("One"),
        "the earliest section wins a tie"
    );
    assert_eq!(idx.query("zebra", 2).len(), 2);
}

#[test]
fn anchors_match_the_ids_the_renderer_gives_duplicate_headings() {
    let body =
        "# Page\n\n## Setup\n\nfirst marker\n\n## Setup\n\nsecond marker\n\n### `Code` *Heading*\n\nthird marker\n";
    let html = render_markdown("p.md", body).html;
    let idx = index(vec![page("p.md", "Page", body)]);
    let anchors: Vec<String> = sections(body).into_iter().filter_map(|s| s.anchor).collect();
    assert_eq!(anchors, ["page", "setup", "setup-1", "code-heading"]);
    for anchor in &anchors {
        assert!(
            html.contains(&format!("id=\"{anchor}\"")),
            "renderer has id {anchor}: {html}"
        );
    }
    let second = &idx.query("second", 1)[0];
    assert_eq!(second.anchor.as_deref(), Some("setup-1"));
    assert_eq!(second.heading.as_deref(), Some("Setup"));
}

#[test]
fn text_before_the_first_heading_is_an_unheaded_section() {
    let idx = index(vec![page("a.md", "A", "Lead text here.\n\n## Later\n\nmore\n")]);
    let hit = &idx.query("lead", 1)[0];
    assert_eq!(hit.heading, None);
    assert_eq!(hit.anchor, None);
    let raw = sections("# Only a title\n");
    assert_eq!(raw.len(), 1, "an empty leading section is dropped");
    assert_eq!(raw[0].heading.as_deref(), Some("Only a title"));
    assert_eq!(sections("").len(), 1, "an empty page keeps one section for its title");
}

#[test]
fn an_empty_page_still_matches_by_title() {
    let idx = index(vec![page("notes.md", "Release notes", "")]);
    let hits = idx.query("release", 5);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].snippet, "");
    assert!(hits[0].marks.is_empty());
}

#[test]
fn a_long_section_snippet_is_160_chars_centered_on_the_first_match() {
    let before = "word ".repeat(100);
    let after = " tail".repeat(100);
    let body = format!("{before}needle{after}\n");
    let idx = index(vec![page("a.md", "A", &body)]);
    let hit = &idx.query("needle", 1)[0];
    assert_eq!(hit.snippet.chars().count(), SNIPPET_CHARS);
    let at = hit.snippet.find("needle").expect("match in snippet");
    let lead = hit.snippet[..at].chars().count();
    assert!((75..=80).contains(&lead), "centered: {lead} chars before the match");
    assert_eq!(marked(hit), ["needle"]);
}

#[test]
fn marks_are_utf16_offsets_and_cover_only_the_prefix() {
    let idx = index(vec![page("a.md", "A", "😀 Café CAFÉS and café\n")]);
    let hit = &idx.query("caf", 1)[0];
    assert_eq!(hit.snippet, "😀 Café CAFÉS and café");
    assert_eq!(hit.marks[0], [3, 6], "the emoji is two UTF-16 units");
    assert_eq!(marked(hit), ["Caf", "CAF", "caf"]);
    let full = &idx.query("cafés", 1)[0];
    assert_eq!(marked(full), ["CAFÉS"]);
}

#[test]
fn a_title_only_match_snippets_the_section_start_without_marks() {
    let idx = index(vec![page("a.md", "Zeppelin", "Body without the word.\n")]);
    let hit = &idx.query("zep", 1)[0];
    assert_eq!(hit.snippet, "Body without the word.");
    assert!(hit.marks.is_empty());
}

#[test]
fn a_page_breaking_a_static_rule_is_skipped_not_an_error() {
    let mut bad = page("bad.md", "Bad", "");
    bad.bytes = b"# Bad\r\nzebra\r\n".to_vec();
    let mut binary = page("bin.md", "Bin", "");
    binary.bytes = vec![0xff, 0xfe];
    let (idx, skipped) = SearchIndex::build(vec![bad, binary, page("ok.md", "Ok", "zebra\n")]);
    assert_eq!(
        skipped,
        [
            Skipped {
                path: "bad.md".to_string(),
                rule: StaticRule::CarriageReturn
            },
            Skipped {
                path: "bin.md".to_string(),
                rule: StaticRule::NotUtf8
            },
        ]
    );
    assert_eq!(
        skipped[0].to_string(),
        "bad.md is not searchable: the file contains a carriage return (\\r)"
    );
    let hits = idx.query("zebra", 5);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "ok.md");
    assert_eq!(idx.page_count(), 1);
}
