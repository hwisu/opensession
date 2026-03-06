use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
};
#[cfg(test)]
use base64::Engine;
use opensession_api::{
    ParseCandidate as ApiParseCandidate, ParsePreviewErrorResponse, ParsePreviewRequest,
    ParsePreviewResponse, ParseSource, db as dbq,
    parse_preview_source::{self as preview_source, GitSource, GithubSource},
};
use opensession_parsers::ingest::{self as parser_ingest, ParseError as ParserParseError};
use reqwest::header::CONTENT_TYPE;

use crate::AppConfig;
use crate::storage::{Db, sq_execute, sq_query_map, sq_query_row};

const FETCH_TIMEOUT_SECS: u64 = 10;
const MAX_SOURCE_SIZE_BYTES: usize = 10 * 1024 * 1024;

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
    fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: message.into(),
            parser_candidates: Vec::new(),
        }
    }

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

    fn missing_git_credential(status_code: u16) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "missing_git_credential",
            message: format!(
                "remote source returned status {status_code}; connect provider OAuth or register a git credential for this host"
            ),
            parser_candidates: Vec::new(),
        }
    }

    fn git_credential_forbidden(status_code: u16) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: "git_credential_forbidden",
            message: format!(
                "configured credential was rejected by remote source (status {status_code})"
            ),
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

/// POST /api/parse/preview
pub async fn preview(
    State(db): State<Db>,
    State(config): State<AppConfig>,
    headers: HeaderMap,
    Json(req): Json<ParsePreviewRequest>,
) -> Result<Json<ParsePreviewResponse>, (StatusCode, Json<ParsePreviewErrorResponse>)> {
    let user_id =
        resolve_optional_user_id(&headers, &db, &config).map_err(PreviewRouteError::into_http)?;
    let input =
        prepare_parse_input_with_ctx(req.source, Some(&db), Some(&config), user_id.as_deref())
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

fn resolve_optional_user_id(
    headers: &HeaderMap,
    db: &Db,
    config: &AppConfig,
) -> Result<Option<String>, PreviewRouteError> {
    super::auth::try_auth_from_headers(headers, db, config)
        .map(|user| user.map(|row| row.user_id))
        .map_err(|_| PreviewRouteError::unauthorized("invalid authorization token"))
}

#[cfg(test)]
async fn prepare_parse_input(source: ParseSource) -> Result<ParseInput, PreviewRouteError> {
    prepare_parse_input_with_ctx(source, None, None, None).await
}

async fn prepare_parse_input_with_ctx(
    source: ParseSource,
    db: Option<&Db>,
    config: Option<&AppConfig>,
    user_id: Option<&str>,
) -> Result<ParseInput, PreviewRouteError> {
    match source {
        ParseSource::Git {
            remote,
            r#ref,
            path,
        } => {
            let normalized = normalize_git_source(&remote, &r#ref, &path)?;
            let db = db.ok_or_else(|| {
                PreviewRouteError::fetch_failed("git source fetch is unavailable in this context")
            })?;
            let config = config.ok_or_else(|| {
                PreviewRouteError::fetch_failed("git source fetch is unavailable in this context")
            })?;
            let bytes = fetch_git_source(&normalized, db, config, user_id).await?;
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
            let db = db.ok_or_else(|| {
                PreviewRouteError::fetch_failed(
                    "github source fetch is unavailable in this context",
                )
            })?;
            let config = config.ok_or_else(|| {
                PreviewRouteError::fetch_failed(
                    "github source fetch is unavailable in this context",
                )
            })?;
            let bytes = fetch_git_source(
                &GitSource {
                    remote: format!(
                        "https://github.com/{}/{}",
                        normalized.owner, normalized.repo
                    ),
                    r#ref: normalized.r#ref.clone(),
                    path: normalized.path.clone(),
                },
                db,
                config,
                user_id,
            )
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
    preview_source::normalize_git_source(remote, r#ref, path)
        .map_err(|error| PreviewRouteError::invalid_source(error.message()))
}

fn normalize_github_source(
    owner: &str,
    repo: &str,
    r#ref: &str,
    path: &str,
) -> Result<GithubSource, PreviewRouteError> {
    preview_source::normalize_github_source(owner, repo, r#ref, path)
        .map_err(|error| PreviewRouteError::invalid_source(error.message()))
}

fn normalize_filename(filename: &str) -> Result<String, PreviewRouteError> {
    preview_source::normalize_filename(filename)
        .map_err(|error| PreviewRouteError::invalid_source(error.message()))
}

fn decode_inline_content(content_base64: &str) -> Result<Vec<u8>, PreviewRouteError> {
    preview_source::decode_inline_content(content_base64)
        .map_err(|error| PreviewRouteError::invalid_source(error.message()))
}

fn file_name_from_path(path: &str) -> String {
    preview_source::file_name_from_path(path)
}

fn validate_remote_url(remote: &str) -> Result<reqwest::Url, PreviewRouteError> {
    let parsed = reqwest::Url::parse(remote)
        .map_err(|_| PreviewRouteError::invalid_source("remote must be an absolute http(s) URL"))?;

    let scheme = parsed.scheme().to_ascii_lowercase();
    if scheme != "https" {
        return Err(PreviewRouteError::invalid_source("remote must use https"));
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

fn oauth_provider_host_from_url(raw: &str) -> Option<String> {
    reqwest::Url::parse(raw)
        .ok()
        .and_then(|url| url.host_str().map(|value| value.to_ascii_lowercase()))
}

fn configured_gitlab_hosts(config: &AppConfig) -> HashSet<String> {
    config
        .oauth_providers
        .iter()
        .filter(|provider| provider.id == "gitlab")
        .filter_map(|provider| oauth_provider_host_from_url(&provider.token_url))
        .collect()
}

fn is_gitlab_host(host: &str, gitlab_hosts: &HashSet<String>) -> bool {
    host == "gitlab.com" || gitlab_hosts.contains(host)
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

fn encode_segments(value: &str) -> String {
    value
        .split('/')
        .map(|segment| urlencoding::encode(segment).into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn strip_git_suffix(value: &str) -> &str {
    value.strip_suffix(".git").unwrap_or(value)
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

fn build_git_raw_url(
    source: &GitSource,
    gitlab_hosts: &HashSet<String>,
) -> Result<String, PreviewRouteError> {
    let remote = validate_remote_url(&source.remote)?;
    let host = remote
        .host_str()
        .ok_or_else(|| PreviewRouteError::invalid_source("remote host is required"))?
        .to_ascii_lowercase();

    if host == "github.com" {
        return build_github_raw_url(&remote, &source.r#ref, &source.path);
    }
    if is_gitlab_host(&host, gitlab_hosts) {
        return build_gitlab_raw_url(&remote, &source.r#ref, &source.path);
    }

    build_generic_raw_url(&remote, &source.r#ref, &source.path)
}

#[derive(Debug, Clone)]
enum GitCredentialSource {
    Provider,
    Manual {
        credential_id: String,
        user_id: String,
    },
}

#[derive(Debug, Clone)]
struct GitFetchAuthHeader {
    header_name: String,
    header_value: String,
    source: GitCredentialSource,
}

fn provider_for_host(host: &str, gitlab_hosts: &HashSet<String>) -> Option<&'static str> {
    if host == "github.com" {
        return Some("github");
    }
    if is_gitlab_host(host, gitlab_hosts) {
        return Some("gitlab");
    }
    None
}

fn repo_path_from_remote(url: &reqwest::Url) -> Result<String, PreviewRouteError> {
    Ok(repo_path_segments(url)?.join("/"))
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

async fn ensure_remote_resolves_public(remote: &reqwest::Url) -> Result<(), PreviewRouteError> {
    let host = remote
        .host_str()
        .ok_or_else(|| PreviewRouteError::invalid_source("remote host is required"))?;
    if host.parse::<IpAddr>().is_ok() {
        return Ok(());
    }

    let port = remote.port_or_known_default().unwrap_or(443);
    let mut resolved = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| PreviewRouteError::fetch_failed("remote host DNS lookup failed"))?;
    let mut found_any = false;
    for addr in &mut resolved {
        found_any = true;
        let ip = addr.ip();
        let disallowed = match ip {
            IpAddr::V4(v4) => is_disallowed_ipv4(v4),
            IpAddr::V6(v6) => is_disallowed_ipv6(v6),
        };
        if disallowed {
            tracing::warn!(host = %host, ip = %ip, "blocked remote host resolving to disallowed IP");
            return Err(PreviewRouteError::invalid_source(
                "remote host resolves to a disallowed address",
            ));
        }
    }
    if !found_any {
        return Err(PreviewRouteError::fetch_failed(
            "remote host DNS lookup returned no addresses",
        ));
    }
    Ok(())
}

fn resolve_fetch_auth_header(
    source: &GitSource,
    db: &Db,
    config: &AppConfig,
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
    let repo_path = repo_path_from_remote(&remote)?;
    let conn = db.conn();
    let gitlab_hosts = configured_gitlab_hosts(config);

    if let Some(provider) = provider_for_host(&host, &gitlab_hosts) {
        let provider_token_enc: Option<String> = sq_query_row(
            &conn,
            dbq::oauth_provider_tokens::get_by_user_provider_host(user_id, provider, &host),
            |row| row.get(1),
        )
        .ok();
        if let Some(enc) = provider_token_enc {
            let token = keyring.decrypt(&enc).map_err(|_| {
                PreviewRouteError::fetch_failed("failed to decrypt provider credential")
            })?;
            return Ok(Some(GitFetchAuthHeader {
                header_name: "Authorization".to_string(),
                header_value: format!("Bearer {token}"),
                source: GitCredentialSource::Provider,
            }));
        }
    }

    let manual_rows: Vec<(String, String, String, String)> = sq_query_map(
        &conn,
        dbq::git_credentials::list_for_host_with_secret(user_id, &host),
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        },
    )
    .map_err(|_| PreviewRouteError::fetch_failed("failed loading git credentials"))?;

    for (credential_id, path_prefix, header_name, secret_enc) in manual_rows {
        if !path_prefix_matches(&repo_path, &path_prefix) {
            continue;
        }
        let secret = keyring
            .decrypt(&secret_enc)
            .map_err(|_| PreviewRouteError::fetch_failed("failed to decrypt git credential"))?;
        return Ok(Some(GitFetchAuthHeader {
            header_name,
            header_value: secret,
            source: GitCredentialSource::Manual {
                credential_id,
                user_id: user_id.to_string(),
            },
        }));
    }

    Ok(None)
}

async fn fetch_git_source(
    source: &GitSource,
    db: &Db,
    config: &AppConfig,
    user_id: Option<&str>,
) -> Result<Vec<u8>, PreviewRouteError> {
    let remote = validate_remote_url(&source.remote)?;
    ensure_remote_resolves_public(&remote).await?;
    let gitlab_hosts = configured_gitlab_hosts(config);
    let url = build_git_raw_url(source, &gitlab_hosts)?;
    let fetch_auth = resolve_fetch_auth_header(source, db, config, user_id)?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(FETCH_TIMEOUT_SECS))
        .build()
        .map_err(|_| PreviewRouteError::fetch_failed("failed to initialize fetch client"))?;

    let mut request = client
        .get(&url)
        .header(reqwest::header::USER_AGENT, "opensession-ingest-preview");
    if let Some(auth_header) = fetch_auth.as_ref() {
        request = request.header(
            auth_header.header_name.as_str(),
            auth_header.header_value.as_str(),
        );
    }

    let response = request
        .send()
        .await
        .map_err(|_| PreviewRouteError::fetch_failed("failed to fetch source"))?;

    if response.status().is_redirection() {
        tracing::warn!(url = %url, status = %response.status(), "blocked redirect response from remote source");
        return Err(PreviewRouteError::fetch_failed(
            "redirect responses are not allowed",
        ));
    }

    if matches!(response.status().as_u16(), 401 | 403) && user_id.is_some() {
        tracing::warn!(
            url = %url,
            status = response.status().as_u16(),
            has_auth = fetch_auth.is_some(),
            "remote source authentication failed"
        );
        return Err(match fetch_auth {
            Some(_) => PreviewRouteError::git_credential_forbidden(response.status().as_u16()),
            None => PreviewRouteError::missing_git_credential(response.status().as_u16()),
        });
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
        .map_err(|_| PreviewRouteError::fetch_failed("failed reading source body"))?;

    if body.len() > MAX_SOURCE_SIZE_BYTES {
        return Err(PreviewRouteError::file_too_large(body.len()));
    }

    if looks_binary(body.as_ref()) {
        return Err(PreviewRouteError::fetch_failed(
            "binary files are not supported",
        ));
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
        let conn = db.conn();
        let _ = sq_execute(
            &conn,
            dbq::git_credentials::touch_last_used(credential_id.as_str(), user_id.as_str()),
        );
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
        let no_gitlab_hosts = HashSet::new();
        let mut configured_gitlab_hosts = HashSet::new();
        configured_gitlab_hosts.insert("gitlab.internal.example.com".to_string());

        let github = build_git_raw_url(
            &GitSource {
                remote: "https://github.com/hwisu/opensession".to_string(),
                r#ref: "main".to_string(),
                path: "sessions/demo.hail.jsonl".to_string(),
            },
            &no_gitlab_hosts,
        )
        .expect("github raw url should build");
        assert_eq!(
            github,
            "https://raw.githubusercontent.com/hwisu/opensession/main/sessions/demo.hail.jsonl"
        );

        let gitlab = build_git_raw_url(
            &GitSource {
                remote: "https://gitlab.com/group/subgroup/repo".to_string(),
                r#ref: "main".to_string(),
                path: "sessions/demo.hail.jsonl".to_string(),
            },
            &no_gitlab_hosts,
        )
        .expect("gitlab raw url should build");
        assert_eq!(
            gitlab,
            "https://gitlab.com/group/subgroup/repo/-/raw/main/sessions/demo.hail.jsonl"
        );

        let gitlab_self_managed = build_git_raw_url(
            &GitSource {
                remote: "https://gitlab.internal.example.com/group/subgroup/repo.git".to_string(),
                r#ref: "main".to_string(),
                path: "sessions/demo.hail.jsonl".to_string(),
            },
            &configured_gitlab_hosts,
        )
        .expect("self-managed gitlab raw url should build");
        assert_eq!(
            gitlab_self_managed,
            "https://gitlab.internal.example.com/group/subgroup/repo/-/raw/main/sessions/demo.hail.jsonl"
        );

        let generic = build_git_raw_url(
            &GitSource {
                remote: "https://code.example.com/team/repo.git".to_string(),
                r#ref: "main".to_string(),
                path: "sessions/demo.hail.jsonl".to_string(),
            },
            &no_gitlab_hosts,
        )
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

    #[test]
    fn provider_detection_and_path_prefix_matching_are_stable() {
        let no_gitlab_hosts = HashSet::new();
        let mut configured_gitlab_hosts = HashSet::new();
        configured_gitlab_hosts.insert("gitlab.internal.example.com".to_string());

        assert_eq!(
            provider_for_host("github.com", &no_gitlab_hosts),
            Some("github")
        );
        assert_eq!(
            provider_for_host("gitlab.com", &no_gitlab_hosts),
            Some("gitlab")
        );
        assert_eq!(
            provider_for_host("gitlab.internal.example.com", &no_gitlab_hosts),
            None
        );
        assert_eq!(
            provider_for_host("gitlab.internal.example.com", &configured_gitlab_hosts),
            Some("gitlab")
        );
        assert_eq!(
            provider_for_host("evil-gitlab.example", &no_gitlab_hosts),
            None
        );
        assert_eq!(
            provider_for_host("code.example.com", &no_gitlab_hosts),
            None
        );

        assert!(path_prefix_matches("group/sub/repo", ""));
        assert!(path_prefix_matches("group/sub/repo", "group/sub"));
        assert!(path_prefix_matches("group/sub/repo", "group/sub/repo"));
        assert!(!path_prefix_matches("group/sub/repo", "group/su"));
        assert!(!path_prefix_matches("group/sub/repo", "group/sub/repo2"));
    }

    #[test]
    fn git_source_rejects_http_remote() {
        let err = normalize_git_source(
            "http://code.example.com/team/repo",
            "main",
            "sessions/demo.hail.jsonl",
        )
        .expect_err("http remote must be rejected");
        assert_eq!(err.code, "invalid_source");
    }

    #[test]
    fn git_credential_error_codes_are_stable() {
        let missing = PreviewRouteError::missing_git_credential(401);
        assert_eq!(missing.code, "missing_git_credential");
        assert_eq!(missing.status, StatusCode::UNAUTHORIZED);

        let forbidden = PreviewRouteError::git_credential_forbidden(403);
        assert_eq!(forbidden.code, "git_credential_forbidden");
        assert_eq!(forbidden.status, StatusCode::FORBIDDEN);
    }
}
