use crate::defaults::{
    default_lifecycle_cleanup_interval_secs, default_lifecycle_session_ttl_days,
    default_lifecycle_summary_ttl_days, default_true,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_lifecycle_session_ttl_days")]
    pub session_ttl_days: u32,
    #[serde(default = "default_lifecycle_summary_ttl_days")]
    pub summary_ttl_days: u32,
    #[serde(default = "default_lifecycle_cleanup_interval_secs")]
    pub cleanup_interval_secs: u64,
}

impl Default for LifecycleSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            session_ttl_days: default_lifecycle_session_ttl_days(),
            summary_ttl_days: default_lifecycle_summary_ttl_days(),
            cleanup_interval_secs: default_lifecycle_cleanup_interval_secs(),
        }
    }
}
