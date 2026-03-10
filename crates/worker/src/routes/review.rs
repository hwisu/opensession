use opensession_api::{
    JobManifest, JobReviewBundle, JobReviewBundleJob, JobReviewKind, JobReviewRun,
    JobReviewSelectedReview, JobStage, LocalReviewReviewerDigest, LocalReviewReviewerQa,
    LocalReviewSession, ServiceError, job_manifest_from_session,
};
use opensession_core::{ContentBlock, EventType, Session};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use worker::*;

use crate::error::IntoErrResponse;
use crate::storage;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct JobReviewQuery {
    pub kind: JobReviewKind,
    #[serde(default)]
    pub run_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct JobSessionRow {
    git_commit: Option<String>,
    body_storage_key: String,
    body_url: Option<String>,
}

#[derive(Clone)]
struct JobReviewSessionData {
    manifest: JobManifest,
    review_session: LocalReviewSession,
}

pub async fn get_job_review_bundle(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let job_id = ctx
        .param("job_id")
        .ok_or_else(|| Error::from("Missing job_id"))?;
    let job_id = job_id.as_str();
    let query = parse_job_review_query(&req)?;
    validate_job_id(job_id)?;

    let d1 = storage::get_d1(&ctx.env)?;
    let sql = r#"
SELECT git_commit, body_storage_key, body_url
FROM sessions
WHERE job_id = ?1
ORDER BY COALESCE(job_attempt, 0) DESC, created_at DESC
"#;

    let rows = d1
        .prepare(sql)
        .bind(&[worker::wasm_bindgen::JsValue::from_str(job_id)])?
        .all()
        .await?
        .results::<JobSessionRow>()?;

    if rows.is_empty() {
        return ServiceError::NotFound("job review bundle not found".into()).into_err_response();
    }

    let mut sessions = Vec::new();
    for row in rows {
        let body = fetch_session_body(&ctx.env, &row).await?;
        let text = String::from_utf8(body)
            .map_err(|_| Error::RustError("invalid session utf-8".to_string()))?;
        let session = Session::from_jsonl(&text)
            .map_err(|_| Error::RustError("invalid session body".to_string()))?;
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
                ledger_ref: row
                    .body_url
                    .clone()
                    .unwrap_or_else(|| format!("r2://{}", row.body_storage_key)),
                hail_path: row
                    .body_url
                    .clone()
                    .unwrap_or_else(|| row.body_storage_key.clone()),
                commit_shas: row.git_commit.into_iter().collect(),
                session,
            },
        });
    }

    if sessions.is_empty() {
        return ServiceError::NotFound("job review bundle not found".into()).into_err_response();
    }

    Response::from_json(
        &build_job_review_bundle(sessions, query)
            .map_err(|err| Error::RustError(format!("build worker job review bundle: {err}")))?,
    )
}

fn parse_job_review_query(req: &Request) -> Result<JobReviewQuery> {
    let url = req.url()?;
    let mut kind = None;
    let mut run_id = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "kind" => {
                kind = serde_json::from_value::<JobReviewKind>(serde_json::Value::String(
                    value.to_string(),
                ))
                .ok()
            }
            "run_id" => run_id = Some(value.to_string()),
            _ => {}
        }
    }

    let Some(kind) = kind else {
        return Err(Error::RustError("missing review kind".to_string()));
    };
    Ok(JobReviewQuery { kind, run_id })
}

async fn fetch_session_body(env: &Env, row: &JobSessionRow) -> Result<Vec<u8>> {
    if let Some(url) = row
        .body_url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
    {
        let mut init = RequestInit::new();
        init.with_method(Method::Get);
        let request = Request::new_with_init(url, &init)?;
        let mut response = Fetch::Request(request).send().await?;
        let status = response.status_code();
        if !(200..=299).contains(&status) {
            return Err(Error::RustError(format!(
                "body_url fetch failed with status {}",
                status
            )));
        }
        return response.bytes().await;
    }

    storage::get_session_body(env, &row.body_storage_key)
        .await?
        .ok_or_else(|| Error::RustError("session body not found".to_string()))
}

pub(crate) fn validate_job_id(job_id: &str) -> Result<()> {
    if job_id.trim().is_empty() {
        return Err(Error::RustError("job_id is required".to_string()));
    }
    Ok(())
}

fn build_job_review_bundle(
    sessions: Vec<JobReviewSessionData>,
    query: JobReviewQuery,
) -> std::result::Result<JobReviewBundle, String> {
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
            .ok_or_else(|| "requested run_id is not available".to_string())?
    } else {
        ordered_runs
            .iter()
            .position(|run| {
                run.iter()
                    .any(|row| matches_review(&row.manifest, query.kind))
            })
            .ok_or_else(|| "requested review kind is not available".to_string())?
    };

    let selected_run = ordered_runs
        .get(selected_run_index)
        .cloned()
        .ok_or_else(|| "selected run is missing".to_string())?;
    let selected_review = selected_run
        .iter()
        .filter(|row| matches_review(&row.manifest, query.kind))
        .max_by_key(|row| row.review_session.session.context.created_at)
        .ok_or_else(|| "selected run does not contain requested review kind".to_string())?;

    let mut bundle_runs = Vec::new();
    for run in ordered_runs.iter().cloned() {
        let filtered = filter_run_sessions(run, query.kind);
        if filtered.is_empty() {
            continue;
        }
        let run_manifest = filtered
            .last()
            .map(|row| &row.manifest)
            .ok_or_else(|| "run manifest missing".to_string())?;
        bundle_runs.push(JobReviewRun {
            run_id: run_manifest.run_id.clone(),
            attempt: run_manifest.attempt,
            status: run_manifest.status,
            sessions: filtered
                .iter()
                .map(|row| row.review_session.clone())
                .collect(),
            artifacts: dedupe_artifacts(&filtered),
        });
    }

    let selected_filtered = filter_run_sessions(selected_run.clone(), query.kind);
    let artifacts = dedupe_artifacts(&selected_filtered);
    let handoff_artifact_uri = artifacts
        .iter()
        .find(|artifact| artifact.kind.eq_ignore_ascii_case("handoff"))
        .map(|artifact| artifact.uri.clone());

    Ok(JobReviewBundle {
        job: JobReviewBundleJob {
            protocol: selected_review.manifest.protocol,
            system: selected_review.manifest.system.clone(),
            job_id: selected_review.manifest.job_id.clone(),
            job_title: selected_review.manifest.job_title.clone(),
        },
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
        review_digest: build_reviewer_digest_for_sessions(
            &selected_filtered
                .iter()
                .map(|row| row.review_session.clone())
                .collect::<Vec<_>>(),
        ),
        semantic_summary: None,
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
    manifest.stage == JobStage::Review && manifest.review_kind == Some(kind)
}

fn include_session_for_kind(manifest: &JobManifest, kind: JobReviewKind) -> bool {
    match kind {
        JobReviewKind::Todo => {
            manifest.stage == JobStage::Planning || matches_review(manifest, JobReviewKind::Todo)
        }
        JobReviewKind::Done => {
            matches_review(manifest, JobReviewKind::Todo)
                || manifest.stage == JobStage::Execution
                || matches_review(manifest, JobReviewKind::Done)
                || manifest.stage == JobStage::Handoff
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

#[cfg(test)]
mod tests {
    use super::{JobReviewQuery, JobReviewSessionData, build_job_review_bundle, validate_job_id};
    use opensession_api::{
        JobArtifactRef, JobManifest, JobProtocol, JobReviewKind, JobStage, JobStatus,
        LocalReviewSession,
    };
    use opensession_core::{Agent, Content, Event, EventType, Session};

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
            (JobStage::Review, None) => unreachable!(),
        };
        let session = make_session(session_id, created_at, event_type, text, source);
        JobReviewSessionData {
            manifest,
            review_session: LocalReviewSession {
                session_id: session.session_id.clone(),
                ledger_ref: format!("r2://{session_id}"),
                hail_path: format!("session/{session_id}.jsonl"),
                commit_shas: vec![],
                session,
            },
        }
    }

    #[test]
    fn job_id_rejects_empty_values() {
        assert!(validate_job_id("").is_err());
        assert!(validate_job_id("   ").is_err());
        assert!(validate_job_id("AUTH-123").is_ok());
    }

    #[test]
    fn todo_review_defaults_to_latest_run() {
        let base = chrono::DateTime::parse_from_rfc3339("2026-03-10T00:00:00Z")
            .expect("base time")
            .with_timezone(&chrono::Utc);

        let bundle = build_job_review_bundle(
            vec![
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
                    base + chrono::Duration::hours(1),
                ),
            ],
            JobReviewQuery {
                kind: JobReviewKind::Todo,
                run_id: None,
            },
        )
        .expect("build bundle");

        assert_eq!(bundle.selected_review.run_id, "run-2");
        assert_eq!(bundle.runs[0].run_id, "run-2");
    }

    #[test]
    fn done_review_includes_handoff_artifact() {
        let base = chrono::DateTime::parse_from_rfc3339("2026-03-10T10:00:00Z")
            .expect("base time")
            .with_timezone(&chrono::Utc);
        let handoff_uri = "https://example.com/handoff.md".to_string();

        let bundle = build_job_review_bundle(
            vec![
                make_review_row(
                    "todo",
                    JobManifest {
                        protocol: JobProtocol::AgentCommunicationProtocol,
                        system: "symphony".to_string(),
                        job_id: "AUTH-456".to_string(),
                        job_title: "Ship feature".to_string(),
                        run_id: "run-3".to_string(),
                        attempt: 3,
                        stage: JobStage::Review,
                        review_kind: Some(JobReviewKind::Todo),
                        status: JobStatus::Completed,
                        thread_id: None,
                        artifacts: vec![],
                    },
                    base,
                ),
                make_review_row(
                    "exec",
                    JobManifest {
                        protocol: JobProtocol::AgentCommunicationProtocol,
                        system: "symphony".to_string(),
                        job_id: "AUTH-456".to_string(),
                        job_title: "Ship feature".to_string(),
                        run_id: "run-3".to_string(),
                        attempt: 3,
                        stage: JobStage::Execution,
                        review_kind: None,
                        status: JobStatus::Completed,
                        thread_id: None,
                        artifacts: vec![],
                    },
                    base + chrono::Duration::minutes(1),
                ),
                make_review_row(
                    "done",
                    JobManifest {
                        protocol: JobProtocol::AgentCommunicationProtocol,
                        system: "symphony".to_string(),
                        job_id: "AUTH-456".to_string(),
                        job_title: "Ship feature".to_string(),
                        run_id: "run-3".to_string(),
                        attempt: 3,
                        stage: JobStage::Review,
                        review_kind: Some(JobReviewKind::Done),
                        status: JobStatus::Completed,
                        thread_id: None,
                        artifacts: vec![],
                    },
                    base + chrono::Duration::minutes(2),
                ),
                make_review_row(
                    "handoff",
                    JobManifest {
                        protocol: JobProtocol::AgentCommunicationProtocol,
                        system: "symphony".to_string(),
                        job_id: "AUTH-456".to_string(),
                        job_title: "Ship feature".to_string(),
                        run_id: "run-3".to_string(),
                        attempt: 3,
                        stage: JobStage::Handoff,
                        review_kind: None,
                        status: JobStatus::Completed,
                        thread_id: None,
                        artifacts: vec![JobArtifactRef {
                            kind: "handoff".to_string(),
                            label: "handoff".to_string(),
                            uri: handoff_uri.clone(),
                            mime_type: Some("text/markdown".to_string()),
                            metadata: None,
                        }],
                    },
                    base + chrono::Duration::minutes(3),
                ),
            ],
            JobReviewQuery {
                kind: JobReviewKind::Done,
                run_id: None,
            },
        )
        .expect("build bundle");

        assert_eq!(
            bundle.handoff_artifact_uri.as_deref(),
            Some(handoff_uri.as_str())
        );
        assert_eq!(bundle.runs[0].sessions.len(), 4);
    }
}
