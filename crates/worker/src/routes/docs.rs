use worker::*;

const DOCS_MD_EN: &str = include_str!("../../../../docs.md");
const DOCS_MD_KO: &str = include_str!("../../../../docs.ko.md");

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
    let url = req.url()?;
    let lang = url
        .query_pairs()
        .find_map(|(key, value)| (key == "lang").then(|| value.into_owned()));
    if accept.contains("text/markdown") {
        let headers = Headers::new();
        headers.set("Content-Type", "text/markdown; charset=utf-8")?;
        headers.set("Cache-Control", "public, max-age=3600")?;
        return Ok(Response::ok(docs_markdown_for_locale(lang.as_deref()))?.with_headers(headers));
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

fn docs_markdown_for_locale(locale: Option<&str>) -> &'static str {
    if locale
        .map(str::trim)
        .is_some_and(|value| value.eq_ignore_ascii_case("ko") || value.to_ascii_lowercase().starts_with("ko-"))
    {
        DOCS_MD_KO
    } else {
        DOCS_MD_EN
    }
}

#[cfg(test)]
mod tests {
    use super::docs_markdown_for_locale;

    #[test]
    fn returns_korean_docs_for_korean_locale() {
        let selected = docs_markdown_for_locale(Some("ko"));
        assert!(selected.starts_with("# 문서"));
    }

    #[test]
    fn returns_english_docs_for_other_locales() {
        let selected = docs_markdown_for_locale(Some("en-US"));
        assert!(selected.starts_with("# Documentation"));
    }
}
