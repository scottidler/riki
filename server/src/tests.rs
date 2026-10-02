use super::*;

#[tokio::test]
async fn non_default_listen_is_what_gets_bound() {
    // A non-default loopback address with port 0: the bound address proves `listen` was used,
    // and the kernel picks the port at bind time, so no free-then-rebind race with other tests.
    // (That `listen`'s port is parsed is `config::tests::non_default_listen_takes_effect`.)
    let yaml = "listen: 127.0.0.2:0\ncontent:\n  remote: x\n";
    let config = Config::from_yaml(yaml, Some(std::path::Path::new("/home/test"))).expect("loads");
    let listener = bind(&config).await.expect("binds");
    let bound = listener.local_addr().expect("addr");
    assert_eq!(bound.ip().to_string(), "127.0.0.2");
    assert_ne!(bound.port(), 0);
}

#[tokio::test]
async fn bind_fails_loudly_when_the_port_is_taken() {
    let held = std::net::TcpListener::bind("127.0.0.1:0").expect("hold");
    let port = held.local_addr().expect("addr").port();
    let yaml = format!("listen: 127.0.0.1:{port}\ncontent:\n  remote: x\n");
    let config = Config::from_yaml(&yaml, Some(std::path::Path::new("/home/test"))).expect("loads");
    assert!(bind(&config).await.is_err());
}
