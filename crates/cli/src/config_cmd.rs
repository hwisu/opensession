use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub action: ConfigAction,
}

#[derive(Debug, Clone, Subcommand)]
pub enum ConfigAction {
    /// Initialize `.opensession/config.toml` in the current repo (or cwd).
    Init {
        /// Override default web base URL.
        #[arg(long)]
        base_url: Option<String>,
    },
    /// Show effective `.opensession/config.toml`.
    Show,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoConfig {
    pub share: ShareConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareConfig {
    pub base_url: String,
}

impl Default for RepoConfig {
    fn default() -> Self {
        Self {
            share: ShareConfig {
                base_url: "https://opensession.io".to_string(),
            },
        }
    }
}

pub fn run(args: ConfigArgs) -> Result<()> {
    match args.action {
        ConfigAction::Init { base_url } => run_init(base_url),
        ConfigAction::Show => run_show(),
    }
}

pub fn load_repo_config(cwd: &Path) -> Result<(PathBuf, RepoConfig)> {
    let path = config_path(cwd)?;
    if !path.exists() {
        bail!(
            "missing config: {} (run `opensession config init`)",
            path.display()
        );
    }

    let raw = std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let parsed: RepoConfig =
        toml::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
    if parsed.share.base_url.trim().is_empty() {
        bail!(
            "invalid config: share.base_url is empty ({})",
            path.display()
        );
    }
    Ok((path, parsed))
}

fn run_init(base_url: Option<String>) -> Result<()> {
    let cwd = std::env::current_dir().context("read current directory")?;
    let path = config_path(&cwd)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }

    let mut cfg = if path.exists() {
        let (_, loaded) = load_repo_config(&cwd)?;
        loaded
    } else {
        RepoConfig::default()
    };

    if let Some(url) = base_url {
        cfg.share.base_url = normalize_base_url(&url)?;
    } else {
        cfg.share.base_url = normalize_base_url(&cfg.share.base_url)?;
    }

    let body = toml::to_string_pretty(&cfg).context("serialize config")?;
    std::fs::write(&path, body).with_context(|| format!("write {}", path.display()))?;

    println!("config: {}", path.display());
    println!("base_url: {}", cfg.share.base_url);
    Ok(())
}

fn run_show() -> Result<()> {
    let cwd = std::env::current_dir().context("read current directory")?;
    let (path, cfg) = load_repo_config(&cwd)?;
    println!("config: {}", path.display());
    println!("base_url: {}", cfg.share.base_url);
    Ok(())
}

fn config_path(cwd: &Path) -> Result<PathBuf> {
    let root =
        opensession_core::object_store::find_repo_root(cwd).unwrap_or_else(|| cwd.to_path_buf());
    Ok(root.join(".opensession").join("config.toml"))
}

fn normalize_base_url(value: &str) -> Result<String> {
    let trimmed = value.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        bail!("base_url cannot be empty");
    }
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        bail!("base_url must start with http:// or https://");
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::{normalize_base_url, RepoConfig};

    #[test]
    fn default_repo_config_has_base_url() {
        assert_eq!(
            RepoConfig::default().share.base_url,
            "https://opensession.io".to_string()
        );
    }

    #[test]
    fn normalize_base_url_strips_trailing_slash() {
        assert_eq!(
            normalize_base_url("https://opensession.io/").expect("normalize"),
            "https://opensession.io".to_string()
        );
    }
}
