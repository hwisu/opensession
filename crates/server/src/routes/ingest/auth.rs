use opensession_api::{db as dbq, parse_preview_source::GitSource};

use crate::AppConfig;
use crate::storage::{Db, sq_query_map, sq_query_row};

use super::errors::PreviewRouteError;
use super::remote::{
    configured_gitlab_hosts, path_prefix_matches, provider_for_host, repo_path_from_remote,
    validate_remote_url,
};

pub(super) fn resolve_optional_user_id(
    headers: &axum::http::HeaderMap,
    db: &Db,
    config: &AppConfig,
) -> Result<Option<String>, PreviewRouteError> {
    super::super::auth::try_auth_from_headers(headers, db, config)
        .map(|user| user.map(|row| row.user_id))
        .map_err(|_| PreviewRouteError::unauthorized("invalid authorization token"))
}

#[derive(Debug, Clone)]
pub(super) enum GitCredentialSource {
    Provider,
    Manual {
        credential_id: String,
        user_id: String,
    },
}

#[derive(Debug, Clone)]
pub(super) struct GitFetchAuthHeader {
    pub(super) header_name: String,
    pub(super) header_value: String,
    pub(super) source: GitCredentialSource,
}

pub(super) fn resolve_fetch_auth_header(
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
