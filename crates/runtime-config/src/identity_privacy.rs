use crate::defaults::{default_exclude_patterns, default_nickname, default_true};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentitySettings {
    #[serde(default = "default_nickname")]
    pub nickname: String,
}

impl Default for IdentitySettings {
    fn default() -> Self {
        Self {
            nickname: default_nickname(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacySettings {
    #[serde(default = "default_true")]
    pub strip_paths: bool,
    #[serde(default = "default_true")]
    pub strip_env_vars: bool,
    #[serde(default = "default_exclude_patterns")]
    pub exclude_patterns: Vec<String>,
    #[serde(default)]
    pub exclude_tools: Vec<String>,
}

impl Default for PrivacySettings {
    fn default() -> Self {
        Self {
            strip_paths: true,
            strip_env_vars: true,
            exclude_patterns: default_exclude_patterns(),
            exclude_tools: Vec::new(),
        }
    }
}
