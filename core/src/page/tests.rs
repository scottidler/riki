use super::*;

fn assert_split(file: &str, front_matter: &str) {
    let split = split_front_matter(file);
    assert_eq!(
        format!("{}{}", split.front_matter, split.body),
        file,
        "prefix + body == file"
    );
    assert_eq!(split.front_matter, front_matter, "{file:?}");
}

#[test]
fn no_front_matter_is_an_empty_prefix() {
    assert_split("# Title\n\nbody\n", "");
    assert_split("", "");
    assert_split("--- not an opener\nx\n---\n", "");
}

#[test]
fn front_matter_then_body() {
    assert_split("---\ntitle: x\n---\n# Title\n", "---\ntitle: x\n---\n");
}

#[test]
fn one_blank_line_after_the_closer_belongs_to_the_front_matter() {
    assert_split("---\ntitle: x\n---\n\n# Title\n", "---\ntitle: x\n---\n\n");
}

#[test]
fn front_matter_only_with_a_trailing_newline() {
    assert_split("---\ntitle: x\n---\n", "---\ntitle: x\n---\n");
}

#[test]
fn front_matter_only_without_a_trailing_newline() {
    assert_split("---\ntitle: x\n---", "---\ntitle: x\n---");
}

#[test]
fn a_thematic_break_in_the_body_is_not_front_matter() {
    assert_split("# Title\n\n---\n\nmore\n", "");
    assert_split("---\na: 1\n---\nbody\n\n---\n\ntail\n", "---\na: 1\n---\n");
}

#[test]
fn an_unterminated_opener_is_not_front_matter() {
    assert_split("---\ntitle: x\nno closer here\n", "");
}

#[test]
fn a_leading_bom_stays_in_the_prefix() {
    assert_split("\u{feff}---\na: 1\n---\nbody\n", "\u{feff}---\na: 1\n---\n");
}

#[test]
fn static_rules_name_the_first_broken_rule() {
    assert_eq!(check_static_rules(b"# ok\n"), Ok("# ok\n"));
    assert_eq!(check_static_rules(b"a\xffb"), Err(StaticRule::NotUtf8));
    assert_eq!(
        check_static_rules("\u{feff}# x\n".as_bytes()),
        Err(StaticRule::LeadingBom)
    );
    assert_eq!(check_static_rules(b"a\r\nb\r\n"), Err(StaticRule::CarriageReturn));
    assert_eq!(check_static_rules(b"a\rb"), Err(StaticRule::CarriageReturn));
}

#[test]
fn trailing_newlines_is_the_final_run() {
    assert_eq!(trailing_newlines("a\n"), "\n");
    assert_eq!(trailing_newlines("a\n\n"), "\n\n");
    assert_eq!(trailing_newlines("a"), "");
    assert_eq!(trailing_newlines(""), "");
}

#[test]
fn compose_applies_the_trailing_newline_state() {
    assert_eq!(compose("", "new\n", ""), "new");
    assert_eq!(compose("", "new", "\n"), "new\n");
    assert_eq!(compose("", "new\n\n\n", "\n\n"), "new\n\n");
    assert_eq!(compose("", "new page", NEW_PAGE_TRAILING), "new page\n");
}

#[test]
fn compose_reattaches_front_matter() {
    assert_eq!(compose("---\na: 1\n---\n", "body\n", "\n"), "---\na: 1\n---\nbody\n");
    assert_eq!(compose("---\na: 1\n---\n", "", "\n"), "---\na: 1\n---\n");
    assert_eq!(compose("---\na: 1\n---", "", ""), "---\na: 1\n---");
    assert_eq!(compose("---\na: 1\n---", "body", ""), "---\na: 1\n---\nbody");
}

#[test]
fn an_unedited_body_composes_back_to_the_file() {
    for file in [
        "---\ntitle: x\n---\n\n# T\n\nbody",
        "---\ntitle: x\n---\n# T\n",
        "---\ntitle: x\n---\n",
        "---\ntitle: x\n---",
        "# T\n\nbody\n\n",
        "",
    ] {
        let split = split_front_matter(file);
        let again = compose(split.front_matter, split.body, trailing_newlines(file));
        assert_eq!(again, file);
    }
}

#[test]
fn first_diff_line_ignores_trailing_newlines_only() {
    assert_eq!(first_diff_line("a\nb\n", "a\nb"), None);
    assert_eq!(first_diff_line("a\nb\n\n", "a\nb\n"), None);
    assert_eq!(first_diff_line("a\nb\nc\n", "a\nB\nc\n"), Some(2));
    assert_eq!(first_diff_line("a\nb\n", "a\nb\nc\n"), Some(3));
    assert_eq!(first_diff_line("a\n", "a \n"), Some(1));
}

#[test]
fn page_paths_must_be_valid_markdown_and_unreserved() {
    assert_eq!(validate_page_path("a/b.md"), Ok(()));
    assert_eq!(validate_page_path("README.md"), Ok(()));
    assert!(matches!(
        validate_page_path("a/b.txt"),
        Err(PagePathError::NotMarkdown(_))
    ));
    assert!(matches!(
        validate_page_path("a/../b.md"),
        Err(PagePathError::Path(PathError::DotDot(_)))
    ));
    assert!(matches!(validate_page_path("status.md"), Err(PagePathError::Reserved { name, .. }) if name == "status"));
    assert!(matches!(
        validate_page_path("_riki/x.md"),
        Err(PagePathError::Reserved { .. })
    ));
    assert_eq!(validate_page_path("docs/status.md"), Ok(()));
}
