use clap::Parser;
use eyre::Result;
use riki_server::cli::Cli;
use riki_server::config::Config;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    riki_server::observability::init(&cli.log_level)?;
    let config = Config::load(cli.config.as_deref())?;
    riki_server::run(config).await
}
