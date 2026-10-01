use std::path::PathBuf;

use clap::Parser;

/// Request-time Markdown wiki server backed by a git repo.
#[derive(Debug, Parser)]
#[command(name = "riki", version = env!("GIT_DESCRIBE"), about)]
pub struct Cli {
    /// Config file (default: $XDG_CONFIG_HOME/riki/riki.yml, i.e. ~/.config/riki/riki.yml)
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    /// Log filter (tracing EnvFilter syntax, e.g. `info` or `riki_server=debug`)
    #[arg(long, default_value = "info")]
    pub log_level: String,
}
