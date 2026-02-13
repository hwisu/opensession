use worker::*;

use opensession_api::db;
use opensession_api::{
    saturating_i64, LinkType, ServiceError, SessionDetail, SessionLink, SessionListQuery,
    SessionListResponse, SessionSummary, UploadResponse,
};

use crate::db_helpers::values_to_js;
use crate::error::IntoErrResponse;
use crate::storage;

impl From<storage::SessionRow> for SessionSummary {
    fn from(s: storage::SessionRow) -> Self {
        Self {
            id: s.id,
            user_id: s.user_id,
            nickname: s.nickname,
            team_id: s.team_id,
            tool: s.tool,
            agent_provider: s.agent_provider,
            agent_model: s.agent_model,
            title: s.title,
            description: s.description,
            tags: s.tags,
            created_at: s.created_at,
            uploaded_at: s.uploaded_at,
            message_count: s.message_count,
            task_count: s.task_count,
            event_count: s.event_count,
            duration_seconds: s.duration_seconds,
            total_input_tokens: s.total_input_tokens,
            total_output_tokens: s.total_output_tokens,
            git_remote: s.git_remote,
            git_branch: s.git_branch,
            git_commit: s.git_commit,
            git_repo_name: s.git_repo_name,
            pr_number: s.pr_number,
            pr_url: s.pr_url,
            working_directory: s.working_directory,
            files_modified: s.files_modified,
            files_read: s.files_read,
            has_errors: s.has_errors,
        }
    }
}

/// POST /api/sessions — upload a new session
pub async fn create(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user = match storage::require_auth(&req, &ctx.env).await {
        Ok(u) => u,
        Err(resp) => return Ok(resp),
    };

    let body: serde_json::Value = req.json().await?;

    let session_value = body
        .get("session")
        .ok_or_else(|| Error::from("missing 'session' field"))?;

    // team_id is optional: absent or null → "personal"
    let team_id = body
        .get("team_id")
        .and_then(|v| v.as_str())
        .unwrap_or("personal")
        .to_string();

    // body_url: external storage link (git repo, etc.)
    let body_url = body
        .get("body_url")
        .and_then(|v| v.as_str())
        .map(String::from);

    // Git context from upload request
    let git_remote = body
        .get("git_remote")
        .and_then(|v| v.as_str())
        .map(String::from);
    let git_branch = body
        .get("git_branch")
        .and_then(|v| v.as_str())
        .map(String::from);
    let git_commit = body
        .get("git_commit")
        .and_then(|v| v.as_str())
        .map(String::from);
    let git_repo_name = body
        .get("git_repo_name")
        .and_then(|v| v.as_str())
        .map(String::from);
    let pr_number = body.get("pr_number").and_then(|v| v.as_i64());
    let pr_url = body
        .get("pr_url")
        .and_then(|v| v.as_str())
        .map(String::from);

    let d1 = storage::get_d1(&ctx.env)?;

    // Verify team membership (skip for personal sessions)
    if team_id != "personal" {
        let (sql, values) = db::teams::member_exists(&team_id, &user.id);
        let member_check = d1
            .prepare(&sql)
            .bind(&values_to_js(&values))?
            .first::<storage::CountRow>(None)
            .await?;

        if member_check.map(|r| r.count).unwrap_or(0) == 0 {
            return ServiceError::Forbidden("not a member of this team".into()).into_err_response();
        }
    }

    // Parse the session using opensession-core
    let session: opensession_core::Session = serde_json::from_value(session_value.clone())
        .map_err(|e| Error::from(format!("Invalid session: {e}")))?;

    // Serialize body to HAIL JSONL
    let body_jsonl = session
        .to_jsonl()
        .map_err(|e| Error::from(format!("Failed to serialize session: {e}")))?;

    let session_id = uuid::Uuid::new_v4().to_string();
    let storage_key = format!("{session_id}.hail.jsonl");

    // Store full body in R2 only if no external body_url is provided
    if body_url.is_none() {
        storage::put_session_body(&ctx.env, &storage_key, body_jsonl.as_bytes()).await?;
    }

    let meta = opensession_core::extract::extract_upload_metadata(&session);

    // If body_url is provided, use empty storage key (body is external)
    let effective_storage_key = if body_url.is_some() {
        String::new()
    } else {
        storage_key
    };

    // Store metadata in D1 using sea-query INSERT
    let (sql, values) = db::sessions::insert(&db::sessions::InsertParams {
        id: &session_id,
        user_id: &user.id,
        team_id: &team_id,
        tool: &session.agent.tool,
        agent_provider: &session.agent.provider,
        agent_model: &session.agent.model,
        title: meta.title.as_deref().unwrap_or(""),
        description: meta.description.as_deref().unwrap_or(""),
        tags: meta.tags.as_deref().unwrap_or(""),
        created_at: &meta.created_at,
        message_count: saturating_i64(session.stats.message_count),
        task_count: saturating_i64(session.stats.task_count),
        event_count: saturating_i64(session.stats.event_count),
        duration_seconds: saturating_i64(session.stats.duration_seconds),
        total_input_tokens: saturating_i64(session.stats.total_input_tokens),
        total_output_tokens: saturating_i64(session.stats.total_output_tokens),
        body_storage_key: &effective_storage_key,
        body_url: body_url.as_deref(),
        git_remote: git_remote.as_deref(),
        git_branch: git_branch.as_deref(),
        git_commit: git_commit.as_deref(),
        git_repo_name: git_repo_name.as_deref(),
        pr_number,
        pr_url: pr_url.as_deref(),
        working_directory: meta.working_directory.as_deref(),
        files_modified: meta.files_modified.as_deref(),
        files_read: meta.files_read.as_deref(),
        has_errors: meta.has_errors,
    });
    d1.prepare(&sql).bind(&values_to_js(&values))?.run().await?;

    // Insert session links: prefer explicit linked_session_ids, fall back to context.related_session_ids
    let explicit_links: Vec<String> = body
        .get("linked_session_ids")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let mut linked_ids: std::collections::HashSet<String> = explicit_links.into_iter().collect();
    for id in &session.context.related_session_ids {
        linked_ids.insert(id.clone());
    }

    for linked_id in &linked_ids {
        let (sql, values) = db::sessions::insert_link(&session_id, linked_id, LinkType::Handoff);
        let _ = d1.prepare(&sql).bind(&values_to_js(&values))?.run().await;
    }

    let base_url = ctx
        .env
        .var("BASE_URL")
        .map(|v| v.to_string())
        .unwrap_or_else(|_| "https://opensession.io".to_string());
    let url = format!("{base_url}/session/{session_id}");

    let mut resp = Response::from_json(&UploadResponse {
        id: session_id,
        url,
    })?;
    resp = resp.with_status(201);
    Ok(resp)
}

/// GET /api/sessions — list sessions (public, paginated, filtered)
pub async fn list(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let url = req.url()?;
    let query_pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();

    let q = SessionListQuery {
        page: query_pairs
            .iter()
            .find(|(k, _)| k == "page")
            .and_then(|(_, v)| v.parse().ok())
            .unwrap_or(1),
        per_page: query_pairs
            .iter()
            .find(|(k, _)| k == "per_page")
            .and_then(|(_, v)| v.parse().ok())
            .unwrap_or(20),
        search: query_pairs
            .iter()
            .find(|(k, _)| k == "search")
            .map(|(_, v)| v.clone()),
        tool: query_pairs
            .iter()
            .find(|(k, _)| k == "tool")
            .map(|(_, v)| v.clone()),
        team_id: query_pairs
            .iter()
            .find(|(k, _)| k == "team_id")
            .map(|(_, v)| v.clone()),
        sort: query_pairs
            .iter()
            .find(|(k, _)| k == "sort")
            .and_then(|(_, v)| serde_json::from_value(serde_json::Value::String(v.clone())).ok()),
        time_range: query_pairs
            .iter()
            .find(|(k, _)| k == "time_range")
            .and_then(|(_, v)| serde_json::from_value(serde_json::Value::String(v.clone())).ok()),
    };

    let built = db::sessions::list(&q);
    let d1 = storage::get_d1(&ctx.env)?;

    // Count total
    let count_params = values_to_js(&built.count_query.1);
    let count_result = d1
        .prepare(&built.count_query.0)
        .bind(&count_params)?
        .first::<storage::CountRow>(None)
        .await?;
    let total = count_result.map(|r| r.count).unwrap_or(0);

    // Fetch page
    let select_params = values_to_js(&built.select_query.1);
    let rows_result = d1
        .prepare(&built.select_query.0)
        .bind(&select_params)?
        .all()
        .await?;

    let sessions: Vec<SessionSummary> = rows_result
        .results::<storage::SessionRow>()?
        .into_iter()
        .map(SessionSummary::from)
        .collect();

    Response::from_json(&SessionListResponse {
        sessions,
        total,
        page: built.page,
        per_page: built.per_page,
    })
}

/// GET /api/sessions/:id — get session detail
pub async fn get(_req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let id = ctx.param("id").ok_or_else(|| Error::from("Missing id"))?;

    let d1 = storage::get_d1(&ctx.env)?;

    let (sql, values) = db::sessions::get_by_id(id);
    let row = d1
        .prepare(&sql)
        .bind(&values_to_js(&values))?
        .first::<storage::SessionRow>(None)
        .await?;

    match row {
        Some(s) => {
            let summary = SessionSummary::from(s);

            // Get team name
            let (sql, values) = db::teams::get_name(&summary.team_id);
            let team_name = d1
                .prepare(&sql)
                .bind(&values_to_js(&values))?
                .first::<storage::TeamNameRow>(None)
                .await?
                .map(|r| r.name);

            // Fetch linked sessions
            let (sql, values) = db::sessions::links_by_session(id);
            let linked_sessions: Vec<SessionLink> = d1
                .prepare(&sql)
                .bind(&values_to_js(&values))?
                .all()
                .await?
                .results::<SessionLink>()
                .unwrap_or_default();

            Response::from_json(&SessionDetail {
                summary,
                team_name,
                linked_sessions,
            })
        }
        None => ServiceError::NotFound("session not found".into()).into_err_response(),
    }
}

/// GET /api/sessions/:id/raw — get the full HAIL JSONL from R2 or redirect to body_url
pub async fn get_raw(_req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let id = ctx.param("id").ok_or_else(|| Error::from("Missing id"))?;

    let d1 = storage::get_d1(&ctx.env)?;

    // Get storage key and body_url from D1
    let (sql, values) = db::sessions::get_storage_info(id);
    let row = d1
        .prepare(&sql)
        .bind(&values_to_js(&values))?
        .first::<storage::StorageInfoRow>(None)
        .await?;

    let row = match row {
        Some(r) => r,
        None => {
            return ServiceError::NotFound("session not found".into()).into_err_response();
        }
    };

    // If body_url is set, redirect to external storage
    if let Some(ref url) = row.body_url {
        if !url.is_empty() {
            let headers = Headers::new();
            headers.set("Location", url)?;
            return Ok(Response::empty()?.with_status(302).with_headers(headers));
        }
    }

    // Fetch from R2
    match storage::get_session_body(&ctx.env, &row.body_storage_key).await? {
        Some(bytes) => {
            let headers = Headers::new();
            headers.set("Content-Type", "application/jsonl")?;
            headers.set(
                "Content-Disposition",
                "attachment; filename=\"session.hail.jsonl\"",
            )?;
            Ok(Response::from_bytes(bytes)?.with_headers(headers))
        }
        None => ServiceError::NotFound("session body not found".into()).into_err_response(),
    }
}
