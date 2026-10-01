//! riki's config: `~/.config/riki/riki.yml`, kebab-case, unknown keys rejected, durations via
//! humantime. No secrets live here; the push credential comes from the host's git setup.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use eyre::{Context, Result, eyre};
use riki_core::store::StoreConfig;
use serde::Deserialize;
use tracing::debug;

pub const DEFAULT_LISTEN: &str = "127.0.0.1:8737";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Config {
    #[serde(default = "default_listen")]
    pub listen: SocketAddr,
    pub content: ContentConfig,
    #[serde(default)]
    pub git: GitConfig,
    #[serde(default)]
    pub committer: CommitterConfig,
    #[serde(default)]
    pub identity: IdentityConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ContentConfig {
    pub remote: String,
    #[serde(default = "default_branch")]
    pub branch: String,
    #[serde(default = "default_cache_dir")]
    pub cache_dir: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct GitConfig {
    #[serde(with = "humantime_serde", default = "default_git_duration")]
    pub timeout: Duration,
    #[serde(with = "humantime_serde", default = "default_git_duration")]
    pub poll_interval: Duration,
    #[serde(default = "default_push_retries")]
    pub push_retries: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct CommitterConfig {
    #[serde(default = "default_committer_name")]
    pub name: String,
    #[serde(default = "default_committer_email")]
    pub email: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct IdentityConfig {
    #[serde(default)]
    pub mode: IdentityMode,
    #[serde(default = "default_email_header")]
    pub email_header: String,
    #[serde(default = "default_name_header")]
    pub name_header: String,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IdentityMode {
    #[default]
    Header,
}

fn default_listen() -> SocketAddr {
    DEFAULT_LISTEN
        .parse()
        .expect("DEFAULT_LISTEN is a valid socket address")
}
fn default_branch() -> String {
    "main".to_string()
}
fn default_cache_dir() -> PathBuf {
    PathBuf::from("~/.cache/riki/content.git")
}
fn default_git_duration() -> Duration {
    Duration::from_secs(30)
}
fn default_push_retries() -> u32 {
    1
}
fn default_committer_name() -> String {
    "riki".to_string()
}
fn default_committer_email() -> String {
    "riki@localhost".to_string()
}
fn default_email_header() -> String {
    "Remote-Email".to_string()
}
fn default_name_header() -> String {
    "Remote-Name".to_string()
}

impl Default for GitConfig {
    fn default() -> Self {
        Self {
            timeout: default_git_duration(),
            poll_interval: default_git_duration(),
            push_retries: default_push_retries(),
        }
    }
}

impl Default for CommitterConfig {
    fn default() -> Self {
        Self {
            name: default_committer_name(),
            email: default_committer_email(),
        }
    }
}

impl Default for IdentityConfig {
    fn default() -> Self {
        Self {
            mode: IdentityMode::default(),
            email_header: default_email_header(),
            name_header: default_name_header(),
        }
    }
}

impl Config {
    /// The git store settings, in the shape `riki-core` takes.
    pub fn store(&self) -> StoreConfig {
        StoreConfig {
            remote: self.content.remote.clone(),
            branch: self.content.branch.clone(),
            cache_dir: self.content.cache_dir.clone(),
            timeout: self.git.timeout,
        }
    }

    /// Load the config at `path`, or at the XDG default when `path` is `None`. A missing or
    /// invalid file is an error: riki never starts on guessed settings.
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let path = match path {
            Some(path) => path.to_path_buf(),
            None => default_config_path()?,
        };
        debug!("Config::load: path={}", path.display());
        let text = std::fs::read_to_string(&path).with_context(|| format!("reading config {}", path.display()))?;
        let home = dirs::home_dir();
        Self::from_yaml(&text, home.as_deref()).with_context(|| format!("loading config {}", path.display()))
    }

    /// Parse YAML and expand `~` in path settings against `home`.
    pub fn from_yaml(text: &str, home: Option<&Path>) -> Result<Self> {
        let mut config: Config = serde_yaml::from_str(text)?;
        config.content.cache_dir = expand_tilde(&config.content.cache_dir, home)?;
        config.validate()?;
        Ok(config)
    }

    /// Cross-field rules a single key cannot express. Header identity trusts whatever the
    /// identity headers say, so only a loopback listener (the local edge) may receive requests.
    pub fn validate(&self) -> Result<()> {
        match self.identity.mode {
            IdentityMode::Header if !self.listen.ip().is_loopback() => Err(eyre!(
                "identity.mode `header` requires a loopback `listen` address (127.0.0.0/8 or ::1), got {}: \
                 a non-loopback listener would trust forgeable identity headers",
                self.listen
            )),
            IdentityMode::Header => Ok(()),
        }
    }
}

/// `$XDG_CONFIG_HOME/riki/riki.yml`, else `~/.config/riki/riki.yml`.
pub fn default_config_path() -> Result<PathBuf> {
    let base = match std::env::var("XDG_CONFIG_HOME") {
        Ok(dir) if Path::new(&dir).is_absolute() => PathBuf::from(dir),
        _ => dirs::home_dir()
            .ok_or_else(|| eyre!("cannot resolve the home directory"))?
            .join(".config"),
    };
    Ok(base.join("riki").join("riki.yml"))
}

/// Expand a leading `~` to `home`. Only `~` and `~/...` are expanded; `~user` is left alone.
pub fn expand_tilde(path: &Path, home: Option<&Path>) -> Result<PathBuf> {
    let Ok(rest) = path.strip_prefix("~") else {
        return Ok(path.to_path_buf());
    };
    let home = home.ok_or_else(|| eyre!("cannot expand `~` in {}: no home directory", path.display()))?;
    Ok(home.join(rest))
}

#[cfg(test)]
mod tests;
