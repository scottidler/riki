use eyre::{Result, eyre};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

/// Install the JSON tracing subscriber. The filter comes from `--log-level` only; `RUST_LOG` is
/// deliberately not consulted, and a bad filter fails startup rather than falling back.
pub fn init(log_level: &str) -> Result<()> {
    let filter = EnvFilter::try_new(log_level).map_err(|e| eyre!("invalid --log-level {log_level:?}: {e}"))?;
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().json())
        .try_init()
        .map_err(|e| eyre!("installing tracing subscriber: {e}"))
}
