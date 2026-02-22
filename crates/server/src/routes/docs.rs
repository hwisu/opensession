use axum::{
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};

const DOCS_MD: &str = include_str!("../../../../docs.md");

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
- `GET /api/sessions` — List sessions
- `GET /api/sessions/:id` — Get session detail
- `GET /api/sessions/:id/raw` — Download raw HAIL JSONL
- `DELETE /api/admin/sessions/:id` — Delete session (admin key)

## Open Source

- [GitHub](https://github.com/hwisu/opensession)
- Local server: `cargo run -p opensession-server`
- CLI: `cargo install opensession`
";

pub async fn handle(headers: HeaderMap) -> Response {
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
            DOCS_MD,
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
