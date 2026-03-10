use crate::app::session_summary::migrate_summary_storage_backend;
use crate::app::vector::{vector_embed_endpoint, vector_embed_model, vector_preflight_for_runtime};
use crate::{
    DesktopApiResult, desktop_error, enum_label, load_runtime_config, open_local_db,
    save_runtime_config,
};
use opensession_api::{
    DesktopChangeReaderScope, DesktopChangeReaderVoiceProvider, DesktopRuntimeChangeReaderSettings,
    DesktopRuntimeChangeReaderVoiceSettings, DesktopRuntimeLifecycleSettings,
    DesktopRuntimeSettingsResponse, DesktopRuntimeSettingsUpdateRequest,
    DesktopRuntimeSummaryBatchSettings, DesktopRuntimeSummaryPromptSettings,
    DesktopRuntimeSummaryProviderSettings, DesktopRuntimeSummaryResponseSettings,
    DesktopRuntimeSummarySettings, DesktopRuntimeSummaryStorageSettings,
    DesktopRuntimeSummaryUiConstraints, DesktopRuntimeVectorSearchSettings,
    DesktopSummaryBatchExecutionMode, DesktopSummaryBatchScope, DesktopSummaryOutputShape,
    DesktopSummaryProviderDetectResponse, DesktopSummaryProviderId,
    DesktopSummaryProviderTransport, DesktopSummaryResponseStyle, DesktopSummarySourceMode,
    DesktopSummaryStorageBackend, DesktopSummaryTriggerMode, DesktopVectorChunkingMode,
    DesktopVectorSearchGranularity, DesktopVectorSearchProvider,
};
use opensession_runtime_config::{
    ChangeReaderScope, ChangeReaderVoiceProvider, DaemonConfig, LifecycleSettings,
    SessionDefaultView, SummaryBatchExecutionMode as RuntimeSummaryBatchExecutionMode,
    SummaryBatchScope as RuntimeSummaryBatchScope, SummaryOutputShape, SummaryProvider,
    SummaryResponseStyle, SummarySourceMode, SummaryStorageBackend, SummaryTriggerMode,
    VectorChunkingMode, VectorSearchGranularity, VectorSearchProvider,
};
use opensession_summary::validate_summary_prompt_template;
use serde_json::json;

pub(crate) fn map_summary_provider_id_from_runtime(
    value: &SummaryProvider,
) -> DesktopSummaryProviderId {
    match value {
        SummaryProvider::Disabled => DesktopSummaryProviderId::Disabled,
        SummaryProvider::Ollama => DesktopSummaryProviderId::Ollama,
        SummaryProvider::CodexExec => DesktopSummaryProviderId::CodexExec,
        SummaryProvider::ClaudeCli => DesktopSummaryProviderId::ClaudeCli,
    }
}

fn map_summary_provider_id_to_runtime(value: &DesktopSummaryProviderId) -> SummaryProvider {
    match value {
        DesktopSummaryProviderId::Disabled => SummaryProvider::Disabled,
        DesktopSummaryProviderId::Ollama => SummaryProvider::Ollama,
        DesktopSummaryProviderId::CodexExec => SummaryProvider::CodexExec,
        DesktopSummaryProviderId::ClaudeCli => SummaryProvider::ClaudeCli,
    }
}

fn map_summary_transport_from_runtime(
    value: &opensession_runtime_config::SummaryProviderTransport,
) -> DesktopSummaryProviderTransport {
    match value {
        opensession_runtime_config::SummaryProviderTransport::None => {
            DesktopSummaryProviderTransport::None
        }
        opensession_runtime_config::SummaryProviderTransport::Cli => {
            DesktopSummaryProviderTransport::Cli
        }
        opensession_runtime_config::SummaryProviderTransport::Http => {
            DesktopSummaryProviderTransport::Http
        }
    }
}

fn map_summary_source_mode_from_runtime(value: &SummarySourceMode) -> DesktopSummarySourceMode {
    match value {
        SummarySourceMode::SessionOnly => DesktopSummarySourceMode::SessionOnly,
        SummarySourceMode::SessionOrGitChanges => DesktopSummarySourceMode::SessionOrGitChanges,
    }
}

fn map_summary_source_mode_to_runtime(value: &DesktopSummarySourceMode) -> SummarySourceMode {
    match value {
        DesktopSummarySourceMode::SessionOnly => SummarySourceMode::SessionOnly,
        DesktopSummarySourceMode::SessionOrGitChanges => SummarySourceMode::SessionOrGitChanges,
    }
}

fn map_summary_response_style_from_runtime(
    value: &SummaryResponseStyle,
) -> DesktopSummaryResponseStyle {
    match value {
        SummaryResponseStyle::Compact => DesktopSummaryResponseStyle::Compact,
        SummaryResponseStyle::Standard => DesktopSummaryResponseStyle::Standard,
        SummaryResponseStyle::Detailed => DesktopSummaryResponseStyle::Detailed,
    }
}

fn map_summary_response_style_to_runtime(
    value: &DesktopSummaryResponseStyle,
) -> SummaryResponseStyle {
    match value {
        DesktopSummaryResponseStyle::Compact => SummaryResponseStyle::Compact,
        DesktopSummaryResponseStyle::Standard => SummaryResponseStyle::Standard,
        DesktopSummaryResponseStyle::Detailed => SummaryResponseStyle::Detailed,
    }
}

fn map_summary_output_shape_from_runtime(value: &SummaryOutputShape) -> DesktopSummaryOutputShape {
    match value {
        SummaryOutputShape::Layered => DesktopSummaryOutputShape::Layered,
        SummaryOutputShape::FileList => DesktopSummaryOutputShape::FileList,
        SummaryOutputShape::SecurityFirst => DesktopSummaryOutputShape::SecurityFirst,
    }
}

fn map_summary_output_shape_to_runtime(value: &DesktopSummaryOutputShape) -> SummaryOutputShape {
    match value {
        DesktopSummaryOutputShape::Layered => SummaryOutputShape::Layered,
        DesktopSummaryOutputShape::FileList => SummaryOutputShape::FileList,
        DesktopSummaryOutputShape::SecurityFirst => SummaryOutputShape::SecurityFirst,
    }
}

fn map_summary_trigger_mode_from_runtime(value: &SummaryTriggerMode) -> DesktopSummaryTriggerMode {
    match value {
        SummaryTriggerMode::Manual => DesktopSummaryTriggerMode::Manual,
        SummaryTriggerMode::OnSessionSave => DesktopSummaryTriggerMode::OnSessionSave,
    }
}

fn map_summary_trigger_mode_to_runtime(value: &DesktopSummaryTriggerMode) -> SummaryTriggerMode {
    match value {
        DesktopSummaryTriggerMode::Manual => SummaryTriggerMode::Manual,
        DesktopSummaryTriggerMode::OnSessionSave => SummaryTriggerMode::OnSessionSave,
    }
}

fn map_summary_storage_backend_from_runtime(
    value: &SummaryStorageBackend,
) -> DesktopSummaryStorageBackend {
    match value {
        SummaryStorageBackend::HiddenRef => DesktopSummaryStorageBackend::HiddenRef,
        SummaryStorageBackend::LocalDb => DesktopSummaryStorageBackend::LocalDb,
        SummaryStorageBackend::None => DesktopSummaryStorageBackend::None,
    }
}

fn map_summary_storage_backend_to_runtime(
    value: &DesktopSummaryStorageBackend,
) -> SummaryStorageBackend {
    match value {
        DesktopSummaryStorageBackend::HiddenRef => SummaryStorageBackend::HiddenRef,
        DesktopSummaryStorageBackend::LocalDb => SummaryStorageBackend::LocalDb,
        DesktopSummaryStorageBackend::None => SummaryStorageBackend::None,
    }
}

fn map_summary_batch_execution_mode_from_runtime(
    value: &RuntimeSummaryBatchExecutionMode,
) -> DesktopSummaryBatchExecutionMode {
    match value {
        RuntimeSummaryBatchExecutionMode::Manual => DesktopSummaryBatchExecutionMode::Manual,
        RuntimeSummaryBatchExecutionMode::OnAppStart => {
            DesktopSummaryBatchExecutionMode::OnAppStart
        }
    }
}

fn map_summary_batch_execution_mode_to_runtime(
    value: &DesktopSummaryBatchExecutionMode,
) -> RuntimeSummaryBatchExecutionMode {
    match value {
        DesktopSummaryBatchExecutionMode::Manual => RuntimeSummaryBatchExecutionMode::Manual,
        DesktopSummaryBatchExecutionMode::OnAppStart => {
            RuntimeSummaryBatchExecutionMode::OnAppStart
        }
    }
}

fn map_summary_batch_scope_from_runtime(
    value: &RuntimeSummaryBatchScope,
) -> DesktopSummaryBatchScope {
    match value {
        RuntimeSummaryBatchScope::RecentDays => DesktopSummaryBatchScope::RecentDays,
        RuntimeSummaryBatchScope::All => DesktopSummaryBatchScope::All,
    }
}

fn map_summary_batch_scope_to_runtime(
    value: &DesktopSummaryBatchScope,
) -> RuntimeSummaryBatchScope {
    match value {
        DesktopSummaryBatchScope::RecentDays => RuntimeSummaryBatchScope::RecentDays,
        DesktopSummaryBatchScope::All => RuntimeSummaryBatchScope::All,
    }
}

fn map_vector_provider_from_runtime(value: &VectorSearchProvider) -> DesktopVectorSearchProvider {
    match value {
        VectorSearchProvider::Ollama => DesktopVectorSearchProvider::Ollama,
    }
}

fn map_vector_provider_to_runtime(value: &DesktopVectorSearchProvider) -> VectorSearchProvider {
    match value {
        DesktopVectorSearchProvider::Ollama => VectorSearchProvider::Ollama,
    }
}

fn map_vector_granularity_from_runtime(
    value: &VectorSearchGranularity,
) -> DesktopVectorSearchGranularity {
    match value {
        VectorSearchGranularity::EventLineChunk => DesktopVectorSearchGranularity::EventLineChunk,
    }
}

fn map_vector_granularity_to_runtime(
    value: &DesktopVectorSearchGranularity,
) -> VectorSearchGranularity {
    match value {
        DesktopVectorSearchGranularity::EventLineChunk => VectorSearchGranularity::EventLineChunk,
    }
}

fn map_vector_chunking_mode_from_runtime(value: &VectorChunkingMode) -> DesktopVectorChunkingMode {
    match value {
        VectorChunkingMode::Auto => DesktopVectorChunkingMode::Auto,
        VectorChunkingMode::Manual => DesktopVectorChunkingMode::Manual,
    }
}

fn map_vector_chunking_mode_to_runtime(value: &DesktopVectorChunkingMode) -> VectorChunkingMode {
    match value {
        DesktopVectorChunkingMode::Auto => VectorChunkingMode::Auto,
        DesktopVectorChunkingMode::Manual => VectorChunkingMode::Manual,
    }
}

pub(crate) fn map_change_reader_scope_from_runtime(
    value: &ChangeReaderScope,
) -> DesktopChangeReaderScope {
    match value {
        ChangeReaderScope::SummaryOnly => DesktopChangeReaderScope::SummaryOnly,
        ChangeReaderScope::FullContext => DesktopChangeReaderScope::FullContext,
    }
}

fn map_change_reader_scope_to_runtime(value: &DesktopChangeReaderScope) -> ChangeReaderScope {
    match value {
        DesktopChangeReaderScope::SummaryOnly => ChangeReaderScope::SummaryOnly,
        DesktopChangeReaderScope::FullContext => ChangeReaderScope::FullContext,
    }
}

fn map_change_reader_voice_provider_from_runtime(
    value: &ChangeReaderVoiceProvider,
) -> DesktopChangeReaderVoiceProvider {
    match value {
        ChangeReaderVoiceProvider::Openai => DesktopChangeReaderVoiceProvider::Openai,
    }
}

fn map_change_reader_voice_provider_to_runtime(
    value: &DesktopChangeReaderVoiceProvider,
) -> ChangeReaderVoiceProvider {
    match value {
        DesktopChangeReaderVoiceProvider::Openai => ChangeReaderVoiceProvider::Openai,
    }
}

fn desktop_summary_settings_from_runtime(config: &DaemonConfig) -> DesktopRuntimeSummarySettings {
    let source_mode = SummarySourceMode::SessionOnly;
    DesktopRuntimeSummarySettings {
        provider: DesktopRuntimeSummaryProviderSettings {
            id: map_summary_provider_id_from_runtime(&config.summary.provider.id),
            transport: map_summary_transport_from_runtime(&config.summary.provider_transport()),
            endpoint: config.summary.provider.endpoint.clone(),
            model: config.summary.provider.model.clone(),
        },
        prompt: DesktopRuntimeSummaryPromptSettings {
            template: config.summary.prompt.template.clone(),
            default_template: opensession_summary::DEFAULT_SUMMARY_PROMPT_TEMPLATE_V2.to_string(),
        },
        response: DesktopRuntimeSummaryResponseSettings {
            style: map_summary_response_style_from_runtime(&config.summary.response.style),
            shape: map_summary_output_shape_from_runtime(&config.summary.response.shape),
        },
        storage: DesktopRuntimeSummaryStorageSettings {
            trigger: map_summary_trigger_mode_from_runtime(&config.summary.storage.trigger),
            backend: map_summary_storage_backend_from_runtime(&config.summary.storage.backend),
        },
        source_mode: map_summary_source_mode_from_runtime(&source_mode),
        batch: DesktopRuntimeSummaryBatchSettings {
            execution_mode: map_summary_batch_execution_mode_from_runtime(
                &config.summary.batch.execution_mode,
            ),
            scope: map_summary_batch_scope_from_runtime(&config.summary.batch.scope),
            recent_days: config.summary.batch.recent_days.max(1),
        },
    }
}

fn desktop_lifecycle_settings_from_runtime(
    config: &DaemonConfig,
) -> DesktopRuntimeLifecycleSettings {
    DesktopRuntimeLifecycleSettings {
        enabled: config.lifecycle.enabled,
        session_ttl_days: config.lifecycle.session_ttl_days.max(1),
        summary_ttl_days: config.lifecycle.summary_ttl_days.max(1),
        cleanup_interval_secs: config.lifecycle.cleanup_interval_secs.max(60),
    }
}

fn desktop_vector_settings_from_runtime(
    config: &DaemonConfig,
) -> DesktopRuntimeVectorSearchSettings {
    DesktopRuntimeVectorSearchSettings {
        enabled: config.vector_search.enabled,
        provider: map_vector_provider_from_runtime(&config.vector_search.provider),
        model: vector_embed_model(config),
        endpoint: vector_embed_endpoint(config),
        granularity: map_vector_granularity_from_runtime(&config.vector_search.granularity),
        chunking_mode: map_vector_chunking_mode_from_runtime(&config.vector_search.chunking_mode),
        chunk_size_lines: config.vector_search.chunk_size_lines.max(1),
        chunk_overlap_lines: config.vector_search.chunk_overlap_lines,
        top_k_chunks: config.vector_search.top_k_chunks.max(1),
        top_k_sessions: config.vector_search.top_k_sessions.max(1),
    }
}

fn desktop_change_reader_settings_from_runtime(
    config: &DaemonConfig,
) -> DesktopRuntimeChangeReaderSettings {
    DesktopRuntimeChangeReaderSettings {
        enabled: config.change_reader.enabled,
        scope: map_change_reader_scope_from_runtime(&config.change_reader.scope),
        qa_enabled: config.change_reader.qa_enabled,
        max_context_chars: config.change_reader.max_context_chars.max(1),
        voice: DesktopRuntimeChangeReaderVoiceSettings {
            enabled: config.change_reader.voice.enabled,
            provider: map_change_reader_voice_provider_from_runtime(
                &config.change_reader.voice.provider,
            ),
            model: config.change_reader.voice.model.clone(),
            voice: config.change_reader.voice.voice.clone(),
            api_key_configured: !config.change_reader.voice.api_key.trim().is_empty(),
        },
    }
}

fn map_session_default_view_from_str(raw: &str) -> Option<SessionDefaultView> {
    match raw.trim() {
        "full" => Some(SessionDefaultView::Full),
        "compressed" => Some(SessionDefaultView::Compressed),
        _ => None,
    }
}

#[tauri::command]
pub(crate) fn desktop_get_runtime_settings() -> DesktopApiResult<DesktopRuntimeSettingsResponse> {
    let config = load_runtime_config()?;
    let session_default_view = match config.daemon.session_default_view {
        SessionDefaultView::Full => "full",
        SessionDefaultView::Compressed => "compressed",
    }
    .to_string();

    Ok(DesktopRuntimeSettingsResponse {
        session_default_view,
        summary: desktop_summary_settings_from_runtime(&config),
        vector_search: desktop_vector_settings_from_runtime(&config),
        change_reader: desktop_change_reader_settings_from_runtime(&config),
        lifecycle: desktop_lifecycle_settings_from_runtime(&config),
        ui_constraints: DesktopRuntimeSummaryUiConstraints {
            source_mode_locked: true,
            source_mode_locked_value: DesktopSummarySourceMode::SessionOnly,
        },
    })
}

#[tauri::command]
pub(crate) fn desktop_update_runtime_settings(
    request: DesktopRuntimeSettingsUpdateRequest,
) -> DesktopApiResult<DesktopRuntimeSettingsResponse> {
    let mut config = load_runtime_config()?;
    let current_summary_backend = config.summary.storage.backend.clone();
    let mut requested_summary_backend: Option<SummaryStorageBackend> = None;

    if let Some(session_default_view) = request.session_default_view.as_deref() {
        let mapped = map_session_default_view_from_str(session_default_view).ok_or_else(|| {
            desktop_error(
                "desktop.runtime_settings_invalid_view",
                422,
                "invalid session_default_view (expected full|compressed)",
                Some(json!({ "session_default_view": session_default_view })),
            )
        })?;
        config.daemon.session_default_view = mapped;
    }

    if let Some(summary) = request.summary {
        if !matches!(summary.source_mode, DesktopSummarySourceMode::SessionOnly) {
            return Err(desktop_error(
                "desktop.runtime_settings_source_mode_locked",
                422,
                "desktop source_mode is locked to session_only",
                Some(json!({ "source_mode": summary.source_mode })),
            ));
        }
        validate_summary_prompt_template(summary.prompt.template.as_str()).map_err(|cause| {
            desktop_error(
                "desktop.runtime_settings_invalid_prompt_template",
                422,
                "invalid summary.prompt.template",
                Some(json!({ "cause": cause })),
            )
        })?;

        config.summary.provider.id = map_summary_provider_id_to_runtime(&summary.provider.id);
        config.summary.provider.endpoint = summary.provider.endpoint.trim().to_string();
        config.summary.provider.model = summary.provider.model.trim().to_string();
        config.summary.prompt.template = summary.prompt.template;
        config.summary.response.style =
            map_summary_response_style_to_runtime(&summary.response.style);
        config.summary.response.shape =
            map_summary_output_shape_to_runtime(&summary.response.shape);
        config.summary.storage.trigger =
            map_summary_trigger_mode_to_runtime(&summary.storage.trigger);
        let mapped_backend = map_summary_storage_backend_to_runtime(&summary.storage.backend);
        config.summary.storage.backend = mapped_backend.clone();
        requested_summary_backend = Some(mapped_backend);
        config.summary.source_mode = map_summary_source_mode_to_runtime(&summary.source_mode);
        if summary.batch.recent_days == 0 {
            return Err(desktop_error(
                "desktop.runtime_settings_invalid_summary_batch_recent_days",
                422,
                "summary.batch.recent_days must be greater than zero",
                Some(json!({ "recent_days": summary.batch.recent_days })),
            ));
        }
        config.summary.batch.execution_mode =
            map_summary_batch_execution_mode_to_runtime(&summary.batch.execution_mode);
        config.summary.batch.scope = map_summary_batch_scope_to_runtime(&summary.batch.scope);
        config.summary.batch.recent_days = summary.batch.recent_days.max(1);
    }

    if let Some(vector_search) = request.vector_search {
        if vector_search.chunk_size_lines == 0 {
            return Err(desktop_error(
                "desktop.runtime_settings_invalid_vector_chunk_size",
                422,
                "vector_search.chunk_size_lines must be greater than zero",
                Some(json!({ "chunk_size_lines": vector_search.chunk_size_lines })),
            ));
        }
        if vector_search.chunk_overlap_lines >= vector_search.chunk_size_lines {
            return Err(desktop_error(
                "desktop.runtime_settings_invalid_vector_overlap",
                422,
                "vector_search.chunk_overlap_lines must be smaller than chunk_size_lines",
                Some(json!({
                    "chunk_size_lines": vector_search.chunk_size_lines,
                    "chunk_overlap_lines": vector_search.chunk_overlap_lines
                })),
            ));
        }
        if vector_search.top_k_chunks == 0 || vector_search.top_k_sessions == 0 {
            return Err(desktop_error(
                "desktop.runtime_settings_invalid_vector_limits",
                422,
                "vector_search.top_k_chunks and vector_search.top_k_sessions must be greater than zero",
                Some(json!({
                    "top_k_chunks": vector_search.top_k_chunks,
                    "top_k_sessions": vector_search.top_k_sessions
                })),
            ));
        }

        config.vector_search.enabled = vector_search.enabled;
        config.vector_search.provider = map_vector_provider_to_runtime(&vector_search.provider);
        config.vector_search.model = vector_search.model.trim().to_string();
        config.vector_search.endpoint = vector_search.endpoint.trim().to_string();
        config.vector_search.granularity =
            map_vector_granularity_to_runtime(&vector_search.granularity);
        config.vector_search.chunking_mode =
            map_vector_chunking_mode_to_runtime(&vector_search.chunking_mode);
        config.vector_search.chunk_size_lines = vector_search.chunk_size_lines.max(1);
        config.vector_search.chunk_overlap_lines = vector_search.chunk_overlap_lines;
        config.vector_search.top_k_chunks = vector_search.top_k_chunks.max(1);
        config.vector_search.top_k_sessions = vector_search.top_k_sessions.max(1);

        if config.vector_search.model.trim().is_empty() {
            config.vector_search.model = "bge-m3".to_string();
        }
        if config.vector_search.endpoint.trim().is_empty() {
            config.vector_search.endpoint = "http://127.0.0.1:11434".to_string();
        }

        if config.vector_search.enabled {
            let preflight = vector_preflight_for_runtime(&config);
            if !preflight.model_installed {
                return Err(desktop_error(
                    "desktop.vector_model_not_installed",
                    422,
                    "cannot enable vector search because model is not installed",
                    Some(json!({
                        "model": preflight.model,
                        "endpoint": preflight.endpoint,
                        "hint": "install model from Settings > Vector Search first"
                    })),
                ));
            }
        }
    }

    if let Some(change_reader) = request.change_reader {
        if change_reader.max_context_chars == 0 {
            return Err(desktop_error(
                "desktop.runtime_settings_invalid_change_reader_context",
                422,
                "change_reader.max_context_chars must be greater than zero",
                Some(json!({ "max_context_chars": change_reader.max_context_chars })),
            ));
        }
        config.change_reader.enabled = change_reader.enabled;
        config.change_reader.scope = map_change_reader_scope_to_runtime(&change_reader.scope);
        config.change_reader.qa_enabled = change_reader.qa_enabled;
        config.change_reader.max_context_chars = change_reader.max_context_chars.max(1);
        config.change_reader.voice.enabled = change_reader.voice.enabled;
        config.change_reader.voice.provider =
            map_change_reader_voice_provider_to_runtime(&change_reader.voice.provider);
        config.change_reader.voice.model = change_reader.voice.model.trim().to_string();
        config.change_reader.voice.voice = change_reader.voice.voice.trim().to_string();
        if let Some(api_key) = change_reader.voice.api_key {
            config.change_reader.voice.api_key = api_key.trim().to_string();
        }
        if config.change_reader.voice.model.trim().is_empty() {
            config.change_reader.voice.model = "gpt-4o-mini-tts".to_string();
        }
        if config.change_reader.voice.voice.trim().is_empty() {
            config.change_reader.voice.voice = "alloy".to_string();
        }
        if config.change_reader.voice.enabled
            && config.change_reader.voice.api_key.trim().is_empty()
        {
            return Err(desktop_error(
                "desktop.runtime_settings_change_reader_voice_api_key_required",
                422,
                "voice playback requires a configured API key",
                Some(json!({
                    "provider": enum_label(&config.change_reader.voice.provider),
                    "hint": "add a Voice API key in Settings > Runtime > Change Reader before enabling voice playback"
                })),
            ));
        }
    }

    if let Some(lifecycle) = request.lifecycle {
        if lifecycle.session_ttl_days == 0 {
            return Err(desktop_error(
                "desktop.runtime_settings_invalid_session_ttl_days",
                422,
                "lifecycle.session_ttl_days must be greater than zero",
                Some(json!({ "session_ttl_days": lifecycle.session_ttl_days })),
            ));
        }
        if lifecycle.summary_ttl_days == 0 {
            return Err(desktop_error(
                "desktop.runtime_settings_invalid_summary_ttl_days",
                422,
                "lifecycle.summary_ttl_days must be greater than zero",
                Some(json!({ "summary_ttl_days": lifecycle.summary_ttl_days })),
            ));
        }
        if lifecycle.cleanup_interval_secs < 60 {
            return Err(desktop_error(
                "desktop.runtime_settings_invalid_cleanup_interval",
                422,
                "lifecycle.cleanup_interval_secs must be at least 60 seconds",
                Some(json!({ "cleanup_interval_secs": lifecycle.cleanup_interval_secs })),
            ));
        }

        config.lifecycle = LifecycleSettings {
            enabled: lifecycle.enabled,
            session_ttl_days: lifecycle.session_ttl_days.max(1),
            summary_ttl_days: lifecycle.summary_ttl_days.max(1),
            cleanup_interval_secs: lifecycle.cleanup_interval_secs.max(60),
        };
    }

    if let Some(target_summary_backend) = requested_summary_backend {
        if target_summary_backend != current_summary_backend {
            let db = open_local_db()?;
            let stats = migrate_summary_storage_backend(
                &db,
                &current_summary_backend,
                &target_summary_backend,
            )?;
            if stats.migrated_summaries > 0 {
                eprintln!(
                    "summary storage migration complete: {} -> {} (migrated {} of {} summaries across {} sessions)",
                    enum_label(&current_summary_backend),
                    enum_label(&target_summary_backend),
                    stats.migrated_summaries,
                    stats.found_summaries,
                    stats.scanned_sessions,
                );
            }
        }
    }

    save_runtime_config(&config)?;
    desktop_get_runtime_settings()
}

#[tauri::command]
pub(crate) fn desktop_detect_summary_provider() -> DesktopSummaryProviderDetectResponse {
    if let Some(profile) = opensession_summary_runtime::detect_local_summary_profile() {
        return DesktopSummaryProviderDetectResponse {
            detected: true,
            provider: Some(map_summary_provider_id_from_runtime(&profile.provider)),
            transport: Some(map_summary_transport_from_runtime(
                &profile.provider.transport(),
            )),
            model: (!profile.model.trim().is_empty()).then_some(profile.model),
            endpoint: (!profile.endpoint.trim().is_empty()).then_some(profile.endpoint),
        };
    }

    DesktopSummaryProviderDetectResponse {
        detected: false,
        provider: None,
        transport: None,
        model: None,
        endpoint: None,
    }
}
