use opensession_api::parse_preview_source::GitSource;

use crate::AppConfig;
use crate::storage::Db;

use super::errors::PreviewRouteError;
use super::remote::{
    configured_gitlab_hosts, path_prefix_matches, provider_for_host, repo_path_from_remote,
    validate_remote_url,
};

pub(super) async fn resolve_optional_user_id(
    headers: &axum::http::HeaderMap,
    db: &Db,
    config: &AppConfig,
) -> Result<Option<String>, PreviewRouteError> {
    super::super::auth::try_auth_from_headers(headers, db, config)
        .await
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

pub(super) async fn resolve_fetch_auth_header(
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
    let gitlab_hosts = configured_gitlab_hosts(config);

    if let Some(provider) = provider_for_host(&host, &gitlab_hosts) {
        let provider_token_enc = db
            .get_provider_token_enc(user_id, provider, &host)
            .await
            .map_err(|_| PreviewRouteError::fetch_failed("failed loading provider credential"))?;
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

    let manual_rows = db
        .list_git_credential_secrets_for_host(user_id, &host)
        .await
        .map_err(|_| PreviewRouteError::fetch_failed("failed loading git credentials"))?;

    for row in manual_rows {
        if !path_prefix_matches(&repo_path, &row.path_prefix) {
            continue;
        }
        let secret = keyring
            .decrypt(&row.header_value_enc)
            .map_err(|_| PreviewRouteError::fetch_failed("failed to decrypt git credential"))?;
        return Ok(Some(GitFetchAuthHeader {
            header_name: row.header_name,
            header_value: secret,
            source: GitCredentialSource::Manual {
                credential_id: row.credential_id,
                user_id: user_id.to_string(),
            },
        }));
    }

    Ok(None)
}
