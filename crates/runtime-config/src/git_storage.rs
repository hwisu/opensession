use crate::defaults::{
    default_false, default_git_retention_interval_secs, default_git_retention_keep_days,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitStorageSettings {
    #[serde(default)]
    pub method: GitStorageMethod,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub retention: GitRetentionSettings,
}

impl Default for GitStorageSettings {
    fn default() -> Self {
        Self {
            method: GitStorageMethod::Native,
            token: String::new(),
            retention: GitRetentionSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitRetentionSettings {
    #[serde(default = "default_false")]
    pub enabled: bool,
    #[serde(default = "default_git_retention_keep_days")]
    pub keep_days: u32,
    #[serde(default = "default_git_retention_interval_secs")]
    pub interval_secs: u64,
}

impl Default for GitRetentionSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            keep_days: default_git_retention_keep_days(),
            interval_secs: default_git_retention_interval_secs(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum GitStorageMethod {
    #[default]
    Native,
    Sqlite,
}
