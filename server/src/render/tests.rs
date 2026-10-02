use riki_core::Oid;
use riki_core::index::NavIndex;

use super::*;

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
    let html = page("T", "<p>__SIDEBAR__</p>", "<ul>side</ul>", "", EDIT);
    assert!(html.contains("<p>__SIDEBAR__</p>"), "{html}");
}

const EDIT: Action<'static> = Action::Edit {
    file: "a/b.md",
    source: None,
};

#[test]
fn page_escapes_title_and_offers_edit() {
    let html = page("<b>", "body", "side", "", EDIT);
    assert!(html.contains("<title>&lt;b&gt; - riki</title>"), "{html}");
    assert!(html.contains("id=\"riki-edit\""), "{html}");
    assert!(!html.contains("Create this page"), "{html}");
}

#[test]
fn the_edit_button_is_live_and_names_the_file() {
    let html = page("T", "body", "side", "", EDIT);
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
    let html = page("T", "body", "side", "", edit);
    assert!(
        html.contains(r#"data-source="https://github.com/o/r/blob/main/a/b.md?x=&quot;1&quot;""#),
        "{html}"
    );
}

#[test]
fn pages_load_the_editor_bundle_from_this_origin() {
    let html = page("T", "body", "side", "", EDIT);
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
fn missing_page_offers_create_with_the_file_path() {
    let html = page("T", "body", "side", "", Action::Create { file: "a/\"b\".md" });
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

#[test]
fn sidebar_nests_marks_current_and_labels_readmeless_dirs() {
    let index = NavIndex::build(
        Oid::ZERO_SHA1,
        ["README.md", "a/README.md", "a/b.md", "docs/x.md", "z y.md"],
    );
    let html = sidebar(index.tree(), "a/b");
    assert!(html.contains("<a href=\"/\">Home</a>"), "{html}");
    assert!(html.contains("<a href=\"/a/b\" class=\"current\">b</a>"), "{html}");
    assert!(html.contains("<li>docs<ul>"), "docs has no README, so a label: {html}");
    assert!(html.contains("href=\"/z%20y\""), "{html}");
}
