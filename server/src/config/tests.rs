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
