use anyhow::{Context, Result};
use opensession_runtime_config::{apply_compat_fallbacks, DaemonConfig, CONFIG_FILE_NAME};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const DEFAULT_SERVER_URL: &str = "https://opensession.io";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CliConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub daemon: DaemonRefConfig,
    #[serde(default)]
    pub privacy: PrivacyConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_server_url")]
    pub url: String,
    #[serde(default)]
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonRefConfig {
    #[serde(default = "default_true")]
    pub auto_start: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PrivacyConfig {
    #[serde(default)]
    pub exclude_tools: Vec<String>,
}

fn default_server_url() -> String {
    DEFAULT_SERVER_URL.to_string()
}

fn default_true() -> bool {
    true
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            url: default_server_url(),
            api_key: String::new(),
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
    Ok(PathBuf::from(home).join(".config").join("opensession"))
}

/// Canonical config file path.
pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join(CONFIG_FILE_NAME))
}

fn read_config_doc(path: &Path) -> Result<toml::Value> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config at {}", path.display()))?;
    let doc = toml::from_str::<toml::Value>(&content)
        .with_context(|| format!("Failed to parse config at {}", path.display()))?;
    Ok(doc)
}

fn ensure_root_table(doc: &mut toml::Value) -> &mut toml::map::Map<String, toml::Value> {
    if !doc.is_table() {
        *doc = toml::Value::Table(toml::map::Map::new());
    }
    doc.as_table_mut().expect("toml root table")
}

fn ensure_child_table<'a>(
    parent: &'a mut toml::map::Map<String, toml::Value>,
    key: &str,
) -> &'a mut toml::map::Map<String, toml::Value> {
    let entry = parent
        .entry(key.to_string())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    if !entry.is_table() {
        *entry = toml::Value::Table(toml::map::Map::new());
    }
    entry.as_table_mut().expect("toml child table")
}

fn set_bool(doc: &mut toml::Value, section: &str, key: &str, value: bool) {
    let root = ensure_root_table(doc);
    let section_table = ensure_child_table(root, section);
    section_table.insert(key.to_string(), toml::Value::Boolean(value));
}

fn get_value<'a>(root: &'a toml::Value, path: &[&str]) -> Option<&'a toml::Value> {
    let mut current = root;
    for key in path {
        current = current.get(*key)?;
    }
    Some(current)
}

fn get_bool(root: &toml::Value, path: &[&str]) -> Option<bool> {
    get_value(root, path).and_then(toml::Value::as_bool)
}

fn load_runtime_config_from_doc(doc: &toml::Value) -> DaemonConfig {
    let mut config = doc
        .clone()
        .try_into::<DaemonConfig>()
        .unwrap_or_else(|_| DaemonConfig::default());
    apply_compat_fallbacks(&mut config, Some(doc));
    config
}

fn load_runtime_config_from_disk() -> Result<(DaemonConfig, bool, bool)> {
    let path = config_path()?;
    let mut auto_start = true;
    let mut legacy_team_id_present = false;

    let config = if path.exists() {
        let doc = read_config_doc(&path)?;
        auto_start = get_bool(&doc, &["cli", "auto_start"]).unwrap_or(true);
        legacy_team_id_present = [("server", "team_id"), ("identity", "team_id")]
            .into_iter()
            .any(|(section, key)| {
                get_value(&doc, &[section, key])
                    .and_then(toml::Value::as_str)
                    .is_some_and(|v| !v.trim().is_empty())
            });
        load_runtime_config_from_doc(&doc)
    } else {
        DaemonConfig::default()
    };

    Ok((config, auto_start, legacy_team_id_present))
}

fn write_runtime_config(config: &DaemonConfig, auto_start: bool) -> Result<()> {
    let dir = config_dir()?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create config dir at {}", dir.display()))?;

    let mut doc = toml::Value::try_from(config.clone()).context("Failed to serialize config")?;
    set_bool(&mut doc, "cli", "auto_start", auto_start);

    let content = toml::to_string_pretty(&doc).context("Failed to serialize config")?;
    let path = config_path()?;
    std::fs::write(&path, content)
        .with_context(|| format!("Failed to write config at {}", path.display()))?;
    Ok(())
}

/// Load CLI-facing config from disk, returning default if not found.
pub fn load_config() -> Result<CliConfig> {
    let (runtime, auto_start, legacy_team_id_present) = load_runtime_config_from_disk()?;
    if legacy_team_id_present {
        eprintln!("Warning: legacy team_id fields are ignored and can be removed from config.");
    }
    Ok(CliConfig {
        server: ServerConfig {
            url: if runtime.server.url.trim().is_empty() {
                default_server_url()
            } else {
                runtime.server.url
            },
            api_key: runtime.server.api_key,
        },
        daemon: DaemonRefConfig { auto_start },
        privacy: PrivacyConfig {
            exclude_tools: runtime.privacy.exclude_tools,
        },
    })
}

/// Save CLI-facing config to disk (in `opensession.toml`).
pub fn save_config(config: &CliConfig) -> Result<()> {
    let (mut runtime, _, _) = load_runtime_config_from_disk()?;
    runtime.server.url = config.server.url.clone();
    runtime.server.api_key = config.server.api_key.clone();
    runtime.privacy.exclude_tools = config.privacy.exclude_tools.clone();
    write_runtime_config(&runtime, config.daemon.auto_start)
}

/// Load daemon runtime config from the canonical config file.
pub fn load_daemon_config() -> Result<DaemonConfig> {
    let (runtime, _, _) = load_runtime_config_from_disk()?;
    Ok(runtime)
}

/// Save daemon runtime config while preserving CLI-specific flags.
pub fn save_daemon_config(config: &DaemonConfig) -> Result<()> {
    let (_, auto_start, _) = load_runtime_config_from_disk()?;
    write_runtime_config(config, auto_start)
}

/// Print current config.
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
            format!(
                "{}...",
                &config.server.api_key[..8.min(config.server.api_key.len())]
            )
        }
    );
    println!();
    println!("[daemon]");
    println!("  auto_start = {}", config.daemon.auto_start);
    let daemon_cfg = load_daemon_config()?;
    println!("  watch paths = {}", daemon_cfg.watchers.custom_paths.len());
    for path in daemon_cfg.watchers.custom_paths {
        println!("    - {}", path);
    }
    Ok(())
}

/// Update config with provided values.
pub fn set_config(server_url: Option<String>, api_key: Option<String>) -> Result<()> {
    let mut config = load_config()?;

    if let Some(url) = server_url {
        config.server.url = url;
    }
    if let Some(key) = api_key {
        config.server.api_key = key;
    }

    save_config(&config)?;
    println!("Configuration updated.");
    show_config()?;
    Ok(())
}

/// Return current daemon watch paths for CLI display/updates.
pub fn daemon_watch_paths() -> Result<Vec<String>> {
    let daemon = load_daemon_config()?;
    Ok(daemon.watchers.custom_paths)
}

/// Update daemon watch paths in one write.
pub fn set_daemon_watch_paths(repos: Vec<String>) -> Result<()> {
    let mut daemon = load_daemon_config()?;
    daemon.watchers.custom_paths = repos;
    save_daemon_config(&daemon)
}
