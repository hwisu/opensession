use anyhow::{Context, Result};
use opensession_runtime_config::DaemonConfig;
use opensession_summary::provider::LocalSummaryProfile;
use std::path::PathBuf;

pub fn runtime_config_path() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .context("Could not determine home directory")?;
    Ok(PathBuf::from(home)
        .join(".config")
        .join("opensession")
        .join(opensession_runtime_config::CONFIG_FILE_NAME))
}

pub fn load_runtime_config() -> Result<DaemonConfig> {
    let path = runtime_config_path()?;
    if !path.exists() {
        return Ok(DaemonConfig::default());
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read runtime config at {}", path.display()))?;
    toml::from_str(&content)
        .with_context(|| format!("Failed to parse runtime config at {}", path.display()))
}

pub fn save_runtime_config(config: &DaemonConfig) -> Result<PathBuf> {
    let path = runtime_config_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create runtime config dir {}", parent.display()))?;
    }
    let body = toml::to_string_pretty(config).context("serialize runtime config")?;
    std::fs::write(&path, body)
        .with_context(|| format!("write runtime config {}", path.display()))?;
    Ok(path)
}

pub fn detect_local_summary_profile() -> Option<LocalSummaryProfile> {
    opensession_summary::detect_summary_provider()
}

pub fn apply_summary_profile(config: &mut DaemonConfig, profile: &LocalSummaryProfile) {
    config.summary.provider.id = profile.provider.clone();
    config.summary.provider.endpoint = profile.endpoint.clone();
    config.summary.provider.model = profile.model.clone();
}
