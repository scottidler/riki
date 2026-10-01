//! Process lifecycle state and the shaping behind the standard service routes.
//!
//! The JSON shapes match marquee's `core/src/runtime.rs` (Tatari's Standard Routes contract), so
//! `sdv probe` reads riki like any other service. No `env!` here: `cargo:rustc-env` from a
//! `build.rs` applies only to the crate being built, so the shell reads its own compile-time git
//! facts and passes them in as [`Build`].

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Instant, SystemTime};

use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;
use tracing::debug;

/// Process-lifecycle state: when this process started and whether it is ready to serve.
#[derive(Debug)]
pub struct Runtime {
    start_instant: Instant,
    start_system: SystemTime,
    ready: AtomicBool,
}

impl Runtime {
    /// Capture process start and begin not-ready.
    pub fn new() -> Self {
        Self {
            start_instant: Instant::now(),
            start_system: SystemTime::now(),
            ready: AtomicBool::new(false),
        }
    }

    /// Seconds since init, from the monotonic clock.
    pub fn uptime_secs(&self) -> f64 {
        self.start_instant.elapsed().as_secs_f64()
    }

    /// Wall-clock time captured at init; the `deployed_at` value.
    pub fn start_system(&self) -> SystemTime {
        self.start_system
    }

    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::SeqCst)
    }

    /// Latch ready once the process can serve.
    pub fn mark_ready(&self) {
        debug!("mark_ready: latching ready");
        self.ready.store(true, Ordering::SeqCst);
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

/// The shell's compile-time git facts, supplied from its own `build.rs`.
#[derive(Debug, Clone, Copy)]
pub struct Build {
    pub branch: &'static str,
    pub revision: &'static str,
    pub describe: &'static str,
    pub git_sha: &'static str,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    status: &'static str,
}

#[derive(Debug, Serialize)]
pub struct ReadyResponse {
    ready: bool,
}

impl ReadyResponse {
    pub fn is_ready(&self) -> bool {
        self.ready
    }
}

/// `GET /version`.
#[derive(Debug, Serialize)]
pub struct VersionResponse {
    branch: String,
    revision: String,
    version: String,
    git_sha: String,
}

/// `GET /deployed`.
#[derive(Debug, Serialize)]
pub struct DeployedResponse {
    deployed_at: String,
    environment: String,
    deployer: String,
}

/// `GET /status`: `{status, uptime}` when healthy, plus `error` when degraded.
#[derive(Debug, Serialize)]
pub struct StatusResponse {
    status: &'static str,
    uptime: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// The `/health` body: "this process is running", nothing more.
pub fn health() -> HealthResponse {
    HealthResponse { status: "ok" }
}

pub fn ready(is_ready: bool) -> ReadyResponse {
    debug!("ready: is_ready={is_ready}");
    ReadyResponse { ready: is_ready }
}

/// The `/status` body: `ok`, or `degraded` carrying `error` when one is given.
pub fn status(uptime_secs: f64, error: Option<String>) -> StatusResponse {
    let uptime = (uptime_secs * 100.0).round() / 100.0;
    debug!("status: uptime={uptime} error={error:?}");
    StatusResponse {
        status: if error.is_some() { "degraded" } else { "ok" },
        uptime,
        error,
    }
}

pub fn version(build: &Build) -> VersionResponse {
    debug!("version: describe={}", build.describe);
    VersionResponse {
        branch: build.branch.to_string(),
        revision: build.revision.to_string(),
        version: build.describe.to_string(),
        git_sha: build.git_sha.to_string(),
    }
}

/// The `/deployed` body: `deployed_at` is the process start time; `environment` and `deployer`
/// come from the `ENV` and `GIT_AUTHOR` env vars the standard-routes contract names.
pub fn deployed(start: SystemTime) -> DeployedResponse {
    debug!("deployed: start={start:?}");
    DeployedResponse {
        deployed_at: DateTime::<Utc>::from(start).to_rfc3339_opts(SecondsFormat::Secs, true),
        environment: env_usable("ENV")
            .map(|v| v.to_lowercase())
            .unwrap_or_else(|| "unknown".to_string()),
        deployer: env_usable("GIT_AUTHOR").unwrap_or_default(),
    }
}

fn env_usable(name: &str) -> Option<String> {
    let value = std::env::var(name).ok()?;
    let trimmed = value.trim();
    (!trimmed.is_empty() && trimmed != "unknown").then(|| trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    const BUILD: Build = Build {
        branch: "main",
        revision: "abc123",
        describe: "v0.1.0-1-gabc123",
        git_sha: "abc123",
    };

    fn to_json<T: Serialize>(value: &T) -> Value {
        serde_json::to_value(value).expect("serializes")
    }

    #[test]
    fn health_shape() {
        assert_eq!(to_json(&health()), json!({"status": "ok"}));
    }

    #[test]
    fn ready_reflects_latch() {
        let runtime = Runtime::new();
        assert!(!ready(runtime.is_ready()).is_ready());
        runtime.mark_ready();
        assert!(ready(runtime.is_ready()).is_ready());
    }

    #[test]
    fn version_keys_are_the_standard_set() {
        let v = to_json(&version(&BUILD));
        let mut keys: Vec<&str> = v.as_object().expect("object").keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["branch", "git_sha", "revision", "version"]);
        assert_eq!(v["version"], "v0.1.0-1-gabc123");
    }

    #[test]
    fn status_ok_omits_error() {
        assert_eq!(to_json(&status(1.234, None)), json!({"status": "ok", "uptime": 1.23}));
    }

    #[test]
    fn status_degraded_carries_error() {
        let v = to_json(&status(0.0, Some("upstream unreachable".to_string())));
        assert_eq!(v["status"], "degraded");
        assert_eq!(v["error"], "upstream unreachable");
    }

    #[test]
    fn deployed_has_the_three_keys() {
        let v = to_json(&deployed(SystemTime::UNIX_EPOCH));
        assert_eq!(v["deployed_at"], "1970-01-01T00:00:00Z");
        assert!(v.get("environment").is_some());
        assert!(v.get("deployer").is_some());
    }
}
