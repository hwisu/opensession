use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// CLI configuration stored at ~/.config/opensession/config.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub struct CliConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub daemon: DaemonRefConfig,
    #[serde(default)]
    pub custom_parsers: Vec<CustomParserConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_server_url")]
    pub url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub team_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonRefConfig {
    #[serde(default = "default_true")]
    pub auto_start: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomParserConfig {
    pub name: String,
    pub command: String,
    pub glob: String,
}

fn default_server_url() -> String {
    "https://opensession.io".to_string()
}

fn default_true() -> bool {
    true
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            url: default_server_url(),
            api_key: String::new(),
            team_id: String::new(),
        }
    }
}

impl Default for DaemonRefConfig {
    fn default() -> Self {
        Self { auto_start: true }
    }
}


/// Get the config directory path (~/.config/opensession/)
pub fn config_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .context("Could not determine home directory")?;
    Ok(PathBuf::from(home)
        .join(".config")
        .join("opensession"))
}

/// Get the config file path
pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.toml"))
}

/// Load config from disk, returning default if not found
pub fn load_config() -> Result<CliConfig> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(CliConfig::default());
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read config at {}", path.display()))?;
    let config: CliConfig = toml::from_str(&content)
        .with_context(|| format!("Failed to parse config at {}", path.display()))?;
    Ok(config)
}

/// Save config to disk
pub fn save_config(config: &CliConfig) -> Result<()> {
    let dir = config_dir()?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create config dir at {}", dir.display()))?;
    let path = config_path()?;
    let content = toml::to_string_pretty(config)
        .context("Failed to serialize config")?;
    std::fs::write(&path, content)
        .with_context(|| format!("Failed to write config at {}", path.display()))?;
    Ok(())
}

/// Print current config
pub fn show_config() -> Result<()> {
    let config = load_config()?;
    let path = config_path()?;
    println!("Config file: {}", path.display());
    println!();
    println!("[server]");
    println!("  url     = {}", config.server.url);
    println!(
        "  api_key = {}",
        if config.server.api_key.is_empty() {
            "(not set)".to_string()
        } else {
            format!("{}...", &config.server.api_key[..8.min(config.server.api_key.len())])
        }
    );
    println!(
        "  team_id = {}",
        if config.server.team_id.is_empty() {
            "(not set)".to_string()
        } else {
            config.server.team_id.clone()
        }
    );
    println!();
    println!("[daemon]");
    println!("  auto_start = {}", config.daemon.auto_start);
    Ok(())
}

/// Update config with provided values
pub fn set_config(server_url: Option<String>, api_key: Option<String>, team_id: Option<String>) -> Result<()> {
    let mut config = load_config()?;

    if let Some(url) = server_url {
        config.server.url = url;
    }
    if let Some(key) = api_key {
        config.server.api_key = key;
    }
    if let Some(tid) = team_id {
        config.server.team_id = tid;
    }

    save_config(&config)?;
    println!("Configuration updated.");
    show_config()?;
    Ok(())
}
