use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
    Json,
};

use opensession_api::{
    db, LinkType, SessionDetail, SessionLink, SessionListQuery, SessionListResponse, SessionSummary,
};

use crate::error::ApiErr;
use crate::routes::auth::AuthUser;
use crate::storage::{session_from_row, sq_query_map, sq_query_row, Db};
use crate::AppConfig;

const PUBLIC_LIST_CACHE_CONTROL: &str = "public, max-age=30, stale-while-revalidate=60";

#[derive(Debug, PartialEq, Eq)]
enum RawBodySource {
    LocalStorage(String),
    RedirectUrl(String),
}

fn resolve_raw_body_source(
    body_storage_key: String,
    body_url: Option<String>,
) -> Result<RawBodySource, ApiErr> {
    if let Some(url) = body_url.map(|v| v.trim().to_string()) {
        if !url.is_empty() {
            return Ok(RawBodySource::RedirectUrl(url));
        }
    }

    let key = body_storage_key.trim().to_string();
    if key.is_empty() {
        return Err(ApiErr::not_found("session body not found"));
    }

    Ok(RawBodySource::LocalStorage(key))
}

// ---------------------------------------------------------------------------
// List sessions
// ---------------------------------------------------------------------------

/// GET /api/sessions — list sessions (public, paginated, filtered).
pub async fn list_sessions(
    State(db): State<Db>,
    State(config): State<AppConfig>,
    auth_user: Result<AuthUser, ApiErr>,
    Query(q): Query<SessionListQuery>,
    headers: HeaderMap,
) -> Result<axum::response::Response, ApiErr> {
    let has_auth_header = headers.get(header::AUTHORIZATION).is_some();
    let is_authenticated = auth_user.is_ok();
    if !can_access_session_list(config.public_feed_enabled, is_authenticated) {
        return Err(ApiErr::unauthorized(
            "public session feed is disabled; authentication required",
        ));
    }

    let built = db::sessions::list(&q);
    let conn = db.conn();

    // Count total
    let total: i64 = sq_query_row(&conn, built.count_query, |row| row.get(0))
        .map_err(ApiErr::from_db("count sessions"))?;

    // Fetch page
    let sessions: Vec<SessionSummary> = sq_query_map(&conn, built.select_query, session_from_row)
        .map_err(ApiErr::from_db("list sessions"))?;

    let mut resp = Json(SessionListResponse {
        sessions,
        total,
        page: built.page,
        per_page: built.per_page,
    })
    .into_response();

    let has_session_cookie = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|cookie| cookie.contains("session="));
    if q.is_public_feed_cacheable(has_auth_header, has_session_cookie) {
        resp.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static(PUBLIC_LIST_CACHE_CONTROL),
        );
    }

    Ok(resp)
}

fn can_access_session_list(public_feed_enabled: bool, is_authenticated: bool) -> bool {
    public_feed_enabled || is_authenticated
}

// ---------------------------------------------------------------------------
// Get session detail
// ---------------------------------------------------------------------------

/// GET /api/sessions/:id — get session detail with linked sessions.
pub async fn get_session(
    State(db): State<Db>,
    Path(id): Path<String>,
) -> Result<Json<SessionDetail>, ApiErr> {
    let conn = db.conn();

    let summary: SessionSummary =
        sq_query_row(&conn, db::sessions::get_by_id(&id), session_from_row)
            .map_err(|_| ApiErr::not_found("session not found"))?;

    // Fetch linked sessions
    let linked_sessions: Vec<SessionLink> =
        sq_query_map(&conn, db::sessions::links_by_session(&id), |row| {
            let lt: String = row.get(2)?;
            Ok(SessionLink {
                session_id: row.get(0)?,
                linked_session_id: row.get(1)?,
                link_type: match lt.as_str() {
                    "related" => LinkType::Related,
                    "parent" => LinkType::Parent,
                    "child" => LinkType::Child,
                    _ => LinkType::Handoff,
                },
                created_at: row.get(3)?,
            })
        })
        .map_err(ApiErr::from_db("query session_links"))?;

    Ok(Json(SessionDetail {
        summary,
        linked_sessions,
    }))
}

// ---------------------------------------------------------------------------
// Get raw session body
// ---------------------------------------------------------------------------

/// GET /api/sessions/:id/raw — download the full HAIL JSONL body.
pub async fn get_session_raw(
    State(db): State<Db>,
    Path(id): Path<String>,
) -> Result<axum::response::Response, ApiErr> {
    let conn = db.conn();

    let (body_storage_key, body_url): (String, Option<String>) =
        sq_query_row(&conn, db::sessions::get_storage_info(&id), |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .map_err(|_| ApiErr::not_found("session not found"))?;

    drop(conn);

    match resolve_raw_body_source(body_storage_key, body_url)? {
        RawBodySource::RedirectUrl(url) => {
            let location = HeaderValue::from_str(&url)
                .map_err(|_| ApiErr::internal("invalid session body URL"))?;
            let mut response = StatusCode::FOUND.into_response();
            response.headers_mut().insert(header::LOCATION, location);
            Ok(response)
        }
        RawBodySource::LocalStorage(storage_key) => {
            let body = db.read_body(&storage_key).map_err(|e| {
                tracing::error!("read body: {e}");
                ApiErr::internal("failed to read session body")
            })?;

            Ok((
                StatusCode::OK,
                [
                    (axum::http::header::CONTENT_TYPE, "application/jsonl"),
                    (
                        axum::http::header::CONTENT_DISPOSITION,
                        "attachment; filename=\"session.hail.jsonl\"",
                    ),
                ],
                body,
            )
                .into_response())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{can_access_session_list, resolve_raw_body_source, RawBodySource};

    #[test]
    fn session_list_access_rules_follow_public_feed_flag() {
        assert!(can_access_session_list(true, false));
        assert!(can_access_session_list(true, true));
        assert!(can_access_session_list(false, true));
        assert!(!can_access_session_list(false, false));
    }

    #[test]
    fn raw_body_source_prefers_redirect_when_body_url_present() {
        let source = match resolve_raw_body_source(
            "".to_string(),
            Some("https://example.com/a".to_string()),
        ) {
            Ok(source) => source,
            Err(_) => panic!("body_url should resolve to redirect"),
        };
        match source {
            RawBodySource::RedirectUrl(url) => assert_eq!(url, "https://example.com/a"),
            RawBodySource::LocalStorage(_) => panic!("expected redirect source"),
        }
    }

    #[test]
    fn raw_body_source_uses_storage_key_when_no_body_url() {
        let source = match resolve_raw_body_source("abc.hail.jsonl".to_string(), None) {
            Ok(source) => source,
            Err(_) => panic!("storage key should resolve to local storage"),
        };
        match source {
            RawBodySource::LocalStorage(key) => assert_eq!(key, "abc.hail.jsonl"),
            RawBodySource::RedirectUrl(_) => panic!("expected local storage source"),
        }
    }

    #[test]
    fn raw_body_source_rejects_empty_storage_and_body_url() {
        let err = resolve_raw_body_source("".to_string(), Some("   ".to_string()))
            .expect_err("empty storage/body_url should fail");
        let response = axum::response::IntoResponse::into_response(err);
        assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
    }
}
