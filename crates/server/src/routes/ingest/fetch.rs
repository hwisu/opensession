use std::time::Duration;

use opensession_api::parse_preview_source::GitSource;
use reqwest::header::CONTENT_TYPE;

use crate::AppConfig;
use crate::storage::Db;

use super::auth::{GitCredentialSource, GitFetchAuthHeader, resolve_fetch_auth_header};
use super::errors::PreviewRouteError;
use super::remote::{
    build_git_raw_url, configured_gitlab_hosts, ensure_remote_resolves_public, validate_remote_url,
};
use super::{FETCH_TIMEOUT_SECS, MAX_SOURCE_SIZE_BYTES};

pub(super) async fn fetch_git_source(
    source: &GitSource,
    db: &Db,
    config: &AppConfig,
    user_id: Option<&str>,
) -> Result<Vec<u8>, PreviewRouteError> {
    let remote = validate_remote_url(&source.remote)?;
    ensure_remote_resolves_public(&remote).await?;
    let gitlab_hosts = configured_gitlab_hosts(config);
    let url = build_git_raw_url(source, &gitlab_hosts)?;
    let fetch_auth = resolve_fetch_auth_header(source, db, config, user_id).await?;
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
        let _ = db
            .touch_git_credential_last_used(credential_id.as_str(), user_id.as_str())
            .await;
    }

    Ok(body.to_vec())
}

pub(super) fn is_allowed_content_type(content_type: &str) -> bool {
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
