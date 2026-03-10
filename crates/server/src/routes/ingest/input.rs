use opensession_api::{
    ParseSource,
    parse_preview_source::{self as preview_source, GitSource, GithubSource},
};

use crate::AppConfig;
use crate::storage::Db;

use super::MAX_SOURCE_SIZE_BYTES;
use super::errors::PreviewRouteError;
use super::fetch::fetch_git_source;

#[derive(Debug, Clone)]
pub(super) struct ParseInput {
    pub(super) source: ParseSource,
    pub(super) filename: String,
    pub(super) bytes: Vec<u8>,
}

#[cfg(test)]
pub(super) async fn prepare_parse_input(
    source: ParseSource,
) -> Result<ParseInput, PreviewRouteError> {
    prepare_parse_input_with_ctx(source, None, None, None).await
}

pub(super) async fn prepare_parse_input_with_ctx(
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

pub(super) fn normalize_git_source(
    remote: &str,
    r#ref: &str,
    path: &str,
) -> Result<GitSource, PreviewRouteError> {
    preview_source::normalize_git_source(remote, r#ref, path)
        .map_err(|error| PreviewRouteError::invalid_source(error.message()))
}

pub(super) fn normalize_github_source(
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

pub(super) fn file_name_from_path(path: &str) -> String {
    preview_source::file_name_from_path(path)
}
