use crate::defaults::{
    default_change_reader_max_context_chars, default_change_reader_voice_model,
    default_change_reader_voice_name, default_false, default_true,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeReaderSettings {
    #[serde(default = "default_false")]
    pub enabled: bool,
    #[serde(default)]
    pub scope: ChangeReaderScope,
    #[serde(default = "default_true")]
    pub qa_enabled: bool,
    #[serde(default = "default_change_reader_max_context_chars")]
    pub max_context_chars: u32,
    #[serde(default)]
    pub voice: ChangeReaderVoiceSettings,
}

impl Default for ChangeReaderSettings {
    fn default() -> Self {
        Self {
            enabled: default_false(),
            scope: ChangeReaderScope::default(),
            qa_enabled: default_true(),
            max_context_chars: default_change_reader_max_context_chars(),
            voice: ChangeReaderVoiceSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeReaderVoiceSettings {
    #[serde(default = "default_false")]
    pub enabled: bool,
    #[serde(default)]
    pub provider: ChangeReaderVoiceProvider,
    #[serde(default = "default_change_reader_voice_model")]
    pub model: String,
    #[serde(default = "default_change_reader_voice_name")]
    pub voice: String,
    #[serde(default)]
    pub api_key: String,
}

impl Default for ChangeReaderVoiceSettings {
    fn default() -> Self {
        Self {
            enabled: default_false(),
            provider: ChangeReaderVoiceProvider::default(),
            model: default_change_reader_voice_model(),
            voice: default_change_reader_voice_name(),
            api_key: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChangeReaderScope {
    #[default]
    SummaryOnly,
    FullContext,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChangeReaderVoiceProvider {
    #[default]
    Openai,
}
