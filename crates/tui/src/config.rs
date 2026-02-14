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
    RealtimeDebounceMs,
    DetailRealtimePreviewEnabled,
    HealthCheckSecs,
    MaxRetries,
    SummaryEnabled,
    SummaryProvider,
    SummaryCliAgent,
    SummaryEventWindow,
    SummaryDebounceMs,
    SummaryMaxInflight,
    StreamWriteClaude,
    StreamWriteCodex,
    StreamWriteCursor,
    StreamWriteGemini,
    StreamWriteOpenCode,
    WatchClaudeCode,
    WatchOpenCode,
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
            Self::Header(title) => {
                let _ = title;
                None
            }
            Self::Field { field, .. } => Some(*field),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsGroup {
    Workspace,
    CaptureSync,
    TimelineIntelligence,
    StoragePrivacy,
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
    SettingItem::Header("Daemon Publish"),
    SettingItem::Field {
        field: SettingField::AutoPublish,
        label: "Auto Publish",
        description: "Automatically upload captured sessions",
        dependency_hint: Some("Applies when daemon is running"),
    },
    SettingItem::Field {
        field: SettingField::PublishMode,
        label: "Publish Mode",
        description: "When to send data: session_end / realtime / manual",
        dependency_hint: Some("Applies when daemon is running"),
    },
    SettingItem::Field {
        field: SettingField::DebounceSecs,
        label: "Debounce (secs)",
        description: "Seconds to wait after last event before publishing",
        dependency_hint: Some("Applies when daemon is running"),
    },
    SettingItem::Field {
        field: SettingField::RealtimeDebounceMs,
        label: "Realtime Poll (ms)",
        description: "Global polling interval for daemon realtime publish and detail auto-refresh checks",
        dependency_hint: Some("Used by daemon realtime publish and detail auto-refresh"),
    },
    SettingItem::Field {
        field: SettingField::DetailRealtimePreviewEnabled,
        label: "Detail Auto-Refresh",
        description: "Auto-reload Session Detail when selected source file mtime changes",
        dependency_hint: Some("Global toggle; stream-write tools are skipped"),
    },
    SettingItem::Field {
        field: SettingField::HealthCheckSecs,
        label: "Health Check (secs)",
        description: "How often the daemon checks server connectivity",
        dependency_hint: Some("Applies when daemon is running"),
    },
    SettingItem::Field {
        field: SettingField::MaxRetries,
        label: "Max Retries",
        description: "Maximum retry attempts for failed uploads",
        dependency_hint: Some("Applies when daemon is running"),
    },
    SettingItem::Header("LLM Summary"),
    SettingItem::Field {
        field: SettingField::SummaryEnabled,
        label: "LLM Summary Enabled",
        description: "Generate LLM timeline summaries in Session Detail",
        dependency_hint: Some("If summary backend is unavailable, this auto-turns OFF"),
    },
    SettingItem::Field {
        field: SettingField::SummaryProvider,
        label: "LLM Summary Mode",
        description: "auto(API), API provider, API:OpenAI-Compatible, or CLI mode",
        dependency_hint: Some("CLI mode requires a configured LLM Summary CLI Agent"),
    },
    SettingItem::Field {
        field: SettingField::SummaryCliAgent,
        label: "LLM Summary CLI Agent",
        description: "Which CLI to call for summary (auto/codex/claude/cursor/gemini)",
        dependency_hint: Some("Active only when LLM Summary Enabled=ON and LLM Summary Mode=CLI"),
    },
    SettingItem::Field {
        field: SettingField::SummaryEventWindow,
        label: "LLM Summary Window",
        description: "Checkpoint size in events (set 0 or 'auto' for full-turn auto segmentation)",
        dependency_hint: Some("Active only when LLM Summary Enabled=ON"),
    },
    SettingItem::Field {
        field: SettingField::SummaryDebounceMs,
        label: "LLM Summary Debounce (ms)",
        description: "Minimum time to wait before scheduling next summary request",
        dependency_hint: Some("Active only when LLM Summary Enabled=ON"),
    },
    SettingItem::Field {
        field: SettingField::SummaryMaxInflight,
        label: "LLM Summary Max Inflight",
        description: "Maximum concurrent summary requests (separate from debounce)",
        dependency_hint: Some("Active only when LLM Summary Enabled=ON"),
    },
    SettingItem::Header("Realtime Session Files"),
    SettingItem::Field {
        field: SettingField::StreamWriteClaude,
        label: "Claude Code Stream",
        description: "Treat as stream-write session file (skip detail realtime+summary)",
        dependency_hint: Some("Per-agent realtime file mode; usually OFF unless using hook"),
    },
    SettingItem::Field {
        field: SettingField::StreamWriteCodex,
        label: "Codex Stream",
        description: "Treat as stream-write session file (skip detail realtime+summary)",
        dependency_hint: Some("Per-agent realtime file mode"),
    },
    SettingItem::Field {
        field: SettingField::StreamWriteCursor,
        label: "Cursor Stream",
        description: "Treat as stream-write session file (skip detail realtime+summary)",
        dependency_hint: Some("Per-agent realtime file mode"),
    },
    SettingItem::Field {
        field: SettingField::StreamWriteGemini,
        label: "Gemini Stream",
        description: "Treat as stream-write session file (skip detail realtime+summary)",
        dependency_hint: Some("Per-agent realtime file mode"),
    },
    SettingItem::Field {
        field: SettingField::StreamWriteOpenCode,
        label: "OpenCode Stream",
        description: "Treat as stream-write session file (skip detail realtime+summary)",
        dependency_hint: Some("Per-agent realtime file mode"),
    },
    SettingItem::Header("Watchers"),
    SettingItem::Field {
        field: SettingField::WatchClaudeCode,
        label: "Claude Code",
        description: "Monitor Claude Code sessions for auto-capture",
        dependency_hint: Some("Applies when daemon is running"),
    },
    SettingItem::Field {
        field: SettingField::WatchOpenCode,
        label: "OpenCode",
        description: "Monitor OpenCode sessions for auto-capture",
        dependency_hint: Some("Applies when daemon is running"),
    },
    SettingItem::Field {
        field: SettingField::WatchCursor,
        label: "Cursor",
        description: "Monitor Cursor sessions (experimental)",
        dependency_hint: Some("Applies when daemon is running"),
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
                | Self::DetailRealtimePreviewEnabled
                | Self::SummaryEnabled
                | Self::StreamWriteClaude
                | Self::StreamWriteCodex
                | Self::StreamWriteCursor
                | Self::StreamWriteGemini
                | Self::StreamWriteOpenCode
                | Self::WatchClaudeCode
                | Self::WatchOpenCode
                | Self::WatchCursor
                | Self::StripPaths
                | Self::StripEnvVars
        )
    }

    /// Whether this field cycles through enum options.
    pub fn is_enum(self) -> bool {
        matches!(
            self,
            Self::PublishMode
                | Self::GitStorageMethod
                | Self::SummaryProvider
                | Self::SummaryCliAgent
        )
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
            Self::RealtimeDebounceMs => config.daemon.realtime_debounce_ms.to_string(),
            Self::DetailRealtimePreviewEnabled => {
                on_off(config.daemon.detail_realtime_preview_enabled)
            }
            Self::HealthCheckSecs => config.daemon.health_check_interval_secs.to_string(),
            Self::MaxRetries => config.daemon.max_retries.to_string(),
            Self::SummaryEnabled => on_off(config.daemon.summary_enabled),
            Self::SummaryProvider => summary_mode_label(config.daemon.summary_provider.as_deref()),
            Self::SummaryCliAgent => {
                summary_cli_agent_label(config.daemon.summary_provider.as_deref())
            }
            Self::SummaryEventWindow => {
                if config.daemon.summary_event_window == 0 {
                    "AUTO(turn)".to_string()
                } else {
                    config.daemon.summary_event_window.to_string()
                }
            }
            Self::SummaryDebounceMs => config.daemon.summary_debounce_ms.to_string(),
            Self::SummaryMaxInflight => config.daemon.summary_max_inflight.to_string(),
            Self::StreamWriteClaude => on_off(stream_write_enabled(config, "claude-code")),
            Self::StreamWriteCodex => on_off(stream_write_enabled(config, "codex")),
            Self::StreamWriteCursor => on_off(stream_write_enabled(config, "cursor")),
            Self::StreamWriteGemini => on_off(stream_write_enabled(config, "gemini")),
            Self::StreamWriteOpenCode => on_off(stream_write_enabled(config, "opencode")),
            Self::WatchClaudeCode => on_off(config.watchers.claude_code),
            Self::WatchOpenCode => on_off(config.watchers.opencode),
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
            Self::RealtimeDebounceMs => config.daemon.realtime_debounce_ms.to_string(),
            Self::HealthCheckSecs => config.daemon.health_check_interval_secs.to_string(),
            Self::MaxRetries => config.daemon.max_retries.to_string(),
            Self::SummaryEventWindow => {
                if config.daemon.summary_event_window == 0 {
                    "auto".to_string()
                } else {
                    config.daemon.summary_event_window.to_string()
                }
            }
            Self::SummaryDebounceMs => config.daemon.summary_debounce_ms.to_string(),
            Self::SummaryMaxInflight => config.daemon.summary_max_inflight.to_string(),
            Self::GitStorageToken => config.git_storage.token.clone(),
            _ => String::new(),
        }
    }

    /// Toggle a boolean field in the config.
    pub fn toggle(self, config: &mut DaemonConfig) {
        match self {
            Self::AutoPublish => config.daemon.auto_publish = !config.daemon.auto_publish,
            Self::DetailRealtimePreviewEnabled => {
                config.daemon.detail_realtime_preview_enabled =
                    !config.daemon.detail_realtime_preview_enabled;
            }
            Self::StreamWriteClaude => {
                toggle_stream_write(config, "claude-code");
            }
            Self::StreamWriteCodex => {
                toggle_stream_write(config, "codex");
            }
            Self::StreamWriteCursor => {
                toggle_stream_write(config, "cursor");
            }
            Self::StreamWriteGemini => {
                toggle_stream_write(config, "gemini");
            }
            Self::StreamWriteOpenCode => {
                toggle_stream_write(config, "opencode");
            }
            Self::WatchClaudeCode => config.watchers.claude_code = !config.watchers.claude_code,
            Self::WatchOpenCode => config.watchers.opencode = !config.watchers.opencode,
            Self::WatchCursor => config.watchers.cursor = !config.watchers.cursor,
            Self::SummaryEnabled => config.daemon.summary_enabled = !config.daemon.summary_enabled,
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
            Self::SummaryProvider => {
                let mode = summary_mode_key(config.daemon.summary_provider.as_deref());
                match mode {
                    "auto" => config.daemon.summary_provider = Some("anthropic".to_string()),
                    "anthropic" => config.daemon.summary_provider = Some("openai".to_string()),
                    "openai" => {
                        config.daemon.summary_provider = Some("openai-compatible".to_string())
                    }
                    "openai-compatible" => {
                        config.daemon.summary_provider = Some("gemini".to_string())
                    }
                    "gemini" => {
                        let agent =
                            summary_cli_agent_key(config.daemon.summary_provider.as_deref());
                        config.daemon.summary_provider = Some(cli_provider_for_agent(agent));
                    }
                    "cli" => config.daemon.summary_provider = None,
                    _ => config.daemon.summary_provider = None,
                }
            }
            Self::SummaryCliAgent => {
                let next = match summary_cli_agent_key(config.daemon.summary_provider.as_deref()) {
                    "auto" => "codex",
                    "codex" => "claude",
                    "claude" => "cursor",
                    "cursor" => "gemini",
                    "gemini" => "auto",
                    _ => "auto",
                };
                config.daemon.summary_provider = Some(cli_provider_for_agent(next));
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
            Self::RealtimeDebounceMs => {
                if let Ok(v) = value.parse() {
                    config.daemon.realtime_debounce_ms = v;
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
            Self::SummaryEventWindow => {
                let normalized = value.trim().to_ascii_lowercase();
                if normalized == "auto" {
                    config.daemon.summary_event_window = 0;
                } else if let Ok(v) = normalized.parse::<u32>() {
                    config.daemon.summary_event_window = v;
                }
            }
            Self::SummaryDebounceMs => {
                if let Ok(v) = value.parse() {
                    config.daemon.summary_debounce_ms = v;
                }
            }
            Self::SummaryMaxInflight => {
                if let Ok(v) = value.parse::<u32>() {
                    config.daemon.summary_max_inflight = v.max(1);
                }
            }
            Self::GitStorageToken => {
                config.git_storage.token = value.to_string();
            }
            Self::SummaryEnabled => {
                let lowered = value.to_lowercase();
                config.daemon.summary_enabled =
                    matches!(lowered.as_str(), "on" | "1" | "true" | "yes");
            }
            Self::DetailRealtimePreviewEnabled => {
                let lowered = value.to_lowercase();
                config.daemon.detail_realtime_preview_enabled =
                    matches!(lowered.as_str(), "on" | "1" | "true" | "yes");
            }
            Self::SummaryProvider => {
                let normalized = value.to_lowercase();
                let cleaned = normalized.trim();
                let mapped = match cleaned {
                    "" | "auto" => None,
                    "anthropic" | "openai" | "openai-compatible" | "gemini" => {
                        Some(cleaned.to_string())
                    }
                    "cli" => Some(cli_provider_for_agent(summary_cli_agent_key(
                        config.daemon.summary_provider.as_deref(),
                    ))),
                    "cli:auto" | "cli:codex" | "cli:claude" | "cli:cursor" | "cli:gemini" => {
                        Some(cleaned.to_string())
                    }
                    _ => Some(String::from("auto")),
                };
                config.daemon.summary_provider = mapped;
            }
            Self::SummaryCliAgent => {
                let normalized = value.to_lowercase();
                let cleaned = normalized.trim();
                let agent = match cleaned {
                    "" | "auto" => "auto",
                    "codex" => "codex",
                    "claude" => "claude",
                    "cursor" => "cursor",
                    "gemini" => "gemini",
                    _ => "auto",
                };
                config.daemon.summary_provider = Some(cli_provider_for_agent(agent));
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

fn stream_write_enabled(config: &DaemonConfig, tool: &str) -> bool {
    config
        .daemon
        .stream_write
        .iter()
        .any(|item| item.eq_ignore_ascii_case(tool))
}

fn toggle_stream_write(config: &mut DaemonConfig, tool: &str) {
    if let Some(idx) = config
        .daemon
        .stream_write
        .iter()
        .position(|item| item.eq_ignore_ascii_case(tool))
    {
        config.daemon.stream_write.remove(idx);
    } else {
        config.daemon.stream_write.push(tool.to_string());
    }
}

fn summary_mode_key(provider: Option<&str>) -> &'static str {
    match provider.unwrap_or("auto").to_ascii_lowercase().as_str() {
        "" | "auto" => "auto",
        "anthropic" => "anthropic",
        "openai" => "openai",
        "openai-compatible" => "openai-compatible",
        "gemini" => "gemini",
        "cli" | "cli:auto" | "cli:codex" | "cli:claude" | "cli:cursor" | "cli:gemini" => "cli",
        _ => "auto",
    }
}

fn summary_mode_label(provider: Option<&str>) -> String {
    match summary_mode_key(provider) {
        "auto" => "Auto(API)".to_string(),
        "anthropic" => "API:Anthropic".to_string(),
        "openai" => "API:OpenAI".to_string(),
        "openai-compatible" => "API:OpenAI-Compatible".to_string(),
        "gemini" => "API:Gemini".to_string(),
        "cli" => "CLI".to_string(),
        _ => "Auto(API)".to_string(),
    }
}

fn summary_cli_agent_key(provider: Option<&str>) -> &'static str {
    match provider.unwrap_or("cli:auto").to_ascii_lowercase().as_str() {
        "cli:codex" => "codex",
        "cli:claude" => "claude",
        "cli:cursor" => "cursor",
        "cli:gemini" => "gemini",
        _ => "auto",
    }
}

fn summary_cli_agent_label(provider: Option<&str>) -> String {
    match summary_cli_agent_key(provider) {
        "auto" => "Auto".to_string(),
        "codex" => "Codex".to_string(),
        "claude" => "Claude".to_string(),
        "cursor" => "Cursor".to_string(),
        "gemini" => "Gemini".to_string(),
        _ => "Auto".to_string(),
    }
}

fn cli_provider_for_agent(agent: &str) -> String {
    match agent {
        "codex" => "cli:codex".to_string(),
        "claude" => "cli:claude".to_string(),
        "cursor" => "cli:cursor".to_string(),
        "gemini" => "cli:gemini".to_string(),
        _ => "cli:auto".to_string(),
    }
}

fn group_for_field(field: SettingField) -> SettingsGroup {
    match field {
        SettingField::ServerUrl
        | SettingField::ApiKey
        | SettingField::TeamId
        | SettingField::Nickname => SettingsGroup::Workspace,

        SettingField::AutoPublish
        | SettingField::PublishMode
        | SettingField::DebounceSecs
        | SettingField::RealtimeDebounceMs
        | SettingField::HealthCheckSecs
        | SettingField::MaxRetries
        | SettingField::StreamWriteClaude
        | SettingField::StreamWriteCodex
        | SettingField::StreamWriteCursor
        | SettingField::StreamWriteGemini
        | SettingField::StreamWriteOpenCode
        | SettingField::WatchClaudeCode
        | SettingField::WatchOpenCode
        | SettingField::WatchCursor => SettingsGroup::CaptureSync,

        SettingField::DetailRealtimePreviewEnabled
        | SettingField::SummaryEnabled
        | SettingField::SummaryProvider
        | SettingField::SummaryCliAgent
        | SettingField::SummaryEventWindow
        | SettingField::SummaryDebounceMs
        | SettingField::SummaryMaxInflight => SettingsGroup::TimelineIntelligence,

        SettingField::GitStorageMethod
        | SettingField::GitStorageToken
        | SettingField::StripPaths
        | SettingField::StripEnvVars => SettingsGroup::StoragePrivacy,
    }
}

pub fn selectable_fields(section: SettingsGroup) -> Vec<SettingField> {
    SETTINGS_LAYOUT
        .iter()
        .filter_map(SettingItem::field)
        .filter(|field| group_for_field(*field) == section)
        .collect()
}

pub fn selectable_field_count(section: SettingsGroup) -> usize {
    selectable_fields(section).len()
}

pub fn nth_selectable_field(section: SettingsGroup, n: usize) -> Option<SettingField> {
    selectable_fields(section).into_iter().nth(n)
}

pub fn field_item(field: SettingField) -> &'static SettingItem {
    SETTINGS_LAYOUT
        .iter()
        .find(|item| item.field() == Some(field))
        .unwrap_or_else(|| panic!("missing setting metadata for {:?}", field))
}
