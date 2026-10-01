use super::*;

#[test]
fn resolve_is_against_the_source_files_directory() {
    assert_eq!(resolve("a/b/README.md", "../x.md").as_deref(), Some("a/x.md"));
    assert_eq!(resolve("a/b/README.md", "img.png").as_deref(), Some("a/b/img.png"));
    assert_eq!(resolve("README.md", "./x.md").as_deref(), Some("x.md"));
    assert_eq!(resolve("a/b.md", "c/../d.md").as_deref(), Some("a/d.md"));
}

#[test]
fn resolve_refuses_to_climb_out_of_the_repo() {
    assert_eq!(resolve("a.md", "../x.md"), None);
    assert_eq!(resolve("a/b.md", "../../x.md"), None);
}

#[test]
fn links_map_md_files_to_riki_urls() {
    assert_eq!(rewrite_link("a/b/README.md", "../x.md"), "/a/x");
    assert_eq!(rewrite_link("README.md", "a/README.md"), "/a");
    assert_eq!(rewrite_link("a/b.md", "../README.md"), "/");
    assert_eq!(rewrite_link("a/b.md", "c.md#top"), "/a/c#top");
    assert_eq!(rewrite_link("a/b.md", "c.md?x=1"), "/a/c?x=1");
    assert_eq!(rewrite_link("a/b.md", "report.pdf"), "/a/report.pdf");
}

#[test]
fn absolute_external_and_anchor_links_pass_through() {
    for url in [
        "https://example.com/x.md",
        "mailto:a@b.c",
        "/abs/page",
        "//cdn.example.com/x",
        "#section",
        "",
        "javascript:alert(1)",
    ] {
        assert_eq!(rewrite_link("a/b.md", url), url);
        assert_eq!(rewrite_image("a/b.md", url), url);
    }
}

#[test]
fn a_link_that_climbs_out_of_the_repo_is_left_alone() {
    assert_eq!(rewrite_link("a.md", "../x.md"), "../x.md");
}

#[test]
fn images_map_to_the_raw_route() {
    assert_eq!(rewrite_image("a/b/README.md", "img.png"), "/_riki/raw/a/b/img.png");
    assert_eq!(rewrite_image("a/b/README.md", "../img.png"), "/_riki/raw/a/img.png");
}

#[test]
fn rendering_resolves_links_and_images_in_html() {
    let html = render_markdown("a/b/README.md", "[x](../x.md) ![alt](img.png)\n");
    assert!(html.contains(r#"href="/a/x""#), "{html}");
    assert!(html.contains(r#"src="/_riki/raw/a/b/img.png""#), "{html}");
}

#[test]
fn golden_alert_renders_with_github_classes() {
    let html = render_markdown("README.md", "> [!NOTE]\n> Mind the gap.\n");
    assert_eq!(
        html,
        "<div class=\"markdown-alert markdown-alert-note\">\n<p class=\"markdown-alert-title\">Note</p>\n<p>Mind the gap.</p>\n</div>\n"
    );
}

#[test]
fn gfm_extensions_render() {
    let html = render_markdown(
        "README.md",
        "| a | b |\n|---|---|\n| 1 | 2 |\n\n~~gone~~\n\n- [x] done\n\nsee https://example.com\n",
    );
    assert!(html.contains("<table>"), "{html}");
    assert!(html.contains("<del>gone</del>"), "{html}");
    assert!(html.contains(r#"type="checkbox""#), "{html}");
    assert!(html.contains(r#"<a href="https://example.com">"#), "{html}");
}

#[test]
fn headings_get_unprefixed_ids() {
    let html = render_markdown("README.md", "# Hello World\n");
    assert!(html.contains(r#"id="hello-world""#), "{html}");
}

#[test]
fn front_matter_is_not_rendered() {
    let html = render_markdown("README.md", "---\ntitle: secret\n---\n# Body\n");
    assert!(!html.contains("secret"), "{html}");
    assert!(html.contains("Body"), "{html}");
}

#[test]
fn safe_mode_omits_raw_html_and_javascript_hrefs() {
    let html = render_markdown("README.md", "<script>alert(1)</script>\n\n[x](javascript:alert(1))\n");
    assert!(!html.contains("<script"), "{html}");
    assert!(!html.contains("javascript:"), "{html}");
}

#[test]
fn escape_html_covers_markup_characters() {
    assert_eq!(
        escape_html(r#"<a href="x">&'"#),
        "&lt;a href=&quot;x&quot;&gt;&amp;&#x27;"
    );
}

#[test]
fn encode_path_keeps_slashes_and_escapes_the_rest() {
    assert_eq!(encode_path("a b/c#d"), "a%20b/c%23d");
    assert_eq!(encode_path("plain/path-1_x.y"), "plain/path-1_x.y");
}
