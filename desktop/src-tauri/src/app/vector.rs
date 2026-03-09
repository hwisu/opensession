use opensession_api::{
    DesktopApiError, DesktopVectorIndexState, DesktopVectorIndexStatusResponse,
    DesktopVectorInstallState, DesktopVectorInstallStatusResponse, DesktopVectorPreflightResponse,
    DesktopVectorSearchProvider, DesktopVectorSearchResponse, DesktopVectorSessionMatch,
    SessionListResponse, SessionSummary,
};
use opensession_core::trace::Session as HailSession;
use opensession_local_db::{LocalDb, LocalSessionFilter, VectorChunkUpsert, VectorIndexJobRow};
use opensession_local_store::sha256_hex;
use opensession_runtime_config::{DaemonConfig, VectorChunkingMode};
use serde_json::json;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::Command;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use crate::{
    DesktopApiResult, desktop_error, load_normalized_session_body, load_runtime_config,
    open_local_db, session_summary_from_local_row_with_score,
};

const VECTOR_EMBED_BATCH_SIZE: usize = 24;
const VECTOR_FTS_CANDIDATE_LIMIT_MULTIPLIER: u32 = 8;

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

pub(crate) fn vector_embed_endpoint(runtime: &DaemonConfig) -> String {
    let configured = runtime.vector_search.endpoint.trim();
    if !configured.is_empty() {
        return configured.trim_end_matches('/').to_string();
    }
    "http://127.0.0.1:11434".to_string()
}

pub(crate) fn vector_embed_model(runtime: &DaemonConfig) -> String {
    let configured = runtime.vector_search.model.trim();
    if configured.is_empty() {
        return "bge-m3".to_string();
    }
    configured.to_string()
}

fn truncate_vector_error_body(raw: &str, max_len: usize) -> String {
    let compact = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= max_len {
        return compact;
    }
    let truncated = compact.chars().take(max_len).collect::<String>();
    format!("{truncated}...")
}

fn extract_vector_error_reason(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    serde_json::from_str::<serde_json::Value>(trimmed)
        .ok()
        .and_then(|payload| {
            payload
                .get("error")
                .and_then(serde_json::Value::as_str)
                .or_else(|| payload.get("message").and_then(serde_json::Value::as_str))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| value.to_string())
        })
        .or_else(|| Some(truncate_vector_error_body(trimmed, 220)))
}

fn detail_string(details: Option<&serde_json::Value>, key: &str) -> Option<String> {
    details
        .and_then(|value| value.get(key))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
}

fn detail_u64(details: Option<&serde_json::Value>, key: &str) -> Option<u64> {
    details
        .and_then(|value| value.get(key))
        .and_then(serde_json::Value::as_u64)
}

fn format_vector_error_message(error: &DesktopApiError) -> String {
    let details = error.details.as_ref();
    let mut lines = vec![error.message.clone()];
    if let Some(reason) = detail_string(details, "reason").or_else(|| {
        detail_string(details, "body").and_then(|body| extract_vector_error_reason(&body))
    }) {
        lines.push(format!("Reason: {reason}"));
    }
    if let Some(status) = detail_u64(details, "status") {
        lines.push(format!("HTTP: {status}"));
    }
    if let Some(endpoint) = detail_string(details, "endpoint") {
        lines.push(format!("Endpoint: {endpoint}"));
    }
    if let Some(batch_reason) = detail_string(details, "batch_reason").or_else(|| {
        detail_string(details, "batch_body").and_then(|body| extract_vector_error_reason(&body))
    }) {
        lines.push(format!("Batch reason: {batch_reason}"));
    }
    if let Some(batch_status) = detail_u64(details, "batch_status") {
        lines.push(format!("Batch HTTP: {batch_status}"));
    }
    if let Some(batch_endpoint) = detail_string(details, "batch_endpoint") {
        lines.push(format!("Batch endpoint: {batch_endpoint}"));
    }
    if let Some(model) = detail_string(details, "model") {
        lines.push(format!("Model: {model}"));
    }
    if let Some(hint) = detail_string(details, "hint") {
        lines.push(format!("Action: {hint}"));
    }
    lines.join("\n")
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
    let (batch_status, batch_body, batch_reason) = match client
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
            (
                None,
                None,
                Some(format!(
                    "expected {} embeddings but received a different payload shape",
                    inputs.len()
                )),
            )
        }
        Ok(response) => {
            let status = response.status().as_u16();
            let body = response.text().unwrap_or_default();
            (
                Some(status),
                Some(body.clone()),
                extract_vector_error_reason(&body),
            )
        }
        Err(error) => (None, None, Some(error.to_string())),
    };

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
                        "batch_endpoint": batch_url.clone(),
                        "batch_status": batch_status,
                        "batch_body": batch_body.clone(),
                        "batch_reason": batch_reason.clone(),
                        "model": model.to_string(),
                        "hint": "start local ollama and ensure embeddings endpoint is reachable"
                    })),
                )
            })?;
        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().unwrap_or_default();
            let reason = extract_vector_error_reason(&body);
            return Err(desktop_error(
                "desktop.vector_search_unavailable",
                422,
                format!("vector search endpoint returned HTTP {status}"),
                Some(json!({
                    "endpoint": single_url,
                    "status": status,
                    "body": body,
                    "reason": reason,
                    "batch_endpoint": batch_url.clone(),
                    "batch_status": batch_status,
                    "batch_body": batch_body.clone(),
                    "batch_reason": batch_reason.clone(),
                    "model": model.to_string(),
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

fn ollama_cli_available() -> bool {
    Command::new("ollama")
        .arg("--version")
        .output()
        .map(|output| {
            output.status.success() || !output.stdout.is_empty() || !output.stderr.is_empty()
        })
        .unwrap_or(false)
}

fn ollama_unreachable_message(endpoint: &str, error: &reqwest::Error) -> String {
    if ollama_cli_available() {
        format!(
            "ollama is installed but not reachable at {endpoint}; start it with `ollama serve` ({error})"
        )
    } else {
        "ollama CLI is not installed. Install from https://ollama.com/download, then run `ollama serve`.".to_string()
    }
}

pub(crate) fn vector_preflight_for_runtime(
    runtime: &DaemonConfig,
) -> DesktopVectorPreflightResponse {
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
            response.message = Some(ollama_unreachable_message(&endpoint, &error));
            return response;
        }
    };
    if !tags_response.status().is_success() {
        response.message = Some(format!(
            "ollama endpoint {endpoint} returned status {}; verify `ollama serve` and endpoint configuration",
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
    } else if response.message.is_none() {
        response.message = Some(format!(
            "model `{model}` is not installed. Run `ollama pull {model}` or use Install model."
        ));
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
            let (message, hint) = if ollama_cli_available() {
                (
                    "failed to connect to ollama model pull endpoint",
                    "start local ollama with `ollama serve`".to_string(),
                )
            } else {
                (
                    "ollama CLI is not installed",
                    "install Ollama from https://ollama.com/download and run `ollama serve`"
                        .to_string(),
                )
            };
            desktop_error(
                "desktop.vector_install_unavailable",
                422,
                message,
                Some(json!({
                    "cause": error.to_string(),
                    "endpoint": pull_url,
                    "hint": hint
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

pub(crate) fn cosine_similarity(lhs: &[f32], rhs: &[f32]) -> f32 {
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
    ensure_vector_provider_ready(runtime, true)
}

pub(crate) fn validate_vector_preflight_ready(
    preflight: &DesktopVectorPreflightResponse,
    runtime_enabled: bool,
    require_enabled: bool,
) -> DesktopApiResult<()> {
    if require_enabled && !runtime_enabled {
        return Err(desktop_error(
            "desktop.vector_search_disabled",
            422,
            "semantic vector search is disabled in runtime settings",
            Some(json!({ "hint": "enable vector_search in Settings and save runtime settings" })),
        ));
    }
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
    Ok(())
}

fn ensure_vector_provider_ready(
    runtime: &DaemonConfig,
    require_enabled: bool,
) -> DesktopApiResult<DesktopVectorPreflightResponse> {
    let preflight = vector_preflight_for_runtime(runtime);
    validate_vector_preflight_ready(&preflight, runtime.vector_search.enabled, require_enabled)?;
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

pub(crate) fn list_sessions_with_vector_rank(
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

pub(crate) fn extract_vector_lines(session: &HailSession) -> Vec<String> {
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

fn resolve_vector_chunking_profile(line_count: usize, runtime: &DaemonConfig) -> (usize, usize) {
    if matches!(
        runtime.vector_search.chunking_mode,
        VectorChunkingMode::Manual
    ) {
        let chunk_size = runtime.vector_search.chunk_size_lines.max(1) as usize;
        let overlap = runtime
            .vector_search
            .chunk_overlap_lines
            .min(runtime.vector_search.chunk_size_lines.saturating_sub(1))
            as usize;
        return (chunk_size, overlap);
    }

    if line_count <= 40 {
        (8, 2)
    } else if line_count <= 120 {
        (12, 3)
    } else if line_count <= 300 {
        (18, 4)
    } else if line_count <= 800 {
        (24, 6)
    } else {
        (32, 8)
    }
}

pub(crate) fn build_vector_chunks_for_session(
    session: &HailSession,
    source_hash: &str,
    runtime: &DaemonConfig,
) -> Vec<VectorChunkUpsert> {
    let lines = extract_vector_lines(session);
    let (chunk_size, overlap) = resolve_vector_chunking_profile(lines.len(), runtime);
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

fn is_vector_index_skippable_error(error: &DesktopApiError) -> bool {
    matches!(
        error.code.as_str(),
        "desktop.vector_search_unavailable"
            | "desktop.vector_search_parse_failed"
            | "desktop.vector_index_embedding_mismatch"
    )
}

pub(crate) fn persist_vector_index_failure_snapshot(
    db: &LocalDb,
    error: &DesktopApiError,
) -> DesktopApiResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    let previous = db.get_vector_index_job().ok().flatten();
    set_vector_index_job_snapshot(
        db,
        VectorIndexJobRow {
            status: "failed".to_string(),
            processed_sessions: previous
                .as_ref()
                .map(|row| row.processed_sessions)
                .unwrap_or(0),
            total_sessions: previous.as_ref().map(|row| row.total_sessions).unwrap_or(0),
            message: Some(format_vector_error_message(error)),
            started_at: previous
                .and_then(|row| row.started_at)
                .or(Some(now.clone())),
            finished_at: Some(now),
        },
    )
}

pub(crate) fn rebuild_vector_index_blocking(
    db: &LocalDb,
    runtime: &DaemonConfig,
) -> DesktopApiResult<()> {
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
    let mut failed_sessions = 0u32;
    let mut skipped_sessions = 0u32;
    let mut last_failure: Option<String> = None;
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

        let session_result: DesktopApiResult<bool> = (|| {
            let normalized = match load_normalized_session_body(db, &row.id) {
                Ok(body) => body,
                Err(_) => return Ok(false),
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
                return Ok(false);
            }

            let session = match HailSession::from_jsonl(&normalized) {
                Ok(session) => session,
                Err(_) => return Ok(false),
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
                return Ok(false);
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
            Ok(true)
        })();

        match session_result {
            Ok(indexed) => {
                if !indexed {
                    skipped_sessions = skipped_sessions.saturating_add(1);
                }
            }
            Err(error) if is_vector_index_skippable_error(&error) => {
                failed_sessions = failed_sessions.saturating_add(1);
                last_failure = Some(format!("{}: {}", row.id, error.message));
                eprintln!(
                    "vector index rebuild: skipped {}: {}",
                    row.id, error.message
                );
            }
            Err(error) => return Err(error),
        }

        let processed_sessions = idx.saturating_add(1) as u32;
        let progress_message = match (failed_sessions, skipped_sessions, last_failure.as_deref()) {
            (0, 0, _) => format!("processed {processed_sessions}/{total_sessions} sessions"),
            (0, skipped, _) => format!(
                "processed {processed_sessions}/{total_sessions} sessions ({skipped} skipped)"
            ),
            (failed, 0, Some(last)) => format!(
                "processed {processed_sessions}/{total_sessions} sessions ({failed} failed) | last failure: {last}"
            ),
            (failed, skipped, Some(last)) => format!(
                "processed {processed_sessions}/{total_sessions} sessions ({failed} failed, {skipped} skipped) | last failure: {last}"
            ),
            (failed, 0, _) => format!(
                "processed {processed_sessions}/{total_sessions} sessions ({failed} failed)"
            ),
            (failed, skipped, _) => format!(
                "processed {processed_sessions}/{total_sessions} sessions ({failed} failed, {skipped} skipped)"
            ),
        };
        set_vector_index_job_snapshot(
            db,
            VectorIndexJobRow {
                status: "running".to_string(),
                processed_sessions,
                total_sessions,
                message: Some(progress_message),
                started_at: Some(started_at.clone()),
                finished_at: None,
            },
        )?;
    }

    let message = match (failed_sessions, skipped_sessions) {
        (0, 0) => "vector indexing complete".to_string(),
        (0, skipped) => format!("vector indexing complete ({skipped} skipped)"),
        (failed, 0) => format!("vector indexing complete ({failed} failed)"),
        (failed, skipped) => {
            format!("vector indexing complete ({failed} failed, {skipped} skipped)")
        }
    };
    set_vector_index_job_snapshot(
        db,
        VectorIndexJobRow {
            status: "complete".to_string(),
            processed_sessions: total_sessions,
            total_sessions,
            message: Some(message),
            started_at: Some(started_at),
            finished_at: Some(chrono::Utc::now().to_rfc3339()),
        },
    )?;
    Ok(())
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
pub(crate) fn desktop_vector_preflight() -> DesktopApiResult<DesktopVectorPreflightResponse> {
    let runtime = load_runtime_config()?;
    Ok(vector_preflight_for_runtime(&runtime))
}

#[tauri::command]
pub(crate) fn desktop_vector_install_model(
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
pub(crate) fn desktop_vector_index_rebuild() -> DesktopApiResult<DesktopVectorIndexStatusResponse> {
    let runtime = load_runtime_config()?;
    ensure_vector_provider_ready(&runtime, false)?;

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
                let _ = persist_vector_index_failure_snapshot(&db, &error);
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
pub(crate) fn desktop_vector_index_status() -> DesktopApiResult<DesktopVectorIndexStatusResponse> {
    let db = open_local_db()?;
    desktop_vector_index_status_from_db(&db)
}

#[tauri::command]
pub(crate) fn desktop_search_sessions_vector(
    query: String,
    cursor: Option<String>,
    limit: Option<u32>,
) -> DesktopApiResult<DesktopVectorSearchResponse> {
    let db = open_local_db()?;
    let runtime = load_runtime_config()?;
    search_sessions_vector_internal(&db, &runtime, &query, cursor, limit, None)
}
