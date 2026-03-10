use axum::{
    extract::Query,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;

const DOCS_MD_EN: &str = include_str!("../../../../docs.md");
const DOCS_MD_KO: &str = include_str!("../../../../docs.ko.md");

const LLMS_TXT: &str = "\
# OpenSession

> Open-source platform for recording, sharing, and analyzing AI coding sessions.

OpenSession captures Human-AI Interaction Logs (HAIL) from tools like Claude Code, \
Cursor, and Codex, then provides a timeline viewer, repo analytics, and search.

## Docs

- [Documentation](/docs): Full docs (also available as `Accept: text/markdown`)

## API

Base URL: `/api`

- `POST /api/parse/preview` — Preview parser output from source descriptors
- `GET /api/review/local/:review_id` — Load local PR review bundle (local mode)
- `GET /api/sessions` — List sessions
- `GET /api/sessions/:id` — Get session detail
- `GET /api/sessions/:id/raw` — Download raw session JSONL
- `DELETE /api/admin/sessions/:id` — Delete session (admin key)

## Open Source

- [GitHub](https://github.com/hwisu/opensession)
- Local server: `cargo run -p opensession-server`
- CLI: `cargo install opensession`
";

#[derive(Debug, Default, Deserialize)]
pub struct DocsQuery {
    lang: Option<String>,
}

fn is_korean_locale(locale: Option<&str>) -> bool {
    locale.map(str::trim).is_some_and(|value| {
        value.eq_ignore_ascii_case("ko") || value.to_ascii_lowercase().starts_with("ko-")
    })
}

fn docs_markdown_for_locale(locale: Option<&str>) -> &'static str {
    if is_korean_locale(locale) {
        DOCS_MD_KO
    } else {
        DOCS_MD_EN
    }
}

pub async fn handle(Query(query): Query<DocsQuery>, headers: HeaderMap) -> Response {
    let accept = headers
        .get("accept")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if accept.contains("text/markdown") {
        return (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "text/markdown; charset=utf-8"),
                (header::CACHE_CONTROL, "public, max-age=3600"),
            ],
            docs_markdown_for_locale(query.lang.as_deref()),
        )
            .into_response();
    }

    // For browsers: serve index.html (SPA routing)
    let web_dir = std::env::var("OPENSESSION_WEB_DIR").unwrap_or_else(|_| "web/build".into());
    let index_path = std::path::Path::new(&web_dir).join("index.html");

    match tokio::fs::read(&index_path).await {
        Ok(content) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            content,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::docs_markdown_for_locale;

    #[test]
    fn selects_korean_docs_for_korean_locale() {
        let selected = docs_markdown_for_locale(Some("ko-KR"));
        assert!(selected.starts_with("# 문서"));
    }

    #[test]
    fn defaults_to_english_docs() {
        let selected = docs_markdown_for_locale(Some("en-US"));
        assert!(selected.starts_with("# Documentation"));
    }
}

pub async fn llms_txt() -> impl IntoResponse {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/markdown; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        LLMS_TXT,
    )
}
