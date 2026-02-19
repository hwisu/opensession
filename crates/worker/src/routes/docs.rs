use worker::*;

const DOCS_MD: &str = include_str!("../../../../docs.md");

const LLMS_TXT: &str = "\
# OpenSession

> Open-source platform for recording, sharing, and analyzing AI coding sessions.

OpenSession captures Human-AI Interaction Logs (HAIL) from tools like Claude Code, \
Cursor, and Codex, then provides a timeline viewer and public-session search.

## Docs

- [Documentation](https://opensession.io/docs): Full docs (also available as `Accept: text/markdown`)

## API

Base URL: `https://opensession.io/api`

- `GET /api/sessions` — List sessions
- `GET /api/sessions/:id` — Get session detail
- `GET /api/sessions/:id/raw` — Download raw HAIL JSONL

## Open Source

- [GitHub](https://github.com/hwisu/opensession)
- Local server (ingest/read): `cargo run -p opensession-server`
- CLI: `cargo install opensession`
";

pub async fn handle(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let accept = req.headers().get("Accept")?.unwrap_or_default();
    if accept.contains("text/markdown") {
        let headers = Headers::new();
        headers.set("Content-Type", "text/markdown; charset=utf-8")?;
        headers.set("Cache-Control", "public, max-age=3600")?;
        return Ok(Response::ok(DOCS_MD)?.with_headers(headers));
    }
    // HTML: delegate to ASSETS binding for SPA serving
    let assets: Fetcher = ctx.env.service("ASSETS")?;
    assets.fetch_request(req).await
}

pub async fn llms_txt(_req: Request, _ctx: RouteContext<()>) -> Result<Response> {
    let headers = Headers::new();
    headers.set("Content-Type", "text/markdown; charset=utf-8")?;
    headers.set("Cache-Control", "public, max-age=3600")?;
    Ok(Response::ok(LLMS_TXT)?.with_headers(headers))
}
