#![allow(dead_code)]

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

// Re-export shared runtime config types
pub use opensession_runtime_config::{
    apply_compat_fallbacks, CalendarDisplayMode, DaemonConfig, GitStorageMethod, PublishMode,
    CONFIG_FILE_NAME,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GitStorageMode {
    Native,
    Sqlite,
}

impl GitStorageMode {
    fn from_core(method: GitStorageMethod) -> Self {
        match method {
            GitStorageMethod::Native => Self::Native,
            GitStorageMethod::Sqlite => Self::Sqlite,
            GitStorageMethod::Unknown => Self::Native,
        }
    }

    fn to_core(self) -> GitStorageMethod {
        match self {
            Self::Native => GitStorageMethod::Native,
            Self::Sqlite => GitStorageMethod::Sqlite,
        }
    }

    fn as_toml_method(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Sqlite => "sqlite",
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
        Some(v) if v == "native" || v == "platform_api" || v == "platform-api" || v == "api" => {
            GitStorageMode::Native
        }
        Some(v) if v == "sqlite" || v == "sqlite_local" || v == "sqlite-local" || v == "none" => {
            GitStorageMode::Sqlite
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
        if migrated {
            let _ = save_daemon_config(&config);
        }
        return config;
    }

    let mut config = DaemonConfig::default();
    sync_runtime_config_extensions(None, &mut config);
    config
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
    apply_runtime_extensions_to_toml(&mut doc);
    let content = toml::to_string_pretty(&doc).context("Failed to serialize config")?;
    std::fs::write(&path, content)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

pub const TIMELINE_PRESET_SLOT_MIN: u8 = 1;
pub const TIMELINE_PRESET_SLOT_MAX: u8 = 5;

fn default_timeline_detail_auto_expand_selected_event() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineIntelPreset {
    pub detail_realtime_preview_enabled: bool,
    #[serde(default = "default_timeline_detail_auto_expand_selected_event")]
    pub detail_auto_expand_selected_event: bool,
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
            detail_auto_expand_selected_event: config.daemon.detail_auto_expand_selected_event,
        }
    }

    pub fn apply_to_config(&self, config: &mut DaemonConfig) {
        config.daemon.detail_realtime_preview_enabled = self.detail_realtime_preview_enabled;
        config.daemon.detail_auto_expand_selected_event = self.detail_auto_expand_selected_event;
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
    Nickname,
    AutoPublish,
    DebounceSecs,
    RealtimeDebounceMs,
    DetailRealtimePreviewEnabled,
    DetailAutoExpandSelectedEvent,
    CalendarDisplayMode,
    HealthCheckSecs,
    MaxRetries,
    WatchPaths,
    GitStorageMethod,
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
    StoragePrivacy,
}

/// The ordered list of items shown in the settings view.
pub const SETTINGS_LAYOUT: &[SettingItem] = &[
    SettingItem::Header("Web Share (Public Git)"),
    SettingItem::Field {
        field: SettingField::ServerUrl,
        label: "Web Endpoint URL",
        description: "Public web endpoint used for git-backed log sharing",
        dependency_hint: None,
    },
    SettingItem::Field {
        field: SettingField::ApiKey,
        label: "Web API Key",
        description: "Personal auth key used for public web share registration and uploads",
        dependency_hint: None,
    },
    SettingItem::Header("Profile"),
    SettingItem::Field {
        field: SettingField::Nickname,
        label: "Handle",
        description: "Display handle shown on your shared sessions",
        dependency_hint: None,
    },
    SettingItem::Field {
        field: SettingField::CalendarDisplayMode,
        label: "Calendar Mode",
        description: "Date format in lists: smart / relative / absolute",
        dependency_hint: None,
    },
    SettingItem::Header("Capture Runtime"),
    SettingItem::Field {
        field: SettingField::AutoPublish,
        label: "Capture + Auto Sync",
        description: "ON: capture continuously and auto-sync at session end. OFF: manual sync only",
        dependency_hint: None,
    },
    SettingItem::Field {
        field: SettingField::DebounceSecs,
        label: "Sync Debounce (secs)",
        description: "Wait time after last event before sync upload starts",
        dependency_hint: Some("Applies when daemon is running"),
    },
    SettingItem::Field {
        field: SettingField::RealtimeDebounceMs,
        label: "Realtime Sync Poll (ms)",
        description:
            "Polling interval for daemon realtime sync and Session Detail auto-refresh checks",
        dependency_hint: Some("Shared by realtime sync and detail auto-refresh loops"),
    },
    SettingItem::Field {
        field: SettingField::HealthCheckSecs,
        label: "Sync Health Check (secs)",
        description: "How often capture runtime checks endpoint connectivity before sync",
        dependency_hint: Some("Applies when daemon is running"),
    },
    SettingItem::Field {
        field: SettingField::MaxRetries,
        label: "Sync Retry Limit",
        description: "Maximum retry attempts for failed sync uploads",
        dependency_hint: Some("Applies when daemon is running"),
    },
    SettingItem::Header("Capture Scope"),
    SettingItem::Field {
        field: SettingField::WatchPaths,
        label: "Parse Paths",
        description: "Folders watched for local agent logs (comma-separated)",
        dependency_hint: Some("Applies when daemon is running"),
    },
    SettingItem::Header("Detail View Sync"),
    SettingItem::Field {
        field: SettingField::DetailRealtimePreviewEnabled,
        label: "Detail Auto-Refresh",
        description: "Auto-reload Session Detail when selected source file changes",
        dependency_hint: Some("Global toggle for Session Detail live refresh"),
    },
    SettingItem::Field {
        field: SettingField::DetailAutoExpandSelectedEvent,
        label: "Detail Auto-Expand",
        description: "Auto-expand content preview for the currently selected timeline event",
        dependency_hint: Some("ON by default; turn OFF for compact one-line timeline"),
    },
    SettingItem::Header("Session Storage"),
    SettingItem::Field {
        field: SettingField::GitStorageMethod,
        label: "Storage Method",
        description: "Session capture backend: git-native(canonical) · sqlite(local index/cache)",
        dependency_hint: Some("SQLite mode is a local index/cache for fast query and browsing"),
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
                | Self::DetailAutoExpandSelectedEvent
                | Self::StripPaths
                | Self::StripEnvVars
        )
    }

    /// Whether this field cycles through enum options.
    pub fn is_enum(self) -> bool {
        matches!(self, Self::CalendarDisplayMode | Self::GitStorageMethod)
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
            Self::Nickname => config.identity.nickname.clone(),
            Self::AutoPublish => on_off(config.daemon.auto_publish),
            Self::DebounceSecs => config.daemon.debounce_secs.to_string(),
            Self::RealtimeDebounceMs => config.daemon.realtime_debounce_ms.to_string(),
            Self::DetailRealtimePreviewEnabled => {
                on_off(config.daemon.detail_realtime_preview_enabled)
            }
            Self::DetailAutoExpandSelectedEvent => {
                on_off(config.daemon.detail_auto_expand_selected_event)
            }
            Self::CalendarDisplayMode => match calendar_display_mode() {
                CalendarDisplayMode::Smart => "smart".to_string(),
                CalendarDisplayMode::Relative => "relative".to_string(),
                CalendarDisplayMode::Absolute => "absolute".to_string(),
            },
            Self::HealthCheckSecs => config.daemon.health_check_interval_secs.to_string(),
            Self::MaxRetries => config.daemon.max_retries.to_string(),
            Self::WatchPaths => format!("{} paths", config.watchers.custom_paths.len()),
            Self::GitStorageMethod => match git_storage_mode() {
                GitStorageMode::Native => "Git-Native (Branch Based)".to_string(),
                GitStorageMode::Sqlite => "SQLite".to_string(),
            },
            Self::StripPaths => on_off(config.privacy.strip_paths),
            Self::StripEnvVars => on_off(config.privacy.strip_env_vars),
        }
    }

    /// Get the raw (editable) value from the config.
    pub fn raw_value(self, config: &DaemonConfig) -> String {
        match self {
            Self::ServerUrl => config.server.url.clone(),
            Self::ApiKey => config.server.api_key.clone(),
            Self::Nickname => config.identity.nickname.clone(),
            Self::DebounceSecs => config.daemon.debounce_secs.to_string(),
            Self::RealtimeDebounceMs => config.daemon.realtime_debounce_ms.to_string(),
            Self::HealthCheckSecs => config.daemon.health_check_interval_secs.to_string(),
            Self::MaxRetries => config.daemon.max_retries.to_string(),
            Self::WatchPaths => config.watchers.custom_paths.join(", "),
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
            Self::DetailAutoExpandSelectedEvent => {
                config.daemon.detail_auto_expand_selected_event =
                    !config.daemon.detail_auto_expand_selected_event;
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
                let current = GitStorageMode::from_core(config.git_storage.method.clone());
                let next = match current {
                    GitStorageMode::Native => GitStorageMode::Sqlite,
                    GitStorageMode::Sqlite => GitStorageMode::Native,
                };
                set_git_storage_mode(config, next);
            }
            _ => {}
        }
    }

    /// Set a text/number value.
    pub fn set_value(self, config: &mut DaemonConfig, value: &str) {
        match self {
            Self::ServerUrl => config.server.url = value.to_string(),
            Self::ApiKey => config.server.api_key = value.to_string(),
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
            Self::WatchPaths => {
                let parsed = parse_watch_paths(value);
                config.watchers.custom_paths = if parsed.is_empty() {
                    DaemonConfig::default().watchers.custom_paths
                } else {
                    parsed
                };
            }
            Self::DetailRealtimePreviewEnabled => {
                let lowered = value.to_lowercase();
                config.daemon.detail_realtime_preview_enabled =
                    matches!(lowered.as_str(), "on" | "1" | "true" | "yes");
            }
            Self::DetailAutoExpandSelectedEvent => {
                let lowered = value.to_lowercase();
                config.daemon.detail_auto_expand_selected_event =
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
            Self::GitStorageMethod => {
                let mode = match value.trim().to_ascii_lowercase().as_str() {
                    "sqlite" | "sqlite_local" | "sqlite-local" | "none" => GitStorageMode::Sqlite,
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
        | SettingField::Nickname
        | SettingField::CalendarDisplayMode => SettingsGroup::Workspace,

        SettingField::AutoPublish
        | SettingField::DebounceSecs
        | SettingField::RealtimeDebounceMs
        | SettingField::HealthCheckSecs
        | SettingField::MaxRetries
        | SettingField::WatchPaths
        | SettingField::DetailRealtimePreviewEnabled
        | SettingField::DetailAutoExpandSelectedEvent => SettingsGroup::CaptureSync,

        SettingField::GitStorageMethod | SettingField::StripPaths | SettingField::StripEnvVars => {
            SettingsGroup::StoragePrivacy
        }
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
    fn timeline_preset_roundtrip_keeps_detail_auto_expand_flag() {
        let mut cfg = DaemonConfig::default();
        cfg.daemon.detail_auto_expand_selected_event = false;

        let preset = TimelineIntelPreset::from_config(&cfg);
        assert!(!preset.detail_auto_expand_selected_event);

        let mut out = DaemonConfig::default();
        preset.apply_to_config(&mut out);
        assert!(!out.daemon.detail_auto_expand_selected_event);
    }

    #[test]
    fn detail_auto_expand_setting_is_visible_in_capture_sync_section() {
        let capture_fields = selectable_fields(SettingsGroup::CaptureSync);
        assert!(capture_fields.contains(&SettingField::DetailAutoExpandSelectedEvent));
    }

    #[test]
    fn workspace_section_focuses_on_web_share_fields() {
        let workspace_fields = selectable_fields(SettingsGroup::Workspace);
        assert!(workspace_fields.contains(&SettingField::ServerUrl));
        assert!(workspace_fields.contains(&SettingField::ApiKey));
    }

    #[test]
    fn storage_privacy_section_uses_git_native_and_sqlite_wording() {
        let items = section_items(SettingsGroup::StoragePrivacy);

        assert!(items
            .iter()
            .any(|item| matches!(item, SettingItem::Header("Session Storage"))));

        let method_description = items.iter().find_map(|item| match item {
            SettingItem::Field {
                field: SettingField::GitStorageMethod,
                description,
                ..
            } => Some(*description),
            _ => None,
        });
        let method_description =
            method_description.expect("GitStorageMethod field should exist in StoragePrivacy");

        let lowered = method_description.to_ascii_lowercase();
        assert!(lowered.contains("git-native"));
        assert!(lowered.contains("sqlite"));
        assert!(!lowered.contains("platform_api"));
    }

    #[test]
    fn git_storage_method_cycles_between_two_modes() {
        let mut cfg = DaemonConfig::default();
        set_git_storage_mode(&mut cfg, GitStorageMode::Native);

        SettingField::GitStorageMethod.cycle_enum(&mut cfg);
        assert_eq!(cfg.git_storage.method, GitStorageMethod::Sqlite);

        SettingField::GitStorageMethod.cycle_enum(&mut cfg);
        assert_eq!(cfg.git_storage.method, GitStorageMethod::Native);
    }

    #[test]
    fn git_storage_method_set_value_accepts_compat_aliases() {
        let mut cfg = DaemonConfig::default();

        SettingField::GitStorageMethod.set_value(&mut cfg, "none");
        assert_eq!(cfg.git_storage.method, GitStorageMethod::Sqlite);

        SettingField::GitStorageMethod.set_value(&mut cfg, "platform_api");
        assert_eq!(cfg.git_storage.method, GitStorageMethod::Native);
    }
}
