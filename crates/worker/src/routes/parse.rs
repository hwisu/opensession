use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use base64::Engine;
use opensession_api::{
    db as dbq,
    ParseCandidate, ParsePreviewErrorResponse, ParsePreviewRequest, ParsePreviewResponse,
    ParseSource, Session,
};
use serde::Deserialize;
use worker::*;

use crate::config::WorkerConfig;
use crate::db_helpers::values_to_js;
use crate::routes::auth::authenticate_optional;
use crate::storage;

const MAX_SOURCE_SIZE_BYTES: usize = 10 * 1024 * 1024;

#[derive(Debug, Clone)]
struct GitSource {
    remote: String,
    r#ref: String,
    path: String,
}

#[derive(Debug, Clone)]
struct GithubSource {
    owner: String,
    repo: String,
    r#ref: String,
    path: String,
}

#[derive(Debug, Clone)]
struct ParseInput {
    source: ParseSource,
    filename: String,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
struct ParsePreviewResult {
    parser_used: String,
    parser_candidates: Vec<ParseCandidate>,
    session: Session,
    warnings: Vec<String>,
    native_adapter: Option<String>,
}

#[derive(Debug, Clone)]
struct PreviewRouteError {
    status: u16,
    code: &'static str,
    message: String,
    parser_candidates: Vec<ParseCandidate>,
}

impl PreviewRouteError {
    fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: 401,
            code: "unauthorized",
            message: message.into(),
            parser_candidates: Vec::new(),
        }
    }

    fn invalid_source(message: impl Into<String>) -> Self {
        Self {
            status: 400,
            code: "invalid_source",
            message: message.into(),
            parser_candidates: Vec::new(),
        }
    }

    fn fetch_failed(message: impl Into<String>) -> Self {
        Self {
            status: 422,
            code: "fetch_failed",
            message: message.into(),
            parser_candidates: Vec::new(),
        }
    }

    fn missing_git_credential(status_code: u16) -> Self {
        Self {
            status: 401,
            code: "missing_git_credential",
            message: format!(
                "remote source returned status {status_code}; connect provider OAuth or register a git credential for this host"
            ),
            parser_candidates: Vec::new(),
        }
    }

    fn git_credential_forbidden(status_code: u16) -> Self {
        Self {
            status: 403,
            code: "git_credential_forbidden",
            message: format!(
                "configured credential was rejected by remote source (status {status_code})"
            ),
            parser_candidates: Vec::new(),
        }
    }

    fn file_too_large(size: usize) -> Self {
        Self {
            status: 422,
            code: "file_too_large",
            message: format!(
                "source is too large ({} bytes, max {} bytes)",
                size, MAX_SOURCE_SIZE_BYTES
            ),
            parser_candidates: Vec::new(),
        }
    }

    fn parse_failed(message: impl Into<String>, parser_candidates: Vec<ParseCandidate>) -> Self {
        Self {
            status: 422,
            code: "parse_failed",
            message: message.into(),
            parser_candidates,
        }
    }

    fn into_response(self) -> Result<Response> {
        Response::from_json(&ParsePreviewErrorResponse {
            code: self.code.to_string(),
            message: self.message,
            parser_candidates: self.parser_candidates,
        })
        .map(|response| response.with_status(self.status))
    }
}

pub async fn preview(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = WorkerConfig::from_env(&ctx.env);
    let d1 = match storage::get_d1(&ctx.env) {
        Ok(db) => db,
        Err(err) => {
            return PreviewRouteError::fetch_failed(format!("load d1 binding failed: {err}"))
                .into_response();
        }
    };

    let body: ParsePreviewRequest = match req.json().await {
        Ok(body) => body,
        Err(_) => {
            return PreviewRouteError::invalid_source("invalid request body").into_response();
        }
    };

    let user_id = match authenticate_optional(&req, &d1, &config).await {
        Ok(user) => user.map(|u| u.user_id),
        Err(err) => return PreviewRouteError::unauthorized(err.message()).into_response(),
    };

    let input = match prepare_parse_input(body.source, &d1, &config, user_id.as_deref()).await {
        Ok(input) => input,
        Err(err) => return err.into_response(),
    };

    match preview_parse_bytes(&input.filename, &input.bytes, body.parser_hint.as_deref()) {
        Ok(preview) => Response::from_json(&ParsePreviewResponse {
            parser_used: preview.parser_used,
            parser_candidates: preview.parser_candidates,
            session: preview.session,
            source: input.source,
            warnings: preview.warnings,
            native_adapter: preview.native_adapter,
        }),
        Err(err) => err.into_response(),
    }
}

async fn prepare_parse_input(
    source: ParseSource,
    d1: &D1Database,
    config: &WorkerConfig,
    user_id: Option<&str>,
) -> Result<ParseInput, PreviewRouteError> {
    match source {
        ParseSource::Git {
            remote,
            r#ref,
            path,
        } => {
            let normalized = normalize_git_source(&remote, &r#ref, &path)?;
            let bytes = fetch_git_source(&normalized, d1, config, user_id).await?;
            let filename = file_name_from_path(&normalized.path);
            Ok(ParseInput {
                source: ParseSource::Git {
                    remote: normalized.remote,
                    r#ref: normalized.r#ref,
                    path: normalized.path,
                },
                filename,
                bytes,
            })
        }
        ParseSource::Github {
            owner,
            repo,
            r#ref,
            path,
        } => {
            let normalized = normalize_github_source(&owner, &repo, &r#ref, &path)?;
            let bytes = fetch_git_source(&GitSource {
                remote: format!(
                    "https://github.com/{}/{}",
                    normalized.owner, normalized.repo
                ),
                r#ref: normalized.r#ref.clone(),
                path: normalized.path.clone(),
            }, d1, config, user_id)
            .await?;
            let filename = file_name_from_path(&normalized.path);
            Ok(ParseInput {
                source: ParseSource::Github {
                    owner: normalized.owner,
                    repo: normalized.repo,
                    r#ref: normalized.r#ref,
                    path: normalized.path,
                },
                filename,
                bytes,
            })
        }
        ParseSource::Inline {
            filename,
            content_base64,
        } => {
            let filename = normalize_filename(&filename)?;
            let bytes = decode_inline_content(&content_base64)?;
            if bytes.len() > MAX_SOURCE_SIZE_BYTES {
                return Err(PreviewRouteError::file_too_large(bytes.len()));
            }
            Ok(ParseInput {
                source: ParseSource::Inline {
                    filename: filename.clone(),
                    content_base64: String::new(),
                },
                filename,
                bytes,
            })
        }
    }
}

fn preview_parse_bytes(
    filename: &str,
    content_bytes: &[u8],
    parser_hint: Option<&str>,
) -> Result<ParsePreviewResult, PreviewRouteError> {
    if looks_binary(content_bytes) {
        return Err(PreviewRouteError::parse_failed(
            "binary files are not supported",
            Vec::new(),
        ));
    }

    let content = std::str::from_utf8(content_bytes).map_err(|_| {
        PreviewRouteError::parse_failed("input is not valid UTF-8 text", Vec::new())
    })?;

    let parser_candidates = detect_candidates(filename, content);
    let hint = parser_hint.map(str::trim).filter(|value| !value.is_empty());

    if let Some(hint) = hint {
        if hint != "hail" {
            return Err(PreviewRouteError::parse_failed(
                format!("parser '{hint}' is not available in worker runtime"),
                parser_candidates,
            ));
        }
    }

    let session = parse_hail_content(content).map_err(|err| {
        PreviewRouteError::parse_failed(
            format!("failed to parse source as HAIL: {err}"),
            parser_candidates.clone(),
        )
    })?;

    Ok(ParsePreviewResult {
        parser_used: "hail".to_string(),
        parser_candidates,
        session,
        warnings: Vec::new(),
        native_adapter: None,
    })
}

fn parse_hail_content(content: &str) -> std::result::Result<Session, String> {
    if let Ok(session) = Session::from_jsonl(content) {
        return Ok(session);
    }
    serde_json::from_str::<Session>(content).map_err(|err| err.to_string())
}

fn detect_candidates(filename: &str, content: &str) -> Vec<ParseCandidate> {
    let mut candidates = Vec::<ParseCandidate>::new();
    let lower_name = filename.to_ascii_lowercase();
    let trimmed = content.trim();

    if lower_name.ends_with(".hail.jsonl") {
        add_candidate(&mut candidates, "hail", 95, "filename suffix .hail.jsonl");
    }
    if lower_name.ends_with(".jsonl") {
        add_candidate(&mut candidates, "hail", 70, "jsonl extension");
        add_candidate(&mut candidates, "codex", 64, "jsonl extension");
        add_candidate(&mut candidates, "claude-code", 62, "jsonl extension");
        add_candidate(&mut candidates, "gemini", 50, "jsonl extension");
    }
    if lower_name.ends_with(".json") {
        add_candidate(&mut candidates, "gemini", 56, "json extension");
        add_candidate(&mut candidates, "amp", 46, "json extension");
        add_candidate(&mut candidates, "opencode", 44, "json extension");
        add_candidate(&mut candidates, "hail", 34, "json extension");
    }
    if lower_name.ends_with(".vscdb") {
        add_candidate(&mut candidates, "cursor", 92, "vscdb extension");
    }
    if lower_name.ends_with("api_conversation_history.json") {
        add_candidate(
            &mut candidates,
            "cline",
            88,
            "Cline conversation entrypoint filename",
        );
    }

    if looks_like_hail_jsonl(trimmed) {
        add_candidate(&mut candidates, "hail", 100, "HAIL header line");
    }
    if looks_like_hail_json(trimmed) {
        add_candidate(&mut candidates, "hail", 86, "HAIL JSON object fields");
    }
    if looks_like_codex_jsonl(trimmed) {
        add_candidate(&mut candidates, "codex", 90, "Codex event markers");
    }
    if looks_like_claude_jsonl(trimmed) {
        add_candidate(
            &mut candidates,
            "claude-code",
            88,
            "Claude message record markers",
        );
    }
    if looks_like_gemini_json(trimmed) {
        add_candidate(
            &mut candidates,
            "gemini",
            84,
            "Gemini session schema fields",
        );
    }
    if looks_like_amp_json(trimmed) {
        add_candidate(&mut candidates, "amp", 66, "Amp thread schema fields");
    }
    if looks_like_opencode_json(trimmed) {
        add_candidate(
            &mut candidates,
            "opencode",
            60,
            "OpenCode provider/model schema fields",
        );
    }

    candidates.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then_with(|| a.id.cmp(&b.id))
    });
    candidates
}

fn add_candidate(candidates: &mut Vec<ParseCandidate>, id: &str, confidence: u8, reason: &str) {
    if let Some(existing) = candidates.iter_mut().find(|candidate| candidate.id == id) {
        if confidence > existing.confidence {
            existing.confidence = confidence;
            existing.reason = reason.to_string();
        }
        return;
    }

    candidates.push(ParseCandidate {
        id: id.to_string(),
        confidence,
        reason: reason.to_string(),
    });
}

fn looks_like_hail_jsonl(content: &str) -> bool {
    let Some(first_line) = content.lines().find(|line| !line.trim().is_empty()) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(first_line) else {
        return false;
    };
    value.get("type").and_then(|v| v.as_str()) == Some("header")
        && value.get("version").is_some()
        && value.get("session_id").is_some()
}

fn looks_like_hail_json(content: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(content) else {
        return false;
    };
    value.get("version").is_some()
        && value.get("session_id").is_some()
        && value.get("agent").is_some()
        && value.get("context").is_some()
        && value.get("events").is_some()
}

fn looks_like_codex_jsonl(content: &str) -> bool {
    content.contains("\"type\":\"session_meta\"")
        || content.contains("\"type\": \"session_meta\"")
        || content.contains("\"type\":\"response_item\"")
        || content.contains("\"type\":\"event_msg\"")
}

fn looks_like_claude_jsonl(content: &str) -> bool {
    (content.contains("\"type\":\"user\"") || content.contains("\"type\":\"assistant\""))
        && content.contains("\"message\"")
}

fn looks_like_gemini_json(content: &str) -> bool {
    content.contains("\"messages\"")
        && (content.contains("\"session_id\"") || content.contains("\"sessionId\""))
}

fn looks_like_amp_json(content: &str) -> bool {
    content.contains("\"agentMode\"")
        || (content.contains("\"messages\"") && content.contains("\"tool_use\""))
}

fn looks_like_opencode_json(content: &str) -> bool {
    content.contains("\"providerID\"")
        || content.contains("\"providerId\"")
        || content.contains("\"modelID\"")
        || content.contains("\"modelId\"")
}

fn normalize_git_source(
    remote: &str,
    r#ref: &str,
    path: &str,
) -> Result<GitSource, PreviewRouteError> {
    let remote = decode_and_trim(remote, "remote")?;
    let url = validate_remote_url(&remote)?;
    let remote = normalized_remote_origin(&url);

    let r#ref = decode_and_trim(r#ref, "ref")?;
    if !is_valid_ref(r#ref.as_str()) {
        return Err(PreviewRouteError::invalid_source(
            "ref must match [A-Za-z0-9._/-]{1,255} without '..', '~', '^', ':', or '\\'",
        ));
    }

    let path = normalize_repo_path(path)?;

    Ok(GitSource {
        remote,
        r#ref,
        path,
    })
}

fn normalize_github_source(
    owner: &str,
    repo: &str,
    r#ref: &str,
    path: &str,
) -> Result<GithubSource, PreviewRouteError> {
    let owner = decode_and_trim(owner, "owner")?;
    if !is_valid_owner_repo(owner.as_str()) {
        return Err(PreviewRouteError::invalid_source(
            "owner must match [A-Za-z0-9._-]{1,100}",
        ));
    }

    let repo = decode_and_trim(repo, "repo")?;
    if !is_valid_owner_repo(repo.as_str()) {
        return Err(PreviewRouteError::invalid_source(
            "repo must match [A-Za-z0-9._-]{1,100}",
        ));
    }

    let r#ref = decode_and_trim(r#ref, "ref")?;
    if !is_valid_ref(r#ref.as_str()) {
        return Err(PreviewRouteError::invalid_source(
            "ref must match [A-Za-z0-9._/-]{1,255} without '..', '~', '^', ':', or '\\'",
        ));
    }

    let path = normalize_repo_path(path)?;

    Ok(GithubSource {
        owner,
        repo,
        r#ref,
        path,
    })
}

fn decode_and_trim(value: &str, field: &str) -> Result<String, PreviewRouteError> {
    urlencoding::decode(value)
        .map(|decoded| decoded.trim().to_string())
        .map_err(|_| {
            PreviewRouteError::invalid_source(format!("{field} contains invalid percent encoding"))
        })
}

fn normalize_repo_path(path: &str) -> Result<String, PreviewRouteError> {
    let decoded = decode_and_trim(path, "path")?;
    if decoded.is_empty() {
        return Err(PreviewRouteError::invalid_source("path is required"));
    }
    if decoded.starts_with('/') {
        return Err(PreviewRouteError::invalid_source(
            "path must be repository-relative",
        ));
    }

    let mut normalized_segments = Vec::<String>::new();
    for segment in decoded.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(PreviewRouteError::invalid_source(
                "path cannot contain empty, '.' or '..' segments",
            ));
        }
        if segment.contains('\\') {
            return Err(PreviewRouteError::invalid_source(
                "path cannot contain backslash characters",
            ));
        }
        normalized_segments.push(segment.to_string());
    }

    Ok(normalized_segments.join("/"))
}

fn normalize_filename(filename: &str) -> Result<String, PreviewRouteError> {
    let decoded = decode_and_trim(filename, "filename")?;
    let normalized = decoded
        .replace('\\', "/")
        .rsplit('/')
        .next()
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    if normalized.is_empty() || normalized == "." || normalized == ".." {
        return Err(PreviewRouteError::invalid_source(
            "inline filename must be a non-empty filename",
        ));
    }
    if normalized.len() > 255 {
        return Err(PreviewRouteError::invalid_source(
            "inline filename is too long (max 255 chars)",
        ));
    }
    Ok(normalized)
}

fn decode_inline_content(content_base64: &str) -> Result<Vec<u8>, PreviewRouteError> {
    let trimmed = content_base64.trim();
    if trimmed.is_empty() {
        return Err(PreviewRouteError::invalid_source(
            "inline content_base64 is required",
        ));
    }
    base64::engine::general_purpose::STANDARD
        .decode(trimmed)
        .map_err(|_| {
            PreviewRouteError::invalid_source("inline content_base64 must be valid base64")
        })
}

fn file_name_from_path(path: &str) -> String {
    path.rsplit('/')
        .next()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("session.txt")
        .to_string()
}

fn validate_remote_url(remote: &str) -> Result<Url, PreviewRouteError> {
    let parsed = Url::parse(remote)
        .map_err(|_| PreviewRouteError::invalid_source("remote must be an absolute http(s) URL"))?;

    let scheme = parsed.scheme().to_ascii_lowercase();
    if scheme != "https" && scheme != "http" {
        return Err(PreviewRouteError::invalid_source(
            "remote must use http or https",
        ));
    }

    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(PreviewRouteError::invalid_source(
            "remote cannot include credentials",
        ));
    }

    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(PreviewRouteError::invalid_source(
            "remote cannot include query or fragment",
        ));
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| PreviewRouteError::invalid_source("remote host is required"))?;

    if is_disallowed_remote_host(host) {
        return Err(PreviewRouteError::invalid_source(
            "remote host is not allowed",
        ));
    }

    let repo_path = parsed.path().trim_matches('/');
    if repo_path.is_empty() {
        return Err(PreviewRouteError::invalid_source(
            "remote must include repository path",
        ));
    }

    Ok(parsed)
}

fn normalized_remote_origin(url: &Url) -> String {
    let mut normalized = url.clone();
    normalized.set_query(None);
    normalized.set_fragment(None);
    let _ = normalized.set_username("");
    let _ = normalized.set_password(None);

    let trimmed_path = normalized.path().trim_end_matches('/').to_string();
    if trimmed_path.is_empty() {
        normalized.set_path("/");
    } else {
        normalized.set_path(&trimmed_path);
    }

    let mut rendered = normalized.to_string();
    if rendered.ends_with('/') {
        rendered.pop();
    }
    rendered
}

fn is_disallowed_remote_host(host: &str) -> bool {
    let lowered = host.to_ascii_lowercase();
    if lowered == "localhost" || lowered.ends_with(".localhost") {
        return true;
    }

    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => is_disallowed_ipv4(v4),
        Ok(IpAddr::V6(v6)) => is_disallowed_ipv6(v6),
        Err(_) => false,
    }
}

fn is_disallowed_ipv4(ip: Ipv4Addr) -> bool {
    ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || ip.is_unspecified()
}

fn is_disallowed_ipv6(ip: Ipv6Addr) -> bool {
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_unique_local()
        || ip.is_unicast_link_local()
        || ip.is_multicast()
        || is_ipv6_documentation(ip)
}

fn is_ipv6_documentation(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    segments[0] == 0x2001 && segments[1] == 0x0db8
}

fn origin_from_url(url: &Url) -> Result<String, PreviewRouteError> {
    let host = url
        .host_str()
        .ok_or_else(|| PreviewRouteError::invalid_source("remote host is required"))?;
    let mut origin = format!("{}://{host}", url.scheme());
    if let Some(port) = url.port() {
        origin.push(':');
        origin.push_str(&port.to_string());
    }
    Ok(origin)
}

fn repo_path_segments(url: &Url) -> Result<Vec<String>, PreviewRouteError> {
    let mut segments: Vec<String> = url
        .path_segments()
        .ok_or_else(|| PreviewRouteError::invalid_source("remote repository path is invalid"))?
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            urlencoding::decode(segment)
                .map(|decoded| decoded.trim().to_string())
                .map_err(|_| {
                    PreviewRouteError::invalid_source(
                        "remote repository path contains invalid percent encoding",
                    )
                })
        })
        .collect::<Result<Vec<_>, _>>()?;

    if segments.len() < 2 {
        return Err(PreviewRouteError::invalid_source(
            "remote must include owner/group and repository",
        ));
    }

    if let Some(last) = segments.last_mut() {
        *last = strip_git_suffix(last).to_string();
    }

    if segments
        .iter()
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(PreviewRouteError::invalid_source(
            "remote repository path contains invalid segments",
        ));
    }

    Ok(segments)
}

fn strip_git_suffix(value: &str) -> &str {
    value.strip_suffix(".git").unwrap_or(value)
}

fn encode_segments(value: &str) -> String {
    value
        .split('/')
        .map(|segment| urlencoding::encode(segment).into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn build_github_raw_url(
    url: &Url,
    r#ref: &str,
    path: &str,
) -> Result<String, PreviewRouteError> {
    let segments = repo_path_segments(url)?;
    if segments.len() != 2 {
        return Err(PreviewRouteError::invalid_source(
            "github remote must look like https://github.com/{owner}/{repo}",
        ));
    }

    Ok(format!(
        "https://raw.githubusercontent.com/{}/{}/{}/{}",
        segments[0],
        segments[1],
        encode_segments(r#ref),
        encode_segments(path)
    ))
}

fn build_gitlab_raw_url(
    url: &Url,
    r#ref: &str,
    path: &str,
) -> Result<String, PreviewRouteError> {
    let project_path = repo_path_segments(url)?.join("/");
    let origin = origin_from_url(url)?;
    Ok(format!(
        "{}/{}/-/raw/{}/{}",
        origin,
        encode_segments(&project_path),
        encode_segments(r#ref),
        encode_segments(path)
    ))
}

fn build_generic_raw_url(
    url: &Url,
    r#ref: &str,
    path: &str,
) -> Result<String, PreviewRouteError> {
    let repo_path = repo_path_segments(url)?.join("/");
    let origin = origin_from_url(url)?;
    Ok(format!(
        "{}/{}/raw/{}/{}",
        origin,
        encode_segments(&repo_path),
        encode_segments(r#ref),
        encode_segments(path)
    ))
}

fn build_git_raw_url(source: &GitSource) -> Result<String, PreviewRouteError> {
    let remote = validate_remote_url(&source.remote)?;
    let host = remote
        .host_str()
        .ok_or_else(|| PreviewRouteError::invalid_source("remote host is required"))?
        .to_ascii_lowercase();

    if host == "github.com" {
        return build_github_raw_url(&remote, &source.r#ref, &source.path);
    }
    if host == "gitlab.com" || host.contains("gitlab") {
        return build_gitlab_raw_url(&remote, &source.r#ref, &source.path);
    }

    build_generic_raw_url(&remote, &source.r#ref, &source.path)
}

#[derive(Debug, Deserialize)]
struct OAuthProviderTokenRow {
    access_token_enc: String,
}

#[derive(Debug, Deserialize)]
struct GitCredentialSecretRow {
    id: String,
    path_prefix: String,
    header_name: String,
    header_value_enc: String,
}

#[derive(Debug, Clone)]
enum GitCredentialSource {
    Provider,
    Manual { credential_id: String, user_id: String },
}

#[derive(Debug, Clone)]
struct GitFetchAuthHeader {
    header_name: String,
    header_value: String,
    source: GitCredentialSource,
}

fn provider_for_host(host: &str) -> Option<&'static str> {
    if host == "github.com" {
        return Some("github");
    }
    if host == "gitlab.com" || host.contains("gitlab") {
        return Some("gitlab");
    }
    None
}

fn path_prefix_matches(repo_path: &str, prefix: &str) -> bool {
    if prefix.is_empty() {
        return true;
    }
    repo_path == prefix
        || repo_path
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'))
}

async fn d1_first<T: for<'de> Deserialize<'de>>(
    d1: &D1Database,
    built: (String, sea_query::Values),
    context: &str,
) -> Result<Option<T>, PreviewRouteError> {
    let (sql, values) = built;
    let stmt = d1
        .prepare(&sql)
        .bind(&values_to_js(&values))
        .map_err(|err| PreviewRouteError::fetch_failed(format!("{context}: {err}")))?;
    stmt.first(None)
        .await
        .map_err(|err| PreviewRouteError::fetch_failed(format!("{context}: {err}")))
}

async fn d1_all<T: for<'de> Deserialize<'de>>(
    d1: &D1Database,
    built: (String, sea_query::Values),
    context: &str,
) -> Result<Vec<T>, PreviewRouteError> {
    let (sql, values) = built;
    let stmt = d1
        .prepare(&sql)
        .bind(&values_to_js(&values))
        .map_err(|err| PreviewRouteError::fetch_failed(format!("{context}: {err}")))?;
    let result = stmt
        .all()
        .await
        .map_err(|err| PreviewRouteError::fetch_failed(format!("{context}: {err}")))?;
    result
        .results::<T>()
        .map_err(|err| PreviewRouteError::fetch_failed(format!("{context}: {err}")))
}

async fn d1_run(
    d1: &D1Database,
    built: (String, sea_query::Values),
    context: &str,
) -> Result<(), PreviewRouteError> {
    let (sql, values) = built;
    let stmt = d1
        .prepare(&sql)
        .bind(&values_to_js(&values))
        .map_err(|err| PreviewRouteError::fetch_failed(format!("{context}: {err}")))?;
    let result = stmt
        .run()
        .await
        .map_err(|err| PreviewRouteError::fetch_failed(format!("{context}: {err}")))?;
    if !result.success() {
        return Err(PreviewRouteError::fetch_failed(
            result.error().unwrap_or_else(|| format!("{context} failed")),
        ));
    }
    Ok(())
}

async fn resolve_fetch_auth_header(
    source: &GitSource,
    d1: &D1Database,
    config: &WorkerConfig,
    user_id: Option<&str>,
) -> Result<Option<GitFetchAuthHeader>, PreviewRouteError> {
    let Some(user_id) = user_id else {
        return Ok(None);
    };
    let Some(keyring) = config.credential_keyring.as_ref() else {
        return Ok(None);
    };

    let remote = validate_remote_url(&source.remote)?;
    let host = remote
        .host_str()
        .ok_or_else(|| PreviewRouteError::invalid_source("remote host is required"))?
        .to_ascii_lowercase();
    let repo_path = repo_path_segments(&remote)?.join("/");

    if let Some(provider) = provider_for_host(&host) {
        let provider_token = d1_first::<OAuthProviderTokenRow>(
            d1,
            dbq::oauth_provider_tokens::get_by_user_provider(user_id, provider),
            "load oauth provider token",
        )
        .await?;
        if let Some(row) = provider_token {
            let token = keyring
                .decrypt(&row.access_token_enc)
                .map_err(|e| PreviewRouteError::fetch_failed(e.message()))?;
            return Ok(Some(GitFetchAuthHeader {
                header_name: "Authorization".to_string(),
                header_value: format!("Bearer {token}"),
                source: GitCredentialSource::Provider,
            }));
        }
    }

    let manual_rows = d1_all::<GitCredentialSecretRow>(
        d1,
        dbq::git_credentials::list_for_host_with_secret(user_id, &host),
        "load manual git credentials",
    )
    .await?;
    for row in manual_rows {
        if !path_prefix_matches(&repo_path, &row.path_prefix) {
            continue;
        }
        let secret = keyring
            .decrypt(&row.header_value_enc)
            .map_err(|e| PreviewRouteError::fetch_failed(e.message()))?;
        return Ok(Some(GitFetchAuthHeader {
            header_name: row.header_name,
            header_value: secret,
            source: GitCredentialSource::Manual {
                credential_id: row.id,
                user_id: user_id.to_string(),
            },
        }));
    }

    Ok(None)
}

async fn fetch_git_source(
    source: &GitSource,
    d1: &D1Database,
    config: &WorkerConfig,
    user_id: Option<&str>,
) -> Result<Vec<u8>, PreviewRouteError> {
    let raw_url = build_git_raw_url(source)?;
    let fetch_auth = resolve_fetch_auth_header(source, d1, config, user_id).await?;
    let mut init = RequestInit::new();
    init.with_method(Method::Get);
    init.headers
        .set("User-Agent", "opensession-worker-parse-preview")
        .map_err(|err| PreviewRouteError::fetch_failed(format!("failed to set user-agent: {err}")))?;
    if let Some(auth) = fetch_auth.as_ref() {
        init.headers
            .set(&auth.header_name, &auth.header_value)
            .map_err(|err| {
                PreviewRouteError::fetch_failed(format!(
                    "failed to set auth header {}: {err}",
                    auth.header_name
                ))
            })?;
    }

    let req = Request::new_with_init(&raw_url, &init).map_err(|err| {
        PreviewRouteError::fetch_failed(format!(
            "failed to initialize fetch request for '{raw_url}': {err}"
        ))
    })?;
    let mut response = Fetch::Request(req)
        .send()
        .await
        .map_err(|err| PreviewRouteError::fetch_failed(format!("failed to fetch source: {err}")))?;

    let status = response.status_code();
    if (status == 401 || status == 403) && user_id.is_some() {
        return Err(match fetch_auth {
            Some(_) => PreviewRouteError::git_credential_forbidden(status),
            None => PreviewRouteError::missing_git_credential(status),
        });
    }
    if !(200..300).contains(&status) {
        return Err(PreviewRouteError::fetch_failed(format!(
            "source fetch failed with status {status}"
        )));
    }

    if let Some(content_len) = response
        .headers()
        .get("Content-Length")
        .map_err(|err| {
            PreviewRouteError::fetch_failed(format!("failed to read content-length: {err}"))
        })?
        .and_then(|value| value.parse::<usize>().ok())
    {
        if content_len > MAX_SOURCE_SIZE_BYTES {
            return Err(PreviewRouteError::file_too_large(content_len));
        }
    }

    if let Some(content_type) = response
        .headers()
        .get("Content-Type")
        .map_err(|err| PreviewRouteError::fetch_failed(format!("failed to read content-type: {err}")))?
    {
        if !is_allowed_content_type(&content_type) {
            return Err(PreviewRouteError::fetch_failed(format!(
                "unsupported content-type '{content_type}', expected text/json content",
            )));
        }
    }

    let bytes = response.bytes().await.map_err(|err| {
        PreviewRouteError::fetch_failed(format!("failed reading source body: {err}"))
    })?;

    if bytes.len() > MAX_SOURCE_SIZE_BYTES {
        return Err(PreviewRouteError::file_too_large(bytes.len()));
    }

    if let Some(GitFetchAuthHeader {
        source:
            GitCredentialSource::Manual {
                credential_id,
                user_id,
            },
        ..
    }) = fetch_auth
    {
        let _ = d1_run(
            d1,
            dbq::git_credentials::touch_last_used(credential_id.as_str(), user_id.as_str()),
            "touch git credential last_used_at",
        )
        .await;
    }

    Ok(bytes)
}

fn is_allowed_content_type(content_type: &str) -> bool {
    let media_type = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if media_type.is_empty() {
        return true;
    }
    if media_type.starts_with("text/") {
        return true;
    }
    media_type == "application/json"
        || media_type == "application/jsonl"
        || media_type == "application/x-ndjson"
        || media_type.ends_with("+json")
}

fn looks_binary(bytes: &[u8]) -> bool {
    bytes.contains(&0)
}

fn is_valid_owner_repo(value: &str) -> bool {
    let len = value.len();
    (1..=100).contains(&len)
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
}

fn is_valid_ref(value: &str) -> bool {
    let len = value.len();
    if !(1..=255).contains(&len) {
        return false;
    }
    if value.contains("..")
        || value.contains('~')
        || value.contains('^')
        || value.contains(':')
        || value.contains('\\')
    {
        return false;
    }
    if value.starts_with('/') || value.ends_with('/') || value.contains("//") {
        return false;
    }

    value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-' || b == b'/')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_hail_jsonl() -> String {
        [
            r#"{"type":"header","version":"hail-1.0.0","session_id":"s1","agent":{"provider":"openai","model":"gpt-5","tool":"codex"},"context":{"title":"t","description":"d","tags":[],"created_at":"2026-02-01T00:00:00Z","updated_at":"2026-02-01T00:00:00Z","related_session_ids":[],"attributes":{}}}"#,
            r#"{"type":"event","event_id":"e1","timestamp":"2026-02-01T00:00:00Z","event_type":{"type":"UserMessage"},"content":{"blocks":[{"type":"Text","text":"hello"}]},"attributes":{}}"#,
            r#"{"type":"stats","event_count":1,"message_count":1,"tool_call_count":0,"task_count":0,"duration_seconds":0,"total_input_tokens":0,"total_output_tokens":0,"user_message_count":1,"files_changed":0,"lines_added":0,"lines_removed":0}"#,
        ]
        .join("\n")
    }

    #[test]
    fn parse_hail_preview_from_jsonl() {
        let result = preview_parse_bytes("session.hail.jsonl", minimal_hail_jsonl().as_bytes(), None)
            .expect("hail jsonl should parse");
        assert_eq!(result.parser_used, "hail");
        assert!(result.parser_candidates.iter().any(|candidate| candidate.id == "hail"));
    }

    #[test]
    fn parser_hint_for_non_hail_returns_error() {
        let err = preview_parse_bytes("session.jsonl", b"{}", Some("codex"))
            .expect_err("non-hail parser hint should fail on worker");
        assert_eq!(err.code, "parse_failed");
        assert!(err.message.contains("not available"));
    }

    #[test]
    fn normalize_repo_path_rejects_parent_segments() {
        let err = normalize_repo_path("../secret").expect_err("path traversal must fail");
        assert_eq!(err.code, "invalid_source");
    }

    #[test]
    fn decode_inline_content_rejects_empty_input() {
        let err = decode_inline_content("   ").expect_err("empty inline payload must fail");
        assert_eq!(err.code, "invalid_source");
    }

    #[test]
    fn build_git_raw_url_uses_provider_aware_patterns() {
        let github = build_git_raw_url(&GitSource {
            remote: "https://github.com/hwisu/opensession".to_string(),
            r#ref: "main".to_string(),
            path: "sessions/demo.hail.jsonl".to_string(),
        })
        .expect("github raw url should build");
        assert_eq!(
            github,
            "https://raw.githubusercontent.com/hwisu/opensession/main/sessions/demo.hail.jsonl"
        );

        let gitlab = build_git_raw_url(&GitSource {
            remote: "https://gitlab.com/group/subgroup/repo".to_string(),
            r#ref: "main".to_string(),
            path: "sessions/demo.hail.jsonl".to_string(),
        })
        .expect("gitlab raw url should build");
        assert_eq!(
            gitlab,
            "https://gitlab.com/group/subgroup/repo/-/raw/main/sessions/demo.hail.jsonl"
        );

        let gitlab_self_managed = build_git_raw_url(&GitSource {
            remote: "https://gitlab.internal.example.com/group/subgroup/repo.git".to_string(),
            r#ref: "main".to_string(),
            path: "sessions/demo.hail.jsonl".to_string(),
        })
        .expect("self-managed gitlab raw url should build");
        assert_eq!(
            gitlab_self_managed,
            "https://gitlab.internal.example.com/group/subgroup/repo/-/raw/main/sessions/demo.hail.jsonl"
        );

        let generic = build_git_raw_url(&GitSource {
            remote: "https://code.example.com/team/repo.git".to_string(),
            r#ref: "main".to_string(),
            path: "sessions/demo.hail.jsonl".to_string(),
        })
        .expect("generic raw url should build");
        assert_eq!(
            generic,
            "https://code.example.com/team/repo/raw/main/sessions/demo.hail.jsonl"
        );
    }

    #[test]
    fn provider_detection_and_path_prefix_matching_are_stable() {
        assert_eq!(provider_for_host("github.com"), Some("github"));
        assert_eq!(provider_for_host("gitlab.com"), Some("gitlab"));
        assert_eq!(provider_for_host("gitlab.internal.example.com"), Some("gitlab"));
        assert_eq!(provider_for_host("code.example.com"), None);

        assert!(path_prefix_matches("group/sub/repo", ""));
        assert!(path_prefix_matches("group/sub/repo", "group/sub"));
        assert!(path_prefix_matches("group/sub/repo", "group/sub/repo"));
        assert!(!path_prefix_matches("group/sub/repo", "group/su"));
        assert!(!path_prefix_matches("group/sub/repo", "group/sub/repo2"));
    }

    #[test]
    fn git_credential_error_codes_are_stable() {
        let missing = PreviewRouteError::missing_git_credential(401);
        assert_eq!(missing.code, "missing_git_credential");
        assert_eq!(missing.status, 401);

        let forbidden = PreviewRouteError::git_credential_forbidden(403);
        assert_eq!(forbidden.code, "git_credential_forbidden");
        assert_eq!(forbidden.status, 403);
    }
}
