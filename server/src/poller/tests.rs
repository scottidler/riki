use std::time::Duration;

use super::*;
use crate::testkit::Upstream;

#[tokio::test]
async fn spawned_poller_publishes_a_new_upstream_commit() {
    let upstream = Upstream::new();
    upstream.push(&[("README.md", "x\n")]);
    let wiki = upstream.wiki().await;
    poll_once(&wiki).await;
    let pushed = upstream.push(&[("a.md", "laptop\n")]);

    let handle = spawn(wiki.clone(), Duration::from_millis(20));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while wiki.good().map(|good| good.commit()) != Some(pushed) {
        assert!(
            tokio::time::Instant::now() < deadline,
            "poller never published {pushed}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    handle.abort();
}

#[tokio::test]
async fn poll_once_survives_an_unreachable_upstream() {
    let upstream = Upstream::new();
    upstream.push(&[("README.md", "x\n")]);
    let wiki = upstream.wiki().await;
    std::fs::remove_dir_all(&upstream.dir).expect("remove upstream");
    poll_once(&wiki).await;
    assert!(wiki.unreachable().is_some());
}
