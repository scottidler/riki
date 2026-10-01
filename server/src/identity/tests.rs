use axum::http::{HeaderMap, HeaderValue};

use super::*;

fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in pairs {
        map.insert(*name, HeaderValue::from_str(value).expect("header value"));
    }
    map
}

#[test]
fn email_and_name_are_read_from_the_configured_headers() {
    let config = IdentityConfig::default();
    let id = Identity::from_headers(
        &headers(&[("remote-email", "a@x.com"), ("remote-name", "Ann")]),
        &config,
    )
    .expect("identity");
    assert_eq!(id.email, "a@x.com");
    assert_eq!(id.name, "Ann");
}

#[test]
fn name_falls_back_to_the_email() {
    let config = IdentityConfig::default();
    let id = Identity::from_headers(&headers(&[("remote-email", "a@x.com")]), &config).expect("identity");
    assert_eq!(id.name, "a@x.com");
}

#[test]
fn missing_or_blank_email_is_no_identity() {
    let config = IdentityConfig::default();
    assert!(Identity::from_headers(&headers(&[("remote-name", "Ann")]), &config).is_none());
    assert!(Identity::from_headers(&headers(&[("remote-email", "  ")]), &config).is_none());
}

#[test]
fn header_names_come_from_config() {
    let config = IdentityConfig {
        email_header: "X-User-Email".to_string(),
        name_header: "X-User-Name".to_string(),
        ..IdentityConfig::default()
    };
    assert!(Identity::from_headers(&headers(&[("remote-email", "a@x.com")]), &config).is_none());
    let id = Identity::from_headers(&headers(&[("x-user-email", "b@x.com")]), &config).expect("identity");
    assert_eq!(id.email, "b@x.com");
}
