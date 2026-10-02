use super::*;

const MINIMAL: &str = "content:\n  remote: git@example.com:x/y.git\n";

fn home() -> &'static Path {
    Path::new("/home/test")
}

#[test]
fn defaults_apply_when_only_remote_is_given() {
    let config = Config::from_yaml(MINIMAL, Some(home())).expect("loads");
    assert_eq!(config.listen, default_listen());
    assert_eq!(config.listen.to_string(), "127.0.0.1:8737");
    assert_eq!(config.content.branch, "main");
    assert_eq!(config.git.timeout, Duration::from_secs(30));
    assert_eq!(config.git.push_retries, 1);
    assert_eq!(config.identity.email_header, "Remote-Email");
}

#[test]
fn non_default_listen_takes_effect() {
    let yaml = format!("listen: 127.0.0.1:9911\n{MINIMAL}");
    let config = Config::from_yaml(&yaml, Some(home())).expect("loads");
    assert_eq!(config.listen.port(), 9911);
}

#[test]
fn unknown_nested_key_fails() {
    let yaml = "content:\n  remote: x\n  bogus: 1\n";
    let err = Config::from_yaml(yaml, Some(home())).expect_err("must fail");
    assert!(format!("{err:#}").contains("bogus"), "{err:#}");
}

#[test]
fn unknown_top_level_key_fails() {
    let yaml = format!("nope: 1\n{MINIMAL}");
    assert!(Config::from_yaml(&yaml, Some(home())).is_err());
}

#[test]
fn invalid_duration_fails() {
    let yaml = format!("git:\n  timeout: soon\n{MINIMAL}");
    assert!(Config::from_yaml(&yaml, Some(home())).is_err());
}

#[test]
fn humantime_durations_parse() {
    let yaml = format!("git:\n  timeout: 2m\n  poll-interval: 45s\n{MINIMAL}");
    let config = Config::from_yaml(&yaml, Some(home())).expect("loads");
    assert_eq!(config.git.timeout, Duration::from_secs(120));
    assert_eq!(config.git.poll_interval, Duration::from_secs(45));
}

#[test]
fn missing_remote_fails() {
    assert!(Config::from_yaml("listen: 127.0.0.1:1\n", Some(home())).is_err());
}

#[test]
fn tilde_in_cache_dir_expands() {
    let config = Config::from_yaml(MINIMAL, Some(home())).expect("loads");
    assert_eq!(
        config.content.cache_dir,
        Path::new("/home/test/.cache/riki/content.git")
    );
}

#[test]
fn tilde_without_home_fails() {
    assert!(Config::from_yaml(MINIMAL, None).is_err());
}

#[test]
fn absolute_path_is_untouched_and_needs_no_home() {
    let yaml = "content:\n  remote: x\n  cache-dir: /var/cache/riki\n";
    let config = Config::from_yaml(yaml, None).expect("loads");
    assert_eq!(config.content.cache_dir, Path::new("/var/cache/riki"));
}

#[test]
fn missing_file_fails() {
    let err = Config::load(Some(Path::new("/nonexistent/riki.yml"))).expect_err("must fail");
    assert!(format!("{err:#}").contains("reading config"));
}

#[test]
fn shipped_example_loads() {
    let example = include_str!("../../../riki.example.yml");
    let config = Config::from_yaml(example, Some(home())).expect("example loads");
    assert_eq!(config.listen.to_string(), DEFAULT_LISTEN);
}

#[test]
fn store_config_carries_content_and_timeout() {
    let yaml = format!("{MINIMAL}  branch: wiki\n  cache-dir: ~/c.git\ngit:\n  timeout: 5s\n");
    let store = Config::from_yaml(&yaml, Some(home())).expect("loads").store();
    assert_eq!(store.remote, "git@example.com:x/y.git");
    assert_eq!(store.branch, "wiki");
    assert_eq!(store.cache_dir, Path::new("/home/test/c.git"));
    assert_eq!(store.timeout, Duration::from_secs(5));
}

#[test]
fn header_mode_refuses_a_non_loopback_listen() {
    let yaml = format!("listen: 0.0.0.0:8737\n{MINIMAL}");
    let err = Config::from_yaml(&yaml, Some(home())).expect_err("must fail");
    let text = format!("{err:#}");
    assert!(text.contains("requires a loopback"), "{text}");
    assert!(text.contains("0.0.0.0:8737"), "{text}");
}

#[test]
fn header_mode_accepts_loopback_v4_and_v6() {
    for listen in ["127.0.0.1:8737", "127.9.9.9:1", "[::1]:8737"] {
        let yaml = format!("listen: '{listen}'\n{MINIMAL}");
        Config::from_yaml(&yaml, Some(home())).unwrap_or_else(|e| panic!("{listen}: {e:#}"));
    }
}

#[test]
fn github_remotes_map_to_blob_urls() {
    for remote in [
        "git@github.com:scottidler/wiki.git",
        "git@github.com:scottidler/wiki",
        "ssh://git@github.com/scottidler/wiki.git",
        "https://github.com/scottidler/wiki.git",
        "https://github.com/scottidler/wiki",
    ] {
        assert_eq!(
            github_blob_base(remote, "main").as_deref(),
            Some("https://github.com/scottidler/wiki/blob/main/"),
            "{remote}"
        );
    }
}

#[test]
fn non_github_remotes_have_no_blob_url() {
    for remote in [
        "file:///tmp/upstream.git",
        "git@gitlab.com:o/r.git",
        "https://github.com/o",
        "https://github.com/o/r/extra",
        "https://github.com//r",
        "git@github.com:o/../r.git",
    ] {
        assert_eq!(github_blob_base(remote, "main"), None, "{remote}");
    }
}

#[test]
fn the_blob_url_comes_from_the_configured_remote_and_branch() {
    let config = Config::from_yaml(
        "content:\n  remote: git@github.com:o/r.git\n  branch: wiki\n  cache-dir: /c\n",
        Some(home()),
    )
    .expect("config");
    assert_eq!(
        config.github_blob_base().as_deref(),
        Some("https://github.com/o/r/blob/wiki/")
    );
}

#[test]
fn site_name_defaults_to_the_prettified_repo_name() {
    let config = Config::from_yaml(
        "content:\n  remote: git@github.com:o/platform-handbook.git\n",
        Some(home()),
    )
    .expect("loads");
    assert_eq!(
        config.site(),
        Site {
            name: "Platform handbook".to_string(),
            logo: None
        }
    );
}

#[test]
fn site_name_and_logo_come_from_the_config() {
    let yaml =
        format!("{MINIMAL}site:\n  name: Team Wiki\n  logo:\n    light: assets/light.svg\n    dark: assets/dark.png\n");
    let site = Config::from_yaml(&yaml, Some(home())).expect("loads").site();
    assert_eq!(site.name, "Team Wiki");
    assert_eq!(
        site.logo,
        Some(Logo {
            light: "assets/light.svg".to_string(),
            dark: "assets/dark.png".to_string()
        })
    );
}

#[test]
fn an_unknown_site_key_fails() {
    let yaml = format!("{MINIMAL}site:\n  title: x\n");
    let err = Config::from_yaml(&yaml, Some(home())).expect_err("must fail");
    assert!(format!("{err:#}").contains("title"), "{err:#}");
    let yaml = format!("{MINIMAL}site:\n  logo:\n    light: a.svg\n    dark: b.svg\n    href: /\n");
    assert!(Config::from_yaml(&yaml, Some(home())).is_err());
}

#[test]
fn a_logo_needs_both_variants() {
    let yaml = format!("{MINIMAL}site:\n  logo:\n    light: a.svg\n");
    let err = Config::from_yaml(&yaml, Some(home())).expect_err("must fail");
    assert!(format!("{err:#}").contains("dark"), "{err:#}");
}

#[test]
fn a_logo_must_be_a_servable_repo_image() {
    for (light, needle) in [
        ("logo.pdf", "not an image"),
        ("../logo.svg", "not a repo path"),
        ("/abs.svg", "not a repo path"),
    ] {
        let yaml = format!("{MINIMAL}site:\n  logo:\n    light: '{light}'\n    dark: d.svg\n");
        let err = Config::from_yaml(&yaml, Some(home())).expect_err(light);
        let text = format!("{err:#}");
        assert!(
            text.contains("site.logo.light") && text.contains(needle),
            "{light}: {text}"
        );
    }
}

#[test]
fn an_empty_site_name_fails() {
    let yaml = format!("{MINIMAL}site:\n  name: '  '\n");
    let err = Config::from_yaml(&yaml, Some(home())).expect_err("must fail");
    assert!(format!("{err:#}").contains("site.name"), "{err:#}");
}

#[test]
fn repo_names_come_from_any_remote_form() {
    assert_eq!(repo_name("git@github.com:o/handbook.git"), Some("handbook"));
    assert_eq!(repo_name("https://github.com/o/handbook"), Some("handbook"));
    assert_eq!(repo_name("file:///srv/wiki.git/"), Some("wiki"));
    assert_eq!(repo_name("handbook"), Some("handbook"));
    assert_eq!(repo_name(""), None);
    assert_eq!(repo_name("file:///srv/.git"), None);
}

#[test]
fn the_shipped_example_names_the_site() {
    let example = include_str!("../../../riki.example.yml");
    let config = Config::from_yaml(example, Some(home())).expect("example loads");
    assert_eq!(config.site().name, "Platform Handbook");
}
