use crate::defaults::{
    default_debounce, default_detail_auto_expand_selected_event,
    default_detail_realtime_preview_enabled, default_false, default_health_check_interval,
    default_max_retries, default_publish_on, default_realtime_debounce_ms,
    default_session_default_view,
};
use crate::{
    ChangeReaderSettings, GitStorageSettings, IdentitySettings, LifecycleSettings, PrivacySettings,
    ServerSettings, SummarySettings, VectorSearchSettings, WatcherSettings,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DaemonConfig {
    #[serde(default)]
    pub daemon: DaemonSettings,
    #[serde(default)]
    pub server: ServerSettings,
    #[serde(default)]
    pub identity: IdentitySettings,
    #[serde(default)]
    pub privacy: PrivacySettings,
    #[serde(default)]
    pub watchers: WatcherSettings,
    #[serde(default)]
    pub git_storage: GitStorageSettings,
    #[serde(default)]
    pub summary: SummarySettings,
    #[serde(default)]
    pub vector_search: VectorSearchSettings,
    #[serde(default)]
    pub change_reader: ChangeReaderSettings,
    #[serde(default)]
    pub lifecycle: LifecycleSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonSettings {
    #[serde(default = "default_false")]
    pub auto_publish: bool,
    #[serde(default = "default_debounce")]
    pub debounce_secs: u64,
    #[serde(default = "default_publish_on")]
    pub publish_on: PublishMode,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_health_check_interval")]
    pub health_check_interval_secs: u64,
    #[serde(default = "default_realtime_debounce_ms")]
    pub realtime_debounce_ms: u64,
    #[serde(default = "default_detail_realtime_preview_enabled")]
    pub detail_realtime_preview_enabled: bool,
    #[serde(default = "default_detail_auto_expand_selected_event")]
    pub detail_auto_expand_selected_event: bool,
    #[serde(default = "default_session_default_view")]
    pub session_default_view: SessionDefaultView,
}

impl Default for DaemonSettings {
    fn default() -> Self {
        Self {
            auto_publish: false,
            debounce_secs: 5,
            publish_on: PublishMode::Manual,
            max_retries: 3,
            health_check_interval_secs: 300,
            realtime_debounce_ms: 500,
            detail_realtime_preview_enabled: false,
            detail_auto_expand_selected_event: true,
            session_default_view: SessionDefaultView::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PublishMode {
    SessionEnd,
    Realtime,
    Manual,
}

impl PublishMode {
    pub fn cycle(&self) -> Self {
        match self {
            Self::SessionEnd => Self::Realtime,
            Self::Realtime => Self::Manual,
            Self::Manual => Self::SessionEnd,
        }
    }

    pub fn display(&self) -> &'static str {
        match self {
            Self::SessionEnd => "Session End",
            Self::Realtime => "Realtime",
            Self::Manual => "Manual",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CalendarDisplayMode {
    Smart,
    Relative,
    Absolute,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SessionDefaultView {
    #[default]
    Full,
    Compressed,
}
