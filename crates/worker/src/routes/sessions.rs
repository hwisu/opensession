use worker::*;

use std::collections::HashMap;

use opensession_api::db;
use opensession_api::{
    saturating_i64, LinkType, ServiceError, SessionDetail, SessionLink, SessionListQuery,
    SessionListResponse, SessionSummary, UploadRequest, UploadResponse,
};
use opensession_core::extract;
use opensession_core::scoring::SessionScoreRegistry;
use uuid::Uuid;

use crate::config::WorkerConfig;
use crate::db_helpers::values_to_js;
use crate::error::IntoErrResponse;
use crate::routes::auth;
use crate::storage;

const PUBLIC_LIST_CACHE_CONTROL: &str = "public, max-age=30";

fn parse_query_enum<T: serde::de::DeserializeOwned>(
    params: &HashMap<&str, &str>,
    key: &str,
) -> Option<T> {
    params
        .get(key)
        .and_then(|v| serde_json::from_value(serde_json::Value::String((*v).to_string())).ok())
}

impl From<storage::SessionRow> for SessionSummary {
    fn from(s: storage::SessionRow) -> Self {
        Self {
            id: s.id,
            user_id: s.user_id,
            nickname: s.nickname,
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
            max_active_agents: s.max_active_agents,
            session_score: s.session_score,
            score_plugin: s.score_plugin,
        }
    }
}

fn parse_session_list_query(query_pairs: &[(String, String)]) -> SessionListQuery {
    let params: HashMap<&str, &str> = query_pairs
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    SessionListQuery {
        page: params.get("page").and_then(|v| v.parse().ok()).unwrap_or(1),
        per_page: params
            .get("per_page")
            .and_then(|v| v.parse().ok())
            .unwrap_or(20),
        search: params.get("search").map(|v| (*v).to_string()),
        tool: params.get("tool").map(|v| (*v).to_string()),
        sort: parse_query_enum(&params, "sort"),
        time_range: parse_query_enum(&params, "time_range"),
    }
}

fn public_feed_cache_key(query_pairs: &[(String, String)]) -> String {
    let mut sorted = query_pairs.to_vec();
    sorted.sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    if sorted.is_empty() {
        return "https://cache.opensession.io/api/sessions".to_string();
    }

    let mut encoded = String::new();
    for (idx, (k, v)) in sorted.iter().enumerate() {
        if idx > 0 {
            encoded.push('&');
        }
        encoded.push_str(&urlencoding::encode(k));
        encoded.push('=');
        encoded.push_str(&urlencoding::encode(v));
    }
    format!("https://cache.opensession.io/api/sessions?{encoded}")
}

fn worker_public_feed_enabled() -> bool {
    // Worker deployment is public-feed-first by product policy.
    true
}

fn service_internal(context: &str, err: impl std::fmt::Display) -> ServiceError {
    ServiceError::Internal(format!("{context}: {err}"))
}

async fn d1_run(
    d1: &D1Database,
    built: (String, sea_query::Values),
    context: &str,
) -> std::result::Result<(), ServiceError> {
    let (sql, values) = built;
    let stmt = d1
        .prepare(&sql)
        .bind(&values_to_js(&values))
        .map_err(|e| service_internal(context, e))?;
    let result = stmt.run().await.map_err(|e| service_internal(context, e))?;
    if !result.success() {
        return Err(ServiceError::Internal(
            result
                .error()
                .unwrap_or_else(|| format!("{context} failed")),
        ));
    }
    Ok(())
}

fn resolve_base_url(req: &Request, config: &WorkerConfig) -> String {
    if let Some(base_url) = config.base_url.as_ref() {
        return base_url.trim_end_matches('/').to_string();
    }

    if let Ok(url) = req.url() {
        let mut base = format!("{}://{}", url.scheme(), url.host_str().unwrap_or_default());
        if let Some(port) = url.port() {
            base.push(':');
            base.push_str(&port.to_string());
        }
        return base;
    }

    "https://opensession.io".to_string()
}

/// POST /api/sessions — upload a new session (authenticated users only).
pub async fn upload(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let d1 = match storage::get_d1(&ctx.env) {
        Ok(db) => db,
        Err(err) => return service_internal("load d1 binding", err).into_err_response(),
    };
    let config = WorkerConfig::from_env(&ctx.env);
    let user = match auth::authenticate(&req, &d1, &config).await {
        Ok(user) => user,
        Err(err) => return err.into_err_response(),
    };

    let upload_req: UploadRequest = match req.json().await {
        Ok(parsed) => parsed,
        Err(_) => {
            return ServiceError::BadRequest("invalid request body".into()).into_err_response()
        }
    };
    let session = &upload_req.session;

    let body_jsonl = match session.to_jsonl() {
        Ok(body) => body,
        Err(err) => return service_internal("serialize session body", err).into_err_response(),
    };
    let session_id = Uuid::new_v4().to_string();

    let external_body_url = upload_req
        .body_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let (body_storage_key, body_url): (String, Option<&str>) = if let Some(url) =
        external_body_url.as_deref()
    {
        (String::new(), Some(url))
    } else {
        let key = format!("sessions/{session_id}.jsonl");
        if let Err(err) = storage::put_session_body(&ctx.env, &key, body_jsonl.as_bytes()).await {
            return service_internal("store session body in r2", err).into_err_response();
        }
        (key, None)
    };

    let meta = extract::extract_upload_metadata(session);
    let score_registry = SessionScoreRegistry::default();
    let requested_score_plugin = upload_req
        .score_plugin
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let env_score_plugin = std::env::var(opensession_api::deploy::ENV_SESSION_SCORE_PLUGIN)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let selected_score_plugin = requested_score_plugin
        .or(env_score_plugin.as_deref())
        .unwrap_or(opensession_core::scoring::DEFAULT_SCORE_PLUGIN);
    let session_score = match score_registry.score_with(selected_score_plugin, session) {
        Ok(result) => result,
        Err(err) => {
            if requested_score_plugin.is_some() {
                return ServiceError::BadRequest(err.to_string()).into_err_response();
            }
            let fallback = match score_registry.score_default(session) {
                Ok(result) => result,
                Err(fallback_err) => {
                    return service_internal("compute session score", fallback_err)
                        .into_err_response();
                }
            };
            console_log!(
                "score plugin '{}' unavailable; fallback to '{}': {}",
                selected_score_plugin,
                fallback.plugin,
                err
            );
            fallback
        }
    };

    if let Err(err) = d1_run(
        &d1,
        db::sessions::insert(&db::sessions::InsertParams {
            id: &session_id,
            user_id: &user.user_id,
            team_id: "local",
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
            body_storage_key: &body_storage_key,
            body_url,
            git_remote: upload_req.git_remote.as_deref(),
            git_branch: upload_req.git_branch.as_deref(),
            git_commit: upload_req.git_commit.as_deref(),
            git_repo_name: upload_req.git_repo_name.as_deref(),
            pr_number: upload_req.pr_number,
            pr_url: upload_req.pr_url.as_deref(),
            working_directory: meta.working_directory.as_deref(),
            files_modified: meta.files_modified.as_deref(),
            files_read: meta.files_read.as_deref(),
            has_errors: meta.has_errors,
            max_active_agents: saturating_i64(opensession_core::agent_metrics::max_active_agents(
                session,
            ) as u64),
            session_score: session_score.score,
            score_plugin: &session_score.plugin,
        }),
        "insert uploaded session",
    )
    .await
    {
        return err.into_err_response();
    }

    let linked_ids: Vec<String> = upload_req
        .linked_session_ids
        .unwrap_or_default()
        .into_iter()
        .chain(session.context.related_session_ids.iter().cloned())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    for linked_id in &linked_ids {
        if let Err(err) = d1_run(
            &d1,
            db::sessions::insert_link(&session_id, linked_id, LinkType::Handoff),
            "insert session link",
        )
        .await
        {
            console_log!("failed to insert session link: {err}");
        }
    }

    let base_url = resolve_base_url(&req, &config);
    let url = format!("{base_url}/session/{session_id}");
    let payload = UploadResponse {
        id: session_id,
        url,
        session_score: session_score.score,
        score_plugin: session_score.plugin,
    };
    Response::from_json(&payload).map(|resp| resp.with_status(201))
}

/// GET /api/sessions — list sessions (public, paginated, filtered)
pub async fn list(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let url = req.url()?;
    let query_pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();
    let q = parse_session_list_query(&query_pairs);
    let public_feed_enabled = worker_public_feed_enabled();
    let has_auth_header = req.headers().get("Authorization").ok().flatten().is_some();
    let has_session_cookie = req
        .headers()
        .get("Cookie")
        .ok()
        .flatten()
        .is_some_and(|cookie| cookie.contains("session="));
    let cacheable =
        public_feed_enabled && q.is_public_feed_cacheable(has_auth_header, has_session_cookie);
    let cache_key = cacheable.then(|| public_feed_cache_key(&query_pairs));

    if let Some(key) = cache_key.as_deref() {
        if let Ok(Some(mut cached)) = Cache::default().get(key, false).await {
            let _ = cached.headers_mut().set("X-OpenSession-Cache", "HIT");
            return Ok(cached);
        }
    }

    let built = db::sessions::list(&q);
    let d1 = storage::get_d1(&ctx.env)?;

    let count_params = values_to_js(&built.count_query.1);
    let count_stmt = d1.prepare(&built.count_query.0).bind(&count_params)?;

    let select_params = values_to_js(&built.select_query.1);
    let select_stmt = d1.prepare(&built.select_query.0).bind(&select_params)?;

    // Run count + select in one D1 round-trip.
    let batch = d1.batch(vec![count_stmt, select_stmt]).await?;
    let count_result = batch
        .first()
        .ok_or_else(|| Error::from("missing count result in D1 batch"))?;
    let rows_result = batch
        .get(1)
        .ok_or_else(|| Error::from("missing rows result in D1 batch"))?;

    let total = count_result
        .results::<storage::CountRow>()?
        .into_iter()
        .next()
        .map(|r| r.count)
        .unwrap_or(0);

    let sessions: Vec<SessionSummary> = rows_result
        .results::<storage::SessionRow>()?
        .into_iter()
        .map(SessionSummary::from)
        .collect();

    let mut resp = Response::from_json(&SessionListResponse {
        sessions,
        total,
        page: built.page,
        per_page: built.per_page,
    })?;

    if cacheable {
        let _ = resp
            .headers_mut()
            .set("Cache-Control", PUBLIC_LIST_CACHE_CONTROL);
        let _ = resp.headers_mut().set("X-OpenSession-Cache", "MISS");

        if let Some(key) = cache_key.as_deref() {
            if let Ok(mut to_cache) = resp.cloned() {
                let _ = to_cache
                    .headers_mut()
                    .set("Cache-Control", PUBLIC_LIST_CACHE_CONTROL);
                let _ = to_cache.headers_mut().set("X-OpenSession-Cache", "HIT");
                if let Err(e) = Cache::default().put(key, to_cache).await {
                    console_log!("cache put failed for /api/sessions: {e:?}");
                }
            }
        }
    }

    Ok(resp)
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

#[cfg(test)]
mod tests {
    use super::worker_public_feed_enabled;

    #[test]
    fn worker_public_feed_is_forced_enabled() {
        assert!(worker_public_feed_enabled());
    }
}
