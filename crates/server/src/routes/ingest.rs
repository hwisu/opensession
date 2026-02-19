use std::time::Duration;

use axum::{http::StatusCode, Json};
use base64::Engine;
use opensession_api::{
    ParseCandidate as ApiParseCandidate, ParsePreviewErrorResponse, ParsePreviewRequest,
    ParsePreviewResponse, ParseSource,
};
use opensession_parsers::ingest::{self as parser_ingest, ParseError as ParserParseError};
use reqwest::header::CONTENT_TYPE;

const GITHUB_RAW_BASE: &str = "https://raw.githubusercontent.com";
const FETCH_TIMEOUT_SECS: u64 = 10;
const MAX_SOURCE_SIZE_BYTES: usize = 10 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
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
        ParseSource::Github {
            owner,
            repo,
            r#ref,
            path,
        } => {
            let normalized = normalize_github_source(&owner, &repo, &r#ref, &path)?;
            let bytes = fetch_github_source(&normalized).await?;
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

fn build_raw_url(source: &GithubSource) -> String {
    format!(
        "{}/{}/{}/{}/{}",
        GITHUB_RAW_BASE,
        source.owner,
        source.repo,
        encode_segments(&source.r#ref),
        encode_segments(&source.path),
    )
}

async fn fetch_github_source(source: &GithubSource) -> Result<Vec<u8>, PreviewRouteError> {
    let url = build_raw_url(source);
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
        assert_eq!(
            build_raw_url(&source),
            "https://raw.githubusercontent.com/hwisu/opensession/main/sessions/foo%20bar.hail.jsonl"
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
