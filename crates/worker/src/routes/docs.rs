use worker::*;

const DOCS_MD: &str = include_str!("../../../../docs.md");

const LLMS_TXT: &str = "\
# OpenSession

> Open-source platform for recording, sharing, and analyzing AI coding sessions.

OpenSession captures Human-AI Interaction Logs (HAIL) from tools like Claude Code, \
Cursor, and Codex, then provides a timeline viewer, team analytics, and search.

## Docs

- [Documentation](https://opensession.io/docs): Full docs (also available as `Accept: text/markdown`)

## API

Base URL: `https://opensession.io/api`

- `POST /api/auth/register` — Create account
- `POST /api/auth/login` — Sign in
- `POST /api/sessions` — Upload HAIL session
- `GET /api/sessions` — List sessions
- `GET /api/sessions/:id` — Get session detail
- `GET /api/sessions/:id/raw` — Download raw HAIL JSONL
- `GET /api/teams` — List teams
- `GET /api/sync/pull` — Sync endpoint

## Open Source

- [GitHub](https://github.com/hwisu/opensession-core)
- Self-host: `docker run -p 3000:3000 ghcr.io/hwisu/opensession`
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
