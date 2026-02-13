use anyhow::{Context, Result};
use std::path::PathBuf;

// Re-export shared config types from core
pub use opensession_core::config::{DaemonConfig, GitStorageMethod, PublishMode};

// ── File I/O ────────────────────────────────────────────────────────────

pub fn config_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .context("Could not determine home directory")?;
    Ok(PathBuf::from(home).join(".config").join("opensession"))
}

/// Load daemon config from `~/.config/opensession/daemon.toml`.
/// Falls back to migrating from CLI `config.toml` if daemon.toml doesn't exist.
pub fn load_daemon_config() -> DaemonConfig {
    let dir = match config_dir() {
        Ok(d) => d,
        Err(_) => return DaemonConfig::default(),
    };

    let daemon_path = dir.join("daemon.toml");
    if daemon_path.exists() {
        return std::fs::read_to_string(&daemon_path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default();
    }

    // Fallback: migrate from CLI config.toml
    let cli_path = dir.join("config.toml");
    if cli_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&cli_path) {
            if let Ok(cli) = toml::from_str::<toml::Value>(&content) {
                let mut config = DaemonConfig::default();
                if let Some(server) = cli.get("server") {
                    if let Some(url) = server.get("url").and_then(|v| v.as_str()) {
                        config.server.url = url.to_string();
                    }
                    if let Some(key) = server.get("api_key").and_then(|v| v.as_str()) {
                        config.server.api_key = key.to_string();
                    }
                    if let Some(tid) = server.get("team_id").and_then(|v| v.as_str()) {
                        config.identity.team_id = tid.to_string();
                    }
                }
                // Auto-save the migrated config
                let _ = save_daemon_config(&config);
                return config;
            }
        }
    }

    DaemonConfig::default()
}

/// Save daemon config to `~/.config/opensession/daemon.toml`.
pub fn save_daemon_config(config: &DaemonConfig) -> Result<()> {
    let dir = config_dir()?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("daemon.toml");
    let content = toml::to_string_pretty(config).context("Failed to serialize config")?;
    std::fs::write(&path, content)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

/// Get daemon PID from PID file, if it exists.
pub fn daemon_pid() -> Option<u32> {
    let pid_path = config_dir().ok()?.join("daemon.pid");
    let content = std::fs::read_to_string(pid_path).ok()?;
    content.trim().parse().ok()
}

// ── Setting fields enum ─────────────────────────────────────────────────

/// Identifies a single editable setting in the settings view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingField {
    ServerUrl,
    ApiKey,
    TeamId,
    Nickname,
    AutoPublish,
    PublishMode,
    DebounceSecs,
    HealthCheckSecs,
    MaxRetries,
    WatchClaudeCode,
    WatchOpenCode,
    WatchGoose,
    WatchAider,
    WatchCursor,
    GitStorageMethod,
    GitStorageToken,
    StripPaths,
    StripEnvVars,
}

/// A display item in the settings list. Headers are not selectable.
#[derive(Debug, Clone)]
pub enum SettingItem {
    Header(&'static str),
    Field {
        field: SettingField,
        label: &'static str,
        description: &'static str,
        dependency_hint: Option<&'static str>,
    },
}

impl SettingItem {
    pub fn field(&self) -> Option<SettingField> {
        match self {
            Self::Header(_) => None,
            Self::Field { field, .. } => Some(*field),
        }
    }
}

/// The ordered list of items shown in the settings view.
pub const SETTINGS_LAYOUT: &[SettingItem] = &[
    SettingItem::Header("Server"),
    SettingItem::Field {
        field: SettingField::ServerUrl,
        label: "Server URL",
        description: "URL of the OpenSession server to sync with",
        dependency_hint: None,
    },
    SettingItem::Field {
        field: SettingField::ApiKey,
        label: "API Key (personal)",
        description: "Personal authentication key for cloud/team/public sync",
        dependency_hint: None,
    },
    SettingItem::Field {
        field: SettingField::TeamId,
        label: "Team ID",
        description: "Default team to publish sessions to",
        dependency_hint: None,
    },
    SettingItem::Header("Identity"),
    SettingItem::Field {
        field: SettingField::Nickname,
        label: "Handle",
        description: "Display handle shown on your sessions",
        dependency_hint: None,
    },
    SettingItem::Header("Daemon"),
    SettingItem::Field {
        field: SettingField::AutoPublish,
        label: "Auto Publish",
        description: "Automatically publish sessions when they end",
        dependency_hint: Some("Start the daemon to enable this setting"),
    },
    SettingItem::Field {
        field: SettingField::PublishMode,
        label: "Publish Mode",
        description: "When to send data: session_end / realtime / manual",
        dependency_hint: Some("Start the daemon to enable this setting"),
    },
    SettingItem::Field {
        field: SettingField::DebounceSecs,
        label: "Debounce (secs)",
        description: "Seconds to wait after last event before publishing",
        dependency_hint: Some("Start the daemon to enable this setting"),
    },
    SettingItem::Field {
        field: SettingField::HealthCheckSecs,
        label: "Health Check (secs)",
        description: "How often the daemon checks server connectivity",
        dependency_hint: Some("Start the daemon to enable this setting"),
    },
    SettingItem::Field {
        field: SettingField::MaxRetries,
        label: "Max Retries",
        description: "Maximum retry attempts for failed uploads",
        dependency_hint: Some("Start the daemon to enable this setting"),
    },
    SettingItem::Header("Watchers"),
    SettingItem::Field {
        field: SettingField::WatchClaudeCode,
        label: "Claude Code",
        description: "Monitor Claude Code sessions for auto-capture",
        dependency_hint: Some("Start the daemon to enable this setting"),
    },
    SettingItem::Field {
        field: SettingField::WatchOpenCode,
        label: "OpenCode",
        description: "Monitor OpenCode sessions for auto-capture",
        dependency_hint: Some("Start the daemon to enable this setting"),
    },
    SettingItem::Field {
        field: SettingField::WatchGoose,
        label: "Goose",
        description: "Monitor Goose sessions for auto-capture",
        dependency_hint: Some("Start the daemon to enable this setting"),
    },
    SettingItem::Field {
        field: SettingField::WatchAider,
        label: "Aider",
        description: "Monitor Aider sessions for auto-capture",
        dependency_hint: Some("Start the daemon to enable this setting"),
    },
    SettingItem::Field {
        field: SettingField::WatchCursor,
        label: "Cursor",
        description: "Monitor Cursor sessions (experimental)",
        dependency_hint: Some("Start the daemon to enable this setting"),
    },
    SettingItem::Header("Git Storage"),
    SettingItem::Field {
        field: SettingField::GitStorageMethod,
        label: "Method",
        description: "platform_api: GitHub/GitLab API (token required) · native: git objects · none: disabled",
        dependency_hint: None,
    },
    SettingItem::Field {
        field: SettingField::GitStorageToken,
        label: "Token",
        description: "GitHub PAT with 'repo' scope — github.com/settings/tokens",
        dependency_hint: Some("Set method to Platform API or Native first"),
    },
    SettingItem::Header("Privacy"),
    SettingItem::Field {
        field: SettingField::StripPaths,
        label: "Strip Paths",
        description: "Replace absolute paths (e.g. /Users/foo/bar → bar) before publishing",
        dependency_hint: None,
    },
    SettingItem::Field {
        field: SettingField::StripEnvVars,
        label: "Strip Env Vars",
        description: "Redact env var values (e.g. API_KEY=xxx → API_KEY=[REDACTED])",
        dependency_hint: None,
    },
];

impl SettingField {
    /// Whether this field is a boolean toggle.
    pub fn is_toggle(self) -> bool {
        matches!(
            self,
            Self::AutoPublish
                | Self::WatchClaudeCode
                | Self::WatchOpenCode
                | Self::WatchGoose
                | Self::WatchAider
                | Self::WatchCursor
                | Self::StripPaths
                | Self::StripEnvVars
        )
    }

    /// Whether this field cycles through enum options.
    pub fn is_enum(self) -> bool {
        matches!(self, Self::PublishMode | Self::GitStorageMethod)
    }

    /// Get the current value as a display string from the config.
    pub fn display_value(self, config: &DaemonConfig) -> String {
        match self {
            Self::ServerUrl => config.server.url.clone(),
            Self::ApiKey => {
                if config.server.api_key.is_empty() {
                    "(not set)".to_string()
                } else {
                    let key = &config.server.api_key;
                    let visible = key.len().min(8);
                    format!("{}...", &key[..visible])
                }
            }
            Self::TeamId => {
                if config.identity.team_id.is_empty() {
                    "(not set)".to_string()
                } else {
                    config.identity.team_id.clone()
                }
            }
            Self::Nickname => config.identity.nickname.clone(),
            Self::AutoPublish => on_off(config.daemon.auto_publish),
            Self::PublishMode => config.daemon.publish_on.display().to_string(),
            Self::DebounceSecs => config.daemon.debounce_secs.to_string(),
            Self::HealthCheckSecs => config.daemon.health_check_interval_secs.to_string(),
            Self::MaxRetries => config.daemon.max_retries.to_string(),
            Self::WatchClaudeCode => on_off(config.watchers.claude_code),
            Self::WatchOpenCode => on_off(config.watchers.opencode),
            Self::WatchGoose => on_off(config.watchers.goose),
            Self::WatchAider => on_off(config.watchers.aider),
            Self::WatchCursor => on_off(config.watchers.cursor),
            Self::GitStorageMethod => match config.git_storage.method {
                GitStorageMethod::None => "None".to_string(),
                GitStorageMethod::PlatformApi => "Platform API".to_string(),
                GitStorageMethod::Native => "Native".to_string(),
            },
            Self::GitStorageToken => {
                if config.git_storage.token.is_empty() {
                    "(not set)".to_string()
                } else {
                    let len = config.git_storage.token.len();
                    let visible = len.min(4);
                    format!(
                        "{}...{}",
                        &config.git_storage.token[..visible],
                        len - visible
                    )
                }
            }
            Self::StripPaths => on_off(config.privacy.strip_paths),
            Self::StripEnvVars => on_off(config.privacy.strip_env_vars),
        }
    }

    /// Get the raw (editable) value from the config.
    pub fn raw_value(self, config: &DaemonConfig) -> String {
        match self {
            Self::ServerUrl => config.server.url.clone(),
            Self::ApiKey => config.server.api_key.clone(),
            Self::TeamId => config.identity.team_id.clone(),
            Self::Nickname => config.identity.nickname.clone(),
            Self::DebounceSecs => config.daemon.debounce_secs.to_string(),
            Self::HealthCheckSecs => config.daemon.health_check_interval_secs.to_string(),
            Self::MaxRetries => config.daemon.max_retries.to_string(),
            Self::GitStorageToken => config.git_storage.token.clone(),
            _ => String::new(),
        }
    }

    /// Toggle a boolean field in the config.
    pub fn toggle(self, config: &mut DaemonConfig) {
        match self {
            Self::AutoPublish => config.daemon.auto_publish = !config.daemon.auto_publish,
            Self::WatchClaudeCode => config.watchers.claude_code = !config.watchers.claude_code,
            Self::WatchOpenCode => config.watchers.opencode = !config.watchers.opencode,
            Self::WatchGoose => config.watchers.goose = !config.watchers.goose,
            Self::WatchAider => config.watchers.aider = !config.watchers.aider,
            Self::WatchCursor => config.watchers.cursor = !config.watchers.cursor,
            Self::StripPaths => config.privacy.strip_paths = !config.privacy.strip_paths,
            Self::StripEnvVars => config.privacy.strip_env_vars = !config.privacy.strip_env_vars,
            _ => {}
        }
    }

    /// Cycle an enum field.
    pub fn cycle_enum(self, config: &mut DaemonConfig) {
        match self {
            Self::PublishMode => {
                config.daemon.publish_on = config.daemon.publish_on.cycle();
            }
            Self::GitStorageMethod => {
                config.git_storage.method = match config.git_storage.method {
                    GitStorageMethod::None => GitStorageMethod::PlatformApi,
                    GitStorageMethod::PlatformApi => GitStorageMethod::Native,
                    GitStorageMethod::Native => GitStorageMethod::None,
                };
            }
            _ => {}
        }
    }

    /// Set a text/number value.
    pub fn set_value(self, config: &mut DaemonConfig, value: &str) {
        match self {
            Self::ServerUrl => config.server.url = value.to_string(),
            Self::ApiKey => config.server.api_key = value.to_string(),
            Self::TeamId => config.identity.team_id = value.to_string(),
            Self::Nickname => config.identity.nickname = value.to_string(),
            Self::DebounceSecs => {
                if let Ok(v) = value.parse() {
                    config.daemon.debounce_secs = v;
                }
            }
            Self::HealthCheckSecs => {
                if let Ok(v) = value.parse() {
                    config.daemon.health_check_interval_secs = v;
                }
            }
            Self::MaxRetries => {
                if let Ok(v) = value.parse() {
                    config.daemon.max_retries = v;
                }
            }
            Self::GitStorageToken => {
                config.git_storage.token = value.to_string();
            }
            _ => {}
        }
    }
}

fn on_off(v: bool) -> String {
    if v {
        "ON".to_string()
    } else {
        "OFF".to_string()
    }
}

/// Count of selectable (non-header) fields in SETTINGS_LAYOUT.
pub fn selectable_field_count() -> usize {
    SETTINGS_LAYOUT
        .iter()
        .filter(|item| item.field().is_some())
        .count()
}

/// Get the nth selectable field.
pub fn nth_selectable_field(n: usize) -> Option<SettingField> {
    SETTINGS_LAYOUT
        .iter()
        .filter_map(|item| item.field())
        .nth(n)
}
