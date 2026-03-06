use crate::{DesktopApiResult, desktop_error, enum_label};
use opensession_api::DesktopSessionSummaryResponse;
use opensession_git_native::{
    NativeGitStorage, SUMMARY_LEDGER_REF, SessionSummaryLedgerRecord,
    ops::find_repo_root as find_git_repo_root,
};
use opensession_local_db::{LocalDb, LocalSessionRow};
use opensession_runtime_config::{DaemonConfig, SummaryStorageBackend};
use serde_json::json;
use std::path::{Path, PathBuf};

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
