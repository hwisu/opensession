use crate::defaults::{default_summary_batch_recent_days, default_summary_endpoint};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SummarySettings {
    #[serde(default)]
    pub provider: SummaryProviderSettings,
    #[serde(default)]
    pub prompt: SummaryPromptSettings,
    #[serde(default)]
    pub response: SummaryResponseSettings,
    #[serde(default)]
    pub storage: SummaryStorageSettings,
    #[serde(default)]
    pub source_mode: SummarySourceMode,
    #[serde(default)]
    pub batch: SummaryBatchSettings,
}

impl SummarySettings {
    pub fn is_configured(&self) -> bool {
        match self.provider.id {
            SummaryProvider::Disabled => false,
            SummaryProvider::Ollama => !self.provider.model.trim().is_empty(),
            SummaryProvider::CodexExec | SummaryProvider::ClaudeCli => true,
        }
    }

    pub fn provider_transport(&self) -> SummaryProviderTransport {
        self.provider.id.transport()
    }

    pub fn allows_git_changes_fallback(&self) -> bool {
        matches!(self.source_mode, SummarySourceMode::SessionOrGitChanges)
    }

    pub fn should_generate_on_session_save(&self) -> bool {
        matches!(self.storage.trigger, SummaryTriggerMode::OnSessionSave)
    }

    pub fn persists_to_local_db(&self) -> bool {
        matches!(self.storage.backend, SummaryStorageBackend::LocalDb)
    }

    pub fn persists_to_hidden_ref(&self) -> bool {
        matches!(self.storage.backend, SummaryStorageBackend::HiddenRef)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryProviderSettings {
    #[serde(default)]
    pub id: SummaryProvider,
    #[serde(default = "default_summary_endpoint")]
    pub endpoint: String,
    #[serde(default)]
    pub model: String,
}

impl Default for SummaryProviderSettings {
    fn default() -> Self {
        Self {
            id: SummaryProvider::default(),
            endpoint: default_summary_endpoint(),
            model: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SummaryPromptSettings {
    #[serde(default)]
    pub template: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SummaryResponseSettings {
    #[serde(default)]
    pub style: SummaryResponseStyle,
    #[serde(default)]
    pub shape: SummaryOutputShape,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SummaryStorageSettings {
    #[serde(default)]
    pub trigger: SummaryTriggerMode,
    #[serde(default)]
    pub backend: SummaryStorageBackend,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryBatchSettings {
    #[serde(default)]
    pub execution_mode: SummaryBatchExecutionMode,
    #[serde(default)]
    pub scope: SummaryBatchScope,
    #[serde(default = "default_summary_batch_recent_days")]
    pub recent_days: u16,
}

impl Default for SummaryBatchSettings {
    fn default() -> Self {
        Self {
            execution_mode: SummaryBatchExecutionMode::default(),
            scope: SummaryBatchScope::default(),
            recent_days: default_summary_batch_recent_days(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SummaryProvider {
    #[default]
    Disabled,
    Ollama,
    CodexExec,
    ClaudeCli,
}

impl SummaryProvider {
    pub fn transport(&self) -> SummaryProviderTransport {
        match self {
            Self::Disabled => SummaryProviderTransport::None,
            Self::Ollama => SummaryProviderTransport::Http,
            Self::CodexExec | Self::ClaudeCli => SummaryProviderTransport::Cli,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SummaryProviderTransport {
    #[default]
    None,
    Cli,
    Http,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SummaryResponseStyle {
    Compact,
    #[default]
    Standard,
    Detailed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SummarySourceMode {
    #[default]
    SessionOnly,
    SessionOrGitChanges,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SummaryOutputShape {
    #[default]
    Layered,
    FileList,
    SecurityFirst,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SummaryTriggerMode {
    Manual,
    #[default]
    OnSessionSave,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SummaryStorageBackend {
    None,
    #[default]
    HiddenRef,
    LocalDb,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SummaryBatchExecutionMode {
    Manual,
    #[default]
    OnAppStart,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SummaryBatchScope {
    #[default]
    RecentDays,
    All,
}
