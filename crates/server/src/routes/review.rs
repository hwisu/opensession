use axum::{
    Json,
    extract::{Path, Query, State},
};
use opensession_api::{
    JobManifest, JobReviewBundle, JobReviewBundleJob, JobReviewKind, JobReviewRun,
    JobReviewSelectedReview, LocalReviewBundle, LocalReviewLayerFileChange,
    LocalReviewReviewerDigest, LocalReviewReviewerQa, LocalReviewSemanticSummary,
    LocalReviewSession, job_manifest_from_session,
};
use opensession_core::source_uri::{SourceSpec, SourceUri};
use opensession_core::{ContentBlock, EventType, Session};
use opensession_local_db::LocalDb;
use opensession_local_store::{read_local_object, read_local_object_from_uri};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::AppConfig;
use crate::error::ApiErr;

/// GET /api/review/local/:review_id — load a local PR review bundle.
pub async fn get_local_review_bundle(
    State(config): State<AppConfig>,
    Path(review_id): Path<String>,
) -> Result<Json<LocalReviewBundle>, ApiErr> {
    let root = config
        .local_review_root
        .as_ref()
        .ok_or_else(|| ApiErr::not_found("local review API is not enabled on this server"))?;

    validate_review_id(&review_id)?;
    let bundle_path = root.join(&review_id).join("bundle.json");
    let body = tokio::fs::read(&bundle_path)
        .await
        .map_err(|_| ApiErr::not_found("local review bundle not found"))?;

    let parsed: LocalReviewBundle =
        serde_json::from_slice(&body).map_err(|_| ApiErr::internal("invalid review bundle"))?;
    Ok(Json(parsed))
}

#[derive(Debug, Deserialize)]
pub struct JobReviewQuery {
    pub kind: JobReviewKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
}

#[derive(Clone)]
struct JobReviewSessionData {
    manifest: JobManifest,
    review_session: LocalReviewSession,
}

/// GET /api/review/job/:job_id?kind=todo|done[&run_id=...]
pub async fn get_job_review_bundle(
    Path(job_id): Path<String>,
    Query(query): Query<JobReviewQuery>,
) -> Result<Json<JobReviewBundle>, ApiErr> {
    validate_job_id(&job_id)?;
    let db = LocalDb::open().map_err(|_| ApiErr::internal("failed to open local review db"))?;
    let rows = db
        .list_sessions_for_job(&job_id)
        .map_err(|_| ApiErr::internal("failed to load job sessions"))?;
    if rows.is_empty() {
        return Err(ApiErr::not_found("job review bundle not found"));
    }

    let cwd = std::env::current_dir().map_err(|_| ApiErr::internal("read current directory"))?;
    let mut sessions = Vec::new();
    for row in rows {
        let Some(body_ref) = row.body_storage_key.as_deref() else {
            continue;
        };
        let (hail_path, bytes) = read_session_bytes(body_ref, &cwd)
            .map_err(|_| ApiErr::internal("failed to read local session body"))?;
        let text = String::from_utf8(bytes).map_err(|_| ApiErr::internal("invalid hail utf-8"))?;
        let session =
            Session::from_jsonl(&text).map_err(|_| ApiErr::internal("invalid session body"))?;
        let Some(manifest) = job_manifest_from_session(&session) else {
            continue;
        };
        if manifest.job_id != job_id {
            continue;
        }
        sessions.push(JobReviewSessionData {
            manifest,
            review_session: LocalReviewSession {
                session_id: session.session_id.clone(),
                ledger_ref: body_ref.to_string(),
                hail_path,
                commit_shas: row.git_commit.iter().cloned().collect(),
                session,
            },
        });
    }

    if sessions.is_empty() {
        return Err(ApiErr::not_found("job review bundle not found"));
    }

    let bundle = build_job_review_bundle(&db, sessions, query)
        .map_err(|_| ApiErr::internal("failed to build job review bundle"))?;
    Ok(Json(bundle))
}

fn validate_review_id(review_id: &str) -> Result<(), ApiErr> {
    if review_id.is_empty() {
        return Err(ApiErr::bad_request("review_id is required"));
    }
    let is_valid = review_id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.');
    if !is_valid {
        return Err(ApiErr::bad_request("review_id contains invalid characters"));
    }
    Ok(())
}

fn validate_job_id(job_id: &str) -> Result<(), ApiErr> {
    if job_id.trim().is_empty() {
        return Err(ApiErr::bad_request("job_id is required"));
    }
    Ok(())
}

fn read_session_bytes(body_ref: &str, cwd: &std::path::Path) -> Result<(String, Vec<u8>), ApiErr> {
    if let Ok(uri) = SourceUri::parse(body_ref) {
        let (path, bytes) = read_local_object_from_uri(&uri, cwd)
            .map_err(|_| ApiErr::internal("read local body from uri"))?;
        return Ok((path.display().to_string(), bytes));
    }

    let (uri, path, bytes) = read_local_object(body_ref, cwd)
        .map_err(|_| ApiErr::internal("read local body from hash"))?;
    let hail_path = if matches!(uri, SourceUri::Src(SourceSpec::Local { .. })) {
        path.display().to_string()
    } else {
        body_ref.to_string()
    };
    Ok((hail_path, bytes))
}

fn build_job_review_bundle(
    db: &LocalDb,
    sessions: Vec<JobReviewSessionData>,
    query: JobReviewQuery,
) -> anyhow::Result<JobReviewBundle> {
    let mut runs = BTreeMap::<String, Vec<JobReviewSessionData>>::new();
    for session in sessions {
        runs.entry(session.manifest.run_id.clone())
            .or_default()
            .push(session);
    }

    let mut ordered_runs = runs.into_values().collect::<Vec<_>>();
    ordered_runs.sort_by(|lhs, rhs| {
        let lhs_attempt = lhs
            .first()
            .map(|row| row.manifest.attempt)
            .unwrap_or_default();
        let rhs_attempt = rhs
            .first()
            .map(|row| row.manifest.attempt)
            .unwrap_or_default();
        rhs_attempt
            .cmp(&lhs_attempt)
            .then_with(|| latest_created(rhs).cmp(&latest_created(lhs)))
    });

    let selected_run_index = if let Some(run_id) = query.run_id.as_deref() {
        ordered_runs
            .iter()
            .position(|run| run.first().is_some_and(|row| row.manifest.run_id == run_id))
            .ok_or_else(|| anyhow::anyhow!("requested run_id is not available"))?
    } else {
        ordered_runs
            .iter()
            .position(|run| {
                run.iter()
                    .any(|row| matches_review(&row.manifest, query.kind))
            })
            .ok_or_else(|| anyhow::anyhow!("requested review kind is not available"))?
    };

    let selected_run = ordered_runs
        .get(selected_run_index)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("selected run is missing"))?;
    let selected_review = selected_run
        .iter()
        .filter(|row| matches_review(&row.manifest, query.kind))
        .max_by_key(|row| row.review_session.session.context.created_at)
        .ok_or_else(|| anyhow::anyhow!("selected run does not contain requested review kind"))?;

    let mut bundle_runs = Vec::new();
    for run in ordered_runs.iter().cloned() {
        let filtered = filter_run_sessions(run, query.kind);
        if filtered.is_empty() {
            continue;
        }
        let run_manifest = filtered
            .last()
            .map(|row| &row.manifest)
            .ok_or_else(|| anyhow::anyhow!("run manifest missing"))?;
        let sessions = filtered
            .iter()
            .map(|row| row.review_session.clone())
            .collect::<Vec<_>>();
        let artifacts = dedupe_artifacts(&filtered);
        bundle_runs.push(JobReviewRun {
            run_id: run_manifest.run_id.clone(),
            attempt: run_manifest.attempt,
            status: run_manifest.status,
            sessions,
            artifacts,
        });
    }

    let selected_filtered = filter_run_sessions(selected_run.clone(), query.kind);
    let review_digest = build_reviewer_digest_for_sessions(
        &selected_filtered
            .iter()
            .map(|row| row.review_session.clone())
            .collect::<Vec<_>>(),
    );
    let semantic_summary = db
        .get_session_semantic_summary(&selected_review.review_session.session_id)?
        .and_then(local_review_semantic_summary_from_row);
    let handoff_artifact_uri = dedupe_artifacts(&selected_filtered)
        .into_iter()
        .find(|artifact| artifact.kind.eq_ignore_ascii_case("handoff"))
        .map(|artifact| artifact.uri);
    let job = JobReviewBundleJob {
        protocol: selected_review.manifest.protocol,
        system: selected_review.manifest.system.clone(),
        job_id: selected_review.manifest.job_id.clone(),
        job_title: selected_review.manifest.job_title.clone(),
    };

    Ok(JobReviewBundle {
        job,
        selected_review: JobReviewSelectedReview {
            session_id: selected_review.review_session.session_id.clone(),
            run_id: selected_review.manifest.run_id.clone(),
            attempt: selected_review.manifest.attempt,
            kind: query.kind,
            status: selected_review.manifest.status,
            created_at: selected_review
                .review_session
                .session
                .context
                .created_at
                .to_rfc3339(),
        },
        runs: bundle_runs,
        review_digest,
        semantic_summary,
        handoff_artifact_uri,
    })
}

fn latest_created(run: &[JobReviewSessionData]) -> chrono::DateTime<chrono::Utc> {
    run.iter()
        .map(|row| row.review_session.session.context.created_at)
        .max()
        .unwrap_or_else(chrono::Utc::now)
}

fn matches_review(manifest: &JobManifest, kind: JobReviewKind) -> bool {
    manifest.stage == opensession_api::JobStage::Review && manifest.review_kind == Some(kind)
}

fn include_session_for_kind(manifest: &JobManifest, kind: JobReviewKind) -> bool {
    match kind {
        JobReviewKind::Todo => {
            manifest.stage == opensession_api::JobStage::Planning
                || matches_review(manifest, JobReviewKind::Todo)
        }
        JobReviewKind::Done => {
            matches_review(manifest, JobReviewKind::Todo)
                || manifest.stage == opensession_api::JobStage::Execution
                || matches_review(manifest, JobReviewKind::Done)
                || manifest.stage == opensession_api::JobStage::Handoff
        }
    }
}

fn filter_run_sessions(
    mut run: Vec<JobReviewSessionData>,
    kind: JobReviewKind,
) -> Vec<JobReviewSessionData> {
    run.retain(|row| include_session_for_kind(&row.manifest, kind));
    run.sort_by_key(|row| row.review_session.session.context.created_at);
    run
}

fn dedupe_artifacts(rows: &[JobReviewSessionData]) -> Vec<opensession_api::JobArtifactRef> {
    let mut seen = BTreeSet::<(String, String)>::new();
    let mut artifacts = Vec::new();
    for row in rows {
        for artifact in &row.manifest.artifacts {
            let key = (artifact.kind.clone(), artifact.uri.clone());
            if seen.insert(key) {
                artifacts.push(artifact.clone());
            }
        }
    }
    artifacts
}

fn build_reviewer_digest_for_sessions(
    sessions: &[LocalReviewSession],
) -> LocalReviewReviewerDigest {
    let mut pending_questions = VecDeque::<String>::new();
    let mut qa = Vec::<LocalReviewReviewerQa>::new();
    let mut modified_files = BTreeSet::<String>::new();

    for row in sessions {
        for event in &row.session.events {
            let source = event
                .attributes
                .get("source")
                .and_then(|value| value.as_str())
                .map(|value| value.trim().to_ascii_lowercase())
                .unwrap_or_default();

            match &event.event_type {
                EventType::SystemMessage if source == "interactive_question" => {
                    if let Some(text) = first_text_for_digest(&event.content.blocks) {
                        pending_questions.push_back(text);
                    }
                }
                EventType::UserMessage if source == "interactive" => {
                    let Some(answer) = first_text_for_digest(&event.content.blocks) else {
                        continue;
                    };
                    let question = pending_questions
                        .pop_front()
                        .unwrap_or_else(|| "(interactive question missing)".to_string());
                    qa.push(LocalReviewReviewerQa {
                        question,
                        answer: Some(answer),
                    });
                }
                EventType::FileEdit { path, .. }
                | EventType::FileCreate { path }
                | EventType::FileDelete { path } => {
                    let trimmed = path.trim();
                    if !trimmed.is_empty() {
                        modified_files.insert(trimmed.to_string());
                    }
                }
                _ => {}
            }
        }
    }

    qa.extend(
        pending_questions
            .into_iter()
            .map(|question| LocalReviewReviewerQa {
                question,
                answer: None,
            }),
    );
    if qa.len() > 12 {
        qa.truncate(12);
    }

    let modified_files = modified_files.into_iter().collect::<Vec<_>>();
    let test_files = modified_files
        .iter()
        .filter(|path| is_test_file_path(path))
        .cloned()
        .collect::<Vec<_>>();

    LocalReviewReviewerDigest {
        qa,
        modified_files,
        test_files,
    }
}

fn first_text_for_digest(blocks: &[ContentBlock]) -> Option<String> {
    for block in blocks {
        if let ContentBlock::Text { text } = block {
            let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
            let trimmed = compact.trim();
            if trimmed.is_empty() {
                continue;
            }
            return Some(trimmed.chars().take(220).collect());
        }
    }
    None
}

fn is_test_file_path(path: &str) -> bool {
    let normalized = path.trim().replace('\\', "/").to_ascii_lowercase();
    if normalized.is_empty() {
        return false;
    }
    normalized.contains("/tests/")
        || normalized.contains("/test/")
        || normalized.contains("/__tests__/")
        || normalized.ends_with(".test.ts")
        || normalized.ends_with(".test.tsx")
        || normalized.ends_with(".test.js")
        || normalized.ends_with(".test.jsx")
        || normalized.ends_with(".spec.ts")
        || normalized.ends_with(".spec.tsx")
        || normalized.ends_with(".spec.js")
        || normalized.ends_with(".spec.jsx")
        || normalized.ends_with("_test.rs")
        || normalized.ends_with("_spec.rs")
        || normalized.ends_with("_test.py")
}

#[derive(Debug, Deserialize)]
struct StoredSemanticSummary {
    changes: String,
    auth_security: String,
    #[serde(default)]
    layer_file_changes: Vec<LocalReviewLayerFileChange>,
}

fn local_review_semantic_summary_from_row(
    row: opensession_local_db::SessionSemanticSummaryRow,
) -> Option<LocalReviewSemanticSummary> {
    let summary = serde_json::from_str::<StoredSemanticSummary>(&row.summary_json).ok()?;
    let diff_tree = row
        .diff_tree_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Vec<serde_json::Value>>(raw).ok())
        .unwrap_or_default();
    Some(LocalReviewSemanticSummary {
        changes: summary.changes,
        auth_security: summary.auth_security,
        layer_file_changes: summary.layer_file_changes,
        source_kind: row.source_kind,
        generation_kind: row.generation_kind,
        provider: row.provider,
        model: row.model,
        error: row.error,
        diff_tree,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        JobReviewQuery, JobReviewSessionData, build_job_review_bundle, validate_job_id,
        validate_review_id,
    };
    use opensession_api::{
        JobArtifactRef, JobManifest, JobProtocol, JobReviewKind, JobStage, JobStatus,
        LocalReviewSession,
    };
    use opensession_core::{Agent, Content, Event, EventType, Session};
    use opensession_local_db::LocalDb;

    fn temp_db() -> (tempfile::TempDir, LocalDb) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("local.db");
        let db = LocalDb::open_path(&path).expect("open local db");
        (dir, db)
    }

    fn make_session(
        session_id: &str,
        created_at: chrono::DateTime<chrono::Utc>,
        event_type: EventType,
        text: &str,
        source: Option<&str>,
    ) -> Session {
        let mut session = Session::new(
            session_id.to_string(),
            Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );
        session.context.created_at = created_at;
        session.context.updated_at = created_at;
        let mut event = Event {
            event_id: format!("{session_id}-event"),
            timestamp: created_at,
            event_type,
            task_id: None,
            content: Content::text(text),
            duration_ms: None,
            attributes: Default::default(),
        };
        if let Some(source) = source {
            event.attributes.insert(
                "source".to_string(),
                serde_json::Value::String(source.to_string()),
            );
        }
        session.events.push(event);
        session.recompute_stats();
        session
    }

    fn make_review_row(
        session_id: &str,
        manifest: JobManifest,
        created_at: chrono::DateTime<chrono::Utc>,
    ) -> JobReviewSessionData {
        let (event_type, text, source) = match (manifest.stage, manifest.review_kind) {
            (JobStage::Planning, _) => (EventType::UserMessage, "plan the next steps", None),
            (JobStage::Review, Some(JobReviewKind::Todo)) => (
                EventType::SystemMessage,
                "What is the plan?",
                Some("interactive_question"),
            ),
            (JobStage::Review, Some(JobReviewKind::Done)) => (
                EventType::UserMessage,
                "The job is complete.",
                Some("interactive"),
            ),
            (JobStage::Execution, _) => (
                EventType::FileEdit {
                    path: "src/auth.rs".to_string(),
                    diff: None,
                },
                "updated auth flow",
                None,
            ),
            (JobStage::Handoff, _) => (EventType::AgentMessage, "handoff prepared", None),
            (JobStage::Review, None) => unreachable!("review sessions always specify review_kind"),
        };
        let session = make_session(session_id, created_at, event_type, text, source);
        JobReviewSessionData {
            manifest,
            review_session: LocalReviewSession {
                session_id: session.session_id.clone(),
                ledger_ref: format!("os://src/local/{session_id}"),
                hail_path: format!("/tmp/{session_id}.hail.jsonl"),
                commit_shas: vec![],
                session,
            },
        }
    }

    #[test]
    fn review_id_accepts_safe_chars() {
        assert!(validate_review_id("gh-org-repo-pr1-abcdef1").is_ok());
        assert!(validate_review_id("abc.DEF_123").is_ok());
    }

    #[test]
    fn review_id_rejects_empty_or_traversal_tokens() {
        assert!(validate_review_id("").is_err());
        assert!(validate_review_id("../oops").is_err());
        assert!(validate_review_id("bad/name").is_err());
        assert!(validate_review_id("bad%20id").is_err());
    }

    #[test]
    fn job_id_rejects_empty_values() {
        assert!(validate_job_id("").is_err());
        assert!(validate_job_id("   ").is_err());
        assert!(validate_job_id("AUTH-123").is_ok());
    }

    #[test]
    fn job_review_bundle_defaults_to_latest_run_for_requested_kind() {
        let (_dir, db) = temp_db();
        let base = chrono::DateTime::parse_from_rfc3339("2026-03-10T00:00:00Z")
            .expect("base time")
            .with_timezone(&chrono::Utc);

        let sessions = vec![
            make_review_row(
                "run1-plan",
                JobManifest {
                    protocol: JobProtocol::AgentCommunicationProtocol,
                    system: "symphony".to_string(),
                    job_id: "AUTH-123".to_string(),
                    job_title: "Fix auth bug".to_string(),
                    run_id: "run-1".to_string(),
                    attempt: 1,
                    stage: JobStage::Planning,
                    review_kind: None,
                    status: JobStatus::Pending,
                    thread_id: None,
                    artifacts: vec![],
                },
                base,
            ),
            make_review_row(
                "run1-todo",
                JobManifest {
                    protocol: JobProtocol::AgentCommunicationProtocol,
                    system: "symphony".to_string(),
                    job_id: "AUTH-123".to_string(),
                    job_title: "Fix auth bug".to_string(),
                    run_id: "run-1".to_string(),
                    attempt: 1,
                    stage: JobStage::Review,
                    review_kind: Some(JobReviewKind::Todo),
                    status: JobStatus::Pending,
                    thread_id: None,
                    artifacts: vec![],
                },
                base + chrono::Duration::minutes(1),
            ),
            make_review_row(
                "run2-plan",
                JobManifest {
                    protocol: JobProtocol::AgentCommunicationProtocol,
                    system: "symphony".to_string(),
                    job_id: "AUTH-123".to_string(),
                    job_title: "Fix auth bug".to_string(),
                    run_id: "run-2".to_string(),
                    attempt: 2,
                    stage: JobStage::Planning,
                    review_kind: None,
                    status: JobStatus::InProgress,
                    thread_id: None,
                    artifacts: vec![],
                },
                base + chrono::Duration::hours(1),
            ),
            make_review_row(
                "run2-todo",
                JobManifest {
                    protocol: JobProtocol::AgentCommunicationProtocol,
                    system: "symphony".to_string(),
                    job_id: "AUTH-123".to_string(),
                    job_title: "Fix auth bug".to_string(),
                    run_id: "run-2".to_string(),
                    attempt: 2,
                    stage: JobStage::Review,
                    review_kind: Some(JobReviewKind::Todo),
                    status: JobStatus::InProgress,
                    thread_id: None,
                    artifacts: vec![],
                },
                base + chrono::Duration::hours(1) + chrono::Duration::minutes(1),
            ),
        ];

        let bundle = build_job_review_bundle(
            &db,
            sessions,
            JobReviewQuery {
                kind: JobReviewKind::Todo,
                run_id: None,
            },
        )
        .expect("build bundle");

        assert_eq!(bundle.selected_review.run_id, "run-2");
        assert_eq!(bundle.runs[0].run_id, "run-2");
        assert_eq!(bundle.runs[0].sessions.len(), 2);
        assert_eq!(bundle.runs[1].run_id, "run-1");
    }

    #[test]
    fn done_review_bundle_filters_selected_run_and_handoff_artifact() {
        let (_dir, db) = temp_db();
        let base = chrono::DateTime::parse_from_rfc3339("2026-03-10T10:00:00Z")
            .expect("base time")
            .with_timezone(&chrono::Utc);
        let handoff_uri = "file:///tmp/handoff.md".to_string();

        let sessions = vec![
            make_review_row(
                "plan",
                JobManifest {
                    protocol: JobProtocol::AgentCommunicationProtocol,
                    system: "symphony".to_string(),
                    job_id: "AUTH-456".to_string(),
                    job_title: "Ship the feature".to_string(),
                    run_id: "run-3".to_string(),
                    attempt: 3,
                    stage: JobStage::Planning,
                    review_kind: None,
                    status: JobStatus::Completed,
                    thread_id: None,
                    artifacts: vec![],
                },
                base,
            ),
            make_review_row(
                "todo",
                JobManifest {
                    protocol: JobProtocol::AgentCommunicationProtocol,
                    system: "symphony".to_string(),
                    job_id: "AUTH-456".to_string(),
                    job_title: "Ship the feature".to_string(),
                    run_id: "run-3".to_string(),
                    attempt: 3,
                    stage: JobStage::Review,
                    review_kind: Some(JobReviewKind::Todo),
                    status: JobStatus::Completed,
                    thread_id: None,
                    artifacts: vec![],
                },
                base + chrono::Duration::minutes(1),
            ),
            make_review_row(
                "exec",
                JobManifest {
                    protocol: JobProtocol::AgentCommunicationProtocol,
                    system: "symphony".to_string(),
                    job_id: "AUTH-456".to_string(),
                    job_title: "Ship the feature".to_string(),
                    run_id: "run-3".to_string(),
                    attempt: 3,
                    stage: JobStage::Execution,
                    review_kind: None,
                    status: JobStatus::Completed,
                    thread_id: None,
                    artifacts: vec![],
                },
                base + chrono::Duration::minutes(2),
            ),
            make_review_row(
                "done",
                JobManifest {
                    protocol: JobProtocol::AgentCommunicationProtocol,
                    system: "symphony".to_string(),
                    job_id: "AUTH-456".to_string(),
                    job_title: "Ship the feature".to_string(),
                    run_id: "run-3".to_string(),
                    attempt: 3,
                    stage: JobStage::Review,
                    review_kind: Some(JobReviewKind::Done),
                    status: JobStatus::Completed,
                    thread_id: None,
                    artifacts: vec![],
                },
                base + chrono::Duration::minutes(3),
            ),
            make_review_row(
                "handoff",
                JobManifest {
                    protocol: JobProtocol::AgentCommunicationProtocol,
                    system: "symphony".to_string(),
                    job_id: "AUTH-456".to_string(),
                    job_title: "Ship the feature".to_string(),
                    run_id: "run-3".to_string(),
                    attempt: 3,
                    stage: JobStage::Handoff,
                    review_kind: None,
                    status: JobStatus::Completed,
                    thread_id: None,
                    artifacts: vec![JobArtifactRef {
                        kind: "handoff".to_string(),
                        label: "Handoff notes".to_string(),
                        uri: handoff_uri.clone(),
                        mime_type: Some("text/markdown".to_string()),
                        metadata: None,
                    }],
                },
                base + chrono::Duration::minutes(4),
            ),
        ];

        let bundle = build_job_review_bundle(
            &db,
            sessions,
            JobReviewQuery {
                kind: JobReviewKind::Done,
                run_id: Some("run-3".to_string()),
            },
        )
        .expect("build bundle");

        let selected_run = bundle
            .runs
            .iter()
            .find(|run| run.run_id == "run-3")
            .expect("selected run");
        let session_ids = selected_run
            .sessions
            .iter()
            .map(|session| session.session_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(session_ids, vec!["todo", "exec", "done", "handoff"]);
        assert_eq!(
            bundle.handoff_artifact_uri.as_deref(),
            Some(handoff_uri.as_str())
        );
        assert_eq!(
            bundle.review_digest.modified_files,
            vec!["src/auth.rs".to_string()]
        );
    }
}
