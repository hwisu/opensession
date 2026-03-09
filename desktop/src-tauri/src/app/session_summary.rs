use crate::{
    DesktopApiError, DesktopApiResult, desktop_error, enum_label, load_normalized_session_body,
    load_runtime_config, open_local_db,
};
use opensession_api::{
    DesktopSessionSummaryResponse, DesktopSummaryBatchState, DesktopSummaryBatchStatusResponse,
};
use opensession_core::{session::working_directory, trace::Session as HailSession};
use opensession_git_native::{
    NativeGitStorage, SUMMARY_LEDGER_REF, SessionSummaryLedgerRecord, extract_git_context,
    ops::find_repo_root as find_git_repo_root,
};
use opensession_local_db::{LocalDb, LocalSessionFilter, LocalSessionRow, SummaryBatchJobRow};
use opensession_runtime_config::{
    DaemonConfig, SummaryBatchScope as RuntimeSummaryBatchScope, SummaryStorageBackend,
};
use opensession_summary::{GitSummaryRequest, SemanticSummaryArtifact};
use opensession_summary_runtime::summarize_session;
use serde_json::json;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

static SUMMARY_BATCH_RUNNING: LazyLock<Mutex<bool>> = LazyLock::new(|| Mutex::new(false));

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

#[derive(Debug, Clone)]
struct SummaryStorageRecord {
    generated_at: String,
    provider: String,
    model: Option<String>,
    source_kind: String,
    generation_kind: String,
    prompt_fingerprint: Option<String>,
    summary: serde_json::Value,
    source_details: serde_json::Value,
    diff_tree: Vec<serde_json::Value>,
    error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SummaryStorageMigrationStats {
    pub(crate) scanned_sessions: u32,
    pub(crate) found_summaries: u32,
    pub(crate) migrated_summaries: u32,
    pub(crate) failed_summaries: u32,
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

pub(crate) fn load_session_summary_for_runtime(
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

fn resolve_repo_root_from_working_directory(cwd: Option<&str>) -> Option<PathBuf> {
    cwd.map(str::trim)
        .filter(|cwd| !cwd.is_empty())
        .and_then(|cwd| find_git_repo_root(Path::new(cwd)))
}

fn resolve_repo_root_from_source_path(source_path: Option<&str>) -> Option<PathBuf> {
    source_path
        .map(str::trim)
        .filter(|source_path| !source_path.is_empty())
        .and_then(|source_path| find_git_repo_root(Path::new(source_path)))
}

pub(crate) fn resolve_repo_root_from_session_row(row: &LocalSessionRow) -> Option<PathBuf> {
    resolve_repo_root_from_working_directory(row.working_directory.as_deref())
        .or_else(|| resolve_repo_root_from_source_path(row.source_path.as_deref()))
}

pub(crate) fn resolve_summary_repo_root(
    db: &LocalDb,
    session_id: &str,
) -> DesktopApiResult<Option<PathBuf>> {
    let row = db.get_session_by_id(session_id).map_err(|error| {
        desktop_error(
            "desktop.session_summary_repo_resolve_failed",
            500,
            "failed to resolve session repository root",
            Some(json!({ "cause": error.to_string(), "session_id": session_id })),
        )
    })?;
    if let Some(row) = row {
        if let Some(repo_root) = resolve_repo_root_from_session_row(&row) {
            return Ok(Some(repo_root));
        }
    }
    let cwd = std::env::current_dir().ok();
    Ok(cwd.and_then(|path| find_git_repo_root(&path)))
}

pub(crate) fn resolve_summary_repo_root_for_migration(
    db: &LocalDb,
    session_id: &str,
) -> DesktopApiResult<Option<PathBuf>> {
    let row = db.get_session_by_id(session_id).map_err(|error| {
        desktop_error(
            "desktop.session_summary_repo_resolve_failed",
            500,
            "failed to resolve session repository root",
            Some(json!({ "cause": error.to_string(), "session_id": session_id })),
        )
    })?;
    Ok(row.and_then(|row| resolve_repo_root_from_session_row(&row)))
}

pub(crate) fn has_hidden_ref_summary_for_session(
    db: &LocalDb,
    session_id: &str,
) -> DesktopApiResult<bool> {
    let Some(repo_root) = resolve_summary_repo_root_for_migration(db, session_id)? else {
        return Ok(false);
    };
    NativeGitStorage
        .load_summary_at_ref(&repo_root, SUMMARY_LEDGER_REF, session_id)
        .map(|record| record.is_some())
        .map_err(|error| {
            desktop_error(
                "desktop.session_summary_query_failed",
                500,
                "failed to load hidden_ref session summary",
                Some(
                    json!({ "cause": error.to_string(), "session_id": session_id, "repo_root": repo_root }),
                ),
            )
        })
}

fn load_summary_storage_record_from_local_db(
    db: &LocalDb,
    session_id: &str,
) -> DesktopApiResult<Option<SummaryStorageRecord>> {
    let row = db
        .get_session_semantic_summary(session_id)
        .map_err(|error| {
            desktop_error(
                "desktop.session_summary_query_failed",
                500,
                "failed to load session summary from local_db",
                Some(json!({ "cause": error.to_string(), "session_id": session_id })),
            )
        })?;
    let Some(row) = row else {
        return Ok(None);
    };

    let summary = serde_json::from_str::<serde_json::Value>(&row.summary_json).map_err(|error| {
        desktop_error(
            "desktop.runtime_settings_storage_migration_decode_failed",
            500,
            "failed to decode local_db summary_json during storage migration",
            Some(
                json!({ "cause": error.to_string(), "session_id": session_id, "backend": "local_db" }),
            ),
        )
    })?;
    let source_details = match row.source_details_json.as_deref() {
        Some(raw) => serde_json::from_str::<serde_json::Value>(raw).map_err(|error| {
            desktop_error(
                "desktop.runtime_settings_storage_migration_decode_failed",
                500,
                "failed to decode local_db source_details_json during storage migration",
                Some(
                    json!({ "cause": error.to_string(), "session_id": session_id, "backend": "local_db" }),
                ),
            )
        })?,
        None => serde_json::Value::Object(Default::default()),
    };
    let diff_tree = match row.diff_tree_json.as_deref() {
        Some(raw) => serde_json::from_str::<Vec<serde_json::Value>>(raw).map_err(|error| {
            desktop_error(
                "desktop.runtime_settings_storage_migration_decode_failed",
                500,
                "failed to decode local_db diff_tree_json during storage migration",
                Some(
                    json!({ "cause": error.to_string(), "session_id": session_id, "backend": "local_db" }),
                ),
            )
        })?,
        None => Vec::new(),
    };

    Ok(Some(SummaryStorageRecord {
        generated_at: row.generated_at,
        provider: row.provider,
        model: row.model,
        source_kind: row.source_kind,
        generation_kind: row.generation_kind,
        prompt_fingerprint: row.prompt_fingerprint,
        summary,
        source_details,
        diff_tree,
        error: row.error,
    }))
}

fn load_summary_storage_record_from_hidden_ref(
    db: &LocalDb,
    session_id: &str,
) -> DesktopApiResult<Option<SummaryStorageRecord>> {
    let Some(repo_root) = resolve_summary_repo_root_for_migration(db, session_id)? else {
        return Ok(None);
    };
    let loaded = NativeGitStorage
        .load_summary_at_ref(&repo_root, SUMMARY_LEDGER_REF, session_id)
        .map_err(|error| {
            desktop_error(
                "desktop.session_summary_query_failed",
                500,
                "failed to load hidden_ref session summary for storage migration",
                Some(
                    json!({ "cause": error.to_string(), "session_id": session_id, "repo_root": repo_root }),
                ),
            )
        })?;
    let Some(row) = loaded else {
        return Ok(None);
    };
    Ok(Some(SummaryStorageRecord {
        generated_at: row.generated_at,
        provider: row.provider,
        model: row.model,
        source_kind: row.source_kind,
        generation_kind: row.generation_kind,
        prompt_fingerprint: row.prompt_fingerprint,
        summary: row.summary,
        source_details: row.source_details,
        diff_tree: row.diff_tree,
        error: row.error,
    }))
}

fn load_summary_storage_record(
    db: &LocalDb,
    backend: &SummaryStorageBackend,
    session_id: &str,
) -> DesktopApiResult<Option<SummaryStorageRecord>> {
    match backend {
        SummaryStorageBackend::LocalDb => load_summary_storage_record_from_local_db(db, session_id),
        SummaryStorageBackend::HiddenRef => {
            load_summary_storage_record_from_hidden_ref(db, session_id)
        }
        SummaryStorageBackend::None => Ok(None),
    }
}

fn persist_summary_storage_record_to_local_db(
    db: &LocalDb,
    session_id: &str,
    record: &SummaryStorageRecord,
) -> DesktopApiResult<()> {
    let summary_json = serde_json::to_string(&record.summary).map_err(|error| {
        desktop_error(
            "desktop.runtime_settings_storage_migration_encode_failed",
            500,
            "failed to encode summary_json for local_db storage migration",
            Some(json!({ "cause": error.to_string(), "session_id": session_id })),
        )
    })?;
    let source_details_json = match &record.source_details {
        serde_json::Value::Null => None,
        serde_json::Value::Object(map) if map.is_empty() => None,
        value => Some(serde_json::to_string(value).map_err(|error| {
            desktop_error(
                "desktop.runtime_settings_storage_migration_encode_failed",
                500,
                "failed to encode source_details_json for local_db storage migration",
                Some(json!({ "cause": error.to_string(), "session_id": session_id })),
            )
        })?),
    };
    let diff_tree_json = if record.diff_tree.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&record.diff_tree).map_err(|error| {
            desktop_error(
                "desktop.runtime_settings_storage_migration_encode_failed",
                500,
                "failed to encode diff_tree_json for local_db storage migration",
                Some(json!({ "cause": error.to_string(), "session_id": session_id })),
            )
        })?)
    };

    db.upsert_session_semantic_summary(&opensession_local_db::SessionSemanticSummaryUpsert {
        session_id,
        summary_json: &summary_json,
        generated_at: &record.generated_at,
        provider: &record.provider,
        model: record.model.as_deref(),
        source_kind: &record.source_kind,
        generation_kind: &record.generation_kind,
        prompt_fingerprint: record.prompt_fingerprint.as_deref(),
        source_details_json: source_details_json.as_deref(),
        diff_tree_json: diff_tree_json.as_deref(),
        error: record.error.as_deref(),
    })
    .map_err(|error| {
        desktop_error(
            "desktop.runtime_settings_storage_migration_persist_failed",
            500,
            "failed to persist migrated summary to local_db",
            Some(json!({ "cause": error.to_string(), "session_id": session_id })),
        )
    })?;
    Ok(())
}

fn persist_summary_storage_record_to_hidden_ref(
    db: &LocalDb,
    session_id: &str,
    record: &SummaryStorageRecord,
) -> DesktopApiResult<()> {
    let Some(repo_root) = resolve_summary_repo_root_for_migration(db, session_id)? else {
        return Err(desktop_error(
            "desktop.runtime_settings_storage_migration_repo_required",
            422,
            "cannot migrate summary to hidden_ref without a git repository for the session",
            Some(json!({ "session_id": session_id })),
        ));
    };
    let payload = SessionSummaryLedgerRecord {
        session_id: session_id.to_string(),
        generated_at: record.generated_at.clone(),
        provider: record.provider.clone(),
        model: record.model.clone(),
        source_kind: record.source_kind.clone(),
        generation_kind: record.generation_kind.clone(),
        prompt_fingerprint: record.prompt_fingerprint.clone(),
        summary: record.summary.clone(),
        source_details: record.source_details.clone(),
        diff_tree: record.diff_tree.clone(),
        error: record.error.clone(),
    };
    NativeGitStorage
        .store_summary_at_ref(&repo_root, SUMMARY_LEDGER_REF, &payload)
        .map_err(|error| {
            desktop_error(
                "desktop.runtime_settings_storage_migration_persist_failed",
                500,
                "failed to persist migrated summary to hidden_ref",
                Some(
                    json!({ "cause": error.to_string(), "session_id": session_id, "repo_root": repo_root }),
                ),
            )
        })?;
    Ok(())
}

fn persist_summary_storage_record(
    db: &LocalDb,
    backend: &SummaryStorageBackend,
    session_id: &str,
    record: &SummaryStorageRecord,
) -> DesktopApiResult<()> {
    match backend {
        SummaryStorageBackend::LocalDb => {
            persist_summary_storage_record_to_local_db(db, session_id, record)
        }
        SummaryStorageBackend::HiddenRef => {
            persist_summary_storage_record_to_hidden_ref(db, session_id, record)
        }
        SummaryStorageBackend::None => Ok(()),
    }
}

fn summary_storage_migration_session_ids(
    db: &LocalDb,
    from_backend: &SummaryStorageBackend,
) -> DesktopApiResult<Vec<String>> {
    match from_backend {
        SummaryStorageBackend::LocalDb => db.list_session_semantic_summary_ids().map_err(|error| {
            desktop_error(
                "desktop.runtime_settings_storage_migration_source_query_failed",
                500,
                "failed to list local_db summary session ids for migration",
                Some(json!({ "cause": error.to_string(), "backend": "local_db" })),
            )
        }),
        SummaryStorageBackend::HiddenRef => db.list_all_session_ids().map_err(|error| {
            desktop_error(
                "desktop.runtime_settings_storage_migration_source_query_failed",
                500,
                "failed to list sessions for hidden_ref migration",
                Some(json!({ "cause": error.to_string(), "backend": "hidden_ref" })),
            )
        }),
        SummaryStorageBackend::None => Ok(Vec::new()),
    }
}

pub(crate) fn migrate_summary_storage_backend(
    db: &LocalDb,
    from_backend: &SummaryStorageBackend,
    to_backend: &SummaryStorageBackend,
) -> DesktopApiResult<SummaryStorageMigrationStats> {
    if from_backend == to_backend
        || matches!(from_backend, SummaryStorageBackend::None)
        || matches!(to_backend, SummaryStorageBackend::None)
    {
        return Ok(SummaryStorageMigrationStats::default());
    }

    let session_ids = summary_storage_migration_session_ids(db, from_backend)?;
    let mut stats = SummaryStorageMigrationStats {
        scanned_sessions: session_ids.len() as u32,
        ..SummaryStorageMigrationStats::default()
    };
    let mut failure_messages: Vec<String> = Vec::new();

    for session_id in session_ids {
        let Some(record) = load_summary_storage_record(db, from_backend, &session_id)? else {
            continue;
        };
        stats.found_summaries = stats.found_summaries.saturating_add(1);

        if let Err(error) = persist_summary_storage_record(db, to_backend, &session_id, &record) {
            stats.failed_summaries = stats.failed_summaries.saturating_add(1);
            failure_messages.push(format!("{}: {}", session_id, error.message));
            continue;
        }
        stats.migrated_summaries = stats.migrated_summaries.saturating_add(1);
    }

    if stats.failed_summaries > 0 {
        let mut details = json!({
            "from_backend": enum_label(from_backend),
            "to_backend": enum_label(to_backend),
            "scanned_sessions": stats.scanned_sessions,
            "found_summaries": stats.found_summaries,
            "migrated_summaries": stats.migrated_summaries,
            "failed_summaries": stats.failed_summaries,
        });
        if let Some(object) = details.as_object_mut() {
            object.insert(
                "failures".to_string(),
                serde_json::Value::Array(
                    failure_messages
                        .into_iter()
                        .take(10)
                        .map(serde_json::Value::String)
                        .collect(),
                ),
            );
        }
        return Err(desktop_error(
            "desktop.runtime_settings_storage_migration_failed",
            422,
            "failed to migrate existing summaries to the selected storage backend",
            Some(details),
        ));
    }

    Ok(stats)
}

pub(crate) fn persist_summary_to_local_db(
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

pub(crate) fn persist_summary_to_hidden_ref(
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

fn map_summary_batch_state(raw: &str) -> DesktopSummaryBatchState {
    match raw {
        "running" => DesktopSummaryBatchState::Running,
        "complete" => DesktopSummaryBatchState::Complete,
        "failed" => DesktopSummaryBatchState::Failed,
        _ => DesktopSummaryBatchState::Idle,
    }
}

fn desktop_summary_batch_status_from_db(
    db: &LocalDb,
) -> DesktopApiResult<DesktopSummaryBatchStatusResponse> {
    let row = db.get_summary_batch_job().map_err(|error| {
        desktop_error(
            "desktop.summary_batch_status_failed",
            500,
            "failed to read summary batch status",
            Some(json!({ "cause": error.to_string() })),
        )
    })?;

    let Some(row) = row else {
        return Ok(DesktopSummaryBatchStatusResponse {
            state: DesktopSummaryBatchState::Idle,
            processed_sessions: 0,
            total_sessions: 0,
            failed_sessions: 0,
            message: None,
            started_at: None,
            finished_at: None,
        });
    };

    Ok(DesktopSummaryBatchStatusResponse {
        state: map_summary_batch_state(&row.status),
        processed_sessions: row.processed_sessions,
        total_sessions: row.total_sessions,
        failed_sessions: row.failed_sessions,
        message: row.message,
        started_at: row.started_at,
        finished_at: row.finished_at,
    })
}

fn set_summary_batch_job_snapshot(
    db: &LocalDb,
    payload: SummaryBatchJobRow,
) -> DesktopApiResult<()> {
    db.set_summary_batch_job(&payload).map_err(|error| {
        desktop_error(
            "desktop.summary_batch_status_failed",
            500,
            "failed to persist summary batch status",
            Some(json!({ "cause": error.to_string() })),
        )
    })
}

fn is_summary_batch_skippable_error(error: &DesktopApiError) -> bool {
    matches!(
        error.code.as_str(),
        "desktop.session_source_unavailable" | "desktop.session_body_not_found"
    )
}

#[derive(Debug, Default)]
struct SummaryBatchSelection {
    pending_session_ids: Vec<String>,
    already_summarized_sessions: u32,
}

fn summary_batch_session_ids_for_scope(
    db: &LocalDb,
    scope: &RuntimeSummaryBatchScope,
    recent_days: u16,
) -> DesktopApiResult<SummaryBatchSelection> {
    let filter = LocalSessionFilter {
        limit: None,
        offset: None,
        ..LocalSessionFilter::default()
    };
    let mut rows = db.list_sessions(&filter).map_err(|error| {
        desktop_error(
            "desktop.summary_batch_list_failed",
            500,
            "failed to list sessions for summary batch",
            Some(json!({ "cause": error.to_string() })),
        )
    })?;

    if matches!(scope, RuntimeSummaryBatchScope::RecentDays) {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(i64::from(recent_days.max(1)));
        rows.retain(|row| {
            chrono::DateTime::parse_from_rfc3339(&row.created_at)
                .map(|parsed| parsed.with_timezone(&chrono::Utc) >= cutoff)
                .unwrap_or(true)
        });
    }

    let local_summary_ids = db
        .list_session_semantic_summary_ids()
        .map(|ids| ids.into_iter().collect::<HashSet<_>>())
        .map_err(|error| {
            desktop_error(
                "desktop.session_summary_query_failed",
                500,
                "failed to list existing local_db session summaries",
                Some(json!({ "cause": error.to_string() })),
            )
        })?;

    let mut selection = SummaryBatchSelection {
        pending_session_ids: Vec::with_capacity(rows.len()),
        already_summarized_sessions: 0,
    };
    for row in rows {
        let session_id = row.id;
        let already_summarized = local_summary_ids.contains(&session_id)
            || has_hidden_ref_summary_for_session(db, &session_id)?;
        if already_summarized {
            selection.already_summarized_sessions =
                selection.already_summarized_sessions.saturating_add(1);
            continue;
        }
        selection.pending_session_ids.push(session_id);
    }

    Ok(selection)
}

fn build_git_summary_request_for_session(
    session: &HailSession,
    runtime: &DaemonConfig,
) -> Option<GitSummaryRequest> {
    if !runtime.summary.allows_git_changes_fallback() {
        return None;
    }
    working_directory(session)
        .and_then(|cwd| find_git_repo_root(Path::new(cwd)).map(|repo_root| (cwd, repo_root)))
        .map(|(cwd, repo_root)| GitSummaryRequest {
            repo_root,
            commit: extract_git_context(cwd).commit,
        })
}

async fn generate_session_summary_artifact_for_id(
    db: &LocalDb,
    runtime: &DaemonConfig,
    session_id: &str,
) -> DesktopApiResult<SemanticSummaryArtifact> {
    let normalized_session = load_normalized_session_body(db, session_id)?;
    let session = HailSession::from_jsonl(&normalized_session).map_err(|error| {
        desktop_error(
            "desktop.session_summary_parse_failed",
            422,
            "failed to decode session body for summary regeneration",
            Some(json!({ "cause": error.to_string(), "session_id": session_id })),
        )
    })?;
    let git_request = build_git_summary_request_for_session(&session, runtime);
    summarize_session(&session, &runtime.summary, git_request.as_ref())
        .await
        .map_err(|error| {
            desktop_error(
                "desktop.session_summary_generate_failed",
                500,
                "failed to generate semantic summary",
                Some(json!({ "cause": error.to_string(), "session_id": session_id })),
            )
        })
}

fn summary_repo_required_error(session_id: &str) -> DesktopApiError {
    desktop_error(
        "desktop.session_summary_repo_required",
        422,
        "hidden_ref summary backend requires a git repository",
        Some(json!({ "session_id": session_id })),
    )
}

fn summary_response_from_artifact(
    session_id: &str,
    artifact: &SemanticSummaryArtifact,
) -> DesktopSessionSummaryResponse {
    DesktopSessionSummaryResponse {
        session_id: session_id.to_string(),
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
    }
}

fn persist_summary_for_runtime(
    db: &LocalDb,
    runtime: &DaemonConfig,
    session_id: &str,
    artifact: &SemanticSummaryArtifact,
) -> DesktopApiResult<()> {
    match runtime.summary.storage.backend {
        SummaryStorageBackend::LocalDb => persist_summary_to_local_db(db, session_id, artifact),
        SummaryStorageBackend::HiddenRef => {
            let Some(repo_root) = resolve_summary_repo_root(db, session_id)? else {
                return Err(summary_repo_required_error(session_id));
            };
            persist_summary_to_hidden_ref(&repo_root, session_id, artifact)
        }
        SummaryStorageBackend::None => Ok(()),
    }
}

fn summary_response_after_persist(
    db: &LocalDb,
    runtime: &DaemonConfig,
    session_id: &str,
    artifact: &SemanticSummaryArtifact,
) -> DesktopApiResult<DesktopSessionSummaryResponse> {
    if matches!(runtime.summary.storage.backend, SummaryStorageBackend::None) {
        return Ok(summary_response_from_artifact(session_id, artifact));
    }
    load_session_summary_for_runtime(db, runtime, session_id)
}

pub(crate) fn run_summary_batch_for_runtime(
    runtime: DaemonConfig,
) -> DesktopApiResult<DesktopSummaryBatchStatusResponse> {
    let db = open_local_db()?;
    {
        let mut running = SUMMARY_BATCH_RUNNING
            .lock()
            .expect("summary batch mutex poisoned");
        if *running {
            return desktop_summary_batch_status_from_db(&db);
        }
        *running = true;
    }

    let started_at = chrono::Utc::now().to_rfc3339();
    set_summary_batch_job_snapshot(
        &db,
        SummaryBatchJobRow {
            status: "running".to_string(),
            processed_sessions: 0,
            total_sessions: 0,
            failed_sessions: 0,
            message: Some("starting summary batch".to_string()),
            started_at: Some(started_at),
            finished_at: None,
        },
    )?;

    std::thread::spawn(move || {
        let run_result: DesktopApiResult<()> = (|| {
            let db = open_local_db()?;
            let selection = summary_batch_session_ids_for_scope(
                &db,
                &runtime.summary.batch.scope,
                runtime.summary.batch.recent_days,
            )?;
            let total_sessions = selection.pending_session_ids.len() as u32;
            let already_summarized_sessions = selection.already_summarized_sessions;
            let started_at = chrono::Utc::now().to_rfc3339();
            let initial_message = if already_summarized_sessions > 0 {
                format!(
                    "processing semantic summaries ({already_summarized_sessions} already summarized)"
                )
            } else {
                "processing semantic summaries".to_string()
            };
            set_summary_batch_job_snapshot(
                &db,
                SummaryBatchJobRow {
                    status: "running".to_string(),
                    processed_sessions: 0,
                    total_sessions,
                    failed_sessions: 0,
                    message: Some(initial_message),
                    started_at: Some(started_at.clone()),
                    finished_at: None,
                },
            )?;

            if total_sessions == 0 {
                let message = if already_summarized_sessions > 0 {
                    format!(
                        "summary batch complete ({already_summarized_sessions} already summarized)"
                    )
                } else {
                    "summary batch complete (no pending sessions)".to_string()
                };
                set_summary_batch_job_snapshot(
                    &db,
                    SummaryBatchJobRow {
                        status: "complete".to_string(),
                        processed_sessions: 0,
                        total_sessions: 0,
                        failed_sessions: 0,
                        message: Some(message),
                        started_at: Some(started_at),
                        finished_at: Some(chrono::Utc::now().to_rfc3339()),
                    },
                )?;
                return Ok(());
            }

            let mut processed_sessions = 0u32;
            let mut failed_sessions = 0u32;
            let mut skipped_sessions = 0u32;
            for session_id in selection.pending_session_ids {
                set_summary_batch_job_snapshot(
                    &db,
                    SummaryBatchJobRow {
                        status: "running".to_string(),
                        processed_sessions,
                        total_sessions,
                        failed_sessions,
                        message: Some(format!("processing {session_id}")),
                        started_at: Some(started_at.clone()),
                        finished_at: None,
                    },
                )?;

                let generated = tauri::async_runtime::block_on(
                    generate_session_summary_artifact_for_id(&db, &runtime, &session_id),
                );
                if let Err(error) = generated.and_then(|artifact| {
                    persist_summary_for_runtime(&db, &runtime, &session_id, &artifact)
                }) {
                    if is_summary_batch_skippable_error(&error) {
                        skipped_sessions = skipped_sessions.saturating_add(1);
                        eprintln!("summary batch: skipped {session_id}: {}", error.message);
                    } else {
                        failed_sessions = failed_sessions.saturating_add(1);
                        eprintln!(
                            "summary batch: failed to process {session_id}: {}",
                            error.message
                        );
                    }
                }

                processed_sessions = processed_sessions.saturating_add(1);
                set_summary_batch_job_snapshot(
                    &db,
                    SummaryBatchJobRow {
                        status: "running".to_string(),
                        processed_sessions,
                        total_sessions,
                        failed_sessions,
                        message: Some(format!(
                            "processed {processed_sessions}/{total_sessions} sessions"
                        )),
                        started_at: Some(started_at.clone()),
                        finished_at: None,
                    },
                )?;
            }

            let status = if failed_sessions > 0 {
                "failed"
            } else {
                "complete"
            };
            let mut message = if failed_sessions > 0 {
                if skipped_sessions > 0 {
                    format!(
                        "summary batch finished with {failed_sessions} failures ({skipped_sessions} skipped missing sources)"
                    )
                } else {
                    format!("summary batch finished with {failed_sessions} failures")
                }
            } else if skipped_sessions > 0 {
                format!("summary batch complete ({skipped_sessions} skipped missing sources)")
            } else {
                "summary batch complete".to_string()
            };
            if already_summarized_sessions > 0 {
                message.push_str(&format!(
                    "; {already_summarized_sessions} already summarized"
                ));
            }
            set_summary_batch_job_snapshot(
                &db,
                SummaryBatchJobRow {
                    status: status.to_string(),
                    processed_sessions,
                    total_sessions,
                    failed_sessions,
                    message: Some(message),
                    started_at: Some(started_at),
                    finished_at: Some(chrono::Utc::now().to_rfc3339()),
                },
            )?;
            Ok(())
        })();

        if let Err(error) = run_result {
            if let Ok(db) = open_local_db() {
                let now = chrono::Utc::now().to_rfc3339();
                let _ = set_summary_batch_job_snapshot(
                    &db,
                    SummaryBatchJobRow {
                        status: "failed".to_string(),
                        processed_sessions: 0,
                        total_sessions: 0,
                        failed_sessions: 0,
                        message: Some(error.message.clone()),
                        started_at: Some(now.clone()),
                        finished_at: Some(now),
                    },
                );
            }
            eprintln!("summary batch run failed: {}", error.message);
        }

        if let Ok(mut running) = SUMMARY_BATCH_RUNNING.lock() {
            *running = false;
        }
    });

    desktop_summary_batch_status_from_db(&db)
}

pub(crate) fn maybe_start_summary_batch_on_app_start() {
    let runtime = match load_runtime_config() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!(
                "failed to load runtime config for app-start summary batch: {}",
                error.message
            );
            return;
        }
    };

    if !matches!(
        runtime.summary.batch.execution_mode,
        opensession_runtime_config::SummaryBatchExecutionMode::OnAppStart
    ) {
        return;
    }

    if let Err(error) = run_summary_batch_for_runtime(runtime) {
        eprintln!("failed to start app-start summary batch: {}", error.message);
    }
}

#[tauri::command]
pub(crate) fn desktop_get_session_summary(
    id: String,
) -> DesktopApiResult<DesktopSessionSummaryResponse> {
    let db = open_local_db()?;
    let runtime = load_runtime_config()?;
    load_session_summary_for_runtime(&db, &runtime, &id)
}

#[tauri::command]
pub(crate) async fn desktop_regenerate_session_summary(
    id: String,
) -> DesktopApiResult<DesktopSessionSummaryResponse> {
    let db = open_local_db()?;
    let runtime = load_runtime_config()?;
    let artifact = generate_session_summary_artifact_for_id(&db, &runtime, &id).await?;
    persist_summary_for_runtime(&db, &runtime, &id, &artifact)?;
    summary_response_after_persist(&db, &runtime, &id, &artifact)
}

#[tauri::command]
pub(crate) fn desktop_summary_batch_status() -> DesktopApiResult<DesktopSummaryBatchStatusResponse>
{
    let db = open_local_db()?;
    desktop_summary_batch_status_from_db(&db)
}

#[tauri::command]
pub(crate) fn desktop_summary_batch_run() -> DesktopApiResult<DesktopSummaryBatchStatusResponse> {
    let runtime = load_runtime_config()?;
    run_summary_batch_for_runtime(runtime)
}
