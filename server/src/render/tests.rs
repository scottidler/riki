use riki_core::Oid;
use riki_core::index::{NavIndex, PageNode};

use super::*;

/// A page view with these slots and placeholders elsewhere.
fn view<'a>(title: &'a str, body: &'a str, sidebar: &'a str, action: Action<'a>) -> PageView<'a> {
    PageView {
        title,
        site_title: "riki",
        body_html: body,
        toc: &[],
        sidebar_html: sidebar,
        breadcrumbs_html: "",
        banners_html: "",
        action,
    }
}

#[test]
fn fill_is_single_pass() {
    let out = fill("a __X__ b __Y__", &[("__X__", "__Y__"), ("__Y__", "late")]);
    assert_eq!(out, "a __Y__ b late");
}

#[test]
fn fill_passes_unknown_tokens_through() {
    assert_eq!(fill("__NOPE__ and __X__", &[("__X__", "1")]), "__NOPE__ and 1");
}

#[test]
fn page_body_cannot_inject_into_other_slots() {
    let html = page(&view("T", "<p>__SIDEBAR__</p>", "<ul>side</ul>", EDIT));
    assert!(html.contains("<p>__SIDEBAR__</p>"), "{html}");
}

const EDIT: Action<'static> = Action::Edit {
    file: "a/b.md",
    source: None,
};

#[test]
fn page_escapes_title_and_offers_edit() {
    let html = page(&view("<b>", "body", "side", EDIT));
    assert!(html.contains("<title>&lt;b&gt; - riki</title>"), "{html}");
    assert!(html.contains("id=\"riki-edit\""), "{html}");
    assert!(!html.contains("Create this page"), "{html}");
}

#[test]
fn the_edit_button_is_live_and_names_the_file() {
    let html = page(&view("T", "body", "side", EDIT));
    assert!(
        html.contains(r#"<button id="riki-edit" type="button" data-path="a/b.md">Edit</button>"#),
        "{html}"
    );
    assert!(!html.contains("disabled"), "{html}");
}

#[test]
fn the_edit_button_carries_the_escaped_github_link() {
    let edit = Action::Edit {
        file: "a/b.md",
        source: Some("https://github.com/o/r/blob/main/a/b.md?x=\"1\""),
    };
    let html = page(&view("T", "body", "side", edit));
    assert!(
        html.contains(r#"data-source="https://github.com/o/r/blob/main/a/b.md?x=&quot;1&quot;""#),
        "{html}"
    );
}

#[test]
fn pages_load_the_editor_bundle_from_this_origin() {
    let html = page(&view("T", "body", "side", EDIT));
    assert!(
        html.contains(r#"<script src="/_riki/assets/editor.js" defer></script>"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<link rel="stylesheet" href="/_riki/assets/editor.css">"#),
        "{html}"
    );
}

#[test]
fn pages_load_the_theme_from_this_origin_with_no_inline_script_or_style() {
    let html = page(&view("T", "body", "side", EDIT));
    assert!(
        html.contains(r#"<link rel="stylesheet" href="/_riki/assets/riki.css">"#),
        "{html}"
    );
    // Not deferred: a pinned dark theme applies before first paint.
    assert!(
        html.contains(r#"<script src="/_riki/assets/riki.js"></script>"#),
        "{html}"
    );
    assert_eq!(html.matches("<script").count(), 2, "{html}");
    assert_eq!(
        html.matches("<script src=").count(),
        2,
        "every script is external: {html}"
    );
    assert!(!html.contains("<style"), "{html}");
    assert!(!html.contains("style=\""), "{html}");
    assert!(!html.contains(" on"), "no inline event handlers: {html}");
}

#[test]
fn the_page_title_names_the_page_and_the_site() {
    let mut v = view("Setup <guide>", "body", "side", EDIT);
    v.site_title = "Platform & Co";
    let html = page(&v);
    assert!(
        html.contains("<title>Setup &lt;guide&gt; - Platform &amp; Co</title>"),
        "{html}"
    );
    assert!(html.contains("<span>Platform &amp; Co</span></a>"), "{html}");
}

#[test]
fn the_header_offers_the_theme_and_nav_toggles() {
    let html = page(&view("T", "body", "side", EDIT));
    assert!(html.contains(r#"data-riki-toggle="theme""#), "{html}");
    assert!(html.contains(r#"data-riki-toggle="nav""#), "{html}");
    assert!(
        html.contains(r#"<nav class="riki-sidebar" id="riki-nav" aria-label="Pages">side</nav>"#),
        "{html}"
    );
}

#[test]
fn golden_toc_links_every_entry_by_id() {
    let entries = [
        TocEntry {
            level: 2,
            id: "getting-started".to_string(),
            text: "Getting <started>".to_string(),
        },
        TocEntry {
            level: 3,
            id: "ünï code".to_string(),
            text: "Install".to_string(),
        },
    ];
    assert_eq!(
        toc(&entries),
        concat!(
            r#"<aside class="riki-toc" aria-label="On this page"><p class="riki-toc-title">On this page</p><ul>"#,
            r##"<li class="riki-toc-h2"><a href="#getting-started">Getting &lt;started&gt;</a></li>"##,
            r##"<li class="riki-toc-h3"><a href="#%C3%BCn%C3%AF%20code">Install</a></li>"##,
            "</ul></aside>"
        )
    );
}

#[test]
fn an_empty_toc_keeps_its_slot() {
    assert_eq!(
        toc(&[]),
        r#"<aside class="riki-toc" aria-label="On this page"></aside>"#
    );
    let mut v = view("T", "body", "side", EDIT);
    let entries = [TocEntry {
        level: 2,
        id: "a".to_string(),
        text: "A".to_string(),
    }];
    v.toc = &entries;
    assert!(page(&v).contains(r##"<a href="#a">A</a>"##));
}

#[test]
fn missing_page_offers_create_with_the_file_path() {
    let html = page(&view("T", "body", "side", Action::Create { file: "a/\"b\".md" }));
    assert!(html.contains("Create this page"), "{html}");
    assert!(html.contains("data-path=\"a/&quot;b&quot;.md\""), "{html}");
    assert!(!html.contains("riki-edit"), "{html}");
}

#[test]
fn error_page_is_self_contained_and_escaped() {
    let html = error_page("500", "<oops>");
    assert!(html.contains("&lt;oops&gt;"), "{html}");
    for external in ["<script", "src=", "<link", "http://", "https://"] {
        assert!(!html.contains(external), "{external} in {html}");
    }
}

#[test]
fn banners_name_the_error_and_the_commit() {
    let rejected = Rejected {
        commit: Oid::ZERO_SHA1,
        errors: "status.md claims <the reserved name>".to_string(),
    };
    let html = banners(Some(&rejected), None);
    assert!(html.contains(&Oid::ZERO_SHA1.to_string()), "{html}");
    assert!(html.contains("&lt;the reserved name&gt;"), "{html}");
}

#[test]
fn banners_name_the_unreachable_time() {
    let down = Unreachable {
        since: "2026-10-01T12:00:00Z".parse().expect("time"),
        error: "boom".to_string(),
    };
    let html = banners(None, Some(&down));
    assert!(html.contains("unreachable since 2026-10-01T12:00:00Z"), "{html}");
    assert_eq!(banners(None, None), "");
}

fn titled(files: &[&str]) -> NavIndex {
    NavIndex::build(Oid::ZERO_SHA1, files.iter().copied()).with_titles(|file| match file {
        "README.md" => Some("Platform Handbook".to_string()),
        "guide/setup.md" => Some("Setup guide".to_string()),
        "guide/services.md" => Some("Service <catalog>".to_string()),
        _ => None,
    })
}

#[test]
fn sidebar_lists_each_page_and_directory_exactly_once() {
    let index = titled(&["README.md", "guide/setup.md", "guide/services.md"]);
    let html = sidebar(index.tree(), "guide/setup");
    for (needle, what) in [
        (r#"href="/guide/setup""#, "the setup page"),
        (r#"href="/guide/services""#, "the services page"),
        (r#"href="/""#, "the home page"),
        ("<details", "the guide section"),
        (">guide<", "the guide label"),
    ] {
        assert_eq!(html.matches(needle).count(), 1, "{what} ({needle}) once in: {html}");
    }
}

#[test]
fn sidebar_labels_pages_by_title_and_marks_the_current_one() {
    let index = titled(&["README.md", "guide/setup.md", "guide/services.md", "z y.md"]);
    let html = sidebar(index.tree(), "guide/setup");
    assert!(html.contains(r#"<a href="/">Home</a>"#), "{html}");
    assert!(
        html.contains(r#"<a href="/guide/setup" class="current" aria-current="page">Setup guide</a>"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<a href="/guide/services">Service &lt;catalog&gt;</a>"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<a href="/z%20y">z y</a>"#),
        "an untitled page shows its segment: {html}"
    );
    assert!(
        html.contains(r#"<summary><span class="riki-nav-label">guide</span></summary>"#),
        "a directory with no README is a label: {html}"
    );
    assert_eq!(html.matches("aria-current").count(), 1, "{html}");
}

#[test]
fn sidebar_sections_link_their_readme_and_open_toward_the_current_page() {
    let index = NavIndex::build(
        Oid::ZERO_SHA1,
        ["README.md", "a/README.md", "a/b.md", "a/deep/x.md", "a/other/y.md"],
    );
    let html = sidebar(index.tree(), "a/deep/x");
    assert!(
        html.contains(r#"<details class="riki-nav-section" open><summary><a href="/a">a</a></summary>"#),
        "top-level sections are open and head with their README: {html}"
    );
    assert!(
        html.contains(r#"<details class="riki-nav-section" open><summary><span class="riki-nav-label">deep</span>"#),
        "the path to the current page is open: {html}"
    );
    assert!(
        html.contains(r#"<details class="riki-nav-section"><summary><span class="riki-nav-label">other</span>"#),
        "a nested section off the path is closed: {html}"
    );
}

#[test]
fn sidebar_without_a_home_page_has_no_home_link() {
    let index = NavIndex::build(Oid::ZERO_SHA1, ["a.md"]);
    let html = sidebar(index.tree(), "a");
    assert!(!html.contains(r#"href="/""#), "{html}");
    assert!(html.contains(r#"href="/a""#), "{html}");
}

#[test]
fn breadcrumbs_walk_home_then_ancestors_then_the_page() {
    let index = titled(&["README.md", "guide/README.md", "guide/setup.md", "notes/x/y.md"]);
    assert_eq!(
        breadcrumbs(index.tree(), "guide/setup", "Setup guide"),
        concat!(
            r#"<nav class="riki-breadcrumbs" aria-label="Breadcrumb"><ol>"#,
            r#"<li><a href="/">Home</a></li><li><a href="/guide">guide</a></li>"#,
            r#"<li aria-current="page">Setup guide</li></ol></nav>"#
        )
    );
    let deep = breadcrumbs(index.tree(), "notes/x/y", "y");
    assert!(
        deep.contains("<li>notes</li><li>x</li><li aria-current=\"page\">y</li>"),
        "{deep}"
    );
    let missing = breadcrumbs(index.tree(), "nope/deeper", "deeper");
    assert!(
        missing.contains("<li>nope</li><li aria-current=\"page\">deeper</li>"),
        "{missing}"
    );
    assert_eq!(breadcrumbs(index.tree(), "", "Home"), "");
}

#[test]
fn labels_prefer_the_title_then_the_segment() {
    let index = titled(&["README.md", "guide/setup.md", "x.md"]);
    let root = index.tree();
    assert_eq!(label(root, ""), "Platform Handbook");
    assert_eq!(label(&root.children["x"], "x"), "x");
    assert_eq!(label(&PageNode::default(), ""), "Home");
}
