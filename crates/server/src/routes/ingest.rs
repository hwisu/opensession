use std::net::{IpAddr, Ipv6Addr};
use std::time::Duration;

use axum::{http::StatusCode, Json};
use base64::Engine;
use opensession_api::{
    ParseCandidate as ApiParseCandidate, ParsePreviewErrorResponse, ParsePreviewRequest,
    ParsePreviewResponse, ParseSource,
};
use opensession_parsers::ingest::{self as parser_ingest, ParseError as ParserParseError};
use reqwest::header::CONTENT_TYPE;

const FETCH_TIMEOUT_SECS: u64 = 10;
const MAX_SOURCE_SIZE_BYTES: usize = 10 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
struct GithubSource {
    owner: String,
    repo: String,
    r#ref: String,
    path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GitSource {
    remote: String,
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
struct PreviewRouteError {
    status: StatusCode,
    code: &'static str,
    message: String,
    parser_candidates: Vec<ApiParseCandidate>,
}

impl PreviewRouteError {
    fn invalid_source(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "invalid_source",
            message: message.into(),
            parser_candidates: Vec::new(),
        }
    }

    fn fetch_failed(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "fetch_failed",
            message: message.into(),
            parser_candidates: Vec::new(),
        }
    }

    fn file_too_large(size: usize) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "file_too_large",
            message: format!(
                "source is too large ({} bytes, max {} bytes)",
                size, MAX_SOURCE_SIZE_BYTES
            ),
            parser_candidates: Vec::new(),
        }
    }

    fn parse_failed(
        message: impl Into<String>,
        parser_candidates: Vec<parser_ingest::ParseCandidate>,
    ) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "parse_failed",
            message: message.into(),
            parser_candidates: to_api_candidates(parser_candidates),
        }
    }

    fn parser_selection_required(
        message: impl Into<String>,
        parser_candidates: Vec<parser_ingest::ParseCandidate>,
    ) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "parser_selection_required",
            message: message.into(),
            parser_candidates: to_api_candidates(parser_candidates),
        }
    }

    fn into_http(self) -> (StatusCode, Json<ParsePreviewErrorResponse>) {
        (
            self.status,
            Json(ParsePreviewErrorResponse {
                code: self.code.to_string(),
                message: self.message,
                parser_candidates: self.parser_candidates,
            }),
        )
    }
}

/// POST /api/ingest/preview
pub async fn preview(
    Json(req): Json<ParsePreviewRequest>,
) -> Result<Json<ParsePreviewResponse>, (StatusCode, Json<ParsePreviewErrorResponse>)> {
    let input = prepare_parse_input(req.source)
        .await
        .map_err(PreviewRouteError::into_http)?;

    let preview = parser_ingest::preview_parse_bytes(
        &input.filename,
        &input.bytes,
        req.parser_hint.as_deref(),
    )
    .map_err(map_parser_error)
    .map_err(PreviewRouteError::into_http)?;

    Ok(Json(ParsePreviewResponse {
        parser_used: preview.parser_used,
        parser_candidates: to_api_candidates(preview.parser_candidates),
        session: preview.session,
        source: input.source,
        warnings: preview.warnings,
        native_adapter: preview.native_adapter,
    }))
}

fn map_parser_error(err: ParserParseError) -> PreviewRouteError {
    match err {
        ParserParseError::InvalidParserHint { hint } => {
            PreviewRouteError::invalid_source(format!("unsupported parser_hint '{hint}'"))
        }
        ParserParseError::ParserSelectionRequired {
            message,
            parser_candidates,
        } => PreviewRouteError::parser_selection_required(message, parser_candidates),
        ParserParseError::ParseFailed {
            message,
            parser_candidates,
        } => PreviewRouteError::parse_failed(message, parser_candidates),
    }
}

async fn prepare_parse_input(source: ParseSource) -> Result<ParseInput, PreviewRouteError> {
    match source {
        ParseSource::Git {
            remote,
            r#ref,
            path,
        } => {
            let normalized = normalize_git_source(&remote, &r#ref, &path)?;
            let bytes = fetch_git_source(&normalized).await?;
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
            })
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
                    // Do not echo full inline payload in preview responses.
                    content_base64: String::new(),
                },
                filename,
                bytes,
            })
        }
    }
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

fn encode_segments(value: &str) -> String {
    value
        .split('/')
        .map(|segment| urlencoding::encode(segment).into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn strip_git_suffix(segment: &str) -> &str {
    segment.strip_suffix(".git").unwrap_or(segment)
}

fn validate_remote_url(remote: &str) -> Result<reqwest::Url, PreviewRouteError> {
    let parsed = reqwest::Url::parse(remote)
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

fn normalized_remote_origin(url: &reqwest::Url) -> String {
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
        Ok(IpAddr::V4(v4)) => {
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_multicast()
                || v4.is_unspecified()
        }
        Ok(IpAddr::V6(v6)) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
                || v6.is_multicast()
                || is_ipv6_documentation(v6)
        }
        Err(_) => false,
    }
}

fn is_ipv6_documentation(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    segments[0] == 0x2001 && segments[1] == 0x0db8
}

fn origin_from_url(url: &reqwest::Url) -> Result<String, PreviewRouteError> {
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

fn repo_path_segments(url: &reqwest::Url) -> Result<Vec<String>, PreviewRouteError> {
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

fn build_github_raw_url(
    url: &reqwest::Url,
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
    url: &reqwest::Url,
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
    url: &reqwest::Url,
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

async fn fetch_git_source(source: &GitSource) -> Result<Vec<u8>, PreviewRouteError> {
    let url = build_git_raw_url(source)?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(FETCH_TIMEOUT_SECS))
        .build()
        .map_err(|e| {
            PreviewRouteError::fetch_failed(format!("failed to initialize fetch client: {e}"))
        })?;

    let response = client
        .get(url)
        .header(reqwest::header::USER_AGENT, "opensession-ingest-preview")
        .send()
        .await
        .map_err(|e| PreviewRouteError::fetch_failed(format!("failed to fetch source: {e}")))?;

    if response.status().is_redirection() {
        return Err(PreviewRouteError::fetch_failed(
            "redirect responses are not allowed",
        ));
    }

    if !response.status().is_success() {
        return Err(PreviewRouteError::fetch_failed(format!(
            "source fetch failed with status {}",
            response.status().as_u16()
        )));
    }

    if let Some(content_len) = response.content_length() {
        if content_len > MAX_SOURCE_SIZE_BYTES as u64 {
            return Err(PreviewRouteError::file_too_large(content_len as usize));
        }
    }

    if let Some(content_type) = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
    {
        if !is_allowed_content_type(content_type) {
            return Err(PreviewRouteError::fetch_failed(format!(
                "unsupported content-type '{content_type}', expected text/json content",
            )));
        }
    }

    let body = response
        .bytes()
        .await
        .map_err(|e| PreviewRouteError::fetch_failed(format!("failed reading source body: {e}")))?;

    if body.len() > MAX_SOURCE_SIZE_BYTES {
        return Err(PreviewRouteError::file_too_large(body.len()));
    }

    if looks_binary(body.as_ref()) {
        return Err(PreviewRouteError::fetch_failed(
            "binary files are not supported",
        ));
    }

    Ok(body.to_vec())
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
    if bytes.contains(&0) {
        return true;
    }
    std::str::from_utf8(bytes).is_err()
}

fn to_api_candidates(candidates: Vec<parser_ingest::ParseCandidate>) -> Vec<ApiParseCandidate> {
    candidates
        .into_iter()
        .map(|candidate| ApiParseCandidate {
            id: candidate.id,
            confidence: candidate.confidence,
            reason: candidate.reason,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use opensession_api::ParsePreviewRequest;

    fn parse_request_for_inline(filename: &str, raw: &[u8]) -> ParsePreviewRequest {
        ParsePreviewRequest {
            source: ParseSource::Inline {
                filename: filename.to_string(),
                content_base64: base64::engine::general_purpose::STANDARD.encode(raw),
            },
            parser_hint: None,
        }
    }

    #[test]
    fn github_source_rejects_invalid_owner() {
        let err = normalize_github_source("bad owner!", "repo", "main", "sessions/a.hail.jsonl")
            .expect_err("owner validation must fail");
        assert_eq!(err.code, "invalid_source");
    }

    #[test]
    fn github_source_rejects_invalid_ref() {
        let err = normalize_github_source("owner", "repo", "main:prod", "sessions/a.hail.jsonl")
            .expect_err("ref validation must fail");
        assert_eq!(err.code, "invalid_source");
    }

    #[test]
    fn github_source_rejects_path_traversal_segments() {
        let err = normalize_github_source("owner", "repo", "main", "sessions/../a.hail.jsonl")
            .expect_err("path traversal must fail");
        assert_eq!(err.code, "invalid_source");
    }

    #[test]
    fn github_source_accepts_normalized_path() {
        let source = normalize_github_source(
            "hwisu",
            "opensession",
            "main",
            "sessions/foo%20bar.hail.jsonl",
        )
        .expect("source should be valid");
        assert_eq!(source.path, "sessions/foo bar.hail.jsonl");
        let remote = reqwest::Url::parse("https://github.com/hwisu/opensession")
            .expect("remote url should parse");
        assert_eq!(
            build_github_raw_url(&remote, &source.r#ref, &source.path)
                .expect("github raw url should build"),
            "https://raw.githubusercontent.com/hwisu/opensession/main/sessions/foo%20bar.hail.jsonl"
        );
    }

    #[test]
    fn git_source_rejects_localhost_remote() {
        let err = normalize_git_source(
            "http://localhost:3000/hwisu/opensession",
            "main",
            "sessions/demo.hail.jsonl",
        )
        .expect_err("localhost remote must be rejected");
        assert_eq!(err.code, "invalid_source");
    }

    #[test]
    fn git_source_rejects_private_ip_remote() {
        let err = normalize_git_source(
            "http://192.168.0.10/hwisu/opensession",
            "main",
            "sessions/demo.hail.jsonl",
        )
        .expect_err("private ip remote must be rejected");
        assert_eq!(err.code, "invalid_source");
    }

    #[test]
    fn git_source_rejects_credentials_and_query() {
        let with_credentials = normalize_git_source(
            "https://user:secret@example.com/org/repo",
            "main",
            "sessions/demo.hail.jsonl",
        )
        .expect_err("remote credentials must be rejected");
        assert_eq!(with_credentials.code, "invalid_source");

        let with_query = normalize_git_source(
            "https://example.com/org/repo?token=1",
            "main",
            "sessions/demo.hail.jsonl",
        )
        .expect_err("remote query must be rejected");
        assert_eq!(with_query.code, "invalid_source");
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

    #[tokio::test]
    async fn inline_source_too_large_returns_file_too_large() {
        let oversized = vec![b'a'; MAX_SOURCE_SIZE_BYTES + 1];
        let req = parse_request_for_inline("session.hail.jsonl", &oversized);
        let err = prepare_parse_input(req.source)
            .await
            .expect_err("oversized inline source should fail");
        assert_eq!(err.code, "file_too_large");
        assert_eq!(err.status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[test]
    fn parser_selection_required_maps_to_expected_code() {
        let mapped = map_parser_error(ParserParseError::ParserSelectionRequired {
            message: "select parser".to_string(),
            parser_candidates: vec![parser_ingest::ParseCandidate {
                id: "codex".to_string(),
                confidence: 91,
                reason: "fixture".to_string(),
            }],
        });

        assert_eq!(mapped.code, "parser_selection_required");
        assert_eq!(mapped.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(mapped.parser_candidates.len(), 1);
        assert_eq!(mapped.parser_candidates[0].id, "codex");
    }

    #[tokio::test]
    async fn parse_failed_maps_to_expected_code() {
        let req = parse_request_for_inline("unknown.txt", b"not jsonl");
        let input = prepare_parse_input(req.source)
            .await
            .expect("inline source should decode");

        let err = parser_ingest::preview_parse_bytes(&input.filename, &input.bytes, None)
            .expect_err("unrecognized source should fail parsing");
        let mapped = map_parser_error(err);

        assert_eq!(mapped.code, "parse_failed");
        assert_eq!(mapped.status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[test]
    fn invalid_parser_hint_maps_to_invalid_source() {
        let mapped = map_parser_error(ParserParseError::InvalidParserHint {
            hint: "nope".to_string(),
        });
        assert_eq!(mapped.code, "invalid_source");
        assert_eq!(mapped.status, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn content_type_validation_accepts_text_and_json() {
        assert!(is_allowed_content_type("text/plain; charset=utf-8"));
        assert!(is_allowed_content_type("application/json"));
        assert!(is_allowed_content_type("application/x-ndjson"));
        assert!(!is_allowed_content_type("application/octet-stream"));
    }
}
