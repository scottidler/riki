use super::*;

#[tokio::test]
async fn non_default_listen_is_what_gets_bound() {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("probe bind");
    let port = probe.local_addr().expect("addr").port();
    drop(probe);
    let yaml = format!("listen: 127.0.0.1:{port}\ncontent:\n  remote: x\n");
    let config = Config::from_yaml(&yaml, Some(std::path::Path::new("/home/test"))).expect("loads");
    let listener = bind(&config).await.expect("binds");
    assert_eq!(listener.local_addr().expect("addr").port(), port);
}

#[tokio::test]
async fn bind_fails_loudly_when_the_port_is_taken() {
    let held = std::net::TcpListener::bind("127.0.0.1:0").expect("hold");
    let port = held.local_addr().expect("addr").port();
    let yaml = format!("listen: 127.0.0.1:{port}\ncontent:\n  remote: x\n");
    let config = Config::from_yaml(&yaml, Some(std::path::Path::new("/home/test"))).expect("loads");
    assert!(bind(&config).await.is_err());
}
