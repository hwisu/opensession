use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

// Re-export shared runtime config types
pub use opensession_runtime_config::{
    apply_compat_fallbacks, CalendarDisplayMode, DaemonConfig, GitStorageMethod, CONFIG_FILE_NAME,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GitStorageMode {
    None,
    PlatformApi,
    Native,
}

impl GitStorageMode {
    fn from_core(method: GitStorageMethod) -> Self {
        match method {
            GitStorageMethod::None => Self::None,
            GitStorageMethod::PlatformApi => Self::PlatformApi,
            GitStorageMethod::Native => Self::Native,
        }
    }

    fn to_core(self) -> GitStorageMethod {
        match self {
            Self::PlatformApi => GitStorageMethod::PlatformApi,
            Self::Native => GitStorageMethod::Native,
            Self::None => GitStorageMethod::None,
        }
    }

    fn as_toml_method(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::PlatformApi => "platform_api",
            Self::Native => "native",
        }
    }
}

#[derive(Debug, Clone)]
struct UiConfigState {
    calendar_display_mode: CalendarDisplayMode,
    git_storage_mode: GitStorageMode,
}

impl Default for UiConfigState {
    fn default() -> Self {
        Self {
            calendar_display_mode: CalendarDisplayMode::Smart,
            git_storage_mode: GitStorageMode::from_core(DaemonConfig::default().git_storage.method),
        }
    }
}

fn ui_config_state() -> &'static Mutex<UiConfigState> {
    static STATE: OnceLock<Mutex<UiConfigState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(UiConfigState::default()))
}

pub fn calendar_display_mode() -> CalendarDisplayMode {
    ui_config_state()
        .lock()
        .map(|state| state.calendar_display_mode.clone())
        .unwrap_or(CalendarDisplayMode::Smart)
}

fn set_calendar_display_mode(mode: CalendarDisplayMode) {
    if let Ok(mut state) = ui_config_state().lock() {
        state.calendar_display_mode = mode;
    }
}

fn git_storage_mode() -> GitStorageMode {
    ui_config_state()
        .lock()
        .map(|state| state.git_storage_mode)
        .unwrap_or_else(|_| GitStorageMode::from_core(DaemonConfig::default().git_storage.method))
}

fn set_git_storage_mode(config: &mut DaemonConfig, mode: GitStorageMode) {
    if let Ok(mut state) = ui_config_state().lock() {
        state.git_storage_mode = mode;
    }
    config.git_storage.method = mode.to_core();
}

fn sync_runtime_config_extensions(root: Option<&toml::Value>, config: &mut DaemonConfig) {
    let calendar_mode = parse_calendar_display_mode(root);
    let git_mode = parse_git_storage_mode(root, config.git_storage.method.clone());

    set_calendar_display_mode(calendar_mode);
    set_git_storage_mode(config, git_mode);
}

fn parse_calendar_display_mode(root: Option<&toml::Value>) -> CalendarDisplayMode {
    let value = root
        .and_then(|v| v.as_table())
        .and_then(|table| table.get("daemon"))
        .and_then(|section| section.as_table())
        .and_then(|section| section.get("calendar_display_mode"))
        .and_then(toml::Value::as_str)
        .unwrap_or("smart");

    match value.trim().to_ascii_lowercase().as_str() {
        "relative" | "rel" => CalendarDisplayMode::Relative,
        "absolute" | "abs" => CalendarDisplayMode::Absolute,
        _ => CalendarDisplayMode::Smart,
    }
}

fn parse_git_storage_mode(
    root: Option<&toml::Value>,
    fallback: GitStorageMethod,
) -> GitStorageMode {
    let value = root
        .and_then(|v| v.as_table())
        .and_then(|table| table.get("git_storage"))
        .and_then(|section| section.as_table())
        .and_then(|section| section.get("method"))
        .and_then(toml::Value::as_str);

    match value.map(|s| s.trim().to_ascii_lowercase()) {
        Some(v) if v == "none" => GitStorageMode::None,
        Some(v) if v == "platform_api" || v == "platform-api" || v == "api" => {
            GitStorageMode::PlatformApi
        }
        Some(v) if v == "native" => GitStorageMode::Native,
        Some(v) if v == "sqlite_local" || v == "sqlite-local" || v == "sqlite" => {
            GitStorageMode::Native
        }
        _ => GitStorageMode::from_core(fallback),
    }
}

fn calendar_display_mode_toml_value(mode: CalendarDisplayMode) -> &'static str {
    match mode {
        CalendarDisplayMode::Smart => "smart",
        CalendarDisplayMode::Relative => "relative",
        CalendarDisplayMode::Absolute => "absolute",
    }
}

fn apply_runtime_extensions_to_toml(doc: &mut toml::Value) {
    let Ok(state) = ui_config_state().lock().map(|guard| guard.clone()) else {
        return;
    };

    let Some(root) = doc.as_table_mut() else {
        return;
    };

    let daemon_entry = root
        .entry("daemon")
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    if let Some(daemon_table) = daemon_entry.as_table_mut() {
        daemon_table.insert(
            "calendar_display_mode".to_string(),
            toml::Value::String(
                calendar_display_mode_toml_value(state.calendar_display_mode).to_string(),
            ),
        );
    }

    let git_entry = root
        .entry("git_storage")
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    if let Some(git_table) = git_entry.as_table_mut() {
        git_table.insert(
            "method".to_string(),
            toml::Value::String(state.git_storage_mode.as_toml_method().to_string()),
        );
    }
}

// ── File I/O ────────────────────────────────────────────────────────────

pub fn config_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .context("Could not determine home directory")?;
    Ok(PathBuf::from(home).join(".config").join("opensession"))
}

/// Load daemon config from `~/.config/opensession/opensession.toml`.
pub fn load_daemon_config() -> DaemonConfig {
    let dir = match config_dir() {
        Ok(d) => d,
        Err(_) => {
            let mut config = DaemonConfig::default();
            sync_runtime_config_extensions(None, &mut config);
            return config;
        }
    };

    let daemon_path = dir.join(CONFIG_FILE_NAME);
    let mut migrated = false;

    if daemon_path.exists() {
        let content = std::fs::read_to_string(&daemon_path).unwrap_or_default();
        let parsed: Option<toml::Value> = toml::from_str(&content).ok();
        let mut config: DaemonConfig = toml::from_str(&content).unwrap_or_default();
        if apply_compat_fallbacks(&mut config, parsed.as_ref()) {
            migrated = true;
        }
        sync_runtime_config_extensions(parsed.as_ref(), &mut config);
        if migrate_summary_window_v2(&mut config) {
            migrated = true;
        }
        if migrated {
            let _ = save_daemon_config(&config);
        }
        return config;
    }

    let mut config = DaemonConfig::default();
    sync_runtime_config_extensions(None, &mut config);
    config
}

fn migrate_summary_window_v2(config: &mut DaemonConfig) -> bool {
    if config.daemon.summary_window_migrated_v2 {
        return false;
    }
    if config.daemon.summary_event_window != 8 {
        config.daemon.summary_window_migrated_v2 = true;
        return true;
    }
    config.daemon.summary_event_window = 0;
    config.daemon.summary_window_migrated_v2 = true;
    true
}

/// Save daemon config to `~/.config/opensession/opensession.toml`.
pub fn save_daemon_config(config: &DaemonConfig) -> Result<()> {
    let dir = config_dir()?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(CONFIG_FILE_NAME);
    let mut doc = toml::Value::try_from(config.clone()).context("Failed to serialize config")?;
    let root = doc
        .as_table_mut()
        .ok_or_else(|| anyhow!("config root is not a table"))?;
    let server = root
        .entry("server")
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    if !server.is_table() {
        *server = toml::Value::Table(toml::map::Map::new());
    }
    if let Some(server_table) = server.as_table_mut() {
        server_table.insert(
            "team_id".to_string(),
            toml::Value::String(config.identity.team_id.clone()),
        );
    }
    apply_runtime_extensions_to_toml(&mut doc);
    let content = toml::to_string_pretty(&doc).context("Failed to serialize config")?;
    std::fs::write(&path, content)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

pub const TIMELINE_PRESET_SLOT_MIN: u8 = 1;
pub const TIMELINE_PRESET_SLOT_MAX: u8 = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineIntelPreset {
    pub detail_realtime_preview_enabled: bool,
    pub summary_enabled: bool,
    pub summary_provider: Option<String>,
    pub summary_model: Option<String>,
    pub summary_content_mode: String,
    pub summary_disk_cache_enabled: bool,
    pub summary_openai_compat_endpoint: Option<String>,
    pub summary_openai_compat_base: Option<String>,
    pub summary_openai_compat_path: Option<String>,
    pub summary_openai_compat_style: Option<String>,
    pub summary_openai_compat_key: Option<String>,
    pub summary_openai_compat_key_header: Option<String>,
    pub summary_event_window: u32,
    pub summary_debounce_ms: u64,
    pub summary_max_inflight: u32,
}

impl Default for TimelineIntelPreset {
    fn default() -> Self {
        Self::from_config(&DaemonConfig::default())
    }
}

impl TimelineIntelPreset {
    pub fn from_config(config: &DaemonConfig) -> Self {
        Self {
            detail_realtime_preview_enabled: config.daemon.detail_realtime_preview_enabled,
            summary_enabled: config.daemon.summary_enabled,
            summary_provider: config.daemon.summary_provider.clone(),
            summary_model: config.daemon.summary_model.clone(),
            summary_content_mode: config.daemon.summary_content_mode.clone(),
            summary_disk_cache_enabled: config.daemon.summary_disk_cache_enabled,
            summary_openai_compat_endpoint: config.daemon.summary_openai_compat_endpoint.clone(),
            summary_openai_compat_base: config.daemon.summary_openai_compat_base.clone(),
            summary_openai_compat_path: config.daemon.summary_openai_compat_path.clone(),
            summary_openai_compat_style: config.daemon.summary_openai_compat_style.clone(),
            summary_openai_compat_key: config.daemon.summary_openai_compat_key.clone(),
            summary_openai_compat_key_header: config
                .daemon
                .summary_openai_compat_key_header
                .clone(),
            summary_event_window: config.daemon.summary_event_window,
            summary_debounce_ms: config.daemon.summary_debounce_ms,
            summary_max_inflight: config.daemon.summary_max_inflight.max(1),
        }
    }

    pub fn apply_to_config(&self, config: &mut DaemonConfig) {
        config.daemon.detail_realtime_preview_enabled = self.detail_realtime_preview_enabled;
        config.daemon.summary_enabled = self.summary_enabled;
        config.daemon.summary_provider = self.summary_provider.clone();
        config.daemon.summary_model = self.summary_model.clone();
        config.daemon.summary_content_mode = self.summary_content_mode.clone();
        config.daemon.summary_disk_cache_enabled = self.summary_disk_cache_enabled;
        config.daemon.summary_openai_compat_endpoint = self.summary_openai_compat_endpoint.clone();
        config.daemon.summary_openai_compat_base = self.summary_openai_compat_base.clone();
        config.daemon.summary_openai_compat_path = self.summary_openai_compat_path.clone();
        config.daemon.summary_openai_compat_style = self.summary_openai_compat_style.clone();
        config.daemon.summary_openai_compat_key = self.summary_openai_compat_key.clone();
        config.daemon.summary_openai_compat_key_header =
            self.summary_openai_compat_key_header.clone();
        config.daemon.summary_event_window = self.summary_event_window;
        config.daemon.summary_debounce_ms = self.summary_debounce_ms;
        config.daemon.summary_max_inflight = self.summary_max_inflight.max(1);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TimelinePresetSlot {
    pub slot: u8,
    pub preset: TimelineIntelPreset,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct TimelinePresetFile {
    #[serde(default)]
    pub slots: Vec<TimelinePresetSlot>,
}

fn timeline_preset_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("timeline_intel_presets.toml"))
}

fn validate_timeline_preset_slot(slot: u8) -> Result<()> {
    if (TIMELINE_PRESET_SLOT_MIN..=TIMELINE_PRESET_SLOT_MAX).contains(&slot) {
        Ok(())
    } else {
        Err(anyhow!(
            "Invalid timeline preset slot {} (expected {}..={})",
            slot,
            TIMELINE_PRESET_SLOT_MIN,
            TIMELINE_PRESET_SLOT_MAX
        ))
    }
}

fn load_timeline_preset_file() -> Result<TimelinePresetFile> {
    let path = timeline_preset_path()?;
    if !path.exists() {
        return Ok(TimelinePresetFile::default());
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    if content.trim().is_empty() {
        return Ok(TimelinePresetFile::default());
    }
    let mut file: TimelinePresetFile =
        toml::from_str(&content).with_context(|| format!("Failed to parse {}", path.display()))?;
    file.slots.retain(|entry| {
        (TIMELINE_PRESET_SLOT_MIN..=TIMELINE_PRESET_SLOT_MAX).contains(&entry.slot)
    });
    file.slots.sort_by_key(|entry| entry.slot);
    file.slots.dedup_by_key(|entry| entry.slot);
    Ok(file)
}

fn save_timeline_preset_file(file: &TimelinePresetFile) -> Result<()> {
    let dir = config_dir()?;
    std::fs::create_dir_all(&dir)?;
    let path = timeline_preset_path()?;
    let content = toml::to_string_pretty(file).context("Failed to serialize timeline presets")?;
    std::fs::write(&path, content)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

pub fn list_timeline_preset_slots() -> Result<Vec<u8>> {
    let file = load_timeline_preset_file()?;
    Ok(file.slots.iter().map(|entry| entry.slot).collect())
}

pub fn load_timeline_preset(slot: u8) -> Result<Option<TimelineIntelPreset>> {
    validate_timeline_preset_slot(slot)?;
    let file = load_timeline_preset_file()?;
    Ok(file
        .slots
        .into_iter()
        .find(|entry| entry.slot == slot)
        .map(|entry| entry.preset))
}

pub fn save_timeline_preset(slot: u8, config: &DaemonConfig) -> Result<()> {
    validate_timeline_preset_slot(slot)?;
    let mut file = load_timeline_preset_file()?;
    let preset = TimelineIntelPreset::from_config(config);
    if let Some(entry) = file.slots.iter_mut().find(|entry| entry.slot == slot) {
        entry.preset = preset;
    } else {
        file.slots.push(TimelinePresetSlot { slot, preset });
        file.slots.sort_by_key(|entry| entry.slot);
    }
    save_timeline_preset_file(&file)
}

/// Get daemon PID from PID file, if it exists.
pub fn daemon_pid() -> Option<u32> {
    let pid_path = config_dir().ok()?.join("daemon.pid");
    let content = std::fs::read_to_string(pid_path).ok()?;
    let pid: u32 = content.trim().parse().ok()?;
    if process_alive(pid) {
        Some(pid)
    } else {
        let stale_path = config_dir().ok()?.join("daemon.pid");
        let _ = std::fs::remove_file(stale_path);
        None
    }
}

fn process_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
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
    DebounceSecs,
    RealtimeDebounceMs,
    DetailRealtimePreviewEnabled,
    CalendarDisplayMode,
    HealthCheckSecs,
    MaxRetries,
    SummaryEnabled,
    SummaryProvider,
    SummaryCliAgent,
    SummaryModel,
    SummaryContentMode,
    SummaryDiskCacheEnabled,
    SummaryOpenAiCompatEndpoint,
    SummaryOpenAiCompatBase,
    SummaryOpenAiCompatPath,
    SummaryOpenAiCompatStyle,
    SummaryOpenAiCompatApiKey,
    SummaryOpenAiCompatApiKeyHeader,
    SummaryEventWindow,
    SummaryDebounceMs,
    SummaryMaxInflight,
    WatchPaths,
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
    SettingItem::Field {
        field: SettingField::CalendarDisplayMode,
        label: "Calendar Mode",
        description: "Date format in list: smart / relative / absolute",
        dependency_hint: None,
    },
    SettingItem::Header("Daemon Publish"),
    SettingItem::Field {
        field: SettingField::AutoPublish,
        label: "Daemon Capture",
        description: "Toggle daemon capture. ON => forced publish on session end",
        dependency_hint: None,
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
        description:
            "Global polling interval for daemon realtime publish and detail auto-refresh checks",
        dependency_hint: Some("Used by daemon realtime publish and detail auto-refresh"),
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
    SettingItem::Header("Timeline Detail"),
    SettingItem::Field {
        field: SettingField::DetailRealtimePreviewEnabled,
        label: "Detail Auto-Refresh",
        description: "Auto-reload Session Detail when selected source file mtime changes",
        dependency_hint: Some("Global toggle for Session Detail live refresh"),
    },
    SettingItem::Header("LLM Summary (Common)"),
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
        field: SettingField::SummaryModel,
        label: "LLM Summary Model",
        description: "Optional model override for API calls and CLI --model",
        dependency_hint: Some("Leave empty to use provider default model"),
    },
    SettingItem::Field {
        field: SettingField::SummaryDiskCacheEnabled,
        label: "LLM Summary Disk Cache",
        description: "Persist summary results by context hash and reuse across runs",
        dependency_hint: Some("Reduces repeated summary calls for unchanged windows"),
    },
    SettingItem::Field {
        field: SettingField::SummaryContentMode,
        label: "LLM Summary Detail Mode",
        description:
            "normal: richer action detail · minimal: merge low-signal read/open/list actions",
        dependency_hint: Some("Active only when LLM Summary Enabled=ON"),
    },
    SettingItem::Field {
        field: SettingField::SummaryEventWindow,
        label: "LLM Summary Window",
        description: "Checkpoint size in events (set 0 or 'auto' for turn+phase auto segmentation)",
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
    SettingItem::Header("LLM Summary (CLI)"),
    SettingItem::Field {
        field: SettingField::SummaryCliAgent,
        label: "LLM Summary CLI Agent",
        description: "Which CLI to call for summary (auto/codex/claude/cursor/gemini)",
        dependency_hint: Some("Active only when LLM Summary Enabled=ON and LLM Summary Mode=CLI"),
    },
    SettingItem::Header("LLM Summary (API)"),
    SettingItem::Field {
        field: SettingField::SummaryOpenAiCompatEndpoint,
        label: "Summary API Endpoint",
        description: "Full endpoint URL for OpenAI-compatible API",
        dependency_hint: Some("If set, this is used directly"),
    },
    SettingItem::Field {
        field: SettingField::SummaryOpenAiCompatBase,
        label: "Summary API Base URL",
        description: "Base URL used when endpoint is not set",
        dependency_hint: Some("Default: https://api.openai.com/v1"),
    },
    SettingItem::Field {
        field: SettingField::SummaryOpenAiCompatPath,
        label: "Summary API Path",
        description: "Path appended to base URL when endpoint is not set",
        dependency_hint: Some("Default: /chat/completions"),
    },
    SettingItem::Field {
        field: SettingField::SummaryOpenAiCompatStyle,
        label: "Summary API Style",
        description: "Payload style for OpenAI-compatible endpoint: chat/responses",
        dependency_hint: Some("Auto-inferrs from endpoint when empty"),
    },
    SettingItem::Field {
        field: SettingField::SummaryOpenAiCompatApiKey,
        label: "Summary API Key",
        description: "Optional API key for OpenAI-compatible endpoint",
        dependency_hint: Some("Fallback: OPS_TL_SUM_KEY or OPENAI_API_KEY"),
    },
    SettingItem::Field {
        field: SettingField::SummaryOpenAiCompatApiKeyHeader,
        label: "Summary API Key Header",
        description: "Header name for Summary API Key",
        dependency_hint: Some("Default: Authorization: Bearer"),
    },
    SettingItem::Header("Watchers"),
    SettingItem::Field {
        field: SettingField::WatchPaths,
        label: "Parse Paths",
        description: "Folders watched for all supported agents (comma-separated)",
        dependency_hint: Some("Applies when daemon is running"),
    },
    SettingItem::Header("Git Storage"),
    SettingItem::Field {
        field: SettingField::GitStorageMethod,
        label: "Method",
        description: "native: git objects (default) · platform_api: provider API · none: disabled",
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
                | Self::SummaryDiskCacheEnabled
                | Self::StripPaths
                | Self::StripEnvVars
        )
    }

    /// Whether this field cycles through enum options.
    pub fn is_enum(self) -> bool {
        matches!(
            self,
            Self::CalendarDisplayMode
                | Self::GitStorageMethod
                | Self::SummaryProvider
                | Self::SummaryCliAgent
                | Self::SummaryContentMode
                | Self::SummaryOpenAiCompatStyle
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
            Self::DebounceSecs => config.daemon.debounce_secs.to_string(),
            Self::RealtimeDebounceMs => config.daemon.realtime_debounce_ms.to_string(),
            Self::DetailRealtimePreviewEnabled => {
                on_off(config.daemon.detail_realtime_preview_enabled)
            }
            Self::CalendarDisplayMode => match calendar_display_mode() {
                CalendarDisplayMode::Smart => "smart".to_string(),
                CalendarDisplayMode::Relative => "relative".to_string(),
                CalendarDisplayMode::Absolute => "absolute".to_string(),
            },
            Self::HealthCheckSecs => config.daemon.health_check_interval_secs.to_string(),
            Self::MaxRetries => config.daemon.max_retries.to_string(),
            Self::SummaryEnabled => on_off(config.daemon.summary_enabled),
            Self::SummaryProvider => summary_mode_label(config.daemon.summary_provider.as_deref()),
            Self::SummaryCliAgent => {
                summary_cli_agent_label(config.daemon.summary_provider.as_deref())
            }
            Self::SummaryModel => config
                .daemon
                .summary_model
                .as_deref()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or("(default)")
                .to_string(),
            Self::SummaryContentMode => {
                summary_content_mode_label(&config.daemon.summary_content_mode)
            }
            Self::SummaryDiskCacheEnabled => on_off(config.daemon.summary_disk_cache_enabled),
            Self::SummaryOpenAiCompatEndpoint => config
                .daemon
                .summary_openai_compat_endpoint
                .as_deref()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or("(default)")
                .to_string(),
            Self::SummaryOpenAiCompatBase => config
                .daemon
                .summary_openai_compat_base
                .as_deref()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or("(default)")
                .to_string(),
            Self::SummaryOpenAiCompatPath => config
                .daemon
                .summary_openai_compat_path
                .as_deref()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or("/chat/completions")
                .to_string(),
            Self::SummaryOpenAiCompatStyle => summary_openai_compat_style_label(
                config.daemon.summary_openai_compat_style.as_deref(),
            ),
            Self::SummaryOpenAiCompatApiKey => {
                if let Some(key) = config
                    .daemon
                    .summary_openai_compat_key
                    .as_deref()
                    .filter(|v| !v.trim().is_empty())
                {
                    let visible = key.len().min(6);
                    format!("{}...", &key[..visible])
                } else {
                    "(not set)".to_string()
                }
            }
            Self::SummaryOpenAiCompatApiKeyHeader => config
                .daemon
                .summary_openai_compat_key_header
                .as_deref()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or("Authorization")
                .to_string(),
            Self::SummaryEventWindow => {
                if config.daemon.summary_event_window == 0 {
                    "AUTO(turn+phases)".to_string()
                } else {
                    config.daemon.summary_event_window.to_string()
                }
            }
            Self::SummaryDebounceMs => config.daemon.summary_debounce_ms.to_string(),
            Self::SummaryMaxInflight => config.daemon.summary_max_inflight.to_string(),
            Self::WatchPaths => format!("{} paths", config.watchers.custom_paths.len()),
            Self::GitStorageMethod => match git_storage_mode() {
                GitStorageMode::None => "None".to_string(),
                GitStorageMode::PlatformApi => "Platform API".to_string(),
                GitStorageMode::Native => "Native".to_string(),
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
            Self::SummaryModel => config.daemon.summary_model.clone().unwrap_or_default(),
            Self::SummaryContentMode => config.daemon.summary_content_mode.clone(),
            Self::SummaryOpenAiCompatEndpoint => config
                .daemon
                .summary_openai_compat_endpoint
                .clone()
                .unwrap_or_default(),
            Self::SummaryOpenAiCompatBase => config
                .daemon
                .summary_openai_compat_base
                .clone()
                .unwrap_or_default(),
            Self::SummaryOpenAiCompatPath => config
                .daemon
                .summary_openai_compat_path
                .clone()
                .unwrap_or_default(),
            Self::SummaryOpenAiCompatStyle => config
                .daemon
                .summary_openai_compat_style
                .clone()
                .unwrap_or_default(),
            Self::SummaryOpenAiCompatApiKey => config
                .daemon
                .summary_openai_compat_key
                .clone()
                .unwrap_or_default(),
            Self::SummaryOpenAiCompatApiKeyHeader => config
                .daemon
                .summary_openai_compat_key_header
                .clone()
                .unwrap_or_default(),
            Self::SummaryEventWindow => {
                if config.daemon.summary_event_window == 0 {
                    "auto".to_string()
                } else {
                    config.daemon.summary_event_window.to_string()
                }
            }
            Self::SummaryDebounceMs => config.daemon.summary_debounce_ms.to_string(),
            Self::SummaryMaxInflight => config.daemon.summary_max_inflight.to_string(),
            Self::WatchPaths => config.watchers.custom_paths.join(", "),
            Self::GitStorageToken => config.git_storage.token.clone(),
            _ => String::new(),
        }
    }

    /// Toggle a boolean field in the config.
    pub fn toggle(self, config: &mut DaemonConfig) {
        match self {
            // DaemonCapture (AutoPublish field) is handled by App::toggle_daemon
            // so publish policy and daemon process state stay in sync.
            Self::AutoPublish => {}
            Self::DetailRealtimePreviewEnabled => {
                config.daemon.detail_realtime_preview_enabled =
                    !config.daemon.detail_realtime_preview_enabled;
            }
            Self::SummaryEnabled => config.daemon.summary_enabled = !config.daemon.summary_enabled,
            Self::SummaryDiskCacheEnabled => {
                config.daemon.summary_disk_cache_enabled = !config.daemon.summary_disk_cache_enabled
            }
            Self::StripPaths => config.privacy.strip_paths = !config.privacy.strip_paths,
            Self::StripEnvVars => config.privacy.strip_env_vars = !config.privacy.strip_env_vars,
            _ => {}
        }
    }

    /// Cycle an enum field.
    pub fn cycle_enum(self, config: &mut DaemonConfig) {
        match self {
            Self::CalendarDisplayMode => {
                let next = match calendar_display_mode() {
                    CalendarDisplayMode::Smart => CalendarDisplayMode::Relative,
                    CalendarDisplayMode::Relative => CalendarDisplayMode::Absolute,
                    CalendarDisplayMode::Absolute => CalendarDisplayMode::Smart,
                };
                set_calendar_display_mode(next);
            }
            Self::GitStorageMethod => {
                let next = match git_storage_mode() {
                    GitStorageMode::None => GitStorageMode::PlatformApi,
                    GitStorageMode::PlatformApi => GitStorageMode::Native,
                    GitStorageMode::Native => GitStorageMode::None,
                };
                set_git_storage_mode(config, next);
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
            Self::SummaryContentMode => {
                config.daemon.summary_content_mode =
                    if summary_content_mode_key(&config.daemon.summary_content_mode) == "minimal" {
                        "normal".to_string()
                    } else {
                        "minimal".to_string()
                    };
            }
            Self::SummaryOpenAiCompatStyle => {
                let next = match summary_openai_compat_style_key(
                    config.daemon.summary_openai_compat_style.as_deref(),
                ) {
                    "chat" => "responses",
                    _ => "chat",
                };
                config.daemon.summary_openai_compat_style = Some(next.to_string());
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
            Self::SummaryModel => {
                config.daemon.summary_model = normalize_optional_string(value);
            }
            Self::SummaryContentMode => {
                config.daemon.summary_content_mode =
                    match value.trim().to_ascii_lowercase().as_str() {
                        "minimal" | "min" => "minimal".to_string(),
                        _ => "normal".to_string(),
                    };
            }
            Self::SummaryOpenAiCompatEndpoint => {
                config.daemon.summary_openai_compat_endpoint = normalize_optional_string(value);
            }
            Self::SummaryOpenAiCompatBase => {
                config.daemon.summary_openai_compat_base = normalize_optional_string(value);
            }
            Self::SummaryOpenAiCompatPath => {
                config.daemon.summary_openai_compat_path = normalize_optional_string(value);
            }
            Self::SummaryOpenAiCompatStyle => {
                let normalized = value.trim().to_ascii_lowercase();
                let mapped = match normalized.as_str() {
                    "" => None,
                    "chat" => Some("chat".to_string()),
                    "responses" => Some("responses".to_string()),
                    _ => Some("chat".to_string()),
                };
                config.daemon.summary_openai_compat_style = mapped;
            }
            Self::SummaryOpenAiCompatApiKey => {
                config.daemon.summary_openai_compat_key = normalize_optional_string(value);
            }
            Self::SummaryOpenAiCompatApiKeyHeader => {
                config.daemon.summary_openai_compat_key_header = normalize_optional_string(value);
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
            Self::WatchPaths => {
                let parsed = parse_watch_paths(value);
                config.watchers.custom_paths = if parsed.is_empty() {
                    DaemonConfig::default().watchers.custom_paths
                } else {
                    parsed
                };
            }
            Self::GitStorageToken => {
                config.git_storage.token = value.to_string();
            }
            Self::SummaryEnabled => {
                let lowered = value.to_lowercase();
                config.daemon.summary_enabled =
                    matches!(lowered.as_str(), "on" | "1" | "true" | "yes");
            }
            Self::SummaryDiskCacheEnabled => {
                let lowered = value.to_lowercase();
                config.daemon.summary_disk_cache_enabled =
                    matches!(lowered.as_str(), "on" | "1" | "true" | "yes");
            }
            Self::DetailRealtimePreviewEnabled => {
                let lowered = value.to_lowercase();
                config.daemon.detail_realtime_preview_enabled =
                    matches!(lowered.as_str(), "on" | "1" | "true" | "yes");
            }
            Self::CalendarDisplayMode => {
                let mode = match value.trim().to_ascii_lowercase().as_str() {
                    "relative" | "rel" => CalendarDisplayMode::Relative,
                    "absolute" | "abs" => CalendarDisplayMode::Absolute,
                    _ => CalendarDisplayMode::Smart,
                };
                set_calendar_display_mode(mode);
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
            Self::GitStorageMethod => {
                let mode = match value.trim().to_ascii_lowercase().as_str() {
                    "none" => GitStorageMode::None,
                    "platform_api" | "platform-api" | "api" => GitStorageMode::PlatformApi,
                    "sqlite_local" | "sqlite-local" | "sqlite" => GitStorageMode::Native,
                    _ => GitStorageMode::Native,
                };
                set_git_storage_mode(config, mode);
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

fn summary_content_mode_key(mode: &str) -> &'static str {
    match mode.trim().to_ascii_lowercase().as_str() {
        "minimal" | "min" => "minimal",
        _ => "normal",
    }
}

fn summary_content_mode_label(mode: &str) -> String {
    match summary_content_mode_key(mode) {
        "minimal" => "Minimal".to_string(),
        _ => "Normal".to_string(),
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

fn summary_openai_compat_style_key(style: Option<&str>) -> &'static str {
    match style.unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "responses" => "responses",
        _ => "chat",
    }
}

fn summary_openai_compat_style_label(style: Option<&str>) -> String {
    match summary_openai_compat_style_key(style) {
        "responses" => "responses".to_string(),
        _ => "chat".to_string(),
    }
}

fn normalize_optional_string(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn parse_watch_paths(value: &str) -> Vec<String> {
    let normalized = value.replace('\n', ",");
    let mut out = Vec::new();
    for part in normalized.split(',') {
        let path = part.trim();
        if path.is_empty() {
            continue;
        }
        let path = path.to_string();
        if !out.contains(&path) {
            out.push(path);
        }
    }
    out
}

fn group_for_field(field: SettingField) -> SettingsGroup {
    match field {
        SettingField::ServerUrl
        | SettingField::ApiKey
        | SettingField::TeamId
        | SettingField::Nickname
        | SettingField::CalendarDisplayMode => SettingsGroup::Workspace,

        SettingField::AutoPublish
        | SettingField::DebounceSecs
        | SettingField::RealtimeDebounceMs
        | SettingField::HealthCheckSecs
        | SettingField::MaxRetries
        | SettingField::WatchPaths => SettingsGroup::CaptureSync,

        SettingField::DetailRealtimePreviewEnabled
        | SettingField::SummaryEnabled
        | SettingField::SummaryProvider
        | SettingField::SummaryCliAgent
        | SettingField::SummaryModel
        | SettingField::SummaryContentMode
        | SettingField::SummaryDiskCacheEnabled
        | SettingField::SummaryOpenAiCompatEndpoint
        | SettingField::SummaryOpenAiCompatBase
        | SettingField::SummaryOpenAiCompatPath
        | SettingField::SummaryOpenAiCompatStyle
        | SettingField::SummaryOpenAiCompatApiKey
        | SettingField::SummaryOpenAiCompatApiKeyHeader
        | SettingField::SummaryEventWindow
        | SettingField::SummaryDebounceMs
        | SettingField::SummaryMaxInflight => SettingsGroup::TimelineIntelligence,

        SettingField::GitStorageMethod
        | SettingField::GitStorageToken
        | SettingField::StripPaths
        | SettingField::StripEnvVars => SettingsGroup::StoragePrivacy,
    }
}

pub fn section_items(section: SettingsGroup) -> Vec<&'static SettingItem> {
    let mut items: Vec<&'static SettingItem> = Vec::new();
    let mut pending_header: Option<&'static SettingItem> = None;

    for item in SETTINGS_LAYOUT {
        match item {
            SettingItem::Header(_) => {
                pending_header = Some(item);
            }
            SettingItem::Field { field, .. } => {
                if group_for_field(*field) != section {
                    continue;
                }
                if let Some(header) = pending_header.take() {
                    items.push(header);
                }
                items.push(item);
            }
        }
    }

    items
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrate_summary_window_v2_converts_legacy_default_once() {
        let mut cfg = DaemonConfig::default();
        cfg.daemon.summary_event_window = 8;
        cfg.daemon.summary_window_migrated_v2 = false;

        assert!(migrate_summary_window_v2(&mut cfg));
        assert_eq!(cfg.daemon.summary_event_window, 0);
        assert!(cfg.daemon.summary_window_migrated_v2);

        assert!(!migrate_summary_window_v2(&mut cfg));
        assert_eq!(cfg.daemon.summary_event_window, 0);
        assert!(cfg.daemon.summary_window_migrated_v2);
    }

    #[test]
    fn migrate_summary_window_v2_marks_non_legacy_without_overwrite() {
        let mut cfg = DaemonConfig::default();
        cfg.daemon.summary_event_window = 5;
        cfg.daemon.summary_window_migrated_v2 = false;

        assert!(migrate_summary_window_v2(&mut cfg));
        assert_eq!(cfg.daemon.summary_event_window, 5);
        assert!(cfg.daemon.summary_window_migrated_v2);
    }
}
