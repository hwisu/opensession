use opensession_api::{
    CreateGitCredentialRequest, GitCredentialSummary, ListGitCredentialsResponse, OkResponse,
    ServiceError, db as dbq,
};
use serde::Deserialize;
use uuid::Uuid;
use worker::{Request, Response, Result, RouteContext};

use crate::error::IntoErrResponse;

use super::authenticate;
use super::support::{
    ServiceResult, d1_all, d1_first, d1_run, enforce_csrf_if_cookie_auth, json_response,
    parse_json,
};

#[derive(Debug, Deserialize)]
struct GitCredentialSummaryRow {
    id: String,
    label: String,
    host: String,
    path_prefix: String,
    header_name: String,
    created_at: String,
    updated_at: String,
    last_used_at: Option<String>,
}

fn normalize_header_name(raw: &str) -> ServiceResult<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ServiceError::BadRequest("header_name is required".into()));
    }
    if trimmed.len() > 64 {
        return Err(ServiceError::BadRequest(
            "header_name is too long (max 64 chars)".into(),
        ));
    }
    if !trimmed.bytes().all(|b| {
        b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'!' | b'#'
                    | b'$'
                    | b'%'
                    | b'&'
                    | b'\''
                    | b'*'
                    | b'+'
                    | b'-'
                    | b'.'
                    | b'^'
                    | b'_'
                    | b'`'
                    | b'|'
                    | b'~'
            )
    }) {
        return Err(ServiceError::BadRequest(
            "header_name contains invalid characters".into(),
        ));
    }
    Ok(trimmed.to_string())
}

fn normalize_host(raw: &str) -> ServiceResult<String> {
    let trimmed = raw.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return Err(ServiceError::BadRequest("host is required".into()));
    }
    if trimmed.len() > 255 {
        return Err(ServiceError::BadRequest(
            "host is too long (max 255 chars)".into(),
        ));
    }
    if trimmed.contains('/') || trimmed.contains(' ') {
        return Err(ServiceError::BadRequest(
            "host must not contain path separators or spaces".into(),
        ));
    }
    if trimmed
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':'))
    {
        return Ok(trimmed);
    }
    Err(ServiceError::BadRequest(
        "host contains invalid characters".into(),
    ))
}

fn normalize_path_prefix(raw: Option<&str>) -> ServiceResult<String> {
    let trimmed = raw.unwrap_or_default().trim().trim_matches('/').to_string();
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    if trimmed.len() > 512 {
        return Err(ServiceError::BadRequest(
            "path_prefix is too long (max 512 chars)".into(),
        ));
    }
    let mut segments = Vec::<String>::new();
    for part in trimmed.split('/') {
        let seg = part.trim();
        if seg.is_empty() || seg == "." || seg == ".." || seg.contains('\\') {
            return Err(ServiceError::BadRequest(
                "path_prefix contains invalid segments".into(),
            ));
        }
        segments.push(seg.to_string());
    }
    if let Some(last) = segments.last_mut() {
        *last = last.strip_suffix(".git").unwrap_or(last).to_string();
    }
    Ok(segments.join("/"))
}

pub async fn list_git_credentials(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let (config, d1) = match super::load_config_and_d1(&ctx) {
        Ok(values) => values,
        Err(err) => return err.into_err_response(),
    };

    let result: ServiceResult<ListGitCredentialsResponse> = async {
        let user = authenticate(&req, &d1, &config).await?;
        let rows: Vec<GitCredentialSummaryRow> = d1_all(
            &d1,
            dbq::git_credentials::list_by_user(&user.user_id),
            "list git credentials",
        )
        .await?;
        let credentials = rows
            .into_iter()
            .map(|row| GitCredentialSummary {
                id: row.id,
                label: row.label,
                host: row.host,
                path_prefix: row.path_prefix,
                header_name: row.header_name,
                created_at: row.created_at,
                updated_at: row.updated_at,
                last_used_at: row.last_used_at,
            })
            .collect();
        Ok(ListGitCredentialsResponse { credentials })
    }
    .await;

    match result {
        Ok(body) => json_response(&body, 200),
        Err(err) => err.into_err_response(),
    }
}

pub async fn create_git_credential(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let (config, d1) = match super::load_config_and_d1(&ctx) {
        Ok(values) => values,
        Err(err) => return err.into_err_response(),
    };

    let result: ServiceResult<GitCredentialSummary> = async {
        let user = authenticate(&req, &d1, &config).await?;
        enforce_csrf_if_cookie_auth(&req, &config, user.auth_via_cookie)?;
        let payload: CreateGitCredentialRequest = parse_json(&mut req).await?;

        let keyring = config.credential_keyring.as_ref().ok_or_else(|| {
            ServiceError::Internal("credential encryption is not configured".into())
        })?;

        let label = payload.label.trim().to_string();
        if label.is_empty() {
            return Err(ServiceError::BadRequest("label is required".into()));
        }
        if label.len() > 120 {
            return Err(ServiceError::BadRequest(
                "label is too long (max 120 chars)".into(),
            ));
        }
        let host = normalize_host(&payload.host)?;
        let path_prefix = normalize_path_prefix(payload.path_prefix.as_deref())?;
        let header_name = normalize_header_name(&payload.header_name)?;
        let header_value = payload.header_value.trim();
        if header_value.is_empty() {
            return Err(ServiceError::BadRequest("header_value is required".into()));
        }
        let header_value_enc = keyring.encrypt(header_value)?;

        let credential_id = Uuid::new_v4().to_string();
        d1_run(
            &d1,
            dbq::git_credentials::insert(
                &credential_id,
                &user.user_id,
                &label,
                &host,
                &path_prefix,
                &header_name,
                &header_value_enc,
            ),
            "insert git credential",
        )
        .await?;

        let row = d1_first::<GitCredentialSummaryRow>(
            &d1,
            dbq::git_credentials::get_by_id_and_user(&credential_id, &user.user_id),
            "reload git credential",
        )
        .await?
        .ok_or_else(|| ServiceError::Internal("failed to reload git credential".into()))?;

        Ok(GitCredentialSummary {
            id: row.id,
            label: row.label,
            host: row.host,
            path_prefix: row.path_prefix,
            header_name: row.header_name,
            created_at: row.created_at,
            updated_at: row.updated_at,
            last_used_at: row.last_used_at,
        })
    }
    .await;

    match result {
        Ok(body) => json_response(&body, 201),
        Err(err) => err.into_err_response(),
    }
}

pub async fn delete_git_credential(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let (config, d1) = match super::load_config_and_d1(&ctx) {
        Ok(values) => values,
        Err(err) => return err.into_err_response(),
    };

    let result: ServiceResult<OkResponse> = async {
        let user = authenticate(&req, &d1, &config).await?;
        enforce_csrf_if_cookie_auth(&req, &config, user.auth_via_cookie)?;
        let id = ctx
            .param("id")
            .ok_or_else(|| ServiceError::BadRequest("missing credential id".into()))?;

        let existing = d1_first::<GitCredentialSummaryRow>(
            &d1,
            dbq::git_credentials::get_by_id_and_user(id, &user.user_id),
            "lookup git credential",
        )
        .await?;
        if existing.is_none() {
            return Err(ServiceError::NotFound("credential not found".into()));
        }

        d1_run(
            &d1,
            dbq::git_credentials::delete_by_id_and_user(id, &user.user_id),
            "delete git credential",
        )
        .await?;
        Ok(OkResponse { ok: true })
    }
    .await;

    match result {
        Ok(body) => json_response(&body, 200),
        Err(err) => err.into_err_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_header_name, normalize_host, normalize_path_prefix};

    #[test]
    fn normalize_host_accepts_valid_and_rejects_invalid() {
        assert_eq!(
            normalize_host("GitLab.INTERNAL.example.com").expect("valid host"),
            "gitlab.internal.example.com"
        );
        assert!(normalize_host("bad host/path").is_err());
        assert!(normalize_host("").is_err());
    }

    #[test]
    fn normalize_path_prefix_trims_and_strips_git_suffix() {
        assert_eq!(
            normalize_path_prefix(Some("/group/sub/repo.git/")).expect("prefix"),
            "group/sub/repo"
        );
        assert_eq!(normalize_path_prefix(None).expect("empty"), "");
        assert!(normalize_path_prefix(Some("../bad")).is_err());
    }

    #[test]
    fn normalize_header_name_enforces_token_chars() {
        assert_eq!(
            normalize_header_name("X-GitLab-Token").expect("header"),
            "X-GitLab-Token"
        );
        assert!(normalize_header_name("bad header").is_err());
    }
}
