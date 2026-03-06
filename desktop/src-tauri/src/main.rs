#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;

use app::change_reader::{
    desktop_ask_session_changes, desktop_change_reader_tts, desktop_read_session_changes,
    require_non_empty_request_field,
};
use app::handoff::{desktop_build_handoff, desktop_share_session_quick};
use app::launch_route::desktop_take_launch_route;
use app::lifecycle_cleanup::maybe_start_lifecycle_cleanup_loop;
use app::session_access::{
    desktop_get_session_detail, desktop_get_session_raw, desktop_list_repos, desktop_list_sessions,
};
#[cfg(test)]
use app::session_access::{
    force_refresh_discovery_tools, map_link_type, normalize_session_body_to_hail_jsonl,
    session_summary_from_local_row,
};
pub(crate) use app::session_access::{
    load_normalized_session_body, session_summary_from_local_row_with_score,
};
#[cfg(test)]
use app::session_query::{SearchMode, build_local_filter_with_mode};
use app::session_summary::{
    desktop_get_session_summary, desktop_regenerate_session_summary, desktop_summary_batch_run,
    desktop_summary_batch_status, maybe_start_summary_batch_on_app_start,
    migrate_summary_storage_backend,
};
#[cfg(test)]
use app::vector::{
    build_vector_chunks_for_session, cosine_similarity, extract_vector_lines,
    persist_vector_index_failure_snapshot, rebuild_vector_index_blocking,
    validate_vector_preflight_ready,
};
use app::vector::{
    desktop_search_sessions_vector, desktop_vector_index_rebuild, desktop_vector_index_status,
    desktop_vector_install_model, desktop_vector_preflight, vector_embed_endpoint,
    vector_embed_model, vector_preflight_for_runtime,
};
#[cfg(test)]
use app::{
    launch_route::normalize_launch_route,
    lifecycle_cleanup::run_desktop_lifecycle_cleanup_once_with_db,
};
use opensession_api::{
    CapabilitiesResponse, DESKTOP_IPC_CONTRACT_VERSION, DesktopApiError, DesktopChangeReaderScope,
    DesktopChangeReaderVoiceProvider, DesktopContractVersionResponse, DesktopLifecycleCleanupState,
    DesktopLifecycleCleanupStatusResponse, DesktopRuntimeChangeReaderSettings,
    DesktopRuntimeChangeReaderVoiceSettings, DesktopRuntimeLifecycleSettings,
    DesktopRuntimeSettingsResponse, DesktopRuntimeSettingsUpdateRequest,
    DesktopRuntimeSummaryBatchSettings, DesktopRuntimeSummaryPromptSettings,
    DesktopRuntimeSummaryProviderSettings, DesktopRuntimeSummaryResponseSettings,
    DesktopRuntimeSummarySettings, DesktopRuntimeSummaryStorageSettings,
    DesktopRuntimeSummaryUiConstraints, DesktopRuntimeVectorSearchSettings,
    DesktopSessionListQuery, DesktopSummaryBatchExecutionMode, DesktopSummaryBatchScope,
    DesktopSummaryOutputShape, DesktopSummaryProviderDetectResponse, DesktopSummaryProviderId,
    DesktopSummaryProviderTransport, DesktopSummaryResponseStyle, DesktopSummarySourceMode,
    DesktopSummaryStorageBackend, DesktopSummaryTriggerMode, DesktopVectorChunkingMode,
    DesktopVectorSearchGranularity, DesktopVectorSearchProvider,
    oauth::{AuthProvidersResponse, OAuthProviderInfo},
};
use opensession_local_db::{LifecycleCleanupJobRow, LocalDb};
use opensession_runtime_config::{
    ChangeReaderScope, ChangeReaderVoiceProvider, DaemonConfig, LifecycleSettings,
    SessionDefaultView, SummaryBatchExecutionMode as RuntimeSummaryBatchExecutionMode,
    SummaryBatchScope as RuntimeSummaryBatchScope, SummaryOutputShape, SummaryProvider,
    SummaryResponseStyle, SummarySourceMode, SummaryStorageBackend, SummaryTriggerMode,
    VectorChunkingMode, VectorSearchGranularity, VectorSearchProvider,
};
use opensession_summary::validate_summary_prompt_template;
use serde_json::json;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

pub(crate) type DesktopApiResult<T> = Result<T, DesktopApiError>;

const CHANGE_READER_MAX_EVENTS: usize = 180;
const CHANGE_READER_MAX_LINE_CHARS: usize = 220;
static LIFECYCLE_CLEANUP_LOOP_STARTED: LazyLock<Mutex<bool>> = LazyLock::new(|| Mutex::new(false));

pub(crate) fn desktop_error(
    code: &str,
    status: u16,
    message: impl Into<String>,
    details: Option<serde_json::Value>,
) -> DesktopApiError {
    DesktopApiError {
        code: code.to_string(),
        status,
        message: message.into(),
        details,
    }
}

pub(crate) fn enum_label<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value)
        .ok()
        .map(|raw| raw.trim_matches('"').to_string())
        .filter(|raw| !raw.trim().is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

fn open_local_db() -> DesktopApiResult<LocalDb> {
    let custom_path = std::env::var("OPENSESSION_LOCAL_DB_PATH")
        .ok()
        .and_then(|value| {
            let trimmed = value.trim().to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        })
        .map(PathBuf::from);

    let db = if let Some(path) = custom_path {
        LocalDb::open_path(&path)
    } else {
        LocalDb::open()
    };

    db.map_err(|error| {
        desktop_error(
            "desktop.local_db_open_failed",
            500,
            "failed to open local database",
            Some(json!({ "cause": error.to_string() })),
        )
    })
}

fn runtime_config_path() -> DesktopApiResult<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|error| {
            desktop_error(
                "desktop.runtime_config_home_unavailable",
                500,
                "failed to resolve home directory for runtime config",
                Some(json!({ "cause": error.to_string() })),
            )
        })?;
    Ok(PathBuf::from(home)
        .join(".config")
        .join("opensession")
        .join(opensession_runtime_config::CONFIG_FILE_NAME))
}

fn load_runtime_config() -> DesktopApiResult<DaemonConfig> {
    let path = runtime_config_path()?;
    if !path.exists() {
        return Ok(DaemonConfig::default());
    }
    let content = std::fs::read_to_string(&path).map_err(|error| {
        desktop_error(
            "desktop.runtime_config_read_failed",
            500,
            "failed to read runtime config",
            Some(json!({ "cause": error.to_string(), "path": path })),
        )
    })?;
    toml::from_str(&content).map_err(|error| {
        desktop_error(
            "desktop.runtime_config_parse_failed",
            500,
            "failed to parse runtime config",
            Some(json!({ "cause": error.to_string(), "path": path })),
        )
    })
}

fn save_runtime_config(config: &DaemonConfig) -> DesktopApiResult<()> {
    let path = runtime_config_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            desktop_error(
                "desktop.runtime_config_write_failed",
                500,
                "failed to create runtime config directory",
                Some(json!({ "cause": error.to_string(), "path": parent })),
            )
        })?;
    }

    let body = toml::to_string_pretty(config).map_err(|error| {
        desktop_error(
            "desktop.runtime_config_serialize_failed",
            500,
            "failed to serialize runtime config",
            Some(json!({ "cause": error.to_string() })),
        )
    })?;

    std::fs::write(&path, body).map_err(|error| {
        desktop_error(
            "desktop.runtime_config_write_failed",
            500,
            "failed to write runtime config",
            Some(json!({ "cause": error.to_string(), "path": path })),
        )
    })
}

fn map_lifecycle_cleanup_state(raw: &str) -> DesktopLifecycleCleanupState {
    match raw {
        "running" => DesktopLifecycleCleanupState::Running,
        "complete" => DesktopLifecycleCleanupState::Complete,
        "failed" => DesktopLifecycleCleanupState::Failed,
        _ => DesktopLifecycleCleanupState::Idle,
    }
}

fn desktop_lifecycle_cleanup_status_from_db(
    db: &LocalDb,
) -> DesktopApiResult<DesktopLifecycleCleanupStatusResponse> {
    let row = db.get_lifecycle_cleanup_job().map_err(|error| {
        desktop_error(
            "desktop.lifecycle_cleanup_status_failed",
            500,
            "failed to read lifecycle cleanup status",
            Some(json!({ "cause": error.to_string() })),
        )
    })?;

    let Some(row) = row else {
        return Ok(DesktopLifecycleCleanupStatusResponse {
            state: DesktopLifecycleCleanupState::Idle,
            deleted_sessions: 0,
            deleted_summaries: 0,
            message: None,
            started_at: None,
            finished_at: None,
        });
    };

    Ok(DesktopLifecycleCleanupStatusResponse {
        state: map_lifecycle_cleanup_state(&row.status),
        deleted_sessions: row.deleted_sessions,
        deleted_summaries: row.deleted_summaries,
        message: row.message,
        started_at: row.started_at,
        finished_at: row.finished_at,
    })
}

fn set_lifecycle_cleanup_job_snapshot(
    db: &LocalDb,
    payload: LifecycleCleanupJobRow,
) -> DesktopApiResult<()> {
    db.set_lifecycle_cleanup_job(&payload).map_err(|error| {
        desktop_error(
            "desktop.lifecycle_cleanup_status_failed",
            500,
            "failed to persist lifecycle cleanup status",
            Some(json!({ "cause": error.to_string() })),
        )
    })
}

fn map_summary_provider_id_from_runtime(value: &SummaryProvider) -> DesktopSummaryProviderId {
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

fn map_change_reader_scope_from_runtime(value: &ChangeReaderScope) -> DesktopChangeReaderScope {
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
fn desktop_get_capabilities() -> CapabilitiesResponse {
    CapabilitiesResponse::for_runtime(false, false)
}

#[tauri::command]
fn desktop_get_auth_providers() -> AuthProvidersResponse {
    AuthProvidersResponse {
        email_password: false,
        oauth: Vec::<OAuthProviderInfo>::new(),
    }
}

#[tauri::command]
fn desktop_get_contract_version() -> DesktopContractVersionResponse {
    DesktopContractVersionResponse {
        version: DESKTOP_IPC_CONTRACT_VERSION.to_string(),
    }
}

#[tauri::command]
fn desktop_get_docs_markdown() -> String {
    include_str!("../../../docs.md").to_string()
}

#[tauri::command]
fn desktop_get_runtime_settings() -> DesktopApiResult<DesktopRuntimeSettingsResponse> {
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
fn desktop_update_runtime_settings(
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
fn desktop_detect_summary_provider() -> DesktopSummaryProviderDetectResponse {
    if let Some(profile) = opensession_summary::detect_summary_provider() {
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

#[tauri::command]
fn desktop_lifecycle_cleanup_status() -> DesktopApiResult<DesktopLifecycleCleanupStatusResponse> {
    let db = open_local_db()?;
    desktop_lifecycle_cleanup_status_from_db(&db)
}

fn main() {
    maybe_start_summary_batch_on_app_start();
    maybe_start_lifecycle_cleanup_loop();

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            desktop_get_capabilities,
            desktop_get_auth_providers,
            desktop_get_contract_version,
            desktop_get_docs_markdown,
            desktop_get_runtime_settings,
            desktop_update_runtime_settings,
            desktop_lifecycle_cleanup_status,
            desktop_summary_batch_status,
            desktop_summary_batch_run,
            desktop_detect_summary_provider,
            desktop_vector_preflight,
            desktop_vector_install_model,
            desktop_vector_index_rebuild,
            desktop_vector_index_status,
            desktop_search_sessions_vector,
            desktop_list_sessions,
            desktop_list_repos,
            desktop_get_session_detail,
            desktop_get_session_raw,
            desktop_get_session_summary,
            desktop_regenerate_session_summary,
            desktop_read_session_changes,
            desktop_ask_session_changes,
            desktop_change_reader_tts,
            desktop_take_launch_route,
            desktop_build_handoff,
            desktop_share_session_quick
        ])
        .run(tauri::generate_context!())
        .expect("failed to run opensession desktop app");
}

#[cfg(test)]
mod tests {
    use super::{
        DesktopSessionListQuery, SearchMode, build_local_filter_with_mode,
        build_vector_chunks_for_session, cosine_similarity, desktop_ask_session_changes,
        desktop_change_reader_tts, desktop_get_contract_version, desktop_get_runtime_settings,
        desktop_get_session_detail, desktop_get_session_raw, desktop_list_sessions,
        desktop_read_session_changes, desktop_summary_batch_run, desktop_summary_batch_status,
        desktop_update_runtime_settings, extract_vector_lines, force_refresh_discovery_tools,
        map_link_type, normalize_launch_route, normalize_session_body_to_hail_jsonl,
        require_non_empty_request_field, session_summary_from_local_row,
        validate_vector_preflight_ready,
    };
    use crate::app::handoff::{
        artifact_path_for_hash, build_handoff_artifact_record, canonicalize_summaries,
        desktop_share_session_quick, parse_cli_quick_share_response, validate_pin_alias,
    };
    use crate::app::session_query::split_search_mode;
    use opensession_api::{
        DesktopChangeQuestionRequest, DesktopChangeReadRequest, DesktopChangeReaderScope,
        DesktopChangeReaderTtsRequest, DesktopChangeReaderVoiceProvider,
        DesktopLifecycleCleanupState, DesktopQuickShareRequest,
        DesktopRuntimeChangeReaderSettingsUpdate, DesktopRuntimeChangeReaderVoiceSettingsUpdate,
        DesktopRuntimeLifecycleSettingsUpdate, DesktopRuntimeSettingsUpdateRequest,
        DesktopRuntimeSummaryBatchSettingsUpdate, DesktopRuntimeSummaryPromptSettingsUpdate,
        DesktopRuntimeSummaryProviderSettingsUpdate, DesktopRuntimeSummaryResponseSettingsUpdate,
        DesktopRuntimeSummarySettingsUpdate, DesktopRuntimeSummaryStorageSettingsUpdate,
        DesktopSummaryBatchExecutionMode, DesktopSummaryBatchScope, DesktopSummaryBatchState,
        DesktopSummaryOutputShape, DesktopSummaryProviderId, DesktopSummaryResponseStyle,
        DesktopSummarySourceMode, DesktopSummaryStorageBackend, DesktopSummaryTriggerMode,
        DesktopVectorInstallState, DesktopVectorPreflightResponse, DesktopVectorSearchProvider,
    };
    use opensession_core::handoff::HandoffSummary;
    use opensession_core::trace::{Agent, Content, Event, EventType, Session as HailSession};
    use opensession_git_native::{
        NativeGitStorage, SUMMARY_LEDGER_REF, SessionSummaryLedgerRecord,
    };
    use opensession_local_db::git::GitContext;
    use opensession_local_db::{LocalDb, VectorIndexJobRow};
    use opensession_runtime_config::DaemonConfig;
    use opensession_summary::DEFAULT_SUMMARY_PROMPT_TEMPLATE_V2;
    use serde_json::json;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::{LazyLock, Mutex};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    static TEST_ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    fn set_env_for_test(key: &str, value: impl AsRef<std::ffi::OsStr>) {
        // SAFETY: desktop tests mutate process environment only while holding TEST_ENV_LOCK,
        // which serializes the affected test cases and avoids concurrent environment access.
        unsafe { std::env::set_var(key, value) };
    }

    fn remove_env_for_test(key: &str) {
        // SAFETY: desktop tests mutate process environment only while holding TEST_ENV_LOCK,
        // which serializes the affected test cases and avoids concurrent environment access.
        unsafe { std::env::remove_var(key) };
    }

    struct EnvVarGuard {
        key: String,
        previous: Option<String>,
    }

    impl EnvVarGuard {
        fn set(key: &str, value: impl AsRef<std::ffi::OsStr>) -> Self {
            let previous = std::env::var(key).ok();
            set_env_for_test(key, value);
            Self {
                key: key.to_string(),
                previous,
            }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(previous) = self.previous.take() {
                set_env_for_test(&self.key, previous);
            } else {
                remove_env_for_test(&self.key);
            }
        }
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be monotonic")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}-{unique}"));
        std::fs::create_dir_all(&path).expect("create test temp dir");
        path
    }

    fn init_test_git_repo(path: &Path) {
        let output = Command::new("git")
            .arg("init")
            .arg("--quiet")
            .arg(path)
            .output()
            .expect("run git init");
        assert!(
            output.status.success(),
            "git init failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn build_test_session(session_id: &str) -> HailSession {
        let mut session = HailSession::new(
            session_id.to_string(),
            Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );
        session.events.push(Event {
            event_id: "evt-1".to_string(),
            timestamp: chrono::Utc::now(),
            event_type: EventType::UserMessage,
            task_id: None,
            content: Content::text("desktop test message"),
            duration_ms: None,
            attributes: std::collections::HashMap::new(),
        });
        session.recompute_stats();
        session
    }

    fn summary_settings_update_with_backend(
        backend: DesktopSummaryStorageBackend,
    ) -> DesktopRuntimeSummarySettingsUpdate {
        DesktopRuntimeSummarySettingsUpdate {
            provider: DesktopRuntimeSummaryProviderSettingsUpdate {
                id: DesktopSummaryProviderId::Disabled,
                endpoint: String::new(),
                model: String::new(),
            },
            prompt: DesktopRuntimeSummaryPromptSettingsUpdate {
                template: DEFAULT_SUMMARY_PROMPT_TEMPLATE_V2.to_string(),
            },
            response: DesktopRuntimeSummaryResponseSettingsUpdate {
                style: DesktopSummaryResponseStyle::Standard,
                shape: DesktopSummaryOutputShape::Layered,
            },
            storage: DesktopRuntimeSummaryStorageSettingsUpdate {
                trigger: DesktopSummaryTriggerMode::OnSessionSave,
                backend,
            },
            source_mode: DesktopSummarySourceMode::SessionOnly,
            batch: DesktopRuntimeSummaryBatchSettingsUpdate {
                execution_mode: DesktopSummaryBatchExecutionMode::Manual,
                scope: DesktopSummaryBatchScope::All,
                recent_days: 30,
            },
        }
    }

    fn sample_row() -> opensession_local_db::LocalSessionRow {
        opensession_local_db::LocalSessionRow {
            id: "s1".to_string(),
            source_path: Some("/tmp/s1.hail.jsonl".to_string()),
            sync_status: "local_only".to_string(),
            last_synced_at: None,
            user_id: None,
            nickname: None,
            team_id: Some("personal".to_string()),
            tool: "codex".to_string(),
            agent_provider: Some("openai".to_string()),
            agent_model: Some("gpt-5".to_string()),
            title: Some("Sample".to_string()),
            description: Some("sample session".to_string()),
            tags: Some("tag1,tag2".to_string()),
            created_at: "2026-03-03T00:00:00Z".to_string(),
            uploaded_at: None,
            message_count: 5,
            user_message_count: 2,
            task_count: 1,
            event_count: 20,
            duration_seconds: 120,
            total_input_tokens: 100,
            total_output_tokens: 40,
            git_remote: None,
            git_branch: None,
            git_commit: None,
            git_repo_name: None,
            pr_number: None,
            pr_url: None,
            working_directory: None,
            files_modified: None,
            files_read: None,
            has_errors: false,
            max_active_agents: 1,
            is_auxiliary: false,
        }
    }

    #[test]
    fn list_filter_defaults_page_and_per_page() {
        let (filter, page, per_page, mode) =
            build_local_filter_with_mode(DesktopSessionListQuery::default());
        assert_eq!(page, 1);
        assert_eq!(per_page, 20);
        assert_eq!(mode, SearchMode::Keyword);
        assert_eq!(filter.limit, Some(20));
        assert_eq!(filter.offset, Some(0));
        assert!(filter.exclude_low_signal);
    }

    #[test]
    fn list_filter_parses_sort_and_range_values() {
        let (filter, page, per_page, mode) =
            build_local_filter_with_mode(DesktopSessionListQuery {
                page: Some("2".to_string()),
                per_page: Some("30".to_string()),
                search: Some("fix".to_string()),
                tool: Some("codex".to_string()),
                git_repo_name: Some("org/repo".to_string()),
                sort: Some("popular".to_string()),
                time_range: Some("7d".to_string()),
                force_refresh: None,
            });
        assert_eq!(page, 2);
        assert_eq!(per_page, 30);
        assert_eq!(mode, SearchMode::Keyword);
        assert_eq!(filter.search.as_deref(), Some("fix"));
        assert_eq!(filter.tool.as_deref(), Some("codex"));
        assert_eq!(filter.git_repo_name.as_deref(), Some("org/repo"));
        assert_eq!(filter.offset, Some(30));
    }

    #[test]
    fn split_search_mode_detects_vector_prefix() {
        let (query, mode) = split_search_mode(Some("vector: auth regression".to_string()));
        assert_eq!(query.as_deref(), Some("auth regression"));
        assert_eq!(mode, SearchMode::Vector);

        let (query, mode) = split_search_mode(Some("fix parser".to_string()));
        assert_eq!(query.as_deref(), Some("fix parser"));
        assert_eq!(mode, SearchMode::Keyword);
    }

    #[test]
    fn list_filter_with_mode_keeps_vector_query_text() {
        let (filter, page, per_page, mode) =
            build_local_filter_with_mode(DesktopSessionListQuery {
                search: Some("vec: paging bug".to_string()),
                ..DesktopSessionListQuery::default()
            });
        assert_eq!(page, 1);
        assert_eq!(per_page, 20);
        assert_eq!(mode, SearchMode::Vector);
        assert_eq!(filter.search.as_deref(), Some("paging bug"));
    }

    #[test]
    fn desktop_list_sessions_hides_low_signal_metadata_only_rows() {
        let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
        let temp_root = unique_temp_dir("opensession-desktop-low-signal-filter");
        let db_path = temp_root.join("local.db");
        let source_path = temp_root
            .join(".claude")
            .join("projects")
            .join("fixture")
            .join("metadata-only.jsonl");
        let _db_env = EnvVarGuard::set("OPENSESSION_LOCAL_DB_PATH", db_path.as_os_str());

        std::fs::create_dir_all(
            source_path
                .parent()
                .expect("metadata-only source parent must exist"),
        )
        .expect("create metadata-only source dir");
        std::fs::write(
            &source_path,
            r#"{"type":"file-history-snapshot","files":[]}"#,
        )
        .expect("write metadata-only source fixture");

        let session = HailSession::new(
            "metadata-only-session".to_string(),
            Agent {
                provider: "anthropic".to_string(),
                model: "claude-3-5-sonnet".to_string(),
                tool: "claude-code".to_string(),
                tool_version: None,
            },
        );
        let db = LocalDb::open_path(&db_path).expect("open isolated local db");
        db.upsert_local_session(
            &session,
            source_path
                .to_str()
                .expect("metadata-only source path must be valid utf-8"),
            &GitContext::default(),
        )
        .expect("upsert metadata-only local session");

        let listed = desktop_list_sessions(None).expect("list sessions");
        assert!(
            !listed
                .sessions
                .iter()
                .any(|row| row.id == "metadata-only-session"),
            "metadata-only sessions must be excluded from the default desktop session list",
        );

        let _ = std::fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn desktop_list_sessions_force_refresh_reindexes_discovered_sessions() {
        let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
        let temp_home = unique_temp_dir("opensession-desktop-force-refresh-home");
        let temp_db = unique_temp_dir("opensession-desktop-force-refresh-db");
        let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());
        let _codex_home_env = EnvVarGuard::set("CODEX_HOME", temp_home.join(".codex").as_os_str());
        let _db_env = EnvVarGuard::set(
            "OPENSESSION_LOCAL_DB_PATH",
            temp_db.join("local.db").as_os_str(),
        );

        let session_jsonl = [
            r#"{"timestamp":"2026-03-05T00:00:00.097Z","type":"session_meta","payload":{"id":"force-refresh-session","timestamp":"2026-03-05T00:00:00.075Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.94.0"}}"#,
            r#"{"timestamp":"2026-03-05T00:00:00.119Z","type":"event_msg","payload":{"type":"user_message","message":"force refresh regression fixture"}}"#,
            r#"{"timestamp":"2026-03-05T00:00:01.000Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"done"}]}}"#,
        ]
        .join("\n");
        let discovered_path = temp_home
            .join(".codex")
            .join("sessions")
            .join("2026")
            .join("03")
            .join("05")
            .join("rollout-force-refresh-session.jsonl");
        std::fs::create_dir_all(
            discovered_path
                .parent()
                .expect("session discovery parent must exist"),
        )
        .expect("create session discovery dir");
        std::fs::write(&discovered_path, session_jsonl).expect("write discovered session");

        let before = desktop_list_sessions(None).expect("list sessions before force refresh");
        assert!(
            !before
                .sessions
                .iter()
                .any(|row| row.id == "force-refresh-session"),
            "session should not exist in DB before force refresh reindex"
        );

        let after = desktop_list_sessions(Some(DesktopSessionListQuery {
            force_refresh: Some(true),
            ..DesktopSessionListQuery::default()
        }))
        .expect("list sessions after force refresh");
        assert!(
            after
                .sessions
                .iter()
                .any(|row| row.id == "force-refresh-session"),
            "force refresh should reindex discovered session"
        );

        let _ = std::fs::remove_dir_all(&temp_home);
        let _ = std::fs::remove_dir_all(&temp_db);
    }

    #[test]
    fn force_refresh_discovery_tools_skip_cursor_for_fast_path() {
        let tools = force_refresh_discovery_tools();
        assert!(tools.contains(&"codex"));
        assert!(!tools.contains(&"cursor"));
    }

    fn ready_vector_preflight_fixture() -> DesktopVectorPreflightResponse {
        DesktopVectorPreflightResponse {
            provider: DesktopVectorSearchProvider::Ollama,
            endpoint: "http://127.0.0.1:11434".to_string(),
            model: "bge-m3".to_string(),
            ollama_reachable: true,
            model_installed: true,
            install_state: DesktopVectorInstallState::Ready,
            progress_pct: 100,
            message: Some("model is installed and ready".to_string()),
        }
    }

    #[test]
    fn validate_vector_preflight_allows_rebuild_when_vector_disabled() {
        let preflight = ready_vector_preflight_fixture();
        let result = validate_vector_preflight_ready(&preflight, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_vector_preflight_requires_enabled_for_search_path() {
        let preflight = ready_vector_preflight_fixture();
        let err = validate_vector_preflight_ready(&preflight, false, true)
            .expect_err("search path should require vector enabled");
        assert_eq!(err.code, "desktop.vector_search_disabled");
        assert_eq!(err.status, 422);
    }

    #[test]
    fn persist_vector_index_failure_snapshot_preserves_progress() {
        let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
        let temp_db = unique_temp_dir("opensession-desktop-vector-failure-snapshot");
        let db = LocalDb::open_path(&temp_db.join("local.db")).expect("open local db");

        db.set_vector_index_job(&VectorIndexJobRow {
            status: "running".to_string(),
            processed_sessions: 7,
            total_sessions: 42,
            message: Some("indexing session-7".to_string()),
            started_at: Some("2026-03-06T00:00:00Z".to_string()),
            finished_at: None,
        })
        .expect("seed running vector job");

        let error = super::desktop_error(
            "desktop.vector_search_unavailable",
            422,
            "vector search endpoint returned HTTP 404",
            Some(json!({
                "endpoint": "http://127.0.0.1:11434/api/embeddings",
                "status": 404,
                "body": "{\"error\":\"model 'bge-m3' not found\"}",
                "batch_endpoint": "http://127.0.0.1:11434/api/embed",
                "batch_status": 400,
                "batch_body": "{\"error\":\"bad request\"}",
                "model": "bge-m3",
                "hint": "verify embedding model exists in local ollama"
            })),
        );
        super::persist_vector_index_failure_snapshot(&db, &error)
            .expect("persist vector failure snapshot");

        let snapshot = db
            .get_vector_index_job()
            .expect("read vector job")
            .expect("vector job should exist");
        assert_eq!(snapshot.status, "failed");
        assert_eq!(snapshot.processed_sessions, 7);
        assert_eq!(snapshot.total_sessions, 42);
        assert_eq!(
            snapshot.message.as_deref(),
            Some(
                "vector search endpoint returned HTTP 404\nReason: model 'bge-m3' not found\nHTTP: 404\nEndpoint: http://127.0.0.1:11434/api/embeddings\nBatch reason: bad request\nBatch HTTP: 400\nBatch endpoint: http://127.0.0.1:11434/api/embed\nModel: bge-m3\nAction: verify embedding model exists in local ollama"
            )
        );
        assert_eq!(snapshot.started_at.as_deref(), Some("2026-03-06T00:00:00Z"));
        assert!(snapshot.finished_at.is_some());

        let _ = std::fs::remove_dir_all(&temp_db);
    }

    #[test]
    fn rebuild_vector_index_blocking_continues_after_embedding_failures() {
        let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
        let temp_root = unique_temp_dir("opensession-desktop-vector-failure-continue");
        let db = LocalDb::open_path(&temp_root.join("local.db")).expect("open local db");

        for session_id in ["vector-failure-a", "vector-failure-b"] {
            let session = build_test_session(session_id);
            let source_path = temp_root.join(format!("{session_id}.jsonl"));
            std::fs::write(
                &source_path,
                session.to_jsonl().expect("serialize session jsonl"),
            )
            .expect("write session source");
            db.upsert_local_session(
                &session,
                source_path
                    .to_str()
                    .expect("session source path must be valid utf-8"),
                &GitContext::default(),
            )
            .expect("upsert local session");
        }

        let mut runtime = DaemonConfig::default();
        runtime.vector_search.enabled = true;
        runtime.vector_search.endpoint = "http://127.0.0.1:1".to_string();
        runtime.vector_search.model = "bge-m3".to_string();

        super::rebuild_vector_index_blocking(&db, &runtime)
            .expect("skippable embedding failures should not abort rebuild");

        let snapshot = db
            .get_vector_index_job()
            .expect("read vector job")
            .expect("vector job should exist");
        assert_eq!(snapshot.status, "complete");
        assert_eq!(snapshot.processed_sessions, 2);
        assert_eq!(snapshot.total_sessions, 2);
        assert!(
            snapshot
                .message
                .as_deref()
                .is_some_and(|message| message.contains("2 failed"))
        );
        assert!(
            db.list_recent_vector_chunks_for_model("bge-m3", 10)
                .expect("list vector chunks")
                .is_empty(),
            "failed sessions should not leave partial vector chunks behind"
        );

        let _ = std::fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn cosine_similarity_handles_basic_cases() {
        let same = cosine_similarity(&[1.0, 0.0, 1.0], &[1.0, 0.0, 1.0]);
        assert!((same - 1.0).abs() < 1e-6);

        let orthogonal = cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]);
        assert!(orthogonal.abs() < 1e-6);

        let mismatch = cosine_similarity(&[1.0, 2.0], &[1.0]);
        assert_eq!(mismatch, 0.0);
    }

    #[test]
    fn extract_vector_lines_preserves_dot_line_tokens() {
        let mut session = build_test_session("vector-lines");
        session.events.push(Event {
            event_id: "evt-dot".to_string(),
            timestamp: chrono::Utc::now(),
            event_type: EventType::AgentMessage,
            task_id: None,
            content: Content::text(".\nkeep-this-line"),
            duration_ms: None,
            attributes: std::collections::HashMap::new(),
        });
        let lines = extract_vector_lines(&session);
        assert!(lines.iter().any(|line| line == "."));
        assert!(lines.iter().any(|line| line.contains("keep-this-line")));
    }

    #[test]
    fn build_vector_chunks_applies_overlap_rules() {
        let mut session = build_test_session("vector-chunks");
        session.events.push(Event {
            event_id: "evt-overlap".to_string(),
            timestamp: chrono::Utc::now(),
            event_type: EventType::AgentMessage,
            task_id: None,
            content: Content::text("l1\nl2\nl3"),
            duration_ms: None,
            attributes: std::collections::HashMap::new(),
        });
        let mut runtime = DaemonConfig::default();
        runtime.vector_search.chunking_mode =
            opensession_runtime_config::VectorChunkingMode::Manual;
        runtime.vector_search.chunk_size_lines = 2;
        runtime.vector_search.chunk_overlap_lines = 1;
        let chunks = build_vector_chunks_for_session(&session, "source-hash", &runtime);
        assert!(chunks.len() >= 2);
        assert_eq!(chunks[0].start_line, 1);
        assert_eq!(chunks[0].end_line, 2);
        assert_eq!(chunks[1].start_line, 2);
    }

    #[test]
    fn build_vector_chunks_auto_tunes_for_small_session() {
        let mut session = build_test_session("vector-chunks-auto");
        session.events.push(Event {
            event_id: "evt-auto".to_string(),
            timestamp: chrono::Utc::now(),
            event_type: EventType::AgentMessage,
            task_id: None,
            content: Content::text("a\nb\nc\nd\ne\nf"),
            duration_ms: None,
            attributes: std::collections::HashMap::new(),
        });
        let runtime = DaemonConfig::default();
        let chunks = build_vector_chunks_for_session(&session, "source-hash", &runtime);
        assert_eq!(chunks[0].start_line, 1);
        assert_eq!(chunks[0].end_line, 7);
    }

    #[test]
    fn normalize_launch_route_accepts_relative_session_path() {
        assert_eq!(
            normalize_launch_route("/sessions?git_repo_name=org%2Frepo"),
            Some("/sessions?git_repo_name=org%2Frepo".to_string())
        );
    }

    #[test]
    fn normalize_launch_route_rejects_invalid_values() {
        assert_eq!(normalize_launch_route(""), None);
        assert_eq!(
            normalize_launch_route("https://opensession.io/sessions"),
            None
        );
        assert_eq!(normalize_launch_route("//sessions"), None);
    }

    #[test]
    fn local_row_uses_created_at_when_uploaded_at_missing() {
        let summary = session_summary_from_local_row(sample_row());
        assert_eq!(summary.uploaded_at, "2026-03-03T00:00:00Z");
        assert_eq!(
            summary.score_plugin,
            opensession_core::scoring::DEFAULT_SCORE_PLUGIN
        );
    }

    #[test]
    fn unknown_link_type_falls_back_to_handoff() {
        let link = map_link_type("unknown-link");
        assert!(matches!(link, opensession_api::LinkType::Handoff));
    }

    #[test]
    fn normalize_session_body_preserves_hail_jsonl() {
        let hail_jsonl = [
            r#"{"type":"header","version":"hail-1.0.0","session_id":"hail-1","agent":{"provider":"openai","model":"gpt-5","tool":"codex"},"context":{"title":"Title","description":"Desc","tags":[],"created_at":"2026-03-03T00:00:00Z","updated_at":"2026-03-03T00:00:00Z","related_session_ids":[],"attributes":{}}}"#,
            r#"{"type":"event","event_id":"e1","timestamp":"2026-03-03T00:00:00Z","event_type":{"type":"UserMessage"},"content":{"blocks":[{"type":"Text","text":"hello"}]},"attributes":{}}"#,
            r#"{"type":"stats","event_count":1,"message_count":1,"tool_call_count":0,"task_count":0,"duration_seconds":0,"total_input_tokens":0,"total_output_tokens":0,"user_message_count":1,"files_changed":0,"lines_added":0,"lines_removed":0}"#,
        ]
        .join("\n");

        let normalized = normalize_session_body_to_hail_jsonl(&hail_jsonl, None)
            .expect("hail JSONL should normalize");
        let parsed = HailSession::from_jsonl(&normalized).expect("must remain valid hail");
        assert_eq!(parsed.session_id, "hail-1");
    }

    #[test]
    fn normalize_session_body_converts_claude_jsonl() {
        let claude_jsonl = r#"{"type":"user","uuid":"u1","sessionId":"claude-1","timestamp":"2026-03-03T00:00:00Z","message":{"role":"user","content":"hello from claude"},"cwd":"/tmp/project","gitBranch":"main"}"#;

        let normalized = normalize_session_body_to_hail_jsonl(
            claude_jsonl,
            Some("/tmp/70dafb43-dbdd-4009-beb0-b6ac2bd9c4d1.jsonl"),
        )
        .expect("claude JSONL should parse into HAIL");
        let parsed = HailSession::from_jsonl(&normalized).expect("must be valid hail");
        assert_eq!(parsed.session_id, "claude-1");
        assert_eq!(parsed.agent.tool, "claude-code");
        assert!(!parsed.events.is_empty());
    }

    #[test]
    fn normalize_session_body_prefers_source_path_parser_over_extension_fallback() {
        let temp_root = unique_temp_dir("opensession-desktop-normalize-source-path");
        let source_path = temp_root
            .join(".claude")
            .join("projects")
            .join("fixture")
            .join("f0639ede-3aac-4f67-a979-b175ea5f9557.jsonl");
        std::fs::create_dir_all(
            source_path
                .parent()
                .expect("claude fixture parent directory must exist"),
        )
        .expect("create claude fixture directory");

        let snapshot_only = r#"{"type":"file-history-snapshot","files":[]}"#;
        std::fs::write(&source_path, snapshot_only).expect("write claude snapshot fixture");

        let normalized = normalize_session_body_to_hail_jsonl(
            snapshot_only,
            Some(
                source_path
                    .to_str()
                    .expect("fixture source path must be valid utf-8"),
            ),
        )
        .expect("source-path parser should normalize claude snapshot fixture");
        let parsed = HailSession::from_jsonl(&normalized).expect("must be valid hail");
        assert_eq!(parsed.agent.tool, "claude-code");
        assert_eq!(parsed.session_id, "f0639ede-3aac-4f67-a979-b175ea5f9557");

        let _ = std::fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn desktop_contract_version_matches_shared_constant() {
        let payload = desktop_get_contract_version();
        assert_eq!(
            payload.version,
            opensession_api::DESKTOP_IPC_CONTRACT_VERSION
        );
    }

    #[test]
    fn parse_cli_quick_share_response_decodes_expected_fields() {
        let stdout = r#"{
  "uri": "os://src/git/cmVtb3Rl/ref/refs%2Fheads%2Fmain/path/sessions%2Fa.jsonl",
  "source_uri": "os://src/local/abc123",
  "remote": "https://github.com/org/repo.git",
  "push_cmd": "git push origin refs/opensession/branches/bWFpbg:refs/opensession/branches/bWFpbg",
  "quick": true,
  "pushed": true,
  "auto_push_consent": true
}"#;
        let parsed = parse_cli_quick_share_response(stdout).expect("parse quick-share payload");
        assert_eq!(parsed.source_uri, "os://src/local/abc123");
        assert_eq!(
            parsed.shared_uri,
            "os://src/git/cmVtb3Rl/ref/refs%2Fheads%2Fmain/path/sessions%2Fa.jsonl"
        );
        assert_eq!(parsed.remote, "https://github.com/org/repo.git");
        assert!(parsed.pushed);
        assert!(parsed.auto_push_consent);
    }

    #[test]
    fn desktop_quick_share_rejects_empty_session_id() {
        let err = desktop_share_session_quick(DesktopQuickShareRequest {
            session_id: "   ".to_string(),
            remote: None,
        })
        .expect_err("empty session_id should fail");
        assert_eq!(err.code, "desktop.quick_share_invalid_request");
        assert_eq!(err.status, 400);
    }

    #[test]
    fn handoff_canonicalization_orders_by_session_id() {
        let mut session_b = HailSession::new(
            "session-b".to_string(),
            Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );
        session_b.recompute_stats();
        let mut session_a = HailSession::new(
            "session-a".to_string(),
            Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );
        session_a.recompute_stats();

        let summaries = vec![
            HandoffSummary::from_session(&session_b),
            HandoffSummary::from_session(&session_a),
        ];
        let canonical = canonicalize_summaries(&summaries).expect("canonicalize summaries");
        let first_line = canonical.lines().next().expect("canonical line");
        assert!(first_line.contains("\"source_session_id\":\"session-a\""));
    }

    #[test]
    fn handoff_pin_alias_validation_rejects_spaces() {
        assert!(validate_pin_alias("latest").is_ok());
        assert!(validate_pin_alias("bad alias").is_err());
    }

    #[test]
    fn artifact_path_rejects_invalid_hash() {
        assert!(artifact_path_for_hash(Path::new("/tmp"), "abc").is_err());
    }

    #[test]
    fn desktop_list_detail_raw_flow_uses_isolated_db() {
        let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
        let temp_root = unique_temp_dir("opensession-desktop-list-detail-raw");
        let db_path = temp_root.join("local.db");
        let source_path = temp_root.join("session.hail.jsonl");
        let _db_env = EnvVarGuard::set("OPENSESSION_LOCAL_DB_PATH", db_path.as_os_str());

        let session = build_test_session("desktop-flow-session");
        let session_jsonl = session.to_jsonl().expect("serialize session jsonl");
        std::fs::write(&source_path, &session_jsonl).expect("write session source");

        let db = LocalDb::open_path(&db_path).expect("open isolated local db");
        db.upsert_local_session(
            &session,
            source_path
                .to_str()
                .expect("session source path must be valid utf-8"),
            &GitContext::default(),
        )
        .expect("upsert local session");

        let listed = desktop_list_sessions(None).expect("list sessions");
        assert!(
            listed
                .sessions
                .iter()
                .any(|row| row.id == session.session_id),
            "session list must include inserted session",
        );

        let detail =
            desktop_get_session_detail(session.session_id.clone()).expect("get session detail");
        assert_eq!(detail.summary.id, session.session_id);

        let raw = desktop_get_session_raw(detail.summary.id.clone()).expect("get raw session");
        assert!(
            raw.contains("\"session_id\":\"desktop-flow-session\""),
            "raw session output should include the normalized session id",
        );

        drop(db);
        let _ = std::fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn desktop_runtime_settings_update_persists_values() {
        let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
        let temp_home = unique_temp_dir("opensession-desktop-runtime-home");
        let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());

        let updated = desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
            session_default_view: Some("compressed".to_string()),
            summary: Some(DesktopRuntimeSummarySettingsUpdate {
                provider: DesktopRuntimeSummaryProviderSettingsUpdate {
                    id: DesktopSummaryProviderId::Disabled,
                    endpoint: String::new(),
                    model: String::new(),
                },
                prompt: DesktopRuntimeSummaryPromptSettingsUpdate {
                    template: format!("{DEFAULT_SUMMARY_PROMPT_TEMPLATE_V2}\n# customized"),
                },
                response: DesktopRuntimeSummaryResponseSettingsUpdate {
                    style: DesktopSummaryResponseStyle::Compact,
                    shape: DesktopSummaryOutputShape::Layered,
                },
                storage: DesktopRuntimeSummaryStorageSettingsUpdate {
                    trigger: DesktopSummaryTriggerMode::OnSessionSave,
                    backend: DesktopSummaryStorageBackend::HiddenRef,
                },
                source_mode: DesktopSummarySourceMode::SessionOnly,
                batch: DesktopRuntimeSummaryBatchSettingsUpdate {
                    execution_mode: DesktopSummaryBatchExecutionMode::Manual,
                    scope: DesktopSummaryBatchScope::All,
                    recent_days: 45,
                },
            }),
            vector_search: None,
            change_reader: Some(DesktopRuntimeChangeReaderSettingsUpdate {
                enabled: true,
                scope: DesktopChangeReaderScope::FullContext,
                qa_enabled: true,
                max_context_chars: 18_000,
                voice: DesktopRuntimeChangeReaderVoiceSettingsUpdate {
                    enabled: true,
                    provider: DesktopChangeReaderVoiceProvider::Openai,
                    model: "gpt-4o-mini-tts".to_string(),
                    voice: "alloy".to_string(),
                    api_key: Some("sk-test-key".to_string()),
                },
            }),
            lifecycle: Some(DesktopRuntimeLifecycleSettingsUpdate {
                enabled: true,
                session_ttl_days: 45,
                summary_ttl_days: 60,
                cleanup_interval_secs: 120,
            }),
        })
        .expect("update runtime settings");
        assert_eq!(updated.session_default_view, "compressed");

        let loaded = desktop_get_runtime_settings().expect("load runtime settings");
        assert_eq!(loaded.session_default_view, "compressed");
        assert_eq!(
            loaded.summary.provider.id,
            DesktopSummaryProviderId::Disabled
        );
        assert_eq!(
            loaded.summary.response.style,
            DesktopSummaryResponseStyle::Compact
        );
        assert_eq!(
            loaded.summary.storage.backend,
            DesktopSummaryStorageBackend::HiddenRef
        );
        assert_eq!(
            loaded.summary.source_mode,
            DesktopSummarySourceMode::SessionOnly
        );
        assert_eq!(
            loaded.summary.batch.execution_mode,
            DesktopSummaryBatchExecutionMode::Manual
        );
        assert_eq!(loaded.summary.batch.scope, DesktopSummaryBatchScope::All);
        assert_eq!(loaded.summary.batch.recent_days, 45);
        assert!(loaded.summary.prompt.template.contains("customized"));
        assert!(loaded.change_reader.enabled);
        assert_eq!(
            loaded.change_reader.scope,
            DesktopChangeReaderScope::FullContext
        );
        assert!(loaded.change_reader.qa_enabled);
        assert_eq!(loaded.change_reader.max_context_chars, 18_000);
        assert!(loaded.change_reader.voice.enabled);
        assert_eq!(
            loaded.change_reader.voice.provider,
            DesktopChangeReaderVoiceProvider::Openai
        );
        assert_eq!(loaded.change_reader.voice.model, "gpt-4o-mini-tts");
        assert_eq!(loaded.change_reader.voice.voice, "alloy");
        assert!(loaded.change_reader.voice.api_key_configured);
        assert!(loaded.lifecycle.enabled);
        assert_eq!(loaded.lifecycle.session_ttl_days, 45);
        assert_eq!(loaded.lifecycle.summary_ttl_days, 60);
        assert_eq!(loaded.lifecycle.cleanup_interval_secs, 120);

        let _ = std::fs::remove_dir_all(&temp_home);
    }

    #[test]
    fn desktop_runtime_settings_migrates_summary_storage_local_db_to_hidden_ref() {
        let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
        let temp_home = unique_temp_dir("opensession-desktop-runtime-migrate-home");
        let temp_db = unique_temp_dir("opensession-desktop-runtime-migrate-db");
        let repo_root = unique_temp_dir("opensession-desktop-runtime-migrate-repo");
        let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());
        let _db_env = EnvVarGuard::set(
            "OPENSESSION_LOCAL_DB_PATH",
            temp_db.join("local.db").as_os_str(),
        );
        init_test_git_repo(&repo_root);

        let db = LocalDb::open_path(&temp_db.join("local.db")).expect("open local db");
        let mut session = build_test_session("storage-migrate-local-to-hidden");
        session.context.attributes.insert(
            "cwd".to_string(),
            json!(repo_root.to_string_lossy().to_string()),
        );
        let source_path = repo_root.join("storage-migrate-local-to-hidden.hail.jsonl");
        std::fs::write(
            &source_path,
            session.to_jsonl().expect("serialize session jsonl"),
        )
        .expect("write session source");
        db.upsert_local_session(
            &session,
            source_path
                .to_str()
                .expect("session source path must be valid utf-8"),
            &GitContext::default(),
        )
        .expect("upsert local session");
        db.upsert_session_semantic_summary(&opensession_local_db::SessionSemanticSummaryUpsert {
            session_id: "storage-migrate-local-to-hidden",
            summary_json: r#"{"changes":"migrated","auth_security":"none detected","layer_file_changes":[]}"#,
            generated_at: "2026-03-05T10:00:00Z",
            provider: "codex_exec",
            model: Some("gpt-5"),
            source_kind: "session_signals",
            generation_kind: "provider",
            prompt_fingerprint: Some("migrate-fingerprint"),
            source_details_json: Some(r#"{"source":"session"}"#),
            diff_tree_json: Some(r#"[]"#),
            error: None,
        })
        .expect("insert local summary");

        desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
            session_default_view: None,
            summary: Some(summary_settings_update_with_backend(
                DesktopSummaryStorageBackend::LocalDb,
            )),
            vector_search: None,
            change_reader: None,
            lifecycle: None,
        })
        .expect("set summary backend to local_db");
        desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
            session_default_view: None,
            summary: Some(summary_settings_update_with_backend(
                DesktopSummaryStorageBackend::HiddenRef,
            )),
            vector_search: None,
            change_reader: None,
            lifecycle: None,
        })
        .expect("switch summary backend to hidden_ref");

        let migrated = NativeGitStorage
            .load_summary_at_ref(
                &repo_root,
                SUMMARY_LEDGER_REF,
                "storage-migrate-local-to-hidden",
            )
            .expect("load migrated hidden_ref summary")
            .expect("migrated hidden_ref summary should exist");
        assert_eq!(migrated.provider, "codex_exec");
        assert_eq!(migrated.summary["changes"], "migrated");
        assert_eq!(migrated.model.as_deref(), Some("gpt-5"));

        let _ = std::fs::remove_dir_all(&temp_home);
        let _ = std::fs::remove_dir_all(&temp_db);
        let _ = std::fs::remove_dir_all(&repo_root);
    }

    #[test]
    fn desktop_runtime_settings_migrates_summary_storage_hidden_ref_to_local_db() {
        let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
        let temp_home = unique_temp_dir("opensession-desktop-runtime-migrate-hidden-home");
        let temp_db = unique_temp_dir("opensession-desktop-runtime-migrate-hidden-db");
        let repo_root = unique_temp_dir("opensession-desktop-runtime-migrate-hidden-repo");
        let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());
        let _db_env = EnvVarGuard::set(
            "OPENSESSION_LOCAL_DB_PATH",
            temp_db.join("local.db").as_os_str(),
        );
        init_test_git_repo(&repo_root);

        let db = LocalDb::open_path(&temp_db.join("local.db")).expect("open local db");
        let mut session = build_test_session("storage-migrate-hidden-to-local");
        session.context.attributes.insert(
            "cwd".to_string(),
            json!(repo_root.to_string_lossy().to_string()),
        );
        let source_path = repo_root.join("storage-migrate-hidden-to-local.hail.jsonl");
        std::fs::write(
            &source_path,
            session.to_jsonl().expect("serialize session jsonl"),
        )
        .expect("write session source");
        db.upsert_local_session(
            &session,
            source_path
                .to_str()
                .expect("session source path must be valid utf-8"),
            &GitContext::default(),
        )
        .expect("upsert local session");

        let hidden_record = SessionSummaryLedgerRecord {
            session_id: "storage-migrate-hidden-to-local".to_string(),
            generated_at: "2026-03-05T11:00:00Z".to_string(),
            provider: "codex_exec".to_string(),
            model: Some("gpt-5".to_string()),
            source_kind: "session_signals".to_string(),
            generation_kind: "provider".to_string(),
            prompt_fingerprint: Some("hidden-fingerprint".to_string()),
            summary: json!({
                "changes": "from-hidden",
                "auth_security": "none detected",
                "layer_file_changes": []
            }),
            source_details: json!({ "source": "hidden_ref" }),
            diff_tree: Vec::new(),
            error: None,
        };
        NativeGitStorage
            .store_summary_at_ref(&repo_root, SUMMARY_LEDGER_REF, &hidden_record)
            .expect("store hidden_ref summary");

        desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
            session_default_view: None,
            summary: Some(summary_settings_update_with_backend(
                DesktopSummaryStorageBackend::HiddenRef,
            )),
            vector_search: None,
            change_reader: None,
            lifecycle: None,
        })
        .expect("set summary backend to hidden_ref");
        desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
            session_default_view: None,
            summary: Some(summary_settings_update_with_backend(
                DesktopSummaryStorageBackend::LocalDb,
            )),
            vector_search: None,
            change_reader: None,
            lifecycle: None,
        })
        .expect("switch summary backend to local_db");

        let migrated = db
            .get_session_semantic_summary("storage-migrate-hidden-to-local")
            .expect("read migrated local summary")
            .expect("migrated local summary should exist");
        assert_eq!(migrated.provider, "codex_exec");
        let migrated_summary: serde_json::Value =
            serde_json::from_str(&migrated.summary_json).expect("parse migrated summary");
        assert_eq!(migrated_summary["changes"], "from-hidden");
        assert_eq!(migrated.model.as_deref(), Some("gpt-5"));

        let _ = std::fs::remove_dir_all(&temp_home);
        let _ = std::fs::remove_dir_all(&temp_db);
        let _ = std::fs::remove_dir_all(&repo_root);
    }

    #[test]
    fn desktop_lifecycle_cleanup_deletes_expired_sessions_without_daemon() {
        let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
        let temp_root = unique_temp_dir("opensession-desktop-lifecycle-cleanup");
        let repo_root = temp_root.join("repo");
        std::fs::create_dir_all(&repo_root).expect("create repo root");
        init_test_git_repo(&repo_root);

        let db = LocalDb::open_path(&temp_root.join("local.db")).expect("open local db");

        let mut expired = build_test_session("expired-session");
        expired.context.created_at = chrono::Utc::now() - chrono::Duration::days(45);
        expired.context.updated_at = expired.context.created_at;
        expired.context.attributes.insert(
            "cwd".to_string(),
            json!(repo_root.to_string_lossy().to_string()),
        );
        let expired_source = repo_root.join("expired-session.hail.jsonl");
        std::fs::write(
            &expired_source,
            expired.to_jsonl().expect("serialize expired session"),
        )
        .expect("write expired session source");
        db.upsert_local_session(
            &expired,
            expired_source
                .to_str()
                .expect("expired session source path must be valid utf-8"),
            &GitContext::default(),
        )
        .expect("upsert expired session");

        let mut recent = build_test_session("recent-session");
        recent.context.created_at = chrono::Utc::now() - chrono::Duration::days(5);
        recent.context.updated_at = recent.context.created_at;
        recent.context.attributes.insert(
            "cwd".to_string(),
            json!(repo_root.to_string_lossy().to_string()),
        );
        let recent_source = repo_root.join("recent-session.hail.jsonl");
        std::fs::write(
            &recent_source,
            recent.to_jsonl().expect("serialize recent session"),
        )
        .expect("write recent session source");
        db.upsert_local_session(
            &recent,
            recent_source
                .to_str()
                .expect("recent session source path must be valid utf-8"),
            &GitContext::default(),
        )
        .expect("upsert recent session");

        let storage = NativeGitStorage;
        storage
            .store_summary_at_ref(
                &repo_root,
                SUMMARY_LEDGER_REF,
                &SessionSummaryLedgerRecord {
                    session_id: "expired-session".to_string(),
                    generated_at: "2026-01-01T00:00:00Z".to_string(),
                    provider: "codex_exec".to_string(),
                    model: None,
                    source_kind: "session_signals".to_string(),
                    generation_kind: "provider".to_string(),
                    prompt_fingerprint: None,
                    summary: json!({ "changes": "expired" }),
                    source_details: json!({}),
                    diff_tree: vec![],
                    error: None,
                },
            )
            .expect("store expired hidden_ref summary");
        storage
            .store_summary_at_ref(
                &repo_root,
                SUMMARY_LEDGER_REF,
                &SessionSummaryLedgerRecord {
                    session_id: "recent-session".to_string(),
                    generated_at: "2026-01-01T00:00:00Z".to_string(),
                    provider: "codex_exec".to_string(),
                    model: None,
                    source_kind: "session_signals".to_string(),
                    generation_kind: "provider".to_string(),
                    prompt_fingerprint: None,
                    summary: json!({ "changes": "recent" }),
                    source_details: json!({}),
                    diff_tree: vec![],
                    error: None,
                },
            )
            .expect("store recent hidden_ref summary");

        let mut config = DaemonConfig::default();
        config.lifecycle.enabled = true;
        config.lifecycle.session_ttl_days = 30;
        config.lifecycle.summary_ttl_days = 30;
        config.lifecycle.cleanup_interval_secs = 60;

        super::run_desktop_lifecycle_cleanup_once_with_db(&config, &db)
            .expect("run desktop lifecycle cleanup");

        let lifecycle_status = super::desktop_lifecycle_cleanup_status_from_db(&db)
            .expect("read lifecycle cleanup status");
        assert_eq!(
            lifecycle_status.state,
            DesktopLifecycleCleanupState::Complete
        );
        assert_eq!(lifecycle_status.deleted_sessions, 1);
        assert_eq!(lifecycle_status.deleted_summaries, 1);
        assert!(
            lifecycle_status
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("Deleted 1 sessions"),
            "cleanup status should summarize deleted rows"
        );
        assert!(lifecycle_status.started_at.is_some());
        assert!(lifecycle_status.finished_at.is_some());

        assert!(
            db.get_session_by_id("expired-session")
                .expect("query expired session")
                .is_none(),
            "expired session should be deleted by desktop lifecycle cleanup"
        );
        assert!(
            db.get_session_by_id("recent-session")
                .expect("query recent session")
                .is_some(),
            "recent session should remain after desktop lifecycle cleanup"
        );
        assert!(
            storage
                .load_summary_at_ref(&repo_root, SUMMARY_LEDGER_REF, "expired-session")
                .expect("load expired hidden_ref summary")
                .is_none(),
            "expired hidden_ref summary should be deleted by desktop lifecycle cleanup"
        );
        assert!(
            storage
                .load_summary_at_ref(&repo_root, SUMMARY_LEDGER_REF, "recent-session")
                .expect("load recent hidden_ref summary")
                .is_some(),
            "recent hidden_ref summary should remain after desktop lifecycle cleanup"
        );

        let _ = std::fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn desktop_runtime_settings_rejects_non_session_only_source_mode() {
        let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
        let temp_home = unique_temp_dir("opensession-desktop-runtime-source-lock");
        let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());

        let result = desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
            session_default_view: None,
            summary: Some(DesktopRuntimeSummarySettingsUpdate {
                provider: DesktopRuntimeSummaryProviderSettingsUpdate {
                    id: DesktopSummaryProviderId::Disabled,
                    endpoint: String::new(),
                    model: String::new(),
                },
                prompt: DesktopRuntimeSummaryPromptSettingsUpdate {
                    template: DEFAULT_SUMMARY_PROMPT_TEMPLATE_V2.to_string(),
                },
                response: DesktopRuntimeSummaryResponseSettingsUpdate {
                    style: DesktopSummaryResponseStyle::Standard,
                    shape: DesktopSummaryOutputShape::Layered,
                },
                storage: DesktopRuntimeSummaryStorageSettingsUpdate {
                    trigger: DesktopSummaryTriggerMode::OnSessionSave,
                    backend: DesktopSummaryStorageBackend::HiddenRef,
                },
                source_mode: DesktopSummarySourceMode::SessionOrGitChanges,
                batch: DesktopRuntimeSummaryBatchSettingsUpdate {
                    execution_mode: DesktopSummaryBatchExecutionMode::Manual,
                    scope: DesktopSummaryBatchScope::RecentDays,
                    recent_days: 30,
                },
            }),
            vector_search: None,
            change_reader: None,
            lifecycle: None,
        });

        let error = result.expect_err("source mode lock should reject update");
        assert_eq!(error.status, 422);
        assert_eq!(error.code, "desktop.runtime_settings_source_mode_locked");

        let _ = std::fs::remove_dir_all(&temp_home);
    }

    #[test]
    fn desktop_runtime_settings_rejects_zero_summary_batch_recent_days() {
        let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
        let temp_home = unique_temp_dir("opensession-desktop-runtime-summary-batch-invalid");
        let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());

        let result = desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
            session_default_view: None,
            summary: Some(DesktopRuntimeSummarySettingsUpdate {
                provider: DesktopRuntimeSummaryProviderSettingsUpdate {
                    id: DesktopSummaryProviderId::Disabled,
                    endpoint: String::new(),
                    model: String::new(),
                },
                prompt: DesktopRuntimeSummaryPromptSettingsUpdate {
                    template: DEFAULT_SUMMARY_PROMPT_TEMPLATE_V2.to_string(),
                },
                response: DesktopRuntimeSummaryResponseSettingsUpdate {
                    style: DesktopSummaryResponseStyle::Standard,
                    shape: DesktopSummaryOutputShape::Layered,
                },
                storage: DesktopRuntimeSummaryStorageSettingsUpdate {
                    trigger: DesktopSummaryTriggerMode::OnSessionSave,
                    backend: DesktopSummaryStorageBackend::HiddenRef,
                },
                source_mode: DesktopSummarySourceMode::SessionOnly,
                batch: DesktopRuntimeSummaryBatchSettingsUpdate {
                    execution_mode: DesktopSummaryBatchExecutionMode::OnAppStart,
                    scope: DesktopSummaryBatchScope::RecentDays,
                    recent_days: 0,
                },
            }),
            vector_search: None,
            change_reader: None,
            lifecycle: None,
        });

        let error = result.expect_err("recent_days=0 should fail");
        assert_eq!(error.status, 422);
        assert_eq!(
            error.code,
            "desktop.runtime_settings_invalid_summary_batch_recent_days"
        );

        let _ = std::fs::remove_dir_all(&temp_home);
    }

    #[test]
    fn desktop_runtime_settings_rejects_short_lifecycle_interval() {
        let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
        let temp_home = unique_temp_dir("opensession-desktop-runtime-lifecycle-invalid");
        let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());

        let result = desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
            session_default_view: None,
            summary: None,
            vector_search: None,
            change_reader: None,
            lifecycle: Some(DesktopRuntimeLifecycleSettingsUpdate {
                enabled: true,
                session_ttl_days: 30,
                summary_ttl_days: 30,
                cleanup_interval_secs: 59,
            }),
        });

        let error = result.expect_err("cleanup interval under 60 should fail");
        assert_eq!(error.status, 422);
        assert_eq!(
            error.code,
            "desktop.runtime_settings_invalid_cleanup_interval"
        );

        let _ = std::fs::remove_dir_all(&temp_home);
    }

    #[test]
    fn desktop_summary_batch_run_and_status_complete_when_no_sessions() {
        let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
        let temp_home = unique_temp_dir("opensession-desktop-summary-batch-home");
        let temp_db = unique_temp_dir("opensession-desktop-summary-batch-db");
        let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());
        let _db_env = EnvVarGuard::set(
            "OPENSESSION_LOCAL_DB_PATH",
            temp_db.join("local.db").as_os_str(),
        );

        desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
            session_default_view: None,
            summary: Some(DesktopRuntimeSummarySettingsUpdate {
                provider: DesktopRuntimeSummaryProviderSettingsUpdate {
                    id: DesktopSummaryProviderId::Disabled,
                    endpoint: String::new(),
                    model: String::new(),
                },
                prompt: DesktopRuntimeSummaryPromptSettingsUpdate {
                    template: DEFAULT_SUMMARY_PROMPT_TEMPLATE_V2.to_string(),
                },
                response: DesktopRuntimeSummaryResponseSettingsUpdate {
                    style: DesktopSummaryResponseStyle::Standard,
                    shape: DesktopSummaryOutputShape::Layered,
                },
                storage: DesktopRuntimeSummaryStorageSettingsUpdate {
                    trigger: DesktopSummaryTriggerMode::Manual,
                    backend: DesktopSummaryStorageBackend::None,
                },
                source_mode: DesktopSummarySourceMode::SessionOnly,
                batch: DesktopRuntimeSummaryBatchSettingsUpdate {
                    execution_mode: DesktopSummaryBatchExecutionMode::Manual,
                    scope: DesktopSummaryBatchScope::All,
                    recent_days: 30,
                },
            }),
            vector_search: None,
            change_reader: None,
            lifecycle: None,
        })
        .expect("set summary batch config");

        let started = desktop_summary_batch_run().expect("start summary batch");
        assert!(
            matches!(
                started.state,
                DesktopSummaryBatchState::Running | DesktopSummaryBatchState::Complete
            ),
            "initial state should be running or complete"
        );

        let mut final_state = started;
        for _ in 0..40 {
            final_state = desktop_summary_batch_status().expect("read batch status");
            if !matches!(final_state.state, DesktopSummaryBatchState::Running) {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }

        assert_eq!(final_state.state, DesktopSummaryBatchState::Complete);
        assert_eq!(final_state.total_sessions, 0);
        assert_eq!(final_state.processed_sessions, 0);
        assert_eq!(final_state.failed_sessions, 0);

        let _ = std::fs::remove_dir_all(&temp_home);
        let _ = std::fs::remove_dir_all(&temp_db);
    }

    #[test]
    fn desktop_summary_batch_skips_sessions_with_missing_source_files() {
        let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
        let temp_home = unique_temp_dir("opensession-desktop-summary-batch-skip-home");
        let temp_db = unique_temp_dir("opensession-desktop-summary-batch-skip-db");
        let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());
        let _db_env = EnvVarGuard::set(
            "OPENSESSION_LOCAL_DB_PATH",
            temp_db.join("local.db").as_os_str(),
        );

        desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
            session_default_view: None,
            summary: Some(DesktopRuntimeSummarySettingsUpdate {
                provider: DesktopRuntimeSummaryProviderSettingsUpdate {
                    id: DesktopSummaryProviderId::Disabled,
                    endpoint: String::new(),
                    model: String::new(),
                },
                prompt: DesktopRuntimeSummaryPromptSettingsUpdate {
                    template: DEFAULT_SUMMARY_PROMPT_TEMPLATE_V2.to_string(),
                },
                response: DesktopRuntimeSummaryResponseSettingsUpdate {
                    style: DesktopSummaryResponseStyle::Standard,
                    shape: DesktopSummaryOutputShape::Layered,
                },
                storage: DesktopRuntimeSummaryStorageSettingsUpdate {
                    trigger: DesktopSummaryTriggerMode::Manual,
                    backend: DesktopSummaryStorageBackend::None,
                },
                source_mode: DesktopSummarySourceMode::SessionOnly,
                batch: DesktopRuntimeSummaryBatchSettingsUpdate {
                    execution_mode: DesktopSummaryBatchExecutionMode::Manual,
                    scope: DesktopSummaryBatchScope::All,
                    recent_days: 30,
                },
            }),
            vector_search: None,
            change_reader: None,
            lifecycle: None,
        })
        .expect("set summary batch config");

        let db = LocalDb::open_path(&temp_db.join("local.db")).expect("open local db");
        let session = build_test_session("missing-source-session");
        let missing_source = temp_db.join("missing-source-session.jsonl");
        db.upsert_local_session(
            &session,
            missing_source
                .to_str()
                .expect("missing source path must be valid utf-8"),
            &GitContext::default(),
        )
        .expect("upsert local session with missing source path");

        let started = desktop_summary_batch_run().expect("start summary batch");
        assert!(
            matches!(
                started.state,
                DesktopSummaryBatchState::Running | DesktopSummaryBatchState::Complete
            ),
            "initial state should be running or complete"
        );

        let mut final_state = started;
        for _ in 0..40 {
            final_state = desktop_summary_batch_status().expect("read batch status");
            if !matches!(final_state.state, DesktopSummaryBatchState::Running) {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }

        assert_eq!(final_state.state, DesktopSummaryBatchState::Complete);
        assert_eq!(final_state.total_sessions, 1);
        assert_eq!(final_state.processed_sessions, 1);
        assert_eq!(final_state.failed_sessions, 0);
        assert!(
            final_state
                .message
                .as_deref()
                .is_some_and(|message| message.contains("skipped missing sources"))
        );

        let _ = std::fs::remove_dir_all(&temp_home);
        let _ = std::fs::remove_dir_all(&temp_db);
    }

    #[test]
    fn desktop_summary_batch_skips_sessions_with_existing_local_db_summaries() {
        let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
        let temp_home = unique_temp_dir("opensession-desktop-summary-batch-local-summary-home");
        let temp_db = unique_temp_dir("opensession-desktop-summary-batch-local-summary-db");
        let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());
        let _db_env = EnvVarGuard::set(
            "OPENSESSION_LOCAL_DB_PATH",
            temp_db.join("local.db").as_os_str(),
        );

        desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
            session_default_view: None,
            summary: Some(summary_settings_update_with_backend(
                DesktopSummaryStorageBackend::HiddenRef,
            )),
            vector_search: None,
            change_reader: None,
            lifecycle: None,
        })
        .expect("set summary batch config");

        let db = LocalDb::open_path(&temp_db.join("local.db")).expect("open local db");
        let session = build_test_session("existing-local-summary-session");
        let missing_source = temp_db.join("existing-local-summary-session.jsonl");
        db.upsert_local_session(
            &session,
            missing_source
                .to_str()
                .expect("missing source path must be valid utf-8"),
            &GitContext::default(),
        )
        .expect("upsert local session with missing source path");
        db.upsert_session_semantic_summary(&opensession_local_db::SessionSemanticSummaryUpsert {
            session_id: "existing-local-summary-session",
            summary_json: r#"{"changes":"cached","auth_security":"none detected","layer_file_changes":[]}"#,
            generated_at: "2026-03-06T01:00:00Z",
            provider: "codex_exec",
            model: Some("gpt-5"),
            source_kind: "session_signals",
            generation_kind: "provider",
            prompt_fingerprint: Some("local-cache"),
            source_details_json: Some(r#"{"source":"local_db"}"#),
            diff_tree_json: Some(r#"[]"#),
            error: None,
        })
        .expect("insert local summary");

        let started = desktop_summary_batch_run().expect("start summary batch");
        assert!(
            matches!(
                started.state,
                DesktopSummaryBatchState::Running | DesktopSummaryBatchState::Complete
            ),
            "initial state should be running or complete"
        );

        let mut final_state = started;
        for _ in 0..40 {
            final_state = desktop_summary_batch_status().expect("read batch status");
            if !matches!(final_state.state, DesktopSummaryBatchState::Running) {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }

        assert_eq!(final_state.state, DesktopSummaryBatchState::Complete);
        assert_eq!(final_state.total_sessions, 0);
        assert_eq!(final_state.processed_sessions, 0);
        assert_eq!(final_state.failed_sessions, 0);
        assert!(
            final_state
                .message
                .as_deref()
                .is_some_and(|message| message.contains("already summarized"))
        );

        let _ = std::fs::remove_dir_all(&temp_home);
        let _ = std::fs::remove_dir_all(&temp_db);
    }

    #[test]
    fn desktop_summary_batch_skips_sessions_with_existing_hidden_ref_summaries() {
        let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
        let temp_home = unique_temp_dir("opensession-desktop-summary-batch-hidden-summary-home");
        let temp_db = unique_temp_dir("opensession-desktop-summary-batch-hidden-summary-db");
        let repo_root = unique_temp_dir("opensession-desktop-summary-batch-hidden-summary-repo");
        let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());
        let _db_env = EnvVarGuard::set(
            "OPENSESSION_LOCAL_DB_PATH",
            temp_db.join("local.db").as_os_str(),
        );
        init_test_git_repo(&repo_root);

        desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
            session_default_view: None,
            summary: Some(summary_settings_update_with_backend(
                DesktopSummaryStorageBackend::LocalDb,
            )),
            vector_search: None,
            change_reader: None,
            lifecycle: None,
        })
        .expect("set summary batch config");

        let db = LocalDb::open_path(&temp_db.join("local.db")).expect("open local db");
        let mut session = build_test_session("existing-hidden-summary-session");
        session.context.attributes.insert(
            "cwd".to_string(),
            json!(repo_root.to_string_lossy().to_string()),
        );
        let missing_source = repo_root.join("existing-hidden-summary-session.jsonl");
        db.upsert_local_session(
            &session,
            missing_source
                .to_str()
                .expect("missing source path must be valid utf-8"),
            &GitContext::default(),
        )
        .expect("upsert local session with missing source path");
        NativeGitStorage
            .store_summary_at_ref(
                &repo_root,
                SUMMARY_LEDGER_REF,
                &SessionSummaryLedgerRecord {
                    session_id: "existing-hidden-summary-session".to_string(),
                    generated_at: "2026-03-06T02:00:00Z".to_string(),
                    provider: "codex_exec".to_string(),
                    model: Some("gpt-5".to_string()),
                    source_kind: "session_signals".to_string(),
                    generation_kind: "provider".to_string(),
                    prompt_fingerprint: Some("hidden-cache".to_string()),
                    summary: json!({
                        "changes": "cached",
                        "auth_security": "none detected",
                        "layer_file_changes": []
                    }),
                    source_details: json!({ "source": "hidden_ref" }),
                    diff_tree: Vec::new(),
                    error: None,
                },
            )
            .expect("store hidden_ref summary");

        let started = desktop_summary_batch_run().expect("start summary batch");
        assert!(
            matches!(
                started.state,
                DesktopSummaryBatchState::Running | DesktopSummaryBatchState::Complete
            ),
            "initial state should be running or complete"
        );

        let mut final_state = started;
        for _ in 0..40 {
            final_state = desktop_summary_batch_status().expect("read batch status");
            if !matches!(final_state.state, DesktopSummaryBatchState::Running) {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }

        assert_eq!(final_state.state, DesktopSummaryBatchState::Complete);
        assert_eq!(final_state.total_sessions, 0);
        assert_eq!(final_state.processed_sessions, 0);
        assert_eq!(final_state.failed_sessions, 0);
        assert!(
            final_state
                .message
                .as_deref()
                .is_some_and(|message| message.contains("already summarized"))
        );

        let _ = std::fs::remove_dir_all(&temp_home);
        let _ = std::fs::remove_dir_all(&temp_db);
        let _ = std::fs::remove_dir_all(&repo_root);
    }

    #[test]
    fn desktop_change_reader_requires_enabled_setting() {
        let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
        let temp_home = unique_temp_dir("opensession-desktop-change-reader-disabled");
        let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());

        let result = tauri::async_runtime::block_on(desktop_read_session_changes(
            DesktopChangeReadRequest {
                session_id: "session-1".to_string(),
                scope: None,
            },
        ));
        let error = result.expect_err("disabled change reader should fail");
        assert_eq!(error.status, 422);
        assert_eq!(error.code, "desktop.change_reader_disabled");

        let _ = std::fs::remove_dir_all(&temp_home);
    }

    #[test]
    fn require_non_empty_request_field_trims_surrounding_whitespace() {
        let value = require_non_empty_request_field(
            "  session-1 \n",
            "desktop.test_invalid_request",
            "session_id",
        )
        .expect("trimmed field should be accepted");
        assert_eq!(value, "session-1");
    }

    #[test]
    fn require_non_empty_request_field_rejects_blank_values() {
        let error =
            require_non_empty_request_field(" \n\t ", "desktop.test_invalid_request", "session_id")
                .expect_err("blank field should be rejected");
        assert_eq!(error.status, 400);
        assert_eq!(error.code, "desktop.test_invalid_request");
        assert_eq!(error.message, "session_id is required");
    }

    #[test]
    fn desktop_change_reader_qa_respects_toggle() {
        let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
        let temp_home = unique_temp_dir("opensession-desktop-change-reader-qa-disabled");
        let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());

        desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
            session_default_view: None,
            summary: None,
            vector_search: None,
            change_reader: Some(DesktopRuntimeChangeReaderSettingsUpdate {
                enabled: true,
                scope: DesktopChangeReaderScope::SummaryOnly,
                qa_enabled: false,
                max_context_chars: 12_000,
                voice: DesktopRuntimeChangeReaderVoiceSettingsUpdate {
                    enabled: false,
                    provider: DesktopChangeReaderVoiceProvider::Openai,
                    model: "gpt-4o-mini-tts".to_string(),
                    voice: "alloy".to_string(),
                    api_key: None,
                },
            }),
            lifecycle: None,
        })
        .expect("enable change reader with qa disabled");

        let result = tauri::async_runtime::block_on(desktop_ask_session_changes(
            DesktopChangeQuestionRequest {
                session_id: "session-1".to_string(),
                question: "무엇이 바뀌었나요?".to_string(),
                scope: None,
            },
        ));
        let error = result.expect_err("qa disabled should fail");
        assert_eq!(error.status, 422);
        assert_eq!(error.code, "desktop.change_reader_qa_disabled");

        let _ = std::fs::remove_dir_all(&temp_home);
    }

    #[test]
    fn desktop_runtime_settings_rejects_voice_playback_without_api_key() {
        let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
        let temp_home = unique_temp_dir("opensession-desktop-change-reader-voice-key-required");
        let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());

        let result = desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
            session_default_view: None,
            summary: None,
            vector_search: None,
            change_reader: Some(DesktopRuntimeChangeReaderSettingsUpdate {
                enabled: true,
                scope: DesktopChangeReaderScope::SummaryOnly,
                qa_enabled: true,
                max_context_chars: 12_000,
                voice: DesktopRuntimeChangeReaderVoiceSettingsUpdate {
                    enabled: true,
                    provider: DesktopChangeReaderVoiceProvider::Openai,
                    model: "gpt-4o-mini-tts".to_string(),
                    voice: "alloy".to_string(),
                    api_key: None,
                },
            }),
            lifecycle: None,
        });

        let error = result.expect_err("voice playback without api key should fail");
        assert_eq!(error.status, 422);
        assert_eq!(
            error.code,
            "desktop.runtime_settings_change_reader_voice_api_key_required"
        );

        let _ = std::fs::remove_dir_all(&temp_home);
    }

    #[test]
    fn desktop_runtime_settings_allows_voice_playback_with_existing_api_key() {
        let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
        let temp_home = unique_temp_dir("opensession-desktop-change-reader-voice-key-existing");
        let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());

        desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
            session_default_view: None,
            summary: None,
            vector_search: None,
            change_reader: Some(DesktopRuntimeChangeReaderSettingsUpdate {
                enabled: true,
                scope: DesktopChangeReaderScope::SummaryOnly,
                qa_enabled: true,
                max_context_chars: 12_000,
                voice: DesktopRuntimeChangeReaderVoiceSettingsUpdate {
                    enabled: false,
                    provider: DesktopChangeReaderVoiceProvider::Openai,
                    model: "gpt-4o-mini-tts".to_string(),
                    voice: "alloy".to_string(),
                    api_key: Some("sk-existing-voice-key".to_string()),
                },
            }),
            lifecycle: None,
        })
        .expect("store existing voice api key");

        let updated = desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
            session_default_view: None,
            summary: None,
            vector_search: None,
            change_reader: Some(DesktopRuntimeChangeReaderSettingsUpdate {
                enabled: true,
                scope: DesktopChangeReaderScope::SummaryOnly,
                qa_enabled: true,
                max_context_chars: 12_000,
                voice: DesktopRuntimeChangeReaderVoiceSettingsUpdate {
                    enabled: true,
                    provider: DesktopChangeReaderVoiceProvider::Openai,
                    model: "gpt-4o-mini-tts".to_string(),
                    voice: "alloy".to_string(),
                    api_key: None,
                },
            }),
            lifecycle: None,
        })
        .expect("enable voice playback with existing api key");

        assert!(updated.change_reader.voice.enabled);
        assert!(updated.change_reader.voice.api_key_configured);

        let _ = std::fs::remove_dir_all(&temp_home);
    }

    #[test]
    fn desktop_change_reader_tts_requires_voice_enable() {
        let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
        let temp_home = unique_temp_dir("opensession-desktop-change-reader-tts-disabled");
        let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());

        desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
            session_default_view: None,
            summary: None,
            vector_search: None,
            change_reader: Some(DesktopRuntimeChangeReaderSettingsUpdate {
                enabled: true,
                scope: DesktopChangeReaderScope::SummaryOnly,
                qa_enabled: true,
                max_context_chars: 12_000,
                voice: DesktopRuntimeChangeReaderVoiceSettingsUpdate {
                    enabled: false,
                    provider: DesktopChangeReaderVoiceProvider::Openai,
                    model: "gpt-4o-mini-tts".to_string(),
                    voice: "alloy".to_string(),
                    api_key: None,
                },
            }),
            lifecycle: None,
        })
        .expect("enable change reader with voice disabled");

        let result = desktop_change_reader_tts(DesktopChangeReaderTtsRequest {
            text: "변경 내용을 읽어줘".to_string(),
            session_id: None,
            scope: None,
        });
        let error = result.expect_err("voice disabled should fail");
        assert_eq!(error.status, 422);
        assert_eq!(error.code, "desktop.change_reader_tts_disabled");

        let _ = std::fs::remove_dir_all(&temp_home);
    }

    #[test]
    fn handoff_build_writes_artifact_record_and_pin() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be monotonic")
            .as_nanos();
        let repo_root = std::env::temp_dir().join(format!("opensession-desktop-handoff-{unique}"));
        let git_dir = repo_root.join(".git");
        std::fs::create_dir_all(&git_dir).expect("create repo .git");

        let mut session = HailSession::new(
            "session-handoff-test".to_string(),
            Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );
        session.recompute_stats();
        let normalized = session.to_jsonl().expect("serialize session");

        let response =
            build_handoff_artifact_record(&normalized, session, true, &repo_root).expect("build");
        let hash = response
            .artifact_uri
            .strip_prefix("os://artifact/")
            .expect("artifact uri prefix");
        assert_eq!(hash.len(), 64);
        assert_eq!(response.pinned_alias.as_deref(), Some("latest"));
        let expected_download_file_name = format!("handoff-{hash}.jsonl");
        assert_eq!(
            response.download_file_name.as_deref(),
            Some(expected_download_file_name.as_str())
        );
        assert!(
            response.download_content.as_deref().is_some_and(
                |value| value.contains("\"source_session_id\":\"session-handoff-test\"")
            )
        );

        let artifact_path = repo_root
            .join(".opensession")
            .join("artifacts")
            .join("sha256")
            .join(&hash[0..2])
            .join(&hash[2..4])
            .join(format!("{hash}.json"));
        assert!(artifact_path.exists());

        let pin_path = repo_root
            .join(".opensession")
            .join("artifacts")
            .join("pins")
            .join("latest");
        let pin_hash = std::fs::read_to_string(&pin_path).expect("read pin hash");
        assert_eq!(pin_hash.trim(), hash);

        let _ = std::fs::remove_dir_all(&repo_root);
    }
}
