use super::*;

fn html(source_file: &str, markdown: &str) -> String {
    render_markdown(source_file, markdown).html
}

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
    let html = html("a/b/README.md", "[x](../x.md) ![alt](img.png)\n");
    assert!(html.contains(r#"href="/a/x""#), "{html}");
    assert!(html.contains(r#"src="/_riki/raw/a/b/img.png""#), "{html}");
}

#[test]
fn golden_alert_renders_with_github_classes() {
    let html = html("README.md", "> [!NOTE]\n> Mind the gap.\n");
    assert_eq!(
        html,
        concat!(
            "<div class=\"markdown-alert markdown-alert-note\">\n",
            "<p class=\"markdown-alert-title\">",
            "<svg class=\"riki-alert-icon\" viewBox=\"0 0 16 16\" width=\"16\" height=\"16\" aria-hidden=\"true\">",
            "<path d=\"M8 14.25a6.25 6.25 0 1 0 0-12.5 6.25 6.25 0 0 0 0 12.5ZM8 7.25v4M8 4.75v.01\"/></svg>",
            "Note</p>\n",
            "<p>Mind the gap.</p>\n",
            "</div>\n"
        )
    );
}

#[test]
fn golden_alerts_carry_their_own_icon_and_title_per_type() {
    for (marker, kind, title, alert) in [
        ("NOTE", "note", "Note", AlertType::Note),
        ("TIP", "tip", "Tip", AlertType::Tip),
        ("IMPORTANT", "important", "Important", AlertType::Important),
        ("WARNING", "warning", "Warning", AlertType::Warning),
        ("CAUTION", "caution", "Caution", AlertType::Caution),
    ] {
        let html = html("README.md", &format!("> [!{marker}]\n> Body.\n"));
        let icon = alert_icon_path(alert);
        assert!(
            html.starts_with(&format!(
                "<div class=\"markdown-alert markdown-alert-{kind}\">\n<p class=\"markdown-alert-title\"><svg class=\"riki-alert-icon\""
            )),
            "{html}"
        );
        assert!(
            html.contains(&format!("<path d=\"{icon}\"/></svg>{title}</p>")),
            "{html}"
        );
    }
    let paths: std::collections::HashSet<&str> = [
        AlertType::Note,
        AlertType::Tip,
        AlertType::Important,
        AlertType::Warning,
        AlertType::Caution,
    ]
    .into_iter()
    .map(alert_icon_path)
    .collect();
    assert_eq!(paths.len(), 5, "every alert type has its own icon");
}

#[test]
fn an_alert_title_is_escaped_text() {
    let html = html("README.md", "> [!TIP] Use <b>this</b>\n> Body.\n");
    assert!(html.contains("</svg>Use &lt;b&gt;this&lt;/b&gt;</p>"), "{html}");
}

#[test]
fn golden_code_is_highlighted_into_prefixed_classes_not_inline_styles() {
    let html = html("README.md", "```rust\nfn main() {}\n```\n");
    assert_eq!(html, format!("<div class=\"riki-code\">\n{GOLDEN_RUST}</div>\n"));
    assert!(!html.contains("style="), "{html}");
}

const GOLDEN_RUST: &str = concat!(
    r#"<pre class="syntax-highlighting"><code class="language-rust">"#,
    r#"<span class="hl-source hl-rust"><span class="hl-meta hl-function hl-rust"><span class="hl-meta hl-function hl-rust">"#,
    r#"<span class="hl-storage hl-type hl-function hl-rust">fn</span> </span>"#,
    r#"<span class="hl-entity hl-name hl-function hl-rust">main</span></span>"#,
    r#"<span class="hl-meta hl-function hl-rust"><span class="hl-meta hl-function hl-parameters hl-rust">"#,
    r#"<span class="hl-punctuation hl-section hl-parameters hl-begin hl-rust">(</span></span>"#,
    r#"<span class="hl-meta hl-function hl-rust"><span class="hl-meta hl-function hl-parameters hl-rust">"#,
    r#"<span class="hl-punctuation hl-section hl-parameters hl-end hl-rust">)</span></span></span></span>"#,
    r#"<span class="hl-meta hl-function hl-rust"> </span><span class="hl-meta hl-function hl-rust">"#,
    r#"<span class="hl-meta hl-block hl-rust"><span class="hl-punctuation hl-section hl-block hl-begin hl-rust">{</span></span>"#,
    r#"<span class="hl-meta hl-block hl-rust"><span class="hl-punctuation hl-section hl-block hl-end hl-rust">}</span></span></span>"#,
    "\n</span></code></pre>\n"
);

#[test]
fn code_without_a_known_language_is_plain_text_and_still_escaped() {
    for source in ["```\n<b>&\n```\n", "```nosuchlang\n<b>&\n```\n"] {
        let html = html("README.md", source);
        assert!(
            html.starts_with("<div class=\"riki-code\">\n<pre class=\"syntax-highlighting\"><code"),
            "{html}"
        );
        assert!(html.contains("hl-text hl-plain"), "{html}");
        assert!(html.contains("&lt;b&gt;&amp;"), "{html}");
    }
}

#[test]
fn a_fence_title_gets_a_header_bar_and_the_language_is_not_shown() {
    let html = html("README.md", "```rust title=\"src/<main>.rs\"\nfn main() {}\n```\n");
    assert!(
        html.starts_with(concat!(
            r#"<div class="riki-code riki-code-titled"><div class="riki-code-head">"#,
            r#"<span class="riki-code-title">src/&lt;main&gt;.rs</span></div>"#,
            "\n<pre class=\"syntax-highlighting\"><code class=\"language-rust\">"
        )),
        "{html}"
    );
    assert!(html.contains("hl-rust"), "still highlighted as rust: {html}");
    assert!(!html.contains("data-lang"), "{html}");
}

#[test]
fn indented_code_is_framed_too() {
    let html = html("README.md", "    plain\n");
    assert!(html.starts_with("<div class=\"riki-code\">\n<pre"), "{html}");
    assert!(html.trim_end().ends_with("</pre>\n</div>"), "{html}");
}

#[test]
fn code_title_reads_the_info_string() {
    assert_eq!(code_title("bash title=\"install.sh\"").as_deref(), Some("install.sh"));
    assert_eq!(code_title("bash title='a b'").as_deref(), Some("a b"));
    assert_eq!(code_title("bash {1,3} title=x.sh").as_deref(), Some("x.sh"));
    assert_eq!(code_title("rust src/main.rs").as_deref(), Some("src/main.rs"));
    assert_eq!(code_title("js Example title").as_deref(), Some("Example title"));
}

#[test]
fn code_title_is_none_for_a_bare_language_or_attributes() {
    assert_eq!(code_title(""), None);
    assert_eq!(code_title("rust"), None);
    assert_eq!(code_title("rust   "), None);
    assert_eq!(code_title("rust {1,3}"), None);
    assert_eq!(code_title("rust lines=1"), None);
    assert_eq!(code_title("rust title=\"\""), None);
}

#[test]
fn golden_toc_lists_h2_and_h3_under_the_ids_the_html_carries() {
    let rendered = render_markdown(
        "README.md",
        "# Title\n\n## Getting `started`\n\ntext\n\n### Install\n\n#### Deep\n\n## Getting started\n\n## Ünïcode ok\n",
    );
    let toc: Vec<(u8, &str, &str)> = rendered
        .toc
        .iter()
        .map(|entry| (entry.level, entry.id.as_str(), entry.text.as_str()))
        .collect();
    assert_eq!(
        toc,
        [
            (2, "getting-started", "Getting started"),
            (3, "install", "Install"),
            (2, "getting-started-1", "Getting started"),
            (2, "ünïcode-ok", "Ünïcode ok"),
        ]
    );
    for entry in &rendered.toc {
        assert!(
            rendered.html.contains(&format!("id=\"{}\"", entry.id)),
            "{}",
            rendered.html
        );
    }
}

#[test]
fn headings_end_with_an_anchor_link_to_themselves() {
    let html = html("README.md", "## Setup\n");
    assert!(html.starts_with("<h2 id=\"setup\">Setup<a href=\"#setup\""), "{html}");
    assert!(html.contains("class=\"anchor\""), "{html}");
}

#[test]
fn tables_sit_in_a_scroll_container() {
    let html = html("README.md", "| a |\n| --- |\n| 1 |\n");
    assert!(html.starts_with("<div class=\"riki-table\">\n<table>"), "{html}");
    assert!(html.trim_end().ends_with("</table>\n</div>"), "{html}");
}

#[test]
fn task_lists_carry_classes_for_the_stylesheet() {
    let html = html("README.md", "- [x] done\n- [ ] todo\n");
    assert!(html.contains("class=\"contains-task-list\""), "{html}");
    assert!(html.contains("class=\"task-list-item\""), "{html}");
}

#[test]
fn page_title_is_the_first_level_one_heading() {
    assert_eq!(page_title("# Setup guide\n\ntext\n").as_deref(), Some("Setup guide"));
    assert_eq!(
        page_title("intro\n\n## Not this\n\n# This `one`\n").as_deref(),
        Some("This one")
    );
    assert_eq!(
        page_title("---\nauthor: x\n---\n# After front matter\n").as_deref(),
        Some("After front matter"),
        "front matter without a title falls through to the H1"
    );
    assert_eq!(page_title("Setext\n======\n").as_deref(), Some("Setext"));
    assert_eq!(page_title("> # Quoted\n").as_deref(), Some("Quoted"));
}

#[test]
fn page_title_prefers_the_front_matter_title_over_the_h1() {
    assert_eq!(
        page_title("---\ntitle: Short\n---\n# Long heading\n").as_deref(),
        Some("Short")
    );
    assert_eq!(
        page_title("---\ntitle: \"Quoted: yes\"\n---\n").as_deref(),
        Some("Quoted: yes")
    );
    assert_eq!(page_title("---\ntitle: 'Single'\n---\n").as_deref(), Some("Single"));
}

#[test]
fn page_title_ignores_nested_and_empty_front_matter_titles() {
    assert_eq!(
        page_title("---\nmeta:\n  title: nested\n---\n# Heading\n").as_deref(),
        Some("Heading")
    );
    assert_eq!(
        page_title("---\ntitle: \"\"\n---\n# Heading\n").as_deref(),
        Some("Heading")
    );
    assert_eq!(page_title("---\ntitle:\n---\n"), None);
}

#[test]
fn page_title_is_none_without_a_level_one_heading() {
    assert_eq!(page_title("## Only h2\n"), None);
    assert_eq!(page_title("```\n# in code\n```\n"), None);
    assert_eq!(page_title("#\n"), None);
    assert_eq!(page_title(""), None);
}

#[test]
fn gfm_extensions_render() {
    let html = html(
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
    let html = html("README.md", "# Hello World\n");
    assert!(html.contains(r#"id="hello-world""#), "{html}");
}

#[test]
fn front_matter_is_not_rendered() {
    let html = html("README.md", "---\ntitle: secret\n---\n# Body\n");
    assert!(!html.contains("secret"), "{html}");
    assert!(html.contains("Body"), "{html}");
}

#[test]
fn safe_mode_omits_raw_html_and_javascript_hrefs() {
    let html = html("README.md", "<script>alert(1)</script>\n\n[x](javascript:alert(1))\n");
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
