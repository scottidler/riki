use riki_core::Oid;
use riki_core::index::{NavIndex, PageNode};

use super::*;

static RIKI: std::sync::LazyLock<Site> = std::sync::LazyLock::new(Site::default);

/// A page view with these slots and placeholders elsewhere.
fn view<'a>(title: &'a str, body: &'a str, sidebar: &'a str, action: Action<'a>) -> PageView<'a> {
    PageView {
        title,
        site: &RIKI,
        body_html: body,
        toc: &[],
        sidebar_html: sidebar,
        breadcrumbs_html: "",
        trail_html: "",
        pager_html: "",
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
    let site = Site {
        name: "Platform & Co".to_string(),
        logo: None,
    };
    let mut v = view("Setup <guide>", "body", "side", EDIT);
    v.site = &site;
    let html = page(&v);
    assert!(
        html.contains("<title>Setup &lt;guide&gt; - Platform &amp; Co</title>"),
        "{html}"
    );
    assert!(
        html.contains(r#"<a class="riki-brand" href="/"><span class="riki-brand-name">Platform &amp; Co</span></a>"#),
        "with no logo the header shows the name as text: {html}"
    );
    assert!(!html.contains("<img"), "{html}");
}

#[test]
fn a_configured_logo_replaces_the_name_in_the_header() {
    let site = Site {
        name: "Wiki".to_string(),
        logo: Some(Logo {
            light: "brand/logo light.svg".to_string(),
            dark: "brand/dark.png".to_string(),
        }),
    };
    let html = brand(&site);
    assert_eq!(
        html,
        concat!(
            r#"<a class="riki-brand" href="/">"#,
            r#"<img class="riki-logo riki-logo-light" src="/_riki/raw/brand/logo%20light.svg" alt="">"#,
            r#"<img class="riki-logo riki-logo-dark" src="/_riki/raw/brand/dark.png" alt="">"#,
            r#"<span class="riki-sr-only">Wiki</span></a>"#
        )
    );
}

#[test]
fn the_page_csp_allows_only_same_origin_fonts() {
    assert!(CSP_PAGE.contains("font-src 'self'"), "{CSP_PAGE}");
    assert!(CSP_PAGE.starts_with("default-src 'none'"), "{CSP_PAGE}");
}

#[test]
fn the_header_offers_the_theme_and_nav_toggles() {
    let mut v = view("T", "body", "side", EDIT);
    let index = titled(&["README.md", "guide/setup.md"]);
    let bar = trail(index.tree(), "guide/setup", "Setup guide");
    v.trail_html = &bar;
    let html = page(&v);
    for choice in ["system", "light", "dark"] {
        assert_eq!(
            html.matches(&format!(r#"data-riki-theme="{choice}""#)).count(),
            1,
            "{choice}: {html}"
        );
    }
    assert!(
        html.contains(
            r#"data-riki-theme="system" aria-label="Use the system theme" title="System" aria-pressed="true""#
        ),
        "system is the default: {html}"
    );
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
        ("riki-nav-group", "the guide group"),
        (">Guide<", "the guide label"),
    ] {
        assert_eq!(html.matches(needle).count(), 1, "{what} ({needle}) once in: {html}");
    }
}

#[test]
fn sidebar_labels_pages_by_title_and_marks_the_current_one() {
    let index = titled(&[
        "README.md",
        "guide/setup.md",
        "guide/services.md",
        "z y.md",
        "release_notes.md",
    ]);
    let html = sidebar(index.tree(), "guide/setup");
    assert!(
        html.contains(r#"<a href="/">Home</a>"#),
        "the home page stays Home: {html}"
    );
    assert!(
        html.contains(r#"<a href="/guide/setup" class="current" aria-current="page">Setup guide</a>"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<a href="/guide/services">Service &lt;catalog&gt;</a>"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<a href="/z%20y">Z y</a>"#),
        "an untitled page shows its prettified filename: {html}"
    );
    assert!(html.contains(r#"<a href="/release_notes">Release notes</a>"#), "{html}");
    assert_eq!(html.matches("aria-current").count(), 1, "{html}");
}

#[test]
fn a_group_is_labelled_by_its_readme_title_else_its_prettified_folder_name() {
    let index = NavIndex::build(
        Oid::ZERO_SHA1,
        [
            "README.md",
            "guide/README.md",
            "guide/a.md",
            "getting-started/b.md",
            "api_docs/c.md",
        ],
    )
    .with_titles(|file| (file == "guide/README.md").then(|| "Guides".to_string()));
    let html = sidebar(index.tree(), "");
    assert!(
        html.contains(r#"<p class="riki-nav-heading"><a href="/guide">Guides</a></p>"#),
        "a README heads its group, by title: {html}"
    );
    assert!(
        html.contains(r#"<p class="riki-nav-heading">Getting started</p>"#),
        "{html}"
    );
    assert!(html.contains(r#"<p class="riki-nav-heading">Api docs</p>"#), "{html}");
}

#[test]
fn top_level_groups_are_static_headers_and_nested_groups_collapse_toward_the_current_page() {
    let index = NavIndex::build(
        Oid::ZERO_SHA1,
        [
            "README.md",
            "a/README.md",
            "a/b.md",
            "a/deep/x.md",
            "a/other/README.md",
            "a/other/y.md",
        ],
    );
    let html = sidebar(index.tree(), "a/deep/x");
    assert!(!html.contains("<details"), "{html}");
    assert!(
        html.contains(r#"<div class="riki-nav-group"><p class="riki-nav-heading"><a href="/a">A</a></p>"#),
        "a top-level group is a header, with no toggle: {html}"
    );
    assert!(
        html.contains(concat!(
            r#"<li class="riki-nav-sub"><div class="riki-nav-sub-head">"#,
            r#"<button type="button" class="riki-nav-chevron riki-nav-sub-label" data-riki-toggle="group" aria-expanded="true" aria-controls="riki-nav-a/deep" aria-label="Deep"><span>Deep</span></button></div>"#,
            r#"<ul class="riki-nav-list" id="riki-nav-a/deep">"#
        )),
        "the path to the current page is expanded: {html}"
    );
    assert!(
        html.contains(concat!(
            r#"<div class="riki-nav-sub-head"><a href="/a/other">Other</a>"#,
            r#"<button type="button" class="riki-nav-chevron" data-riki-toggle="group" aria-expanded="false" aria-controls="riki-nav-a/other" aria-label="Other"></button></div>"#,
            r#"<ul class="riki-nav-list" id="riki-nav-a/other" hidden>"#
        )),
        "a nested group off the path is collapsed, its README still a link: {html}"
    );
}

#[test]
fn sidebar_without_a_home_page_has_no_home_link() {
    let index = NavIndex::build(Oid::ZERO_SHA1, ["a.md"]);
    let html = sidebar(index.tree(), "a");
    assert!(!html.contains(r#"href="/""#), "{html}");
    assert!(html.contains(r#"href="/a""#), "{html}");
}

/// Every `href` in `html`, in document order.
fn hrefs(html: &str) -> Vec<String> {
    html.split(r#"href="/"#)
        .skip(1)
        .map(|rest| rest.split('"').next().unwrap_or_default().to_string())
        .collect()
}

#[test]
fn reading_order_is_the_sidebar_order() {
    let index = titled(&[
        "README.md",
        "changelog.md",
        "guide/README.md",
        "guide/setup.md",
        "guide/deep/x.md",
        "guide/services.md",
        "reference/config.md",
    ]);
    let order: Vec<String> = reading_order(index.tree())
        .into_iter()
        .map(|(node, _)| encode_path(&node.url))
        .collect();
    assert_eq!(hrefs(&sidebar(index.tree(), "")), order);
    assert_eq!(
        order,
        [
            "",
            "changelog",
            "guide",
            "guide/deep/x",
            "guide/services",
            "guide/setup",
            "reference/config"
        ]
    );
}

#[test]
fn the_pager_links_the_neighbours_in_sidebar_order() {
    let index = titled(&["README.md", "guide/setup.md", "guide/services.md"]);
    assert_eq!(
        pager(index.tree(), "guide/services"),
        concat!(
            r#"<nav class="riki-pager" aria-label="Previous and next pages">"#,
            r#"<a class="riki-pager-card riki-pager-prev" href="/"><span class="riki-pager-label">Previous</span><span class="riki-pager-title">Home</span></a>"#,
            r#"<a class="riki-pager-card riki-pager-next" href="/guide/setup"><span class="riki-pager-label">Next</span><span class="riki-pager-title">Setup guide</span></a>"#,
            "</nav>"
        )
    );
    let first = pager(index.tree(), "");
    assert!(
        !first.contains("riki-pager-prev") && first.contains("riki-pager-next"),
        "{first}"
    );
    let last = pager(index.tree(), "guide/setup");
    assert!(
        last.contains("riki-pager-prev") && !last.contains("riki-pager-next"),
        "{last}"
    );
}

#[test]
fn no_pager_off_the_page_list_or_with_one_page() {
    let index = titled(&["README.md", "guide/setup.md"]);
    assert_eq!(pager(index.tree(), "nope"), "");
    let alone = titled(&["README.md"]);
    assert_eq!(pager(alone.tree(), ""), "");
}

#[test]
fn the_trail_shows_the_section_then_the_page() {
    let index = titled(&["README.md", "guide/setup.md", "getting-started/x.md"]);
    let bar = trail(index.tree(), "guide/setup", "Setup <guide>");
    assert!(bar.contains(r#"data-riki-toggle="nav""#), "{bar}");
    assert!(
        bar.ends_with(
            r#"<ol class="riki-trail-path"><li>Guide</li><li aria-current="page">Setup &lt;guide&gt;</li></ol></div>"#
        ),
        "{bar}"
    );
    let missing = trail(index.tree(), "getting-started/nope", "Nope");
    assert!(missing.contains("<li>Getting started</li>"), "{missing}");
    let home = trail(index.tree(), "", "Platform Handbook");
    assert!(
        home.ends_with(r#"<ol class="riki-trail-path"><li aria-current="page">Platform Handbook</li></ol></div>"#),
        "{home}"
    );
}

#[test]
fn breadcrumbs_walk_home_then_ancestors_then_the_page() {
    let index = titled(&["README.md", "guide/README.md", "guide/setup.md", "notes/x/y.md"]);
    assert_eq!(
        breadcrumbs(index.tree(), "guide/setup", "Setup guide"),
        concat!(
            r#"<nav class="riki-breadcrumbs" aria-label="Breadcrumb"><ol>"#,
            r#"<li><a href="/">Home</a></li><li><a href="/guide">Guide</a></li>"#,
            r#"<li aria-current="page">Setup guide</li></ol></nav>"#
        )
    );
    let deep = breadcrumbs(index.tree(), "notes/x/y", "y");
    assert!(
        deep.contains("<li>Notes</li><li>X</li><li aria-current=\"page\">y</li>"),
        "{deep}"
    );
    let missing = breadcrumbs(index.tree(), "nope/deeper", "deeper");
    assert!(
        missing.contains("<li>Nope</li><li aria-current=\"page\">deeper</li>"),
        "{missing}"
    );
    assert_eq!(breadcrumbs(index.tree(), "", "Home"), "");
}

#[test]
fn labels_prefer_the_title_then_the_prettified_segment() {
    let index = titled(&["README.md", "guide/setup.md", "getting-started.md"]);
    let root = index.tree();
    assert_eq!(label(root, ""), "Platform Handbook");
    assert_eq!(
        label(&root.children["getting-started"], "getting-started"),
        "Getting started"
    );
    assert_eq!(label(&root.children["guide"], "guide"), "Guide");
    assert_eq!(label(&PageNode::default(), ""), "Home");
}
