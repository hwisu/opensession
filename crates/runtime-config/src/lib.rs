//! Shared runtime configuration types.
//!
//! `opensession-daemon`, desktop runtime, and CLI read/write `opensession.toml`
//! using these types. Runtime-specific logic (watch-path resolution, project
//! config merging, UI/IPC adapters) lives in each runtime crate.

mod change_reader;
mod daemon;
mod defaults;
mod git_storage;
mod identity_privacy;
mod lifecycle;
mod server;
mod summary;
mod vector;
mod watcher;

pub use change_reader::{
    ChangeReaderScope, ChangeReaderSettings, ChangeReaderVoiceProvider, ChangeReaderVoiceSettings,
};
pub use daemon::{
    CalendarDisplayMode, DaemonConfig, DaemonSettings, PublishMode, SessionDefaultView,
};
pub use defaults::{CONFIG_FILE_NAME, DEFAULT_WATCH_PATHS, default_watch_paths};
pub use git_storage::{GitRetentionSettings, GitStorageMethod, GitStorageSettings};
pub use identity_privacy::{IdentitySettings, PrivacySettings};
pub use lifecycle::LifecycleSettings;
pub use server::ServerSettings;
pub use summary::{
    SummaryBatchExecutionMode, SummaryBatchScope, SummaryBatchSettings, SummaryOutputShape,
    SummaryPromptSettings, SummaryProvider, SummaryProviderSettings, SummaryProviderTransport,
    SummaryResponseSettings, SummaryResponseStyle, SummarySettings, SummarySourceMode,
    SummaryStorageBackend, SummaryStorageSettings, SummaryTriggerMode,
};
pub use vector::{
    VectorChunkingMode, VectorSearchGranularity, VectorSearchProvider, VectorSearchSettings,
};
pub use watcher::WatcherSettings;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_storage_method_requires_canonical_values() {
        let parsed: Result<DaemonConfig, _> = toml::from_str(
            r#"
[git_storage]
method = "platform_api"
"#,
        );
        assert!(parsed.is_err(), "legacy aliases must be rejected");
    }

    #[test]
    fn unknown_watcher_flags_are_ignored() {
        let cfg: DaemonConfig = toml::from_str(
            r#"
[watchers]
claude_code = false
opencode = false
cursor = false
custom_paths = ["~/.codex/sessions"]
"#,
        )
        .expect("parse watcher config");

        assert_eq!(
            cfg.watchers.custom_paths,
            vec!["~/.codex/sessions".to_string()]
        );
    }

    #[test]
    fn watcher_settings_serialize_only_current_fields() {
        let cfg = DaemonConfig::default();
        let encoded = toml::to_string(&cfg).expect("serialize config");

        assert!(encoded.contains("custom_paths"));
        assert!(!encoded.contains("\nclaude_code ="));
        assert!(!encoded.contains("\nopencode ="));
        assert!(!encoded.contains("\ncursor ="));
    }

    #[test]
    fn git_retention_defaults_are_stable() {
        let cfg = DaemonConfig::default();
        assert!(!cfg.git_storage.retention.enabled);
        assert_eq!(cfg.git_storage.retention.keep_days, 30);
        assert_eq!(cfg.git_storage.retention.interval_secs, 86_400);
    }

    #[test]
    fn git_retention_fields_deserialize_from_toml() {
        let cfg: DaemonConfig = toml::from_str(
            r#"
[git_storage]
method = "native"

[git_storage.retention]
enabled = true
keep_days = 14
interval_secs = 43200
"#,
        )
        .expect("parse config");

        assert!(cfg.git_storage.retention.enabled);
        assert_eq!(cfg.git_storage.retention.keep_days, 14);
        assert_eq!(cfg.git_storage.retention.interval_secs, 43_200);
    }

    #[test]
    fn summary_provider_requires_canonical_values() {
        let parsed: Result<DaemonConfig, _> = toml::from_str(
            r#"
[summary.provider]
id = "openai"
"#,
        );
        assert!(
            parsed.is_err(),
            "unsupported summary provider must be rejected"
        );
    }

    #[test]
    fn summary_settings_deserialize_from_toml() {
        let cfg: DaemonConfig = toml::from_str(
            r#"
[summary]
source_mode = "session_or_git_changes"

[summary.provider]
id = "ollama"
endpoint = "http://localhost:11434"
model = "llama3.2:3b"

[summary.prompt]
template = "Use {{HAIL_COMPACT}} only"

[summary.response]
style = "detailed"
shape = "security_first"

[summary.storage]
trigger = "on_session_save"
backend = "local_db"

[summary.batch]
execution_mode = "manual"
scope = "all"
recent_days = 90
"#,
        )
        .expect("parse summary settings");

        assert_eq!(cfg.summary.provider.id, SummaryProvider::Ollama);
        assert_eq!(cfg.summary.provider.endpoint, "http://localhost:11434");
        assert_eq!(cfg.summary.provider.model, "llama3.2:3b");
        assert_eq!(
            cfg.summary.source_mode,
            SummarySourceMode::SessionOrGitChanges
        );
        assert_eq!(cfg.summary.prompt.template, "Use {{HAIL_COMPACT}} only");
        assert_eq!(cfg.summary.response.style, SummaryResponseStyle::Detailed);
        assert_eq!(
            cfg.summary.response.shape,
            SummaryOutputShape::SecurityFirst
        );
        assert_eq!(
            cfg.summary.storage.trigger,
            SummaryTriggerMode::OnSessionSave
        );
        assert_eq!(cfg.summary.storage.backend, SummaryStorageBackend::LocalDb);
        assert_eq!(
            cfg.summary.batch.execution_mode,
            SummaryBatchExecutionMode::Manual
        );
        assert_eq!(cfg.summary.batch.scope, SummaryBatchScope::All);
        assert_eq!(cfg.summary.batch.recent_days, 90);
        assert!(cfg.summary.is_configured());
    }

    #[test]
    fn summary_batch_defaults_are_stable() {
        let cfg = DaemonConfig::default();
        assert_eq!(
            cfg.summary.batch.execution_mode,
            SummaryBatchExecutionMode::OnAppStart
        );
        assert_eq!(cfg.summary.batch.scope, SummaryBatchScope::RecentDays);
        assert_eq!(cfg.summary.batch.recent_days, 30);
    }

    #[test]
    fn summary_response_style_requires_canonical_values() {
        let parsed: Result<DaemonConfig, _> = toml::from_str(
            r#"
[summary.response]
style = "verbose"
"#,
        );
        assert!(
            parsed.is_err(),
            "unsupported summary response_style must be rejected"
        );
    }

    #[test]
    fn summary_source_mode_requires_canonical_values() {
        let parsed: Result<DaemonConfig, _> = toml::from_str(
            r#"
[summary]
source_mode = "git_only"
"#,
        );
        assert!(
            parsed.is_err(),
            "unsupported summary source_mode must be rejected"
        );
    }

    #[test]
    fn summary_output_shape_requires_canonical_values() {
        let parsed: Result<DaemonConfig, _> = toml::from_str(
            r#"
[summary.response]
shape = "grouped"
"#,
        );
        assert!(
            parsed.is_err(),
            "unsupported summary output_shape must be rejected"
        );
    }

    #[test]
    fn summary_trigger_mode_requires_canonical_values() {
        let parsed: Result<DaemonConfig, _> = toml::from_str(
            r#"
[summary.storage]
trigger = "always"
"#,
        );
        assert!(
            parsed.is_err(),
            "unsupported summary trigger_mode must be rejected"
        );
    }

    #[test]
    fn summary_storage_backend_requires_canonical_values() {
        let parsed: Result<DaemonConfig, _> = toml::from_str(
            r#"
[summary.storage]
backend = "remote_db"
"#,
        );
        assert!(
            parsed.is_err(),
            "unsupported summary storage.backend must be rejected"
        );
    }

    #[test]
    fn summary_batch_execution_mode_requires_canonical_values() {
        let parsed: Result<DaemonConfig, _> = toml::from_str(
            r#"
[summary.batch]
execution_mode = "scheduled"
"#,
        );
        assert!(
            parsed.is_err(),
            "unsupported summary batch execution mode must be rejected"
        );
    }

    #[test]
    fn summary_batch_scope_requires_canonical_values() {
        let parsed: Result<DaemonConfig, _> = toml::from_str(
            r#"
[summary.batch]
scope = "recent_weeks"
"#,
        );
        assert!(
            parsed.is_err(),
            "unsupported summary batch scope must be rejected"
        );
    }

    #[test]
    fn summary_provider_accepts_cli_variants() {
        let codex_cfg: DaemonConfig = toml::from_str(
            r#"
[summary.provider]
id = "codex_exec"
"#,
        )
        .expect("parse codex summary provider");
        assert_eq!(codex_cfg.summary.provider.id, SummaryProvider::CodexExec);
        assert!(codex_cfg.summary.is_configured());

        let claude_cfg: DaemonConfig = toml::from_str(
            r#"
[summary.provider]
id = "claude_cli"
"#,
        )
        .expect("parse claude summary provider");
        assert_eq!(claude_cfg.summary.provider.id, SummaryProvider::ClaudeCli);
        assert!(claude_cfg.summary.is_configured());
    }

    #[test]
    fn summary_is_configured_requires_model_only_for_ollama() {
        let mut cfg = DaemonConfig::default();
        cfg.summary.provider.id = SummaryProvider::Ollama;
        cfg.summary.provider.model.clear();
        assert!(!cfg.summary.is_configured());

        cfg.summary.provider.model = "llama3.2:3b".to_string();
        assert!(cfg.summary.is_configured());

        cfg.summary.provider.id = SummaryProvider::CodexExec;
        cfg.summary.provider.model.clear();
        assert!(cfg.summary.is_configured());

        cfg.summary.provider.id = SummaryProvider::ClaudeCli;
        assert!(cfg.summary.is_configured());
    }

    #[test]
    fn summary_git_fallback_availability_depends_on_source_mode() {
        let mut cfg = DaemonConfig::default();
        cfg.summary.source_mode = SummarySourceMode::SessionOnly;
        assert!(!cfg.summary.allows_git_changes_fallback());

        cfg.summary.source_mode = SummarySourceMode::SessionOrGitChanges;
        assert!(cfg.summary.allows_git_changes_fallback());
    }

    #[test]
    fn summary_default_storage_uses_hidden_ref_backend() {
        let cfg = DaemonConfig::default();
        assert_eq!(
            cfg.summary.storage.trigger,
            SummaryTriggerMode::OnSessionSave
        );
        assert_eq!(
            cfg.summary.storage.backend,
            SummaryStorageBackend::HiddenRef
        );
        assert!(cfg.summary.should_generate_on_session_save());
        assert!(cfg.summary.persists_to_hidden_ref());
    }

    #[test]
    fn summary_provider_transport_matches_provider_kind() {
        let mut cfg = DaemonConfig::default();
        cfg.summary.provider.id = SummaryProvider::Disabled;
        assert_eq!(
            cfg.summary.provider_transport(),
            SummaryProviderTransport::None
        );

        cfg.summary.provider.id = SummaryProvider::Ollama;
        assert_eq!(
            cfg.summary.provider_transport(),
            SummaryProviderTransport::Http
        );

        cfg.summary.provider.id = SummaryProvider::CodexExec;
        assert_eq!(
            cfg.summary.provider_transport(),
            SummaryProviderTransport::Cli
        );
    }

    #[test]
    fn vector_search_defaults_are_stable() {
        let cfg = DaemonConfig::default();
        assert!(!cfg.vector_search.enabled);
        assert_eq!(cfg.vector_search.provider, VectorSearchProvider::Ollama);
        assert_eq!(cfg.vector_search.model, "bge-m3");
        assert_eq!(cfg.vector_search.endpoint, "http://127.0.0.1:11434");
        assert_eq!(
            cfg.vector_search.granularity,
            VectorSearchGranularity::EventLineChunk
        );
        assert_eq!(cfg.vector_search.chunking_mode, VectorChunkingMode::Auto);
        assert_eq!(cfg.vector_search.chunk_size_lines, 12);
        assert_eq!(cfg.vector_search.chunk_overlap_lines, 3);
        assert_eq!(cfg.vector_search.top_k_chunks, 30);
        assert_eq!(cfg.vector_search.top_k_sessions, 20);
    }

    #[test]
    fn vector_search_settings_deserialize_from_toml() {
        let cfg: DaemonConfig = toml::from_str(
            r#"
[vector_search]
enabled = true
provider = "ollama"
model = "bge-m3"
endpoint = "http://localhost:11434"
granularity = "event_line_chunk"
chunking_mode = "manual"
chunk_size_lines = 16
chunk_overlap_lines = 4
top_k_chunks = 60
top_k_sessions = 10
"#,
        )
        .expect("parse vector search settings");

        assert!(cfg.vector_search.enabled);
        assert_eq!(cfg.vector_search.provider, VectorSearchProvider::Ollama);
        assert_eq!(cfg.vector_search.model, "bge-m3");
        assert_eq!(cfg.vector_search.endpoint, "http://localhost:11434");
        assert_eq!(
            cfg.vector_search.granularity,
            VectorSearchGranularity::EventLineChunk
        );
        assert_eq!(cfg.vector_search.chunking_mode, VectorChunkingMode::Manual);
        assert_eq!(cfg.vector_search.chunk_size_lines, 16);
        assert_eq!(cfg.vector_search.chunk_overlap_lines, 4);
        assert_eq!(cfg.vector_search.top_k_chunks, 60);
        assert_eq!(cfg.vector_search.top_k_sessions, 10);
    }

    #[test]
    fn vector_search_provider_requires_canonical_values() {
        let parsed: Result<DaemonConfig, _> = toml::from_str(
            r#"
[vector_search]
provider = "openai"
"#,
        );
        assert!(
            parsed.is_err(),
            "unsupported vector provider must be rejected"
        );
    }

    #[test]
    fn vector_search_granularity_requires_canonical_values() {
        let parsed: Result<DaemonConfig, _> = toml::from_str(
            r#"
[vector_search]
granularity = "session_text"
"#,
        );
        assert!(
            parsed.is_err(),
            "unsupported vector granularity must be rejected"
        );
    }

    #[test]
    fn vector_search_chunking_mode_requires_canonical_values() {
        let parsed: Result<DaemonConfig, _> = toml::from_str(
            r#"
[vector_search]
chunking_mode = "adaptive"
"#,
        );
        assert!(
            parsed.is_err(),
            "unsupported vector chunking mode must be rejected"
        );
    }

    #[test]
    fn change_reader_defaults_are_stable() {
        let cfg = DaemonConfig::default();
        assert!(!cfg.change_reader.enabled);
        assert_eq!(cfg.change_reader.scope, ChangeReaderScope::SummaryOnly);
        assert!(cfg.change_reader.qa_enabled);
        assert_eq!(cfg.change_reader.max_context_chars, 12_000);
        assert!(!cfg.change_reader.voice.enabled);
        assert_eq!(
            cfg.change_reader.voice.provider,
            ChangeReaderVoiceProvider::Openai
        );
        assert_eq!(cfg.change_reader.voice.model, "gpt-4o-mini-tts");
        assert_eq!(cfg.change_reader.voice.voice, "alloy");
        assert!(cfg.change_reader.voice.api_key.is_empty());
    }

    #[test]
    fn change_reader_settings_deserialize_from_toml() {
        let cfg: DaemonConfig = toml::from_str(
            r#"
[change_reader]
enabled = true
scope = "full_context"
qa_enabled = false
max_context_chars = 24000
[change_reader.voice]
enabled = true
provider = "openai"
model = "gpt-4o-mini-tts"
voice = "nova"
api_key = "sk-local"
"#,
        )
        .expect("parse change reader settings");

        assert!(cfg.change_reader.enabled);
        assert_eq!(cfg.change_reader.scope, ChangeReaderScope::FullContext);
        assert!(!cfg.change_reader.qa_enabled);
        assert_eq!(cfg.change_reader.max_context_chars, 24_000);
        assert!(cfg.change_reader.voice.enabled);
        assert_eq!(
            cfg.change_reader.voice.provider,
            ChangeReaderVoiceProvider::Openai
        );
        assert_eq!(cfg.change_reader.voice.model, "gpt-4o-mini-tts");
        assert_eq!(cfg.change_reader.voice.voice, "nova");
        assert_eq!(cfg.change_reader.voice.api_key, "sk-local");
    }

    #[test]
    fn change_reader_scope_requires_canonical_values() {
        let parsed: Result<DaemonConfig, _> = toml::from_str(
            r#"
[change_reader]
scope = "full"
"#,
        );
        assert!(
            parsed.is_err(),
            "unsupported change reader scope must be rejected"
        );
    }

    #[test]
    fn change_reader_voice_provider_requires_canonical_values() {
        let parsed: Result<DaemonConfig, _> = toml::from_str(
            r#"
[change_reader.voice]
provider = "azure"
"#,
        );
        assert!(
            parsed.is_err(),
            "unsupported change reader voice provider must be rejected"
        );
    }

    #[test]
    fn lifecycle_defaults_are_stable() {
        let cfg = DaemonConfig::default();
        assert!(cfg.lifecycle.enabled);
        assert_eq!(cfg.lifecycle.session_ttl_days, 30);
        assert_eq!(cfg.lifecycle.summary_ttl_days, 30);
        assert_eq!(cfg.lifecycle.cleanup_interval_secs, 3_600);
    }

    #[test]
    fn lifecycle_settings_deserialize_from_toml() {
        let cfg: DaemonConfig = toml::from_str(
            r#"
[lifecycle]
enabled = true
session_ttl_days = 45
summary_ttl_days = 14
cleanup_interval_secs = 7200
"#,
        )
        .expect("parse lifecycle settings");

        assert!(cfg.lifecycle.enabled);
        assert_eq!(cfg.lifecycle.session_ttl_days, 45);
        assert_eq!(cfg.lifecycle.summary_ttl_days, 14);
        assert_eq!(cfg.lifecycle.cleanup_interval_secs, 7_200);
    }

    #[test]
    fn daemon_default_session_view_deserializes_from_toml() {
        let cfg: DaemonConfig = toml::from_str(
            r#"
[daemon]
session_default_view = "compressed"
"#,
        )
        .expect("parse daemon session_default_view");

        assert_eq!(
            cfg.daemon.session_default_view,
            SessionDefaultView::Compressed
        );
    }

    #[test]
    fn daemon_default_session_view_requires_canonical_values() {
        let parsed: Result<DaemonConfig, _> = toml::from_str(
            r#"
[daemon]
session_default_view = "compact"
"#,
        );
        assert!(
            parsed.is_err(),
            "unsupported session_default_view must fail"
        );
    }

    #[test]
    fn summary_storage_helpers_reflect_selected_modes() {
        let mut settings = SummarySettings::default();

        settings.source_mode = SummarySourceMode::SessionOnly;
        assert!(!settings.allows_git_changes_fallback());

        settings.source_mode = SummarySourceMode::SessionOrGitChanges;
        assert!(settings.allows_git_changes_fallback());

        settings.storage.trigger = SummaryTriggerMode::Manual;
        assert!(!settings.should_generate_on_session_save());

        settings.storage.trigger = SummaryTriggerMode::OnSessionSave;
        assert!(settings.should_generate_on_session_save());

        settings.storage.backend = SummaryStorageBackend::LocalDb;
        assert!(settings.persists_to_local_db());
        assert!(!settings.persists_to_hidden_ref());

        settings.storage.backend = SummaryStorageBackend::HiddenRef;
        assert!(!settings.persists_to_local_db());
        assert!(settings.persists_to_hidden_ref());
    }
}
