#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use opensession_api::{
    oauth::{AuthProvidersResponse, OAuthProviderInfo},
    CapabilitiesResponse, DesktopApiError, DesktopContractVersionResponse,
    DesktopChangeQuestionRequest, DesktopChangeQuestionResponse, DesktopChangeReadRequest,
    DesktopChangeReadResponse, DesktopChangeReaderScope,
    DesktopHandoffBuildRequest, DesktopHandoffBuildResponse, DesktopQuickShareRequest,
    DesktopQuickShareResponse, DesktopRuntimeSettingsResponse,
    DesktopRuntimeChangeReaderSettings, DesktopRuntimeSettingsUpdateRequest,
    DesktopRuntimeSummaryPromptSettings,
    DesktopRuntimeSummaryProviderSettings, DesktopRuntimeSummaryResponseSettings,
    DesktopRuntimeSummarySettings, DesktopRuntimeSummaryStorageSettings,
    DesktopRuntimeSummaryUiConstraints, DesktopRuntimeVectorSearchSettings,
    DesktopSessionListQuery, DesktopSessionSummaryResponse, DesktopSummaryOutputShape,
    DesktopSummaryProviderDetectResponse, DesktopSummaryProviderId,
    DesktopSummaryProviderTransport, DesktopSummaryResponseStyle, DesktopSummarySourceMode,
    DesktopSummaryStorageBackend, DesktopSummaryTriggerMode, DesktopVectorIndexState,
    DesktopVectorIndexStatusResponse, DesktopVectorInstallState,
    DesktopVectorInstallStatusResponse, DesktopVectorPreflightResponse,
    DesktopVectorSearchGranularity, DesktopVectorSearchProvider, DesktopVectorSearchResponse,
    DesktopVectorSessionMatch, LinkType, SessionDetail, SessionLink, SessionListResponse,
    SessionRepoListResponse, SessionSummary, DESKTOP_IPC_CONTRACT_VERSION,
};
use opensession_core::handoff::{validate_handoff_summaries, HandoffSummary};
use opensession_core::object_store::{
    find_repo_root, global_store_root, sha256_hex, store_local_object,
};
use opensession_core::session::working_directory;
use opensession_core::source_uri::SourceUri;
use opensession_core::trace::{ContentBlock, EventType, Session as HailSession};
use opensession_git_native::{
    extract_git_context, ops::find_repo_root as find_git_repo_root, NativeGitStorage,
    SessionSummaryLedgerRecord, SUMMARY_LEDGER_REF,
};
use opensession_local_db::{
    LocalDb, LocalSessionFilter, LocalSessionLink, LocalSessionRow, VectorChunkUpsert,
    VectorIndexJobRow,
};
use opensession_parsers::ingest::preview_parse_bytes;
use opensession_runtime_config::{
    ChangeReaderScope, DaemonConfig, SessionDefaultView, SummaryOutputShape, SummaryProvider,
    SummaryResponseStyle, SummarySourceMode, SummaryStorageBackend, SummaryTriggerMode,
    VectorSearchGranularity,
    VectorSearchProvider,
};
use opensession_summary::{
    provider::generate_text, summarize_session, validate_summary_prompt_template, GitSummaryRequest,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

type DesktopApiResult<T> = Result<T, DesktopApiError>;

const HANDOFF_RECORD_VERSION: &str = "v1";
const HANDOFF_LATEST_PIN_ALIAS: &str = "latest";
const VECTOR_EMBED_BATCH_SIZE: usize = 24;
const VECTOR_FTS_CANDIDATE_LIMIT_MULTIPLIER: u32 = 8;
const CHANGE_READER_MAX_EVENTS: usize = 180;
const CHANGE_READER_MAX_LINE_CHARS: usize = 220;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchMode {
    Keyword,
    Vector,
}

#[derive(Debug, Clone)]
struct VectorInstallRuntimeState {
    state: DesktopVectorInstallState,
    model: String,
    progress_pct: u8,
    message: Option<String>,
}

#[derive(Debug, Clone)]
struct VectorMatchScore {
    session_id: String,
    chunk_id: String,
    start_line: u32,
    end_line: u32,
    snippet: String,
    best_score: f32,
    hit_count: u32,
}

static VECTOR_INSTALL_STATE: LazyLock<Mutex<VectorInstallRuntimeState>> = LazyLock::new(|| {
    Mutex::new(VectorInstallRuntimeState {
        state: DesktopVectorInstallState::NotInstalled,
        model: "bge-m3".to_string(),
        progress_pct: 0,
        message: None,
    })
});
static VECTOR_INDEX_REBUILD_RUNNING: LazyLock<Mutex<bool>> = LazyLock::new(|| Mutex::new(false));

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DesktopHandoffArtifactRecord {
    version: String,
    sha256: String,
    created_at: String,
    source_uris: Vec<String>,
    canonical_jsonl: String,
    raw_sessions: Vec<HailSession>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    summary_meta: Option<DesktopHandoffSummaryMeta>,
    #[serde(default)]
    validation_reports: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DesktopHandoffSummaryMeta {
    session_default_view: String,
    summary_source_mode: String,
    summary_provider: String,
}

fn desktop_error(
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

fn enum_label<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value)
        .ok()
        .map(|raw| raw.trim_matches('"').to_string())
        .filter(|raw| !raw.trim().is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

fn desktop_launch_route_path() -> DesktopApiResult<PathBuf> {
    let store_root = global_store_root().map_err(|error| {
        desktop_error(
            "desktop.launch_route_root_unavailable",
            500,
            "failed to resolve OpenSession home directory",
            Some(json!({ "cause": error.to_string() })),
        )
    })?;
    let opensession_root = store_root.parent().ok_or_else(|| {
        desktop_error(
            "desktop.launch_route_root_invalid",
            500,
            "invalid OpenSession global store path",
            Some(json!({ "store_root": store_root.to_string_lossy() })),
        )
    })?;
    Ok(opensession_root.join("desktop").join("launch-route"))
}

fn normalize_launch_route(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || !trimmed.starts_with('/') || trimmed.starts_with("//") {
        return None;
    }
    if trimmed.chars().any(|ch| ch.is_control()) {
        return None;
    }
    Some(trimmed.to_string())
}

fn normalize_non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|raw| raw.trim().to_string())
        .and_then(|trimmed| (!trimmed.is_empty()).then_some(trimmed))
}

fn split_search_mode(raw: Option<String>) -> (Option<String>, SearchMode) {
    let normalized = normalize_non_empty(raw);
    let Some(value) = normalized else {
        return (None, SearchMode::Keyword);
    };
    let lower = value.to_ascii_lowercase();
    for prefix in ["vector:", "vec:"] {
        if lower.starts_with(prefix) {
            let query = value[prefix.len()..].trim().to_string();
            return ((!query.is_empty()).then_some(query), SearchMode::Vector);
        }
    }
    (Some(value), SearchMode::Keyword)
}

fn parse_positive_u32(raw: Option<String>, fallback: u32, max: u32) -> u32 {
    let parsed = raw
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(fallback);
    parsed.min(max).max(1)
}

fn map_sort_order(sort: Option<&str>) -> opensession_local_db::LocalSortOrder {
    match sort.unwrap_or_default() {
        "popular" => opensession_local_db::LocalSortOrder::Popular,
        "longest" => opensession_local_db::LocalSortOrder::Longest,
        _ => opensession_local_db::LocalSortOrder::Recent,
    }
}

fn map_time_range(time_range: Option<&str>) -> opensession_local_db::LocalTimeRange {
    match time_range.unwrap_or_default() {
        "24h" => opensession_local_db::LocalTimeRange::Hours24,
        "7d" => opensession_local_db::LocalTimeRange::Days7,
        "30d" => opensession_local_db::LocalTimeRange::Days30,
        _ => opensession_local_db::LocalTimeRange::All,
    }
}

fn build_local_filter_with_mode(
    query: DesktopSessionListQuery,
) -> (LocalSessionFilter, u32, u32, SearchMode) {
    let page = parse_positive_u32(query.page, 1, 10_000);
    let per_page = parse_positive_u32(query.per_page, 20, 200);
    let offset = (page.saturating_sub(1)).saturating_mul(per_page);
    let (search_query, search_mode) = split_search_mode(query.search);

    let filter = LocalSessionFilter {
        search: search_query,
        tool: normalize_non_empty(query.tool),
        git_repo_name: normalize_non_empty(query.git_repo_name),
        sort: map_sort_order(query.sort.as_deref()),
        time_range: map_time_range(query.time_range.as_deref()),
        limit: Some(per_page),
        offset: Some(offset),
        ..Default::default()
    };

    (filter, page, per_page, search_mode)
}

fn session_summary_from_local_row(row: LocalSessionRow) -> SessionSummary {
    session_summary_from_local_row_with_score(
        row,
        0,
        opensession_core::scoring::DEFAULT_SCORE_PLUGIN,
    )
}

fn session_summary_from_local_row_with_score(
    row: LocalSessionRow,
    session_score: i64,
    score_plugin: &str,
) -> SessionSummary {
    SessionSummary {
        id: row.id,
        user_id: row.user_id,
        nickname: row.nickname,
        tool: row.tool,
        agent_provider: row.agent_provider,
        agent_model: row.agent_model,
        title: row.title,
        description: row.description,
        tags: row.tags,
        created_at: row.created_at.clone(),
        uploaded_at: row.uploaded_at.unwrap_or(row.created_at),
        message_count: row.message_count,
        task_count: row.task_count,
        event_count: row.event_count,
        duration_seconds: row.duration_seconds,
        total_input_tokens: row.total_input_tokens,
        total_output_tokens: row.total_output_tokens,
        git_remote: row.git_remote,
        git_branch: row.git_branch,
        git_commit: row.git_commit,
        git_repo_name: row.git_repo_name,
        pr_number: row.pr_number,
        pr_url: row.pr_url,
        working_directory: row.working_directory,
        files_modified: row.files_modified,
        files_read: row.files_read,
        has_errors: row.has_errors,
        max_active_agents: row.max_active_agents,
        session_score,
        score_plugin: score_plugin.to_string(),
    }
}

fn map_link_type(raw: &str) -> LinkType {
    match raw {
        "related" => LinkType::Related,
        "parent" => LinkType::Parent,
        "child" => LinkType::Child,
        _ => LinkType::Handoff,
    }
}

fn session_link_from_local(link: LocalSessionLink) -> SessionLink {
    SessionLink {
        session_id: link.session_id,
        linked_session_id: link.linked_session_id,
        link_type: map_link_type(&link.link_type),
        created_at: link.created_at,
    }
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

fn vector_embed_endpoint(runtime: &DaemonConfig) -> String {
    let configured = runtime.vector_search.endpoint.trim();
    if !configured.is_empty() {
        return configured.trim_end_matches('/').to_string();
    }
    "http://127.0.0.1:11434".to_string()
}

fn vector_embed_model(runtime: &DaemonConfig) -> String {
    let configured = runtime.vector_search.model.trim();
    if configured.is_empty() {
        return "bge-m3".to_string();
    }
    configured.to_string()
}

fn parse_embedding_vector(value: &serde_json::Value) -> Option<Vec<f32>> {
    let items = value.as_array()?;
    let mut output = Vec::with_capacity(items.len());
    for item in items {
        if let Some(number) = item.as_f64() {
            output.push(number as f32);
            continue;
        }
        if let Some(number) = item.as_i64() {
            output.push(number as f32);
            continue;
        }
        if let Some(number) = item.as_u64() {
            output.push(number as f32);
            continue;
        }
        return None;
    }
    Some(output)
}

fn parse_embeddings_payload(payload: &serde_json::Value) -> Option<Vec<Vec<f32>>> {
    if let Some(vectors) = payload.get("embeddings").and_then(|value| value.as_array()) {
        let mut out = Vec::with_capacity(vectors.len());
        for value in vectors {
            out.push(parse_embedding_vector(value)?);
        }
        return Some(out);
    }
    payload
        .get("embedding")
        .and_then(parse_embedding_vector)
        .map(|vector| vec![vector])
}

fn request_ollama_embeddings(
    endpoint: &str,
    model: &str,
    inputs: &[String],
) -> DesktopApiResult<Vec<Vec<f32>>> {
    if inputs.is_empty() {
        return Ok(Vec::new());
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(25))
        .build()
        .map_err(|error| {
            desktop_error(
                "desktop.vector_search_client_failed",
                500,
                "failed to initialize vector search client",
                Some(json!({ "cause": error.to_string() })),
            )
        })?;

    let base = endpoint.trim_end_matches('/');
    let batch_url = format!("{base}/api/embed");
    match client
        .post(&batch_url)
        .json(&json!({ "model": model, "input": inputs }))
        .send()
    {
        Ok(response) if response.status().is_success() => {
            let payload: serde_json::Value = response.json().map_err(|error| {
                desktop_error(
                    "desktop.vector_search_parse_failed",
                    502,
                    "failed to parse embedding response",
                    Some(json!({ "cause": error.to_string(), "endpoint": batch_url })),
                )
            })?;
            if let Some(vectors) = parse_embeddings_payload(&payload) {
                if vectors.len() == inputs.len() {
                    return Ok(vectors);
                }
            }
        }
        _ => {}
    }

    let single_url = format!("{base}/api/embeddings");
    let mut vectors = Vec::with_capacity(inputs.len());
    for input in inputs {
        let response = client
            .post(&single_url)
            .json(&json!({ "model": model, "prompt": input }))
            .send()
            .map_err(|error| {
                desktop_error(
                    "desktop.vector_search_unavailable",
                    422,
                    "vector search endpoint is unavailable",
                    Some(json!({
                        "cause": error.to_string(),
                        "endpoint": single_url,
                        "hint": "start local ollama and ensure embeddings endpoint is reachable"
                    })),
                )
            })?;
        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().unwrap_or_default();
            return Err(desktop_error(
                "desktop.vector_search_unavailable",
                422,
                "vector search endpoint returned an error",
                Some(json!({
                    "endpoint": single_url,
                    "status": status,
                    "body": body,
                    "hint": "verify embedding model exists in local ollama"
                })),
            ));
        }
        let payload: serde_json::Value = response.json().map_err(|error| {
            desktop_error(
                "desktop.vector_search_parse_failed",
                502,
                "failed to parse embedding response",
                Some(json!({ "cause": error.to_string(), "endpoint": single_url })),
            )
        })?;
        let vector = parse_embeddings_payload(&payload)
            .and_then(|mut list| list.pop())
            .ok_or_else(|| {
                desktop_error(
                    "desktop.vector_search_parse_failed",
                    502,
                    "embedding response missing vector payload",
                    Some(json!({ "endpoint": single_url })),
                )
            })?;
        vectors.push(vector);
    }

    Ok(vectors)
}

fn request_ollama_embeddings_in_batches(
    endpoint: &str,
    model: &str,
    inputs: &[String],
) -> DesktopApiResult<Vec<Vec<f32>>> {
    if inputs.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::with_capacity(inputs.len());
    for chunk in inputs.chunks(VECTOR_EMBED_BATCH_SIZE) {
        let vectors = request_ollama_embeddings(endpoint, model, chunk)?;
        out.extend(vectors);
    }
    Ok(out)
}

fn current_vector_install_state() -> VectorInstallRuntimeState {
    VECTOR_INSTALL_STATE
        .lock()
        .expect("vector install state mutex poisoned")
        .clone()
}

fn set_vector_install_state(update: VectorInstallRuntimeState) {
    *VECTOR_INSTALL_STATE
        .lock()
        .expect("vector install state mutex poisoned") = update;
}

fn update_vector_install_progress(
    state: DesktopVectorInstallState,
    model: &str,
    progress_pct: u8,
    message: Option<String>,
) {
    set_vector_install_state(VectorInstallRuntimeState {
        state,
        model: model.to_string(),
        progress_pct: progress_pct.min(100),
        message,
    });
}

fn parse_ollama_model_list(payload: &serde_json::Value, target_model: &str) -> bool {
    let Some(models) = payload.get("models").and_then(|value| value.as_array()) else {
        return false;
    };
    let target = target_model.trim();
    if target.is_empty() {
        return false;
    }
    models.iter().any(|item| {
        item.get("name")
            .and_then(|value| value.as_str())
            .map(|name| name == target || name.starts_with(&format!("{target}:")))
            .unwrap_or(false)
    })
}

fn vector_preflight_for_runtime(runtime: &DaemonConfig) -> DesktopVectorPreflightResponse {
    let endpoint = vector_embed_endpoint(runtime);
    let model = vector_embed_model(runtime);
    let mut response = DesktopVectorPreflightResponse {
        provider: DesktopVectorSearchProvider::Ollama,
        endpoint: endpoint.clone(),
        model: model.clone(),
        ollama_reachable: false,
        model_installed: false,
        install_state: DesktopVectorInstallState::NotInstalled,
        progress_pct: 0,
        message: None,
    };

    let install_state = current_vector_install_state();
    response.install_state = install_state.state;
    response.progress_pct = install_state.progress_pct;
    response.message = install_state.message;

    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            response.message = Some(format!("failed to initialize HTTP client: {error}"));
            return response;
        }
    };

    let tags_url = format!("{}/api/tags", endpoint.trim_end_matches('/'));
    let tags_response = match client.get(tags_url).send() {
        Ok(resp) => resp,
        Err(error) => {
            response.message = Some(format!(
                "ollama is not reachable; start it with `ollama serve` ({error})"
            ));
            return response;
        }
    };
    if !tags_response.status().is_success() {
        response.message = Some(format!(
            "ollama returned unexpected status {}",
            tags_response.status()
        ));
        return response;
    }

    response.ollama_reachable = true;
    let payload: serde_json::Value = match tags_response.json() {
        Ok(payload) => payload,
        Err(error) => {
            response.message = Some(format!("failed to parse ollama model list: {error}"));
            return response;
        }
    };
    response.model_installed = parse_ollama_model_list(&payload, &model);
    if response.model_installed {
        response.install_state = DesktopVectorInstallState::Ready;
        response.progress_pct = 100;
        if response.message.is_none() {
            response.message = Some("model is installed and ready".to_string());
        }
    } else if matches!(response.install_state, DesktopVectorInstallState::Ready) {
        response.install_state = DesktopVectorInstallState::NotInstalled;
        response.progress_pct = 0;
    }
    response
}

fn install_ollama_model_blocking(endpoint: &str, model: &str) -> DesktopApiResult<()> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(|error| {
            desktop_error(
                "desktop.vector_install_client_failed",
                500,
                "failed to initialize vector install client",
                Some(json!({ "cause": error.to_string() })),
            )
        })?;

    let pull_url = format!("{}/api/pull", endpoint.trim_end_matches('/'));
    let response = client
        .post(&pull_url)
        .json(&json!({ "model": model, "stream": true }))
        .send()
        .map_err(|error| {
            desktop_error(
                "desktop.vector_install_unavailable",
                422,
                "failed to connect to ollama model pull endpoint",
                Some(json!({
                    "cause": error.to_string(),
                    "endpoint": pull_url,
                    "hint": "start local ollama with `ollama serve`"
                })),
            )
        })?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.text().unwrap_or_default();
        return Err(desktop_error(
            "desktop.vector_install_failed",
            422,
            "ollama model pull failed",
            Some(json!({ "endpoint": pull_url, "status": status, "body": body })),
        ));
    }

    let mut reader = BufReader::new(response);
    let mut line = String::new();
    loop {
        line.clear();
        let bytes = reader.read_line(&mut line).map_err(|error| {
            desktop_error(
                "desktop.vector_install_failed",
                500,
                "failed while reading model pull stream",
                Some(json!({ "cause": error.to_string(), "endpoint": pull_url })),
            )
        })?;
        if bytes == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(payload) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if let Some(error_text) = payload.get("error").and_then(|value| value.as_str()) {
                return Err(desktop_error(
                    "desktop.vector_install_failed",
                    422,
                    "ollama model pull failed",
                    Some(json!({ "endpoint": pull_url, "error": error_text })),
                ));
            }
            let status = payload
                .get("status")
                .and_then(|value| value.as_str())
                .map(str::to_string);
            let completed = payload.get("completed").and_then(|value| value.as_u64());
            let total = payload.get("total").and_then(|value| value.as_u64());
            let progress_pct = match (completed, total) {
                (Some(done), Some(total)) if total > 0 => ((done * 100) / total).min(100) as u8,
                _ => 0,
            };
            if status.is_some() || progress_pct > 0 {
                update_vector_install_progress(
                    DesktopVectorInstallState::Installing,
                    model,
                    progress_pct,
                    status,
                );
            }
        }
    }

    Ok(())
}

fn cosine_similarity(lhs: &[f32], rhs: &[f32]) -> f32 {
    if lhs.is_empty() || rhs.is_empty() || lhs.len() != rhs.len() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut lhs_norm = 0.0f32;
    let mut rhs_norm = 0.0f32;
    for (l, r) in lhs.iter().zip(rhs.iter()) {
        dot += l * r;
        lhs_norm += l * l;
        rhs_norm += r * r;
    }
    if lhs_norm <= f32::EPSILON || rhs_norm <= f32::EPSILON {
        return 0.0;
    }
    dot / (lhs_norm.sqrt() * rhs_norm.sqrt())
}

fn parse_vector_cursor(cursor: Option<String>) -> usize {
    cursor
        .as_deref()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .unwrap_or(0)
}

fn truncate_snippet(raw: &str, max_chars: usize) -> String {
    let trimmed = raw.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let mut out = String::with_capacity(max_chars + 3);
    for ch in trimmed.chars().take(max_chars) {
        out.push(ch);
    }
    out.push_str("...");
    out
}

fn ensure_vector_enabled_and_ready(
    runtime: &DaemonConfig,
) -> DesktopApiResult<DesktopVectorPreflightResponse> {
    if !runtime.vector_search.enabled {
        return Err(desktop_error(
            "desktop.vector_search_disabled",
            422,
            "semantic vector search is disabled in runtime settings",
            Some(json!({ "hint": "enable vector_search in Settings and save runtime settings" })),
        ));
    }
    let preflight = vector_preflight_for_runtime(runtime);
    if !preflight.ollama_reachable {
        return Err(desktop_error(
            "desktop.vector_search_unavailable",
            422,
            "ollama endpoint is not reachable",
            Some(json!({
                "endpoint": preflight.endpoint,
                "hint": "start local ollama with `ollama serve`"
            })),
        ));
    }
    if !preflight.model_installed {
        return Err(desktop_error(
            "desktop.vector_model_not_installed",
            422,
            "embedding model is not installed",
            Some(json!({
                "model": preflight.model,
                "hint": "use Settings > Vector Search > Install model"
            })),
        ));
    }
    Ok(preflight)
}

fn score_vector_sessions(
    query_vector: &[f32],
    candidates: Vec<opensession_local_db::VectorChunkCandidateRow>,
) -> Vec<VectorMatchScore> {
    let mut by_session: HashMap<String, VectorMatchScore> = HashMap::new();
    for candidate in candidates {
        let score = cosine_similarity(query_vector, &candidate.embedding);
        if !score.is_finite() {
            continue;
        }
        let entry = by_session
            .entry(candidate.session_id.clone())
            .or_insert_with(|| VectorMatchScore {
                session_id: candidate.session_id.clone(),
                chunk_id: candidate.chunk_id.clone(),
                start_line: candidate.start_line,
                end_line: candidate.end_line,
                snippet: truncate_snippet(&candidate.content, 260),
                best_score: score,
                hit_count: 0,
            });
        entry.hit_count = entry.hit_count.saturating_add(1);
        if score > entry.best_score {
            entry.best_score = score;
            entry.chunk_id = candidate.chunk_id;
            entry.start_line = candidate.start_line;
            entry.end_line = candidate.end_line;
            entry.snippet = truncate_snippet(&candidate.content, 260);
        }
    }
    let mut ranked = by_session.into_values().collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        let right_weighted = right.best_score + (right.hit_count as f32 * 0.01);
        let left_weighted = left.best_score + (left.hit_count as f32 * 0.01);
        right_weighted.total_cmp(&left_weighted)
    });
    ranked
}

fn session_matches_vector_filter(
    summary: &SessionSummary,
    filter: Option<&LocalSessionFilter>,
) -> bool {
    let Some(filter) = filter else {
        return true;
    };

    if let Some(tool) = filter.tool.as_deref() {
        if summary.tool != tool {
            return false;
        }
    }
    if let Some(repo) = filter.git_repo_name.as_deref() {
        if summary.git_repo_name.as_deref() != Some(repo) {
            return false;
        }
    }

    if !matches!(filter.time_range, opensession_local_db::LocalTimeRange::All) {
        let created_at = match chrono::DateTime::parse_from_rfc3339(&summary.created_at) {
            Ok(parsed) => parsed.with_timezone(&chrono::Utc),
            Err(_) => return false,
        };
        let now = chrono::Utc::now();
        let min_allowed = match filter.time_range {
            opensession_local_db::LocalTimeRange::Hours24 => now - chrono::Duration::hours(24),
            opensession_local_db::LocalTimeRange::Days7 => now - chrono::Duration::days(7),
            opensession_local_db::LocalTimeRange::Days30 => now - chrono::Duration::days(30),
            opensession_local_db::LocalTimeRange::All => now - chrono::Duration::days(36500),
        };
        if created_at < min_allowed {
            return false;
        }
    }

    true
}

fn search_sessions_vector_internal(
    db: &LocalDb,
    runtime: &DaemonConfig,
    query_text: &str,
    cursor: Option<String>,
    limit: Option<u32>,
    filter: Option<&LocalSessionFilter>,
) -> DesktopApiResult<DesktopVectorSearchResponse> {
    let normalized_query = query_text.trim();
    if normalized_query.is_empty() {
        return Err(desktop_error(
            "desktop.vector_search_query_required",
            422,
            "vector search query is empty",
            Some(json!({ "hint": "provide a non-empty query" })),
        ));
    }

    let _preflight = ensure_vector_enabled_and_ready(runtime)?;
    let endpoint = vector_embed_endpoint(runtime);
    let model = vector_embed_model(runtime);
    let query_embeddings =
        request_ollama_embeddings(&endpoint, &model, &[normalized_query.to_string()])?;
    let query_vector = query_embeddings.into_iter().next().unwrap_or_default();
    if query_vector.is_empty() {
        return Err(desktop_error(
            "desktop.vector_search_parse_failed",
            502,
            "embedding response missing query vector",
            Some(json!({ "model": model, "endpoint": endpoint })),
        ));
    }

    let candidate_limit = (runtime.vector_search.top_k_chunks.max(1) as u32)
        .saturating_mul(VECTOR_FTS_CANDIDATE_LIMIT_MULTIPLIER);
    let mut candidates = db
        .list_vector_chunk_candidates(normalized_query, &model, candidate_limit)
        .map_err(|error| {
            desktop_error(
                "desktop.vector_search_db_failed",
                500,
                "failed to load vector chunk candidates",
                Some(json!({ "cause": error.to_string() })),
            )
        })?;
    if candidates.is_empty() {
        candidates = db
            .list_recent_vector_chunks_for_model(&model, candidate_limit)
            .map_err(|error| {
                desktop_error(
                    "desktop.vector_search_db_failed",
                    500,
                    "failed to load fallback vector chunk candidates",
                    Some(json!({ "cause": error.to_string() })),
                )
            })?;
    }

    let mut ranked = score_vector_sessions(&query_vector, candidates);
    let top_sessions = runtime.vector_search.top_k_sessions.max(1) as usize;
    if ranked.len() > top_sessions {
        ranked.truncate(top_sessions);
    }
    let offset = parse_vector_cursor(cursor);
    let page_limit = limit.unwrap_or(20).clamp(1, 100) as usize;

    let mut materialized = Vec::new();
    for scored in ranked {
        let Some(row) = db.get_session_by_id(&scored.session_id).map_err(|error| {
            desktop_error(
                "desktop.vector_search_db_failed",
                500,
                "failed to read session row for vector search result",
                Some(json!({ "cause": error.to_string(), "session_id": scored.session_id })),
            )
        })?
        else {
            continue;
        };
        let normalized = (scored.best_score.clamp(-1.0, 1.0) * 10_000.0).round() as i64;
        let summary =
            session_summary_from_local_row_with_score(row, normalized, "vector_ollama_bge_m3_v2");
        if !session_matches_vector_filter(&summary, filter) {
            continue;
        }
        materialized.push(DesktopVectorSessionMatch {
            session: summary,
            score: scored.best_score,
            chunk_id: scored.chunk_id,
            start_line: scored.start_line,
            end_line: scored.end_line,
            snippet: scored.snippet,
        });
    }

    let total_candidates = materialized.len() as u32;
    let sessions = materialized
        .into_iter()
        .skip(offset)
        .take(page_limit)
        .collect::<Vec<_>>();

    let next_offset = offset.saturating_add(sessions.len());
    let next_cursor = (next_offset < total_candidates as usize).then_some(next_offset.to_string());

    Ok(DesktopVectorSearchResponse {
        query: normalized_query.to_string(),
        sessions,
        next_cursor,
        total_candidates,
    })
}

fn list_sessions_with_vector_rank(
    db: &LocalDb,
    base_filter: &LocalSessionFilter,
    query_text: &str,
    page: u32,
    per_page: u32,
) -> DesktopApiResult<SessionListResponse> {
    let runtime = load_runtime_config()?;
    let offset = (page.saturating_sub(1)).saturating_mul(per_page) as usize;
    let response = search_sessions_vector_internal(
        db,
        &runtime,
        query_text,
        Some(offset.to_string()),
        Some(per_page),
        Some(base_filter),
    )?;
    Ok(SessionListResponse {
        sessions: response
            .sessions
            .into_iter()
            .map(|hit| hit.session)
            .collect(),
        total: response.total_candidates as i64,
        page,
        per_page,
    })
}

fn push_non_empty_lines(raw: &str, lines: &mut Vec<String>) {
    for line in raw.lines() {
        let normalized = line.trim_end_matches('\r');
        if normalized.trim().is_empty() {
            continue;
        }
        lines.push(normalized.to_string());
    }
}

fn extract_vector_lines(session: &HailSession) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    if let Some(title) = session.context.title.as_deref() {
        push_non_empty_lines(title, &mut lines);
    }
    if let Some(description) = session.context.description.as_deref() {
        push_non_empty_lines(description, &mut lines);
    }

    for event in &session.events {
        for block in &event.content.blocks {
            match block {
                opensession_core::trace::ContentBlock::Text { text } => {
                    push_non_empty_lines(text, &mut lines);
                }
                opensession_core::trace::ContentBlock::Code { code, .. } => {
                    push_non_empty_lines(code, &mut lines);
                }
                opensession_core::trace::ContentBlock::File { path, content } => {
                    push_non_empty_lines(path, &mut lines);
                    if let Some(content) = content.as_deref() {
                        push_non_empty_lines(content, &mut lines);
                    }
                }
                opensession_core::trace::ContentBlock::Json { data } => {
                    if let Ok(serialized) = serde_json::to_string(data) {
                        push_non_empty_lines(&serialized, &mut lines);
                    }
                }
                opensession_core::trace::ContentBlock::Reference { uri, .. } => {
                    push_non_empty_lines(uri, &mut lines);
                }
                opensession_core::trace::ContentBlock::Image { alt, .. } => {
                    if let Some(alt) = alt.as_deref() {
                        push_non_empty_lines(alt, &mut lines);
                    }
                }
                opensession_core::trace::ContentBlock::Video { .. }
                | opensession_core::trace::ContentBlock::Audio { .. } => {}
                _ => {}
            }
        }
    }

    if lines.is_empty() {
        lines.push(session.session_id.clone());
    }
    lines
}

fn build_vector_chunks_for_session(
    session: &HailSession,
    source_hash: &str,
    runtime: &DaemonConfig,
) -> Vec<VectorChunkUpsert> {
    let lines = extract_vector_lines(session);
    let chunk_size = runtime.vector_search.chunk_size_lines.max(1) as usize;
    let overlap = runtime
        .vector_search
        .chunk_overlap_lines
        .min(runtime.vector_search.chunk_size_lines.saturating_sub(1)) as usize;
    let step = chunk_size.saturating_sub(overlap).max(1);

    let mut chunks = Vec::new();
    let mut start = 0usize;
    let mut chunk_index = 0u32;
    while start < lines.len() {
        let end = (start + chunk_size).min(lines.len());
        let content = lines[start..end].join("\n");
        let content_hash = sha256_hex(content.as_bytes());
        let chunk_key = format!(
            "{}:{}:{}:{}:{}",
            session.session_id,
            source_hash,
            chunk_index,
            start + 1,
            end
        );
        chunks.push(VectorChunkUpsert {
            chunk_id: sha256_hex(chunk_key.as_bytes()),
            session_id: session.session_id.clone(),
            chunk_index,
            start_line: (start + 1) as u32,
            end_line: end as u32,
            line_count: (end - start) as u32,
            content,
            content_hash,
            embedding: Vec::new(),
        });

        if end == lines.len() {
            break;
        }
        start = start.saturating_add(step);
        chunk_index = chunk_index.saturating_add(1);
    }
    chunks
}

fn map_vector_index_state(raw: &str) -> DesktopVectorIndexState {
    match raw {
        "running" => DesktopVectorIndexState::Running,
        "complete" => DesktopVectorIndexState::Complete,
        "failed" => DesktopVectorIndexState::Failed,
        _ => DesktopVectorIndexState::Idle,
    }
}

fn desktop_vector_index_status_from_db(
    db: &LocalDb,
) -> DesktopApiResult<DesktopVectorIndexStatusResponse> {
    let row = db.get_vector_index_job().map_err(|error| {
        desktop_error(
            "desktop.vector_index_status_failed",
            500,
            "failed to read vector indexing status",
            Some(json!({ "cause": error.to_string() })),
        )
    })?;

    let Some(row) = row else {
        return Ok(DesktopVectorIndexStatusResponse {
            state: DesktopVectorIndexState::Idle,
            processed_sessions: 0,
            total_sessions: 0,
            message: None,
            started_at: None,
            finished_at: None,
        });
    };

    Ok(DesktopVectorIndexStatusResponse {
        state: map_vector_index_state(&row.status),
        processed_sessions: row.processed_sessions,
        total_sessions: row.total_sessions,
        message: row.message,
        started_at: row.started_at,
        finished_at: row.finished_at,
    })
}

fn set_vector_index_job_snapshot(db: &LocalDb, payload: VectorIndexJobRow) -> DesktopApiResult<()> {
    db.set_vector_index_job(&payload).map_err(|error| {
        desktop_error(
            "desktop.vector_index_status_failed",
            500,
            "failed to persist vector indexing status",
            Some(json!({ "cause": error.to_string() })),
        )
    })
}

fn rebuild_vector_index_blocking(db: &LocalDb, runtime: &DaemonConfig) -> DesktopApiResult<()> {
    let mut filter = LocalSessionFilter::default();
    filter.limit = None;
    filter.offset = None;
    let sessions = db.list_sessions(&filter).map_err(|error| {
        desktop_error(
            "desktop.vector_index_list_failed",
            500,
            "failed to list sessions for vector indexing",
            Some(json!({ "cause": error.to_string() })),
        )
    })?;
    let total_sessions = sessions.len() as u32;
    let started_at = chrono::Utc::now().to_rfc3339();
    set_vector_index_job_snapshot(
        db,
        VectorIndexJobRow {
            status: "running".to_string(),
            processed_sessions: 0,
            total_sessions,
            message: Some("indexing session chunks".to_string()),
            started_at: Some(started_at.clone()),
            finished_at: None,
        },
    )?;

    let endpoint = vector_embed_endpoint(runtime);
    let model = vector_embed_model(runtime);
    for (idx, row) in sessions.iter().enumerate() {
        let processed_sessions = idx as u32;
        set_vector_index_job_snapshot(
            db,
            VectorIndexJobRow {
                status: "running".to_string(),
                processed_sessions,
                total_sessions,
                message: Some(format!("indexing {}", row.id)),
                started_at: Some(started_at.clone()),
                finished_at: None,
            },
        )?;

        let normalized = match load_normalized_session_body(db, &row.id) {
            Ok(body) => body,
            Err(_) => {
                continue;
            }
        };
        let source_hash = sha256_hex(normalized.as_bytes());
        let already_indexed = db
            .vector_index_source_hash(&row.id)
            .map_err(|error| {
                desktop_error(
                    "desktop.vector_index_status_failed",
                    500,
                    "failed to read vector source hash",
                    Some(json!({ "cause": error.to_string(), "session_id": row.id })),
                )
            })?
            .is_some_and(|hash| hash == source_hash);
        if already_indexed {
            continue;
        }

        let session = match HailSession::from_jsonl(&normalized) {
            Ok(session) => session,
            Err(_) => {
                continue;
            }
        };
        let mut chunks = build_vector_chunks_for_session(&session, &source_hash, runtime);
        if chunks.is_empty() {
            db.replace_session_vector_chunks(&row.id, &source_hash, &model, &[])
                .map_err(|error| {
                    desktop_error(
                        "desktop.vector_index_write_failed",
                        500,
                        "failed to clear vector chunks for empty session",
                        Some(json!({ "cause": error.to_string(), "session_id": row.id })),
                    )
                })?;
            continue;
        }

        let inputs = chunks
            .iter()
            .map(|chunk| chunk.content.clone())
            .collect::<Vec<_>>();
        let embeddings = request_ollama_embeddings_in_batches(&endpoint, &model, &inputs)?;
        if embeddings.len() != chunks.len() {
            return Err(desktop_error(
                "desktop.vector_index_embedding_mismatch",
                502,
                "embedding response count mismatch while indexing session",
                Some(json!({
                    "session_id": row.id,
                    "requested": chunks.len(),
                    "received": embeddings.len(),
                    "model": model,
                })),
            ));
        }

        for (chunk, embedding) in chunks.iter_mut().zip(embeddings.into_iter()) {
            chunk.embedding = embedding;
        }
        db.replace_session_vector_chunks(&row.id, &source_hash, &model, &chunks)
            .map_err(|error| {
                desktop_error(
                    "desktop.vector_index_write_failed",
                    500,
                    "failed to persist vector chunks",
                    Some(json!({ "cause": error.to_string(), "session_id": row.id })),
                )
            })?;
    }

    set_vector_index_job_snapshot(
        db,
        VectorIndexJobRow {
            status: "complete".to_string(),
            processed_sessions: total_sessions,
            total_sessions,
            message: Some("vector indexing complete".to_string()),
            started_at: Some(started_at),
            finished_at: Some(chrono::Utc::now().to_rfc3339()),
        },
    )?;
    Ok(())
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
    }
}

fn map_session_default_view_from_str(raw: &str) -> Option<SessionDefaultView> {
    match raw.trim() {
        "full" => Some(SessionDefaultView::Full),
        "compressed" => Some(SessionDefaultView::Compressed),
        _ => None,
    }
}

fn session_to_hail_jsonl(session: HailSession) -> DesktopApiResult<String> {
    session.to_jsonl().map_err(|error| {
        desktop_error(
            "desktop.hail_encode_failed",
            500,
            "failed to encode normalized HAIL JSONL",
            Some(json!({ "cause": error.to_string() })),
        )
    })
}

fn normalize_session_body_to_hail_jsonl(
    body: &str,
    source_path: Option<&str>,
) -> DesktopApiResult<String> {
    if let Ok(session) = HailSession::from_jsonl(body) {
        return session_to_hail_jsonl(session);
    }

    if let Ok(session) = serde_json::from_str::<HailSession>(body) {
        return session_to_hail_jsonl(session);
    }

    let filename = source_path
        .and_then(|path| Path::new(path).file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("session.jsonl");

    let preview = preview_parse_bytes(filename, body.as_bytes(), None).map_err(|error| {
        desktop_error(
            "desktop.session_parse_failed",
            422,
            "failed to parse source session into HAIL format",
            Some(json!({ "cause": error.to_string(), "filename": filename })),
        )
    })?;

    session_to_hail_jsonl(preview.session)
}

fn read_and_normalize_source_session(source_path: &str) -> DesktopApiResult<String> {
    let body = std::fs::read_to_string(source_path).map_err(|error| {
        desktop_error(
            "desktop.session_source_unavailable",
            404,
            format!("session source file is unavailable ({source_path})"),
            Some(json!({ "cause": error.to_string(), "source_path": source_path })),
        )
    })?;
    normalize_session_body_to_hail_jsonl(&body, Some(source_path))
}

fn load_normalized_session_body(db: &LocalDb, session_id: &str) -> DesktopApiResult<String> {
    let source_path = db.get_session_source_path(session_id).map_err(|error| {
        desktop_error(
            "desktop.session_source_path_failed",
            500,
            "failed to resolve session source path",
            Some(json!({ "cause": error.to_string(), "session_id": session_id })),
        )
    })?;

    if let Some(cached) = db.get_cached_body(session_id).map_err(|error| {
        desktop_error(
            "desktop.session_cache_read_failed",
            500,
            "failed to read cached session body",
            Some(json!({ "cause": error.to_string(), "session_id": session_id })),
        )
    })? {
        match String::from_utf8(cached) {
            Ok(text) => match normalize_session_body_to_hail_jsonl(&text, source_path.as_deref()) {
                Ok(normalized) => return Ok(normalized),
                Err(error) if source_path.is_none() => return Err(error),
                Err(_) => {}
            },
            Err(error) if source_path.is_none() => {
                return Err(desktop_error(
                    "desktop.session_cache_invalid_utf8",
                    500,
                    "session body is not valid UTF-8",
                    Some(json!({ "cause": error.to_string(), "session_id": session_id })),
                ));
            }
            Err(_) => {}
        }
    }

    if let Some(source_path) = source_path {
        return read_and_normalize_source_session(&source_path);
    }

    Err(desktop_error(
        "desktop.session_body_not_found",
        404,
        "session body not found in local cache",
        Some(json!({ "session_id": session_id })),
    ))
}

#[derive(Debug, Deserialize)]
struct CliQuickSharePayload {
    uri: String,
    source_uri: String,
    remote: String,
    push_cmd: String,
    pushed: bool,
    #[serde(default)]
    auto_push_consent: bool,
}

fn parse_cli_quick_share_response(stdout: &str) -> DesktopApiResult<DesktopQuickShareResponse> {
    let payload: CliQuickSharePayload = serde_json::from_str(stdout).map_err(|error| {
        desktop_error(
            "desktop.quick_share_parse_failed",
            500,
            "failed to decode quick-share response from CLI",
            Some(json!({ "cause": error.to_string(), "stdout": stdout })),
        )
    })?;
    Ok(DesktopQuickShareResponse {
        source_uri: payload.source_uri,
        shared_uri: payload.uri,
        remote: payload.remote,
        push_cmd: payload.push_cmd,
        pushed: payload.pushed,
        auto_push_consent: payload.auto_push_consent,
    })
}

fn canonicalize_summaries(summaries: &[HandoffSummary]) -> DesktopApiResult<String> {
    let mut sorted = summaries
        .iter()
        .map(|summary| {
            serde_json::to_value(summary)
                .map(|value| (summary.source_session_id.clone(), value))
                .map_err(|error| {
                    desktop_error(
                        "desktop.handoff_serialize_failed",
                        500,
                        "failed to serialize handoff summary",
                        Some(json!({ "cause": error.to_string() })),
                    )
                })
        })
        .collect::<DesktopApiResult<Vec<_>>>()?;
    sorted.sort_by(|left, right| left.0.cmp(&right.0));

    let mut out = String::new();
    for (_session_id, value) in sorted {
        let line = serde_json::to_string(&value).map_err(|error| {
            desktop_error(
                "desktop.handoff_serialize_failed",
                500,
                "failed to serialize canonical handoff line",
                Some(json!({ "cause": error.to_string() })),
            )
        })?;
        out.push_str(&line);
        out.push('\n');
    }
    Ok(out)
}

fn artifact_root_for_cwd(cwd: &Path) -> DesktopApiResult<PathBuf> {
    if let Some(repo_root) = find_repo_root(cwd) {
        return Ok(repo_root.join(".opensession").join("artifacts"));
    }
    let global_objects_root = global_store_root().map_err(|error| {
        desktop_error(
            "desktop.handoff_store_unavailable",
            500,
            "failed to resolve global object store",
            Some(json!({ "cause": error.to_string() })),
        )
    })?;
    let parent = global_objects_root.parent().ok_or_else(|| {
        desktop_error(
            "desktop.handoff_store_unavailable",
            500,
            "invalid global object store path",
            Some(json!({ "path": global_objects_root })),
        )
    })?;
    Ok(parent.join("artifacts"))
}

fn is_valid_sha256(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn artifact_path_for_hash(root: &Path, hash: &str) -> DesktopApiResult<PathBuf> {
    if !is_valid_sha256(hash) {
        return Err(desktop_error(
            "desktop.handoff_invalid_hash",
            400,
            "invalid artifact hash",
            Some(json!({ "hash": hash })),
        ));
    }
    Ok(root
        .join("sha256")
        .join(&hash[0..2])
        .join(&hash[2..4])
        .join(format!("{hash}.json")))
}

fn validate_pin_alias(alias: &str) -> DesktopApiResult<()> {
    let trimmed = alias.trim();
    if trimmed.is_empty() {
        return Err(desktop_error(
            "desktop.handoff_invalid_alias",
            400,
            "pin alias cannot be empty",
            Some(json!({ "alias": alias })),
        ));
    }
    if !trimmed
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'_' || byte == b'-')
    {
        return Err(desktop_error(
            "desktop.handoff_invalid_alias",
            400,
            "pin alias contains invalid characters",
            Some(json!({ "alias": alias })),
        ));
    }
    Ok(())
}

fn pin_path_for_alias(root: &Path, alias: &str) -> DesktopApiResult<PathBuf> {
    validate_pin_alias(alias)?;
    Ok(root.join("pins").join(alias))
}

fn store_handoff_artifact_record(
    record: &DesktopHandoffArtifactRecord,
    cwd: &Path,
) -> DesktopApiResult<()> {
    let root = artifact_root_for_cwd(cwd)?;
    let path = artifact_path_for_hash(&root, &record.sha256)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            desktop_error(
                "desktop.handoff_store_failed",
                500,
                "failed to prepare handoff artifact directory",
                Some(json!({ "cause": error.to_string(), "path": parent })),
            )
        })?;
    }
    if !path.exists() {
        let bytes = serde_json::to_vec_pretty(record).map_err(|error| {
            desktop_error(
                "desktop.handoff_store_failed",
                500,
                "failed to serialize handoff artifact record",
                Some(json!({ "cause": error.to_string() })),
            )
        })?;
        std::fs::write(&path, bytes).map_err(|error| {
            desktop_error(
                "desktop.handoff_store_failed",
                500,
                "failed to store handoff artifact",
                Some(json!({ "cause": error.to_string(), "path": path })),
            )
        })?;
    }
    Ok(())
}

fn set_handoff_pin(alias: &str, hash: &str, cwd: &Path) -> DesktopApiResult<()> {
    validate_pin_alias(alias)?;
    if !is_valid_sha256(hash) {
        return Err(desktop_error(
            "desktop.handoff_invalid_hash",
            400,
            "invalid artifact hash",
            Some(json!({ "hash": hash })),
        ));
    }

    let root = artifact_root_for_cwd(cwd)?;
    let pin_path = pin_path_for_alias(&root, alias)?;
    if let Some(parent) = pin_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            desktop_error(
                "desktop.handoff_store_failed",
                500,
                "failed to prepare handoff pin directory",
                Some(json!({ "cause": error.to_string(), "path": parent })),
            )
        })?;
    }

    std::fs::write(&pin_path, format!("{hash}\n")).map_err(|error| {
        desktop_error(
            "desktop.handoff_store_failed",
            500,
            "failed to write handoff pin alias",
            Some(json!({ "cause": error.to_string(), "path": pin_path, "alias": alias })),
        )
    })
}

fn build_handoff_artifact_record(
    normalized_session: &str,
    session: HailSession,
    pin_latest: bool,
    cwd: &Path,
) -> DesktopApiResult<DesktopHandoffBuildResponse> {
    let summaries = vec![HandoffSummary::from_session(&session)];
    let reports = validate_handoff_summaries(&summaries);
    let has_error_level = reports.iter().any(|report| {
        report
            .findings
            .iter()
            .any(|finding| finding.severity == "error")
    });
    if has_error_level {
        return Err(desktop_error(
            "desktop.handoff_validation_failed",
            422,
            "handoff validation failed with error-level findings",
            Some(json!({ "reports": reports })),
        ));
    }

    let canonical_jsonl = canonicalize_summaries(&summaries)?;
    let artifact_hash = sha256_hex(canonical_jsonl.as_bytes());

    let source_object =
        store_local_object(normalized_session.as_bytes(), cwd).map_err(|error| {
            desktop_error(
                "desktop.handoff_store_failed",
                500,
                "failed to store canonical source object for handoff",
                Some(json!({ "cause": error.to_string() })),
            )
        })?;

    let validation_reports = reports
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            desktop_error(
                "desktop.handoff_serialize_failed",
                500,
                "failed to serialize handoff validation report",
                Some(json!({ "cause": error.to_string() })),
            )
        })?;

    let mut deduped_source_uris = BTreeSet::new();
    deduped_source_uris.insert(source_object.uri.to_string());
    let runtime = load_runtime_config().unwrap_or_default();

    let record = DesktopHandoffArtifactRecord {
        version: HANDOFF_RECORD_VERSION.to_string(),
        sha256: artifact_hash.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
        source_uris: deduped_source_uris.into_iter().collect(),
        canonical_jsonl,
        raw_sessions: vec![session],
        summary_meta: Some(DesktopHandoffSummaryMeta {
            session_default_view: enum_label(&runtime.daemon.session_default_view),
            summary_source_mode: enum_label(&runtime.summary.source_mode),
            summary_provider: enum_label(&runtime.summary.provider.id),
        }),
        validation_reports,
    };
    store_handoff_artifact_record(&record, cwd)?;

    if pin_latest {
        set_handoff_pin(HANDOFF_LATEST_PIN_ALIAS, &artifact_hash, cwd)?;
    }

    let artifact_uri = SourceUri::Artifact {
        sha256: artifact_hash,
    }
    .to_string();
    let download_file_name = artifact_uri
        .strip_prefix("os://artifact/")
        .map(|hash| format!("handoff-{hash}.jsonl"));

    Ok(DesktopHandoffBuildResponse {
        artifact_uri,
        pinned_alias: pin_latest.then_some(HANDOFF_LATEST_PIN_ALIAS.to_string()),
        download_file_name,
        download_content: Some(record.canonical_jsonl),
    })
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
        config.summary.storage.backend =
            map_summary_storage_backend_to_runtime(&summary.storage.backend);
        config.summary.source_mode = map_summary_source_mode_to_runtime(&summary.source_mode);
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

fn install_status_response_from_state(
    state: VectorInstallRuntimeState,
) -> DesktopVectorInstallStatusResponse {
    DesktopVectorInstallStatusResponse {
        state: state.state,
        model: state.model,
        progress_pct: state.progress_pct,
        message: state.message,
    }
}

#[tauri::command]
fn desktop_vector_preflight() -> DesktopApiResult<DesktopVectorPreflightResponse> {
    let runtime = load_runtime_config()?;
    Ok(vector_preflight_for_runtime(&runtime))
}

#[tauri::command]
fn desktop_vector_install_model(
    model: String,
) -> DesktopApiResult<DesktopVectorInstallStatusResponse> {
    let runtime = load_runtime_config()?;
    let endpoint = vector_embed_endpoint(&runtime);
    let selected_model = model.trim().to_string().chars().collect::<String>();
    let selected_model = if selected_model.trim().is_empty() {
        vector_embed_model(&runtime)
    } else {
        selected_model
    };

    let current = current_vector_install_state();
    if matches!(current.state, DesktopVectorInstallState::Installing)
        && current.model == selected_model
    {
        return Ok(install_status_response_from_state(current));
    }

    update_vector_install_progress(
        DesktopVectorInstallState::Installing,
        &selected_model,
        0,
        Some("starting model download".to_string()),
    );

    let endpoint_for_thread = endpoint.clone();
    let model_for_thread = selected_model.clone();
    std::thread::spawn(move || {
        match install_ollama_model_blocking(&endpoint_for_thread, &model_for_thread) {
            Ok(()) => update_vector_install_progress(
                DesktopVectorInstallState::Ready,
                &model_for_thread,
                100,
                Some("model download complete".to_string()),
            ),
            Err(error) => update_vector_install_progress(
                DesktopVectorInstallState::Failed,
                &model_for_thread,
                0,
                Some(error.message),
            ),
        }
    });

    Ok(install_status_response_from_state(
        current_vector_install_state(),
    ))
}

#[tauri::command]
fn desktop_vector_index_rebuild() -> DesktopApiResult<DesktopVectorIndexStatusResponse> {
    let runtime = load_runtime_config()?;
    ensure_vector_enabled_and_ready(&runtime)?;

    {
        let mut running = VECTOR_INDEX_REBUILD_RUNNING
            .lock()
            .expect("vector index rebuild mutex poisoned");
        if *running {
            let db = open_local_db()?;
            return desktop_vector_index_status_from_db(&db);
        }
        *running = true;
    }

    std::thread::spawn(move || {
        let result: DesktopApiResult<()> = (|| {
            let db = open_local_db()?;
            if let Err(error) = rebuild_vector_index_blocking(&db, &runtime) {
                let _ = set_vector_index_job_snapshot(
                    &db,
                    VectorIndexJobRow {
                        status: "failed".to_string(),
                        processed_sessions: 0,
                        total_sessions: 0,
                        message: Some(error.message.clone()),
                        started_at: Some(chrono::Utc::now().to_rfc3339()),
                        finished_at: Some(chrono::Utc::now().to_rfc3339()),
                    },
                );
                return Err(error);
            }
            Ok(())
        })();

        if let Err(error) = result {
            eprintln!("vector index rebuild failed: {}", error.message);
        }
        if let Ok(mut running) = VECTOR_INDEX_REBUILD_RUNNING.lock() {
            *running = false;
        }
    });

    let db = open_local_db()?;
    desktop_vector_index_status_from_db(&db)
}

#[tauri::command]
fn desktop_vector_index_status() -> DesktopApiResult<DesktopVectorIndexStatusResponse> {
    let db = open_local_db()?;
    desktop_vector_index_status_from_db(&db)
}

#[tauri::command]
fn desktop_search_sessions_vector(
    query: String,
    cursor: Option<String>,
    limit: Option<u32>,
) -> DesktopApiResult<DesktopVectorSearchResponse> {
    let db = open_local_db()?;
    let runtime = load_runtime_config()?;
    search_sessions_vector_internal(&db, &runtime, &query, cursor, limit, None)
}

fn session_summary_response_from_row(
    row: opensession_local_db::SessionSemanticSummaryRow,
) -> DesktopSessionSummaryResponse {
    DesktopSessionSummaryResponse {
        session_id: row.session_id,
        summary: serde_json::from_str(&row.summary_json).ok(),
        source_details: row
            .source_details_json
            .as_deref()
            .and_then(|raw| serde_json::from_str(raw).ok()),
        diff_tree: row
            .diff_tree_json
            .as_deref()
            .and_then(|raw| serde_json::from_str(raw).ok())
            .unwrap_or_default(),
        source_kind: Some(row.source_kind),
        generation_kind: Some(row.generation_kind),
        error: row.error,
    }
}

fn session_summary_response_from_hidden_ref(
    row: SessionSummaryLedgerRecord,
) -> DesktopSessionSummaryResponse {
    DesktopSessionSummaryResponse {
        session_id: row.session_id,
        summary: Some(row.summary),
        source_details: match row.source_details {
            serde_json::Value::Object(ref map) if map.is_empty() => None,
            value => Some(value),
        },
        diff_tree: row.diff_tree,
        source_kind: Some(row.source_kind),
        generation_kind: Some(row.generation_kind),
        error: row.error,
    }
}

fn empty_summary_response(session_id: String) -> DesktopSessionSummaryResponse {
    DesktopSessionSummaryResponse {
        session_id,
        summary: None,
        source_details: None,
        diff_tree: Vec::new(),
        source_kind: None,
        generation_kind: None,
        error: None,
    }
}

fn load_session_summary_for_runtime(
    db: &LocalDb,
    runtime: &DaemonConfig,
    session_id: &str,
) -> DesktopApiResult<DesktopSessionSummaryResponse> {
    match runtime.summary.storage.backend {
        SummaryStorageBackend::LocalDb => {
            let summary = db
                .get_session_semantic_summary(session_id)
                .map_err(|error| {
                    desktop_error(
                        "desktop.session_summary_query_failed",
                        500,
                        "failed to load session summary",
                        Some(json!({ "cause": error.to_string(), "session_id": session_id })),
                    )
                })?
                .map(session_summary_response_from_row);
            Ok(summary.unwrap_or_else(|| empty_summary_response(session_id.to_string())))
        }
        SummaryStorageBackend::HiddenRef => {
            let Some(repo_root) = resolve_summary_repo_root(db, session_id)? else {
                return Ok(empty_summary_response(session_id.to_string()));
            };
            let loaded = NativeGitStorage
                .load_summary_at_ref(&repo_root, SUMMARY_LEDGER_REF, session_id)
                .map_err(|error| {
                    desktop_error(
                        "desktop.session_summary_query_failed",
                        500,
                        "failed to load hidden_ref session summary",
                        Some(
                            json!({ "cause": error.to_string(), "session_id": session_id, "repo_root": repo_root }),
                        ),
                    )
                })?
                .map(session_summary_response_from_hidden_ref);
            Ok(loaded.unwrap_or_else(|| empty_summary_response(session_id.to_string())))
        }
        SummaryStorageBackend::None => Ok(empty_summary_response(session_id.to_string())),
    }
}

fn resolve_summary_repo_root(db: &LocalDb, session_id: &str) -> DesktopApiResult<Option<PathBuf>> {
    let row = db.get_session_by_id(session_id).map_err(|error| {
        desktop_error(
            "desktop.session_summary_repo_resolve_failed",
            500,
            "failed to resolve session repository root",
            Some(json!({ "cause": error.to_string(), "session_id": session_id })),
        )
    })?;
    if let Some(row) = row {
        if let Some(cwd) = row.working_directory.as_deref() {
            if let Some(repo_root) = find_git_repo_root(Path::new(cwd)) {
                return Ok(Some(repo_root));
            }
        }
    }
    let cwd = std::env::current_dir().ok();
    Ok(cwd.and_then(|path| find_git_repo_root(&path)))
}

fn persist_summary_to_local_db(
    db: &LocalDb,
    session_id: &str,
    artifact: &opensession_summary::SemanticSummaryArtifact,
) -> DesktopApiResult<()> {
    let summary_json = serde_json::to_string(&artifact.summary).map_err(|error| {
        desktop_error(
            "desktop.session_summary_serialize_failed",
            500,
            "failed to serialize generated summary",
            Some(json!({ "cause": error.to_string() })),
        )
    })?;
    let source_details_json = if artifact.source_details.is_empty() {
        None
    } else {
        Some(
            serde_json::to_string(&artifact.source_details).map_err(|error| {
                desktop_error(
                    "desktop.session_summary_serialize_failed",
                    500,
                    "failed to serialize source details",
                    Some(json!({ "cause": error.to_string() })),
                )
            })?,
        )
    };
    let diff_tree_json = if artifact.diff_tree.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&artifact.diff_tree).map_err(|error| {
            desktop_error(
                "desktop.session_summary_serialize_failed",
                500,
                "failed to serialize diff tree",
                Some(json!({ "cause": error.to_string() })),
            )
        })?)
    };
    let provider = enum_label(&artifact.provider);
    let source_kind = enum_label(&artifact.source_kind);
    let generation_kind = enum_label(&artifact.generation_kind);
    let model = (!artifact.model.trim().is_empty()).then_some(artifact.model.as_str());
    let prompt_fingerprint = (!artifact.prompt_fingerprint.trim().is_empty())
        .then_some(artifact.prompt_fingerprint.as_str());

    db.upsert_session_semantic_summary(&opensession_local_db::SessionSemanticSummaryUpsert {
        session_id,
        summary_json: &summary_json,
        generated_at: &chrono::Utc::now().to_rfc3339(),
        provider: &provider,
        model,
        source_kind: &source_kind,
        generation_kind: &generation_kind,
        prompt_fingerprint,
        source_details_json: source_details_json.as_deref(),
        diff_tree_json: diff_tree_json.as_deref(),
        error: artifact.error.as_deref(),
    })
    .map_err(|error| {
        desktop_error(
            "desktop.session_summary_persist_failed",
            500,
            "failed to persist generated session summary",
            Some(json!({ "cause": error.to_string(), "session_id": session_id })),
        )
    })?;
    Ok(())
}

fn persist_summary_to_hidden_ref(
    repo_root: &Path,
    session_id: &str,
    artifact: &opensession_summary::SemanticSummaryArtifact,
) -> DesktopApiResult<()> {
    let summary = serde_json::to_value(&artifact.summary).map_err(|error| {
        desktop_error(
            "desktop.session_summary_serialize_failed",
            500,
            "failed to serialize generated summary for hidden-ref storage",
            Some(json!({ "cause": error.to_string() })),
        )
    })?;
    let source_details = serde_json::to_value(&artifact.source_details).map_err(|error| {
        desktop_error(
            "desktop.session_summary_serialize_failed",
            500,
            "failed to serialize source details for hidden-ref storage",
            Some(json!({ "cause": error.to_string() })),
        )
    })?;
    let diff_tree_value = serde_json::to_value(&artifact.diff_tree).map_err(|error| {
        desktop_error(
            "desktop.session_summary_serialize_failed",
            500,
            "failed to serialize diff tree for hidden-ref storage",
            Some(json!({ "cause": error.to_string() })),
        )
    })?;
    let diff_tree = diff_tree_value.as_array().cloned().unwrap_or_default();
    let record = SessionSummaryLedgerRecord {
        session_id: session_id.to_string(),
        generated_at: chrono::Utc::now().to_rfc3339(),
        provider: enum_label(&artifact.provider),
        model: (!artifact.model.trim().is_empty()).then_some(artifact.model.clone()),
        source_kind: enum_label(&artifact.source_kind),
        generation_kind: enum_label(&artifact.generation_kind),
        prompt_fingerprint: (!artifact.prompt_fingerprint.trim().is_empty())
            .then_some(artifact.prompt_fingerprint.clone()),
        summary,
        source_details,
        diff_tree,
        error: artifact.error.clone(),
    };

    NativeGitStorage
        .store_summary_at_ref(repo_root, SUMMARY_LEDGER_REF, &record)
        .map_err(|error| {
            desktop_error(
                "desktop.session_summary_persist_failed",
                500,
                "failed to persist generated summary to hidden_ref",
                Some(
                    json!({ "cause": error.to_string(), "session_id": session_id, "repo_root": repo_root }),
                ),
            )
        })?;
    Ok(())
}

#[tauri::command]
fn desktop_get_session_summary(id: String) -> DesktopApiResult<DesktopSessionSummaryResponse> {
    let db = open_local_db()?;
    let runtime = load_runtime_config()?;
    load_session_summary_for_runtime(&db, &runtime, &id)
}

#[tauri::command]
async fn desktop_regenerate_session_summary(
    id: String,
) -> DesktopApiResult<DesktopSessionSummaryResponse> {
    let db = open_local_db()?;
    let normalized_session = load_normalized_session_body(&db, &id)?;
    let session = HailSession::from_jsonl(&normalized_session).map_err(|error| {
        desktop_error(
            "desktop.session_summary_parse_failed",
            422,
            "failed to decode session body for summary regeneration",
            Some(json!({ "cause": error.to_string(), "session_id": id })),
        )
    })?;
    let runtime = load_runtime_config()?;
    let git_request = if runtime.summary.allows_git_changes_fallback() {
        working_directory(&session)
            .and_then(|cwd| find_git_repo_root(Path::new(cwd)).map(|repo_root| (cwd, repo_root)))
            .map(|(cwd, repo_root)| GitSummaryRequest {
                repo_root,
                commit: extract_git_context(cwd).commit,
            })
    } else {
        None
    };
    let artifact = summarize_session(&session, &runtime.summary, git_request.as_ref())
        .await
        .map_err(|error| {
            desktop_error(
                "desktop.session_summary_generate_failed",
                500,
                "failed to generate semantic summary",
                Some(json!({ "cause": error.to_string(), "session_id": id })),
            )
        })?;

    match runtime.summary.storage.backend {
        SummaryStorageBackend::LocalDb => {
            persist_summary_to_local_db(&db, &id, &artifact)?;
            desktop_get_session_summary(id)
        }
        SummaryStorageBackend::HiddenRef => {
            let Some(repo_root) = resolve_summary_repo_root(&db, &id)? else {
                return Err(desktop_error(
                    "desktop.session_summary_repo_required",
                    422,
                    "hidden_ref summary backend requires a git repository",
                    Some(json!({ "session_id": id })),
                ));
            };
            persist_summary_to_hidden_ref(&repo_root, &id, &artifact)?;
            desktop_get_session_summary(id)
        }
        SummaryStorageBackend::None => Ok(DesktopSessionSummaryResponse {
            session_id: id,
            summary: serde_json::to_value(&artifact.summary).ok(),
            source_details: if artifact.source_details.is_empty() {
                None
            } else {
                serde_json::to_value(&artifact.source_details).ok()
            },
            diff_tree: serde_json::to_value(&artifact.diff_tree)
                .ok()
                .and_then(|value| value.as_array().cloned())
                .unwrap_or_default(),
            source_kind: Some(enum_label(&artifact.source_kind)),
            generation_kind: Some(enum_label(&artifact.generation_kind)),
            error: artifact.error.clone(),
        }),
    }
}

#[derive(Debug, Clone)]
struct ChangeReaderContextPayload {
    session_id: String,
    scope: DesktopChangeReaderScope,
    context: String,
    citations: Vec<String>,
    provider: Option<DesktopSummaryProviderId>,
    warning: Option<String>,
}

fn compact_ws(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn trim_chars(raw: &str, max_chars: usize) -> String {
    let normalized = compact_ws(raw);
    if normalized.chars().count() <= max_chars {
        return normalized;
    }
    let mut out = String::new();
    for ch in normalized.chars().take(max_chars.saturating_sub(1)) {
        out.push(ch);
    }
    out.push('…');
    out
}

fn json_value_compact(value: &serde_json::Value, max_chars: usize) -> String {
    let raw = serde_json::to_string(value).unwrap_or_else(|_| value.to_string());
    trim_chars(&raw, max_chars)
}

fn event_type_label(event_type: &EventType) -> &'static str {
    match event_type {
        EventType::UserMessage => "user",
        EventType::AgentMessage => "agent",
        EventType::SystemMessage => "system",
        EventType::Thinking => "thinking",
        EventType::ToolCall { .. } => "tool_call",
        EventType::ToolResult { .. } => "tool_result",
        EventType::FileRead { .. } => "file_read",
        EventType::CodeSearch { .. } => "code_search",
        EventType::FileSearch { .. } => "file_search",
        EventType::FileEdit { .. } => "file_edit",
        EventType::FileCreate { .. } => "file_create",
        EventType::FileDelete { .. } => "file_delete",
        EventType::ShellCommand { .. } => "shell",
        EventType::ImageGenerate { .. } => "image_generate",
        EventType::VideoGenerate { .. } => "video_generate",
        EventType::AudioGenerate { .. } => "audio_generate",
        EventType::WebSearch { .. } => "web_search",
        EventType::WebFetch { .. } => "web_fetch",
        EventType::TaskStart { .. } => "task_start",
        EventType::TaskEnd { .. } => "task_end",
        EventType::Custom { .. } => "custom",
        _ => "event",
    }
}

fn event_type_payload(event_type: &EventType) -> Option<String> {
    match event_type {
        EventType::ToolCall { name } => Some(format!("tool={name}")),
        EventType::ToolResult {
            name,
            is_error,
            call_id,
        } => Some(format!(
            "tool={} result={}{}",
            name,
            if *is_error { "error" } else { "ok" },
            call_id
                .as_deref()
                .map(|id| format!(" call_id={id}"))
                .unwrap_or_default()
        )),
        EventType::FileRead { path }
        | EventType::FileEdit { path, .. }
        | EventType::FileCreate { path }
        | EventType::FileDelete { path } => Some(format!("path={path}")),
        EventType::CodeSearch { query } | EventType::WebSearch { query } => {
            Some(format!("query={}", trim_chars(query, 90)))
        }
        EventType::FileSearch { pattern } => Some(format!("pattern={}", trim_chars(pattern, 90))),
        EventType::ShellCommand { command, exit_code } => Some(format!(
            "cmd={}{}",
            trim_chars(command, 120),
            exit_code
                .map(|code| format!(" exit_code={code}"))
                .unwrap_or_default()
        )),
        EventType::ImageGenerate { prompt }
        | EventType::VideoGenerate { prompt }
        | EventType::AudioGenerate { prompt } => Some(format!("prompt={}", trim_chars(prompt, 90))),
        EventType::WebFetch { url } => Some(format!("url={url}")),
        EventType::TaskStart { title } => title
            .as_deref()
            .map(|raw| format!("title={}", trim_chars(raw, 90))),
        EventType::TaskEnd { summary } => summary
            .as_deref()
            .map(|raw| format!("summary={}", trim_chars(raw, 90))),
        EventType::Custom { kind } => Some(format!("kind={kind}")),
        _ => None,
    }
}

fn event_content_excerpt(event: &opensession_core::trace::Event) -> Option<String> {
    let mut parts = Vec::<String>::new();
    for block in &event.content.blocks {
        let rendered = match block {
            ContentBlock::Text { text } => trim_chars(text, CHANGE_READER_MAX_LINE_CHARS),
            ContentBlock::Code { code, .. } => {
                let first_line = code.lines().next().unwrap_or_default();
                format!("code: {}", trim_chars(first_line, 120))
            }
            ContentBlock::Image { alt, url, .. } => {
                let label = alt.as_deref().unwrap_or("image");
                format!("{label}: {url}")
            }
            ContentBlock::Video { url, .. } => format!("video: {url}"),
            ContentBlock::Audio { url, .. } => format!("audio: {url}"),
            ContentBlock::File { path, content } => {
                let head = content
                    .as_deref()
                    .map(|raw| trim_chars(raw, 80))
                    .unwrap_or_else(|| "content omitted".to_string());
                format!("file {path}: {head}")
            }
            ContentBlock::Json { data } => format!("json: {}", json_value_compact(data, 120)),
            ContentBlock::Reference { uri, .. } => format!("ref: {uri}"),
            _ => String::new(),
        };
        if rendered.trim().is_empty() {
            continue;
        }
        parts.push(rendered);
        if parts.len() >= 2 {
            break;
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" | "))
    }
}

fn summary_lines_for_reader(summary: &DesktopSessionSummaryResponse) -> Vec<String> {
    let mut lines = Vec::<String>::new();
    if let Some(summary_obj) = summary.summary.as_ref().and_then(|value| value.as_object()) {
        if let Some(changes) = summary_obj.get("changes").and_then(|value| value.as_str()) {
            if !changes.trim().is_empty() {
                lines.push(format!("changes: {}", trim_chars(changes, 280)));
            }
        }
        if let Some(auth_security) = summary_obj.get("auth_security").and_then(|value| value.as_str())
        {
            if !auth_security.trim().is_empty() {
                lines.push(format!(
                    "auth_security: {}",
                    trim_chars(auth_security, 200)
                ));
            }
        }
        if let Some(layer_items) = summary_obj
            .get("layer_file_changes")
            .and_then(|value| value.as_array())
        {
            for item in layer_items.iter().take(12) {
                let layer = item
                    .get("layer")
                    .and_then(|value| value.as_str())
                    .unwrap_or("(layer)");
                let detail = item
                    .get("summary")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                let files = item
                    .get("files")
                    .and_then(|value| value.as_array())
                    .map(|entries| {
                        entries
                            .iter()
                            .filter_map(|entry| entry.as_str())
                            .take(5)
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                lines.push(format!(
                    "layer {}: {}{}",
                    trim_chars(layer, 60),
                    trim_chars(detail, 120),
                    if files.is_empty() {
                        String::new()
                    } else {
                        format!(" (files: {files})")
                    }
                ));
            }
        }
    } else if let Some(value) = &summary.summary {
        lines.push(format!(
            "semantic_summary_json: {}",
            json_value_compact(value, 260)
        ));
    }

    for layer in summary.diff_tree.iter().take(8) {
        let Some(layer_obj) = layer.as_object() else {
            continue;
        };
        let layer_name = layer_obj
            .get("layer")
            .and_then(|value| value.as_str())
            .unwrap_or("(layer)");
        let file_count = layer_obj
            .get("file_count")
            .and_then(|value| value.as_u64())
            .unwrap_or_default();
        let added = layer_obj
            .get("lines_added")
            .and_then(|value| value.as_u64())
            .unwrap_or_default();
        let removed = layer_obj
            .get("lines_removed")
            .and_then(|value| value.as_u64())
            .unwrap_or_default();
        lines.push(format!(
            "diff_layer {}: files={} +{} -{}",
            trim_chars(layer_name, 60),
            file_count,
            added,
            removed
        ));
    }

    if let Some(source_kind) = summary.source_kind.as_deref() {
        lines.push(format!("source_kind: {source_kind}"));
    }
    if let Some(generation_kind) = summary.generation_kind.as_deref() {
        lines.push(format!("generation_kind: {generation_kind}"));
    }
    if let Some(error) = summary.error.as_deref() {
        lines.push(format!("generation_error: {}", trim_chars(error, 160)));
    }
    lines
}

fn timeline_lines_for_reader(session: &HailSession) -> Vec<String> {
    session
        .events
        .iter()
        .take(CHANGE_READER_MAX_EVENTS)
        .map(|event| {
            let label = event_type_label(&event.event_type);
            let payload = event_type_payload(&event.event_type).unwrap_or_default();
            let content = event_content_excerpt(event).unwrap_or_default();
            let mut merged = format!("{} {}", event.timestamp.to_rfc3339(), label);
            if !payload.is_empty() {
                merged.push(' ');
                merged.push_str(&payload);
            }
            if !content.is_empty() {
                merged.push_str(" => ");
                merged.push_str(&content);
            }
            trim_chars(&merged, CHANGE_READER_MAX_LINE_CHARS)
        })
        .collect()
}

fn trim_context_to_limit(raw: String, max_chars: usize) -> String {
    if raw.chars().count() <= max_chars {
        return raw;
    }
    let mut out = String::new();
    for ch in raw.chars().take(max_chars.saturating_sub(24)) {
        out.push(ch);
    }
    out.push_str("\n\n[context truncated]");
    out
}

fn provider_for_change_reader(runtime: &DaemonConfig) -> Option<DesktopSummaryProviderId> {
    match runtime.summary.provider.id {
        SummaryProvider::Disabled => None,
        _ => Some(map_summary_provider_id_from_runtime(&runtime.summary.provider.id)),
    }
}

fn build_change_reader_context(
    db: &LocalDb,
    runtime: &DaemonConfig,
    session_id: &str,
    scope_override: Option<DesktopChangeReaderScope>,
) -> DesktopApiResult<ChangeReaderContextPayload> {
    let scope = scope_override
        .unwrap_or_else(|| map_change_reader_scope_from_runtime(&runtime.change_reader.scope));
    let normalized_session = load_normalized_session_body(db, session_id)?;
    let session = HailSession::from_jsonl(&normalized_session).map_err(|error| {
        desktop_error(
            "desktop.change_reader_parse_failed",
            422,
            "failed to parse session payload for change reader",
            Some(json!({ "cause": error.to_string(), "session_id": session_id })),
        )
    })?;
    let summary = load_session_summary_for_runtime(db, runtime, session_id)?;
    let summary_lines = summary_lines_for_reader(&summary);
    let timeline_lines = timeline_lines_for_reader(&session);
    let mut citations = Vec::<String>::new();
    let mut chunks = vec![
        format!("session_id: {}", session.session_id),
        format!(
            "agent: tool={} provider={} model={}",
            session.agent.tool, session.agent.provider, session.agent.model
        ),
    ];
    if let Some(title) = session.context.title.as_deref() {
        if !title.trim().is_empty() {
            chunks.push(format!("title: {}", trim_chars(title, 120)));
        }
    }
    if let Some(description) = session.context.description.as_deref() {
        if !description.trim().is_empty() {
            chunks.push(format!("description: {}", trim_chars(description, 180)));
        }
    }

    let mut warning = None;
    if !summary_lines.is_empty() {
        citations.push("session.semantic_summary".to_string());
        chunks.push("[semantic_summary]".to_string());
        chunks.extend(summary_lines.into_iter().map(|line| format!("- {line}")));
    } else {
        warning = Some(
            "semantic summary is not available; using timeline-derived context".to_string(),
        );
    }

    if matches!(scope, DesktopChangeReaderScope::FullContext)
        || (matches!(scope, DesktopChangeReaderScope::SummaryOnly) && citations.is_empty())
    {
        citations.push("session.timeline".to_string());
        chunks.push("[timeline_excerpt]".to_string());
        chunks.extend(timeline_lines.into_iter().map(|line| format!("- {line}")));
    }

    let max_context_chars = runtime.change_reader.max_context_chars.max(1) as usize;
    let context = trim_context_to_limit(chunks.join("\n"), max_context_chars);
    Ok(ChangeReaderContextPayload {
        session_id: session_id.to_string(),
        scope,
        context,
        citations,
        provider: provider_for_change_reader(runtime),
        warning,
    })
}

fn build_read_prompt(context: &str, scope: &DesktopChangeReaderScope) -> String {
    let scope_label = match scope {
        DesktopChangeReaderScope::SummaryOnly => "summary_only",
        DesktopChangeReaderScope::FullContext => "full_context",
    };
    format!(
        "You are OpenSession Change Reader.\n\
Use only the provided context and do not fabricate facts.\n\
Write a concise, human-readable Korean briefing about what changed.\n\
Include: 핵심 변경, 영향도/리스크, 확인할 테스트 1~2개.\n\
Scope={scope_label}\n\
\n\
Context:\n{context}\n"
    )
}

fn build_question_prompt(question: &str, context: &str, scope: &DesktopChangeReaderScope) -> String {
    let scope_label = match scope {
        DesktopChangeReaderScope::SummaryOnly => "summary_only",
        DesktopChangeReaderScope::FullContext => "full_context",
    };
    format!(
        "You are OpenSession Change Q&A assistant.\n\
Answer only from the given context. If evidence is insufficient, say clearly what is missing.\n\
Respond in Korean and keep it concise.\n\
Scope={scope_label}\n\
Question: {question}\n\
\n\
Context:\n{context}\n"
    )
}

fn fallback_change_narrative(context: &ChangeReaderContextPayload) -> String {
    let lines = context
        .context
        .lines()
        .filter(|line| line.starts_with("- "))
        .take(8)
        .map(|line| line.trim_start_matches("- ").to_string())
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return "변경을 설명할 수 있는 컨텍스트가 충분하지 않습니다.".to_string();
    }
    format!(
        "로컬 변경 브리핑(휴리스틱)\n{}",
        lines
            .into_iter()
            .map(|line| format!("- {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

fn tokenize_question(question: &str) -> Vec<String> {
    question
        .split(|ch: char| ch.is_whitespace() || ",.;:!?/()[]{}".contains(ch))
        .map(|token| token.trim().to_lowercase())
        .filter(|token| token.chars().count() >= 2)
        .take(10)
        .collect()
}

fn fallback_change_answer(question: &str, context: &ChangeReaderContextPayload) -> String {
    let tokens = tokenize_question(question);
    let mut matches = Vec::<String>::new();
    if !tokens.is_empty() {
        for line in context.context.lines() {
            let lowered = line.to_lowercase();
            if tokens.iter().any(|token| lowered.contains(token)) {
                matches.push(trim_chars(line, 180));
            }
            if matches.len() >= 5 {
                break;
            }
        }
    }
    if matches.is_empty() {
        return "질문에 바로 대응되는 근거를 현재 컨텍스트에서 찾지 못했습니다. full_context로 전환하거나 세션 요약을 재생성해 주세요."
            .to_string();
    }
    format!(
        "질문 답변(로컬 휴리스틱)\n{}\n\n근거:\n{}",
        trim_chars(
            &matches
                .first()
                .cloned()
                .unwrap_or_else(|| "근거를 찾지 못했습니다.".to_string()),
            220
        ),
        matches
            .into_iter()
            .take(4)
            .map(|line| format!("- {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

fn merge_warnings(primary: Option<String>, secondary: Option<String>) -> Option<String> {
    match (primary, secondary) {
        (Some(a), Some(b)) => Some(format!("{a}; {b}")),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

#[tauri::command]
async fn desktop_read_session_changes(
    request: DesktopChangeReadRequest,
) -> DesktopApiResult<DesktopChangeReadResponse> {
    let session_id = request.session_id.trim().to_string();
    if session_id.is_empty() {
        return Err(desktop_error(
            "desktop.change_reader_invalid_request",
            400,
            "session_id is required",
            None,
        ));
    }
    let runtime = load_runtime_config()?;
    if !runtime.change_reader.enabled {
        return Err(desktop_error(
            "desktop.change_reader_disabled",
            422,
            "change reader is disabled in runtime settings",
            Some(json!({ "hint": "Enable Change Reader in Settings > Runtime Summary" })),
        ));
    }
    let db = open_local_db()?;
    let context = build_change_reader_context(&db, &runtime, &session_id, request.scope)?;
    let prompt = build_read_prompt(&context.context, &context.scope);

    let (narrative, provider_warning) = if runtime.summary.is_configured() {
        match generate_text(&runtime.summary, &prompt).await {
            Ok(text) if !text.trim().is_empty() => (trim_chars(&text, 4000), None),
            Ok(_) => (
                fallback_change_narrative(&context),
                Some("provider returned empty response".to_string()),
            ),
            Err(error) => (
                fallback_change_narrative(&context),
                Some(format!("provider generation failed: {error}")),
            ),
        }
    } else {
        (
            fallback_change_narrative(&context),
            Some("summary provider is not configured; used local fallback".to_string()),
        )
    };

    Ok(DesktopChangeReadResponse {
        session_id: context.session_id,
        scope: context.scope,
        narrative,
        citations: context.citations,
        provider: context.provider,
        warning: merge_warnings(context.warning, provider_warning),
    })
}

#[tauri::command]
async fn desktop_ask_session_changes(
    request: DesktopChangeQuestionRequest,
) -> DesktopApiResult<DesktopChangeQuestionResponse> {
    let session_id = request.session_id.trim().to_string();
    if session_id.is_empty() {
        return Err(desktop_error(
            "desktop.change_reader_invalid_request",
            400,
            "session_id is required",
            None,
        ));
    }
    let question = request.question.trim().to_string();
    if question.is_empty() {
        return Err(desktop_error(
            "desktop.change_reader_question_required",
            400,
            "question is required",
            None,
        ));
    }
    let runtime = load_runtime_config()?;
    if !runtime.change_reader.enabled {
        return Err(desktop_error(
            "desktop.change_reader_disabled",
            422,
            "change reader is disabled in runtime settings",
            Some(json!({ "hint": "Enable Change Reader in Settings > Runtime Summary" })),
        ));
    }
    if !runtime.change_reader.qa_enabled {
        return Err(desktop_error(
            "desktop.change_reader_qa_disabled",
            422,
            "change reader Q&A is disabled in runtime settings",
            Some(json!({ "hint": "Enable Q&A in Settings > Runtime Summary > Change Reader" })),
        ));
    }

    let db = open_local_db()?;
    let context = build_change_reader_context(&db, &runtime, &session_id, request.scope)?;
    let prompt = build_question_prompt(&question, &context.context, &context.scope);
    let (answer, provider_warning) = if runtime.summary.is_configured() {
        match generate_text(&runtime.summary, &prompt).await {
            Ok(text) if !text.trim().is_empty() => (trim_chars(&text, 4000), None),
            Ok(_) => (
                fallback_change_answer(&question, &context),
                Some("provider returned empty response".to_string()),
            ),
            Err(error) => (
                fallback_change_answer(&question, &context),
                Some(format!("provider generation failed: {error}")),
            ),
        }
    } else {
        (
            fallback_change_answer(&question, &context),
            Some("summary provider is not configured; used local fallback".to_string()),
        )
    };

    Ok(DesktopChangeQuestionResponse {
        session_id: context.session_id,
        question,
        scope: context.scope,
        answer,
        citations: context.citations,
        provider: context.provider,
        warning: merge_warnings(context.warning, provider_warning),
    })
}

#[tauri::command]
fn desktop_list_sessions(
    query: Option<DesktopSessionListQuery>,
) -> DesktopApiResult<SessionListResponse> {
    let db = open_local_db()?;
    let (filter, page, per_page, search_mode) =
        build_local_filter_with_mode(query.unwrap_or_default());

    if search_mode == SearchMode::Vector {
        let vector_query = filter.search.clone().unwrap_or_default();
        return list_sessions_with_vector_rank(&db, &filter, &vector_query, page, per_page);
    }

    let total = db.count_sessions_filtered(&filter).map_err(|error| {
        desktop_error(
            "desktop.session_count_failed",
            500,
            "failed to count local sessions",
            Some(json!({ "cause": error.to_string() })),
        )
    })?;
    let sessions = db.list_sessions(&filter).map_err(|error| {
        desktop_error(
            "desktop.session_list_failed",
            500,
            "failed to list local sessions",
            Some(json!({ "cause": error.to_string() })),
        )
    })?;
    let mapped = sessions
        .into_iter()
        .map(session_summary_from_local_row)
        .collect::<Vec<_>>();

    Ok(SessionListResponse {
        sessions: mapped,
        total,
        page,
        per_page,
    })
}

#[tauri::command]
fn desktop_list_repos() -> DesktopApiResult<SessionRepoListResponse> {
    let db = open_local_db()?;
    let repos = db.list_repos().map_err(|error| {
        desktop_error(
            "desktop.repo_list_failed",
            500,
            "failed to list repository names",
            Some(json!({ "cause": error.to_string() })),
        )
    })?;
    Ok(SessionRepoListResponse { repos })
}

#[tauri::command]
fn desktop_get_session_detail(id: String) -> DesktopApiResult<SessionDetail> {
    let db = open_local_db()?;
    let row = db
        .get_session_by_id(&id)
        .map_err(|error| {
            desktop_error(
                "desktop.session_detail_failed",
                500,
                "failed to load session detail",
                Some(json!({ "cause": error.to_string(), "session_id": id })),
            )
        })?
        .ok_or_else(|| {
            desktop_error(
                "desktop.session_not_found",
                404,
                "session not found",
                Some(json!({ "session_id": id })),
            )
        })?;

    let links = db
        .list_session_links(&id)
        .map_err(|error| {
            desktop_error(
                "desktop.session_links_failed",
                500,
                "failed to load session links",
                Some(json!({ "cause": error.to_string(), "session_id": id })),
            )
        })?
        .into_iter()
        .map(session_link_from_local)
        .collect::<Vec<_>>();

    Ok(SessionDetail {
        summary: session_summary_from_local_row(row),
        linked_sessions: links,
    })
}

#[tauri::command]
fn desktop_get_session_raw(id: String) -> DesktopApiResult<String> {
    let db = open_local_db()?;
    load_normalized_session_body(&db, &id)
}

#[tauri::command]
fn desktop_take_launch_route() -> DesktopApiResult<Option<String>> {
    let path = desktop_launch_route_path()?;
    if !path.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(&path).map_err(|error| {
        desktop_error(
            "desktop.launch_route_read_failed",
            500,
            "failed to read desktop launch route",
            Some(json!({ "cause": error.to_string(), "path": path.to_string_lossy() })),
        )
    })?;
    let _ = fs::remove_file(&path);
    Ok(normalize_launch_route(&contents))
}

#[tauri::command]
fn desktop_build_handoff(
    request: DesktopHandoffBuildRequest,
) -> DesktopApiResult<DesktopHandoffBuildResponse> {
    let session_id = request.session_id.trim().to_string();
    if session_id.is_empty() {
        return Err(desktop_error(
            "desktop.handoff_invalid_request",
            400,
            "session_id is required",
            None,
        ));
    }

    let db = open_local_db()?;
    let normalized_session = load_normalized_session_body(&db, &session_id)?;
    let session = HailSession::from_jsonl(&normalized_session).map_err(|error| {
        desktop_error(
            "desktop.handoff_parse_failed",
            422,
            "failed to decode normalized session for handoff build",
            Some(json!({ "cause": error.to_string(), "session_id": session_id })),
        )
    })?;
    let cwd = std::env::current_dir().map_err(|error| {
        desktop_error(
            "desktop.handoff_store_unavailable",
            500,
            "failed to resolve current workspace directory",
            Some(json!({ "cause": error.to_string() })),
        )
    })?;

    build_handoff_artifact_record(&normalized_session, session, request.pin_latest, &cwd)
}

#[tauri::command]
fn desktop_share_session_quick(
    request: DesktopQuickShareRequest,
) -> DesktopApiResult<DesktopQuickShareResponse> {
    let session_id = request.session_id.trim().to_string();
    if session_id.is_empty() {
        return Err(desktop_error(
            "desktop.quick_share_invalid_request",
            400,
            "session_id is required",
            None,
        ));
    }

    let db = open_local_db()?;
    let normalized_session = load_normalized_session_body(&db, &session_id)?;
    let session = HailSession::from_jsonl(&normalized_session).map_err(|error| {
        desktop_error(
            "desktop.quick_share_parse_failed",
            422,
            "failed to decode normalized session for quick share",
            Some(json!({ "cause": error.to_string(), "session_id": session_id })),
        )
    })?;

    let command_cwd = working_directory(&session)
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .or_else(|| std::env::current_dir().ok())
        .ok_or_else(|| {
            desktop_error(
                "desktop.quick_share_cwd_unavailable",
                500,
                "failed to resolve command working directory",
                Some(json!({ "session_id": session_id })),
            )
        })?;

    let source_object = store_local_object(normalized_session.as_bytes(), &command_cwd).map_err(|error| {
        desktop_error(
            "desktop.quick_share_source_store_failed",
            500,
            "failed to store normalized source object for quick share",
            Some(json!({ "cause": error.to_string(), "session_id": session_id })),
        )
    })?;

    let mut command = Command::new("opensession");
    command
        .arg("share")
        .arg(source_object.uri.to_string())
        .arg("--quick")
        .arg("--json")
        .current_dir(&command_cwd);
    if let Some(remote) = normalize_non_empty(request.remote) {
        command.arg("--remote").arg(remote);
    }

    let output = command.output().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            return desktop_error(
                "desktop.quick_share_cli_missing",
                501,
                "opensession CLI is unavailable. Install/enable the CLI bundle and retry.",
                Some(json!({ "cause": error.to_string() })),
            );
        }
        desktop_error(
            "desktop.quick_share_spawn_failed",
            500,
            "failed to start opensession CLI quick-share command",
            Some(json!({ "cause": error.to_string() })),
        )
    })?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(desktop_error(
            "desktop.quick_share_failed",
            422,
            "quick-share command failed",
            Some(json!({
                "session_id": session_id,
                "source_uri": source_object.uri.to_string(),
                "stdout": stdout,
                "stderr": stderr,
            })),
        ));
    }

    let stdout = String::from_utf8(output.stdout).map_err(|error| {
        desktop_error(
            "desktop.quick_share_invalid_utf8",
            500,
            "quick-share command returned non-UTF8 output",
            Some(json!({ "cause": error.to_string() })),
        )
    })?;
    parse_cli_quick_share_response(&stdout)
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            desktop_get_capabilities,
            desktop_get_auth_providers,
            desktop_get_contract_version,
            desktop_get_docs_markdown,
            desktop_get_runtime_settings,
            desktop_update_runtime_settings,
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
        artifact_path_for_hash, build_handoff_artifact_record, build_local_filter_with_mode,
        build_vector_chunks_for_session, canonicalize_summaries, cosine_similarity,
        desktop_ask_session_changes, desktop_get_contract_version, desktop_get_runtime_settings,
        desktop_get_session_detail, desktop_get_session_raw, desktop_list_sessions,
        desktop_read_session_changes, desktop_share_session_quick, desktop_update_runtime_settings,
        extract_vector_lines, map_link_type, normalize_launch_route,
        normalize_session_body_to_hail_jsonl, session_summary_from_local_row, split_search_mode,
        parse_cli_quick_share_response, validate_pin_alias, DesktopSessionListQuery, SearchMode,
    };
    use opensession_api::{
        DesktopChangeQuestionRequest, DesktopChangeReadRequest, DesktopChangeReaderScope,
        DesktopQuickShareRequest,
        DesktopRuntimeChangeReaderSettingsUpdate,
        DesktopRuntimeSettingsUpdateRequest, DesktopRuntimeSummaryPromptSettingsUpdate,
        DesktopRuntimeSummaryProviderSettingsUpdate, DesktopRuntimeSummaryResponseSettingsUpdate,
        DesktopRuntimeSummarySettingsUpdate, DesktopRuntimeSummaryStorageSettingsUpdate,
        DesktopSummaryOutputShape, DesktopSummaryProviderId, DesktopSummaryResponseStyle,
        DesktopSummarySourceMode, DesktopSummaryStorageBackend, DesktopSummaryTriggerMode,
    };
    use opensession_core::handoff::HandoffSummary;
    use opensession_core::trace::{Agent, Content, Event, EventType, Session as HailSession};
    use opensession_local_db::git::GitContext;
    use opensession_local_db::LocalDb;
    use opensession_runtime_config::DaemonConfig;
    use opensession_summary::DEFAULT_SUMMARY_PROMPT_TEMPLATE_V2;
    use std::path::{Path, PathBuf};
    use std::sync::{LazyLock, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    struct EnvVarGuard {
        key: String,
        previous: Option<String>,
    }

    impl EnvVarGuard {
        fn set(key: &str, value: impl AsRef<std::ffi::OsStr>) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self {
                key: key.to_string(),
                previous,
            }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(previous) = self.previous.take() {
                std::env::set_var(&self.key, previous);
            } else {
                std::env::remove_var(&self.key);
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
        runtime.vector_search.chunk_size_lines = 2;
        runtime.vector_search.chunk_overlap_lines = 1;
        let chunks = build_vector_chunks_for_session(&session, "source-hash", &runtime);
        assert!(chunks.len() >= 2);
        assert_eq!(chunks[0].start_line, 1);
        assert_eq!(chunks[0].end_line, 2);
        assert_eq!(chunks[1].start_line, 2);
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
            }),
            vector_search: None,
            change_reader: Some(DesktopRuntimeChangeReaderSettingsUpdate {
                enabled: true,
                scope: DesktopChangeReaderScope::FullContext,
                qa_enabled: true,
                max_context_chars: 18_000,
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
        assert!(loaded.summary.prompt.template.contains("customized"));
        assert!(loaded.change_reader.enabled);
        assert_eq!(loaded.change_reader.scope, DesktopChangeReaderScope::FullContext);
        assert!(loaded.change_reader.qa_enabled);
        assert_eq!(loaded.change_reader.max_context_chars, 18_000);

        let _ = std::fs::remove_dir_all(&temp_home);
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
            }),
            vector_search: None,
            change_reader: None,
        });

        let error = result.expect_err("source mode lock should reject update");
        assert_eq!(error.status, 422);
        assert_eq!(error.code, "desktop.runtime_settings_source_mode_locked");

        let _ = std::fs::remove_dir_all(&temp_home);
    }

    #[test]
    fn desktop_change_reader_requires_enabled_setting() {
        let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
        let temp_home = unique_temp_dir("opensession-desktop-change-reader-disabled");
        let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());

        let result = tauri::async_runtime::block_on(
            desktop_read_session_changes(DesktopChangeReadRequest {
                session_id: "session-1".to_string(),
                scope: None,
            }),
        );
        let error = result.expect_err("disabled change reader should fail");
        assert_eq!(error.status, 422);
        assert_eq!(error.code, "desktop.change_reader_disabled");

        let _ = std::fs::remove_dir_all(&temp_home);
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
            }),
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
        assert!(response
            .download_content
            .as_deref()
            .is_some_and(|value| value.contains("\"source_session_id\":\"session-handoff-test\"")));

        let artifact_path = PathBuf::from(repo_root.join(".opensession").join("artifacts"))
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
