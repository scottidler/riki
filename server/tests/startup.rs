//! Startup failures observed through the real binary: exit status and stderr.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const STARTUP_DEADLINE: Duration = Duration::from_secs(10);

#[test]
fn header_mode_on_a_public_listen_exits_non_zero_naming_the_rule() {
    let dir = tempfile::tempdir().expect("tmp");
    let config = dir.path().join("riki.yml");
    let yaml = format!(
        "listen: 0.0.0.0:0\ncontent:\n  remote: file://{0}/none.git\n  cache-dir: {0}/content.git\nidentity:\n  mode: header\n",
        dir.path().display()
    );
    std::fs::write(&config, yaml).expect("write config");
    let mut child = Command::new(env!("CARGO_BIN_EXE_riki"))
        .arg("--config")
        .arg(&config)
        .stderr(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .expect("run riki");
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll child") {
            break status;
        }
        if started.elapsed() > STARTUP_DEADLINE {
            child.kill().expect("kill a server that should have refused to start");
            child.wait().expect("reap");
            panic!("riki kept running on a non-loopback listen in header mode");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("stderr")
        .read_to_string(&mut stderr)
        .expect("read stderr");
    assert!(!status.success(), "must exit non-zero");
    assert!(stderr.contains("requires a loopback"), "stderr: {stderr}");
}
