use anyhow::{Context, Result};
use rusqlite::Connection;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use opensession_api::{
    GitCredentialSummary, LinkType, SessionDetail, SessionLink, SessionListQuery,
    SessionListResponse, SessionSummary, db, oauth,
};

/// Shared database state.
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
    data_dir: PathBuf,
}

#[derive(Debug)]
pub enum StorageError {
    Sqlite(rusqlite::Error),
    Join(tokio::task::JoinError),
    Poisoned,
}

impl StorageError {
    pub fn is_constraint_violation(&self) -> bool {
        matches!(
            self,
            Self::Sqlite(rusqlite::Error::SqliteFailure(err, _))
                if err.code == rusqlite::ErrorCode::ConstraintViolation
        )
    }
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(err) => write!(f, "{err}"),
            Self::Join(err) => write!(f, "database worker join failed: {err}"),
            Self::Poisoned => write!(f, "database mutex poisoned"),
        }
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sqlite(err) => Some(err),
            Self::Join(err) => Some(err),
            Self::Poisoned => None,
        }
    }
}

impl From<rusqlite::Error> for StorageError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sqlite(value)
    }
}

#[derive(Debug, Clone)]
pub struct AuthUserRecord {
    pub user_id: String,
    pub nickname: String,
    pub email: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LoginUserRecord {
    pub user_id: String,
    pub nickname: String,
    pub password_hash: Option<String>,
    pub password_salt: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RefreshTokenRecord {
    pub token_id: String,
    pub user_id: String,
    pub expires_at: String,
    pub nickname: String,
}

#[derive(Debug, Clone)]
pub struct PasswordFields {
    pub password_hash: Option<String>,
    pub password_salt: Option<String>,
}

#[derive(Debug)]
pub struct UserSettingsData {
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub oauth_providers: Vec<oauth::LinkedProvider>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct SessionStorageInfo {
    pub body_storage_key: String,
    pub body_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OAuthStateRecord {
    pub provider: String,
    pub expires_at: String,
    pub user_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GitCredentialSecretRecord {
    pub credential_id: String,
    pub path_prefix: String,
    pub header_name: String,
    pub header_value_enc: String,
}

#[derive(Debug, Clone)]
pub struct NewGitCredentialRecord {
    pub id: String,
    pub user_id: String,
    pub label: String,
    pub host: String,
    pub path_prefix: String,
    pub header_name: String,
    pub header_value_enc: String,
}

impl Db {
    async fn with_conn<T, F>(&self, op: F) -> std::result::Result<T, StorageError>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> rusqlite::Result<T> + Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|_| StorageError::Poisoned)?;
            op(&conn).map_err(StorageError::from)
        })
        .await
        .map_err(StorageError::Join)?
    }

    fn bodies_dir(&self) -> PathBuf {
        self.data_dir.join("bodies")
    }

    /// Write a session body as HAIL JSONL to disk, return the storage key.
    pub async fn write_body(&self, session_id: &str, data: &[u8]) -> Result<String> {
        let dir = self.bodies_dir();
        tokio::fs::create_dir_all(&dir).await?;
        let key = format!("{session_id}.hail.jsonl");
        let path = dir.join(&key);
        tokio::fs::write(&path, data)
            .await
            .context("writing session body")?;
        Ok(key)
    }

    /// Read a session body from disk.
    pub async fn read_body(&self, storage_key: &str) -> Result<Vec<u8>> {
        let path = self.bodies_dir().join(storage_key);
        tokio::fs::read(&path).await.context("reading session body")
    }

    pub async fn list_sessions(
        &self,
        query: &SessionListQuery,
    ) -> std::result::Result<SessionListResponse, StorageError> {
        let query = SessionListQuery {
            page: query.page,
            per_page: query.per_page,
            search: query.search.clone(),
            tool: query.tool.clone(),
            git_repo_name: query.git_repo_name.clone(),
            sort: query.sort.clone(),
            time_range: query.time_range.clone(),
        };
        self.with_conn(move |conn| {
            let built = db::sessions::list(&query);
            let total: i64 = sq_query_row(conn, built.count_query, |row| row.get(0))?;
            let sessions = sq_query_map(conn, built.select_query, session_from_row)?;
            Ok(SessionListResponse {
                sessions,
                total,
                page: built.page,
                per_page: built.per_page,
            })
        })
        .await
    }

    pub async fn list_session_repos(&self) -> std::result::Result<Vec<String>, StorageError> {
        self.with_conn(move |conn| {
            sq_query_map(conn, db::sessions::list_repo_names(), |row| row.get(0))
        })
        .await
    }

    pub async fn get_session_detail(
        &self,
        id: &str,
    ) -> std::result::Result<SessionDetail, StorageError> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            let summary = sq_query_row(conn, db::sessions::get_by_id(&id), session_from_row)?;
            let linked_sessions = sq_query_map(conn, db::sessions::links_by_session(&id), |row| {
                let link_type: String = row.get(2)?;
                Ok(SessionLink {
                    session_id: row.get(0)?,
                    linked_session_id: row.get(1)?,
                    link_type: match link_type.as_str() {
                        "related" => LinkType::Related,
                        "parent" => LinkType::Parent,
                        "child" => LinkType::Child,
                        _ => LinkType::Handoff,
                    },
                    created_at: row.get(3)?,
                })
            })?;
            Ok(SessionDetail {
                summary,
                linked_sessions,
            })
        })
        .await
    }

    pub async fn get_session_storage_info(
        &self,
        id: &str,
    ) -> std::result::Result<SessionStorageInfo, StorageError> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            sq_query_row(conn, db::sessions::get_storage_info(&id), |row| {
                Ok(SessionStorageInfo {
                    body_storage_key: row.get(0)?,
                    body_url: row.get(1)?,
                })
            })
        })
        .await
    }

    pub async fn delete_session(&self, id: &str) -> std::result::Result<bool, StorageError> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            let exists = match sq_query_row(conn, db::sessions::get_by_id(&id), |_row| Ok(())) {
                Ok(()) => true,
                Err(rusqlite::Error::QueryReturnedNoRows) => false,
                Err(err) => return Err(err),
            };
            if !exists {
                return Ok(false);
            }

            sq_execute(conn, db::sessions::delete_links(&id))?;
            sq_execute(conn, db::sessions::delete(&id))?;
            let _ = sq_execute(conn, db::sessions::delete_fts(&id));
            Ok(true)
        })
        .await
    }

    pub async fn get_auth_user_by_api_key_hash(
        &self,
        key_hash: &str,
    ) -> std::result::Result<AuthUserRecord, StorageError> {
        let key_hash = key_hash.to_string();
        self.with_conn(move |conn| {
            sq_query_row(
                conn,
                db::api_keys::get_user_by_valid_key_hash(&key_hash),
                |row| {
                    Ok(AuthUserRecord {
                        user_id: row.get(0)?,
                        nickname: row.get(1)?,
                        email: row.get(2)?,
                    })
                },
            )
        })
        .await
    }

    pub async fn get_auth_user_by_id(
        &self,
        user_id: &str,
    ) -> std::result::Result<AuthUserRecord, StorageError> {
        let user_id = user_id.to_string();
        self.with_conn(move |conn| {
            sq_query_row(conn, db::users::get_by_id(&user_id), |row| {
                Ok(AuthUserRecord {
                    user_id: row.get(0)?,
                    nickname: row.get(1)?,
                    email: row.get(2)?,
                })
            })
        })
        .await
    }

    pub async fn insert_refresh_token(
        &self,
        token_id: &str,
        user_id: &str,
        token_hash: &str,
        expires_at: &str,
    ) -> std::result::Result<(), StorageError> {
        let token_id = token_id.to_string();
        let user_id = user_id.to_string();
        let token_hash = token_hash.to_string();
        let expires_at = expires_at.to_string();
        self.with_conn(move |conn| {
            sq_execute(
                conn,
                db::users::insert_refresh_token(&token_id, &user_id, &token_hash, &expires_at),
            )?;
            Ok(())
        })
        .await
    }

    pub async fn email_exists(&self, email: &str) -> std::result::Result<bool, StorageError> {
        let email = email.to_string();
        self.with_conn(move |conn| {
            sq_query_row(conn, db::users::email_exists(&email), |row| row.get(0))
        })
        .await
    }

    pub async fn insert_user_with_email(
        &self,
        user_id: &str,
        nickname: &str,
        email: &str,
        password_hash: &str,
        password_salt: &str,
    ) -> std::result::Result<(), StorageError> {
        let user_id = user_id.to_string();
        let nickname = nickname.to_string();
        let email = email.to_string();
        let password_hash = password_hash.to_string();
        let password_salt = password_salt.to_string();
        self.with_conn(move |conn| {
            sq_execute(
                conn,
                db::users::insert_with_email(
                    &user_id,
                    &nickname,
                    &email,
                    &password_hash,
                    &password_salt,
                ),
            )?;
            Ok(())
        })
        .await
    }

    pub async fn get_login_user(
        &self,
        email: &str,
    ) -> std::result::Result<LoginUserRecord, StorageError> {
        let email = email.to_string();
        self.with_conn(move |conn| {
            sq_query_row(conn, db::users::get_by_email_for_login(&email), |row| {
                Ok(LoginUserRecord {
                    user_id: row.get(0)?,
                    nickname: row.get(1)?,
                    password_hash: row.get(2)?,
                    password_salt: row.get(3)?,
                })
            })
        })
        .await
    }

    pub async fn get_user_id_and_nickname_by_email(
        &self,
        email: &str,
    ) -> std::result::Result<(String, String), StorageError> {
        let email = email.to_string();
        self.with_conn(move |conn| {
            sq_query_row(conn, db::users::get_by_email_for_login(&email), |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
        })
        .await
    }

    pub async fn lookup_refresh_token(
        &self,
        token_hash: &str,
    ) -> std::result::Result<RefreshTokenRecord, StorageError> {
        let token_hash = token_hash.to_string();
        self.with_conn(move |conn| {
            sq_query_row(conn, db::users::lookup_refresh_token(&token_hash), |row| {
                Ok(RefreshTokenRecord {
                    token_id: row.get(0)?,
                    user_id: row.get(1)?,
                    expires_at: row.get(2)?,
                    nickname: row.get(3)?,
                })
            })
        })
        .await
    }

    pub async fn delete_refresh_token_by_id(
        &self,
        token_id: &str,
    ) -> std::result::Result<(), StorageError> {
        let token_id = token_id.to_string();
        self.with_conn(move |conn| {
            sq_execute(conn, db::users::delete_refresh_token_by_id(&token_id))?;
            Ok(())
        })
        .await
    }

    pub async fn delete_refresh_token(
        &self,
        token_hash: &str,
    ) -> std::result::Result<(), StorageError> {
        let token_hash = token_hash.to_string();
        self.with_conn(move |conn| {
            sq_execute(conn, db::users::delete_refresh_token(&token_hash))?;
            Ok(())
        })
        .await
    }

    pub async fn get_password_fields(
        &self,
        user_id: &str,
    ) -> std::result::Result<PasswordFields, StorageError> {
        let user_id = user_id.to_string();
        self.with_conn(move |conn| {
            sq_query_row(conn, db::users::get_password_fields(&user_id), |row| {
                Ok(PasswordFields {
                    password_hash: row.get(0)?,
                    password_salt: row.get(1)?,
                })
            })
        })
        .await
    }

    pub async fn update_password(
        &self,
        user_id: &str,
        password_hash: &str,
        password_salt: &str,
    ) -> std::result::Result<(), StorageError> {
        let user_id = user_id.to_string();
        let password_hash = password_hash.to_string();
        let password_salt = password_salt.to_string();
        self.with_conn(move |conn| {
            sq_execute(
                conn,
                db::users::update_password(&user_id, &password_hash, &password_salt),
            )?;
            Ok(())
        })
        .await
    }

    pub async fn get_user_settings_data(
        &self,
        user_id: &str,
    ) -> std::result::Result<UserSettingsData, StorageError> {
        let user_id = user_id.to_string();
        self.with_conn(move |conn| {
            let (email, avatar_url) =
                sq_query_row(conn, db::users::get_email_avatar(&user_id), |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })?;

            let oauth_providers = sq_query_map(conn, db::oauth::find_by_user(&user_id), |row| {
                let provider: String = row.get(1)?;
                Ok(oauth::LinkedProvider {
                    provider: provider.clone(),
                    provider_username: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    display_name: match provider.as_str() {
                        "github" => "GitHub".to_string(),
                        "gitlab" => "GitLab".to_string(),
                        other => other.to_string(),
                    },
                })
            })?;

            let created_at = sq_query_row(conn, db::users::get_settings_fields(&user_id), |row| {
                row.get(0)
            })
            .unwrap_or_default();

            Ok(UserSettingsData {
                email,
                avatar_url,
                oauth_providers,
                created_at,
            })
        })
        .await
    }

    pub async fn move_active_api_keys_to_grace(
        &self,
        user_id: &str,
        grace_until: &str,
    ) -> std::result::Result<(), StorageError> {
        let user_id = user_id.to_string();
        let grace_until = grace_until.to_string();
        self.with_conn(move |conn| {
            sq_execute(
                conn,
                db::api_keys::move_active_to_grace(&user_id, &grace_until),
            )?;
            Ok(())
        })
        .await
    }

    pub async fn insert_active_api_key(
        &self,
        key_id: &str,
        user_id: &str,
        key_hash: &str,
        key_prefix: &str,
    ) -> std::result::Result<(), StorageError> {
        let key_id = key_id.to_string();
        let user_id = user_id.to_string();
        let key_hash = key_hash.to_string();
        let key_prefix = key_prefix.to_string();
        self.with_conn(move |conn| {
            sq_execute(
                conn,
                db::api_keys::insert_active(&key_id, &user_id, &key_hash, &key_prefix),
            )?;
            Ok(())
        })
        .await
    }

    pub async fn list_git_credentials(
        &self,
        user_id: &str,
    ) -> std::result::Result<Vec<GitCredentialSummary>, StorageError> {
        let user_id = user_id.to_string();
        self.with_conn(move |conn| {
            sq_query_map(conn, db::git_credentials::list_by_user(&user_id), |row| {
                Ok(GitCredentialSummary {
                    id: row.get(0)?,
                    label: row.get(1)?,
                    host: row.get(2)?,
                    path_prefix: row.get(3)?,
                    header_name: row.get(4)?,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                    last_used_at: row.get(7)?,
                })
            })
        })
        .await
    }

    pub async fn insert_git_credential(
        &self,
        record: NewGitCredentialRecord,
    ) -> std::result::Result<(), StorageError> {
        self.with_conn(move |conn| {
            sq_execute(
                conn,
                db::git_credentials::insert(
                    &record.id,
                    &record.user_id,
                    &record.label,
                    &record.host,
                    &record.path_prefix,
                    &record.header_name,
                    &record.header_value_enc,
                ),
            )?;
            Ok(())
        })
        .await
    }

    pub async fn get_git_credential_by_id_and_user(
        &self,
        id: &str,
        user_id: &str,
    ) -> std::result::Result<GitCredentialSummary, StorageError> {
        let id = id.to_string();
        let user_id = user_id.to_string();
        self.with_conn(move |conn| {
            sq_query_row(
                conn,
                db::git_credentials::get_by_id_and_user(&id, &user_id),
                |row| {
                    Ok(GitCredentialSummary {
                        id: row.get(0)?,
                        label: row.get(1)?,
                        host: row.get(2)?,
                        path_prefix: row.get(3)?,
                        header_name: row.get(4)?,
                        created_at: row.get(5)?,
                        updated_at: row.get(6)?,
                        last_used_at: row.get(7)?,
                    })
                },
            )
        })
        .await
    }

    pub async fn delete_git_credential_by_id_and_user(
        &self,
        id: &str,
        user_id: &str,
    ) -> std::result::Result<usize, StorageError> {
        let id = id.to_string();
        let user_id = user_id.to_string();
        self.with_conn(move |conn| {
            sq_execute(
                conn,
                db::git_credentials::delete_by_id_and_user(&id, &user_id),
            )
        })
        .await
    }

    pub async fn upsert_oauth_provider_access_token(
        &self,
        token_id: &str,
        user_id: &str,
        provider: &str,
        provider_host: &str,
        encrypted_token: &str,
    ) -> std::result::Result<(), StorageError> {
        let token_id = token_id.to_string();
        let user_id = user_id.to_string();
        let provider = provider.to_string();
        let provider_host = provider_host.to_string();
        let encrypted_token = encrypted_token.to_string();
        self.with_conn(move |conn| {
            sq_execute(
                conn,
                db::oauth_provider_tokens::upsert_access_token(
                    &token_id,
                    &user_id,
                    &provider,
                    &provider_host,
                    &encrypted_token,
                    None,
                ),
            )?;
            Ok(())
        })
        .await
    }

    pub async fn insert_oauth_state(
        &self,
        state: &str,
        provider: &str,
        expires_at: &str,
        user_id: Option<&str>,
    ) -> std::result::Result<(), StorageError> {
        let state = state.to_string();
        let provider = provider.to_string();
        let expires_at = expires_at.to_string();
        let user_id = user_id.map(ToOwned::to_owned);
        self.with_conn(move |conn| {
            sq_execute(
                conn,
                db::oauth::insert_state(&state, &provider, &expires_at, user_id.as_deref()),
            )?;
            Ok(())
        })
        .await
    }

    pub async fn validate_oauth_state(
        &self,
        state: &str,
    ) -> std::result::Result<OAuthStateRecord, StorageError> {
        let state = state.to_string();
        self.with_conn(move |conn| {
            sq_query_row(conn, db::oauth::validate_state(&state), |row| {
                Ok(OAuthStateRecord {
                    provider: row.get(1)?,
                    expires_at: row.get(2)?,
                    user_id: row.get(3)?,
                })
            })
        })
        .await
    }

    pub async fn delete_oauth_state(&self, state: &str) -> std::result::Result<(), StorageError> {
        let state = state.to_string();
        self.with_conn(move |conn| {
            sq_execute(conn, db::oauth::delete_state(&state))?;
            Ok(())
        })
        .await
    }

    pub async fn find_oauth_user_id_by_provider(
        &self,
        provider: &str,
        provider_user_id: &str,
    ) -> std::result::Result<Option<String>, StorageError> {
        let provider = provider.to_string();
        let provider_user_id = provider_user_id.to_string();
        self.with_conn(move |conn| {
            match sq_query_row(
                conn,
                db::oauth::find_by_provider(&provider, &provider_user_id),
                |row| row.get(0),
            ) {
                Ok(user_id) => Ok(Some(user_id)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(err) => Err(err),
            }
        })
        .await
    }

    pub async fn upsert_oauth_identity(
        &self,
        user_id: &str,
        provider: &str,
        provider_user_id: &str,
        provider_username: Option<&str>,
        avatar_url: Option<&str>,
    ) -> std::result::Result<(), StorageError> {
        let user_id = user_id.to_string();
        let provider = provider.to_string();
        let provider_user_id = provider_user_id.to_string();
        let provider_username = provider_username.map(ToOwned::to_owned);
        let avatar_url = avatar_url.map(ToOwned::to_owned);
        self.with_conn(move |conn| {
            sq_execute(
                conn,
                db::oauth::upsert_identity(
                    &user_id,
                    &provider,
                    &provider_user_id,
                    provider_username.as_deref(),
                    avatar_url.as_deref(),
                    None,
                ),
            )?;
            Ok(())
        })
        .await
    }

    pub async fn get_user_nickname(
        &self,
        user_id: &str,
    ) -> std::result::Result<String, StorageError> {
        let user_id = user_id.to_string();
        self.with_conn(move |conn| {
            sq_query_row(conn, db::users::get_nickname(&user_id), |row| row.get(0))
        })
        .await
    }

    pub async fn insert_oauth_user(
        &self,
        user_id: &str,
        nickname: &str,
        email: Option<&str>,
    ) -> std::result::Result<(), StorageError> {
        let user_id = user_id.to_string();
        let nickname = nickname.to_string();
        let email = email.map(ToOwned::to_owned);
        self.with_conn(move |conn| {
            sq_execute(
                conn,
                db::users::insert_oauth(&user_id, &nickname, email.as_deref()),
            )?;
            Ok(())
        })
        .await
    }

    pub async fn user_has_oauth_provider(
        &self,
        user_id: &str,
        provider: &str,
    ) -> std::result::Result<bool, StorageError> {
        let user_id = user_id.to_string();
        let provider = provider.to_string();
        self.with_conn(move |conn| {
            let count: i64 =
                sq_query_row(conn, db::oauth::has_provider(&user_id, &provider), |row| {
                    row.get(0)
                })?;
            Ok(count > 0)
        })
        .await
    }

    pub async fn get_provider_token_enc(
        &self,
        user_id: &str,
        provider: &str,
        provider_host: &str,
    ) -> std::result::Result<Option<String>, StorageError> {
        let user_id = user_id.to_string();
        let provider = provider.to_string();
        let provider_host = provider_host.to_string();
        self.with_conn(move |conn| {
            match sq_query_row(
                conn,
                db::oauth_provider_tokens::get_by_user_provider_host(
                    &user_id,
                    &provider,
                    &provider_host,
                ),
                |row| row.get(1),
            ) {
                Ok(token) => Ok(Some(token)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(err) => Err(err),
            }
        })
        .await
    }

    pub async fn list_git_credential_secrets_for_host(
        &self,
        user_id: &str,
        host: &str,
    ) -> std::result::Result<Vec<GitCredentialSecretRecord>, StorageError> {
        let user_id = user_id.to_string();
        let host = host.to_string();
        self.with_conn(move |conn| {
            sq_query_map(
                conn,
                db::git_credentials::list_for_host_with_secret(&user_id, &host),
                |row| {
                    Ok(GitCredentialSecretRecord {
                        credential_id: row.get(0)?,
                        path_prefix: row.get(3)?,
                        header_name: row.get(4)?,
                        header_value_enc: row.get(5)?,
                    })
                },
            )
        })
        .await
    }

    pub async fn touch_git_credential_last_used(
        &self,
        credential_id: &str,
        user_id: &str,
    ) -> std::result::Result<(), StorageError> {
        let credential_id = credential_id.to_string();
        let user_id = user_id.to_string();
        self.with_conn(move |conn| {
            sq_execute(
                conn,
                db::git_credentials::touch_last_used(&credential_id, &user_id),
            )?;
            Ok(())
        })
        .await
    }
}

// ── sea-query ↔ rusqlite helpers ──────────────────────────────────────────

/// Built query: `(sql, sea_query::Values)`.
type Built = (String, sea_query::Values);

fn sq_params(values: &sea_query::Values) -> Vec<Box<dyn rusqlite::types::ToSql>> {
    values
        .0
        .iter()
        .map(|v| -> Box<dyn rusqlite::types::ToSql> {
            match v {
                sea_query::Value::Bool(Some(b)) => Box::new(*b),
                sea_query::Value::TinyInt(Some(i)) => Box::new(*i as i32),
                sea_query::Value::SmallInt(Some(i)) => Box::new(*i as i32),
                sea_query::Value::Int(Some(i)) => Box::new(*i),
                sea_query::Value::BigInt(Some(i)) => Box::new(*i),
                sea_query::Value::TinyUnsigned(Some(u)) => Box::new(*u as i64),
                sea_query::Value::SmallUnsigned(Some(u)) => Box::new(*u as i64),
                sea_query::Value::Unsigned(Some(u)) => Box::new(*u as i64),
                sea_query::Value::BigUnsigned(Some(u)) => Box::new(*u as i64),
                sea_query::Value::Float(Some(f)) => Box::new(*f as f64),
                sea_query::Value::Double(Some(f)) => Box::new(*f),
                sea_query::Value::String(Some(s)) => Box::new(s.as_ref().clone()),
                sea_query::Value::Bytes(Some(b)) => Box::new(b.as_ref().clone()),
                _ => Box::new(rusqlite::types::Null),
            }
        })
        .collect()
}

fn sq_execute(conn: &Connection, (sql, values): Built) -> rusqlite::Result<usize> {
    let params = sq_params(&values);
    let refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    conn.execute(&sql, refs.as_slice())
}

fn sq_query_row<T>(
    conn: &Connection,
    (sql, values): Built,
    f: impl FnOnce(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> rusqlite::Result<T> {
    let params = sq_params(&values);
    let refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    conn.query_row(&sql, refs.as_slice(), f)
}

fn sq_query_map<T>(
    conn: &Connection,
    (sql, values): Built,
    f: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> rusqlite::Result<Vec<T>> {
    let params = sq_params(&values);
    let refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(refs.as_slice(), f)?;
    rows.collect()
}

fn session_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionSummary> {
    Ok(SessionSummary {
        id: row.get(0)?,
        user_id: row.get(1)?,
        nickname: row.get(2)?,
        tool: row.get(3)?,
        agent_provider: row.get(4)?,
        agent_model: row.get(5)?,
        title: row.get(6)?,
        description: row.get(7)?,
        tags: row.get(8)?,
        created_at: row.get(9)?,
        uploaded_at: row.get(10)?,
        message_count: row.get(11)?,
        task_count: row.get(12)?,
        event_count: row.get(13)?,
        duration_seconds: row.get(14)?,
        total_input_tokens: row.get(15)?,
        total_output_tokens: row.get(16)?,
        git_remote: row.get(17)?,
        git_branch: row.get(18)?,
        git_commit: row.get(19)?,
        git_repo_name: row.get(20)?,
        pr_number: row.get(21)?,
        pr_url: row.get(22)?,
        working_directory: row.get(23)?,
        files_modified: row.get(24)?,
        files_read: row.get(25)?,
        has_errors: row.get::<_, i64>(26).unwrap_or(0) != 0,
        max_active_agents: row.get(27).unwrap_or(1),
        session_score: row.get(28).unwrap_or(0),
        score_plugin: row
            .get::<_, String>(29)
            .unwrap_or_else(|_| opensession_core::scoring::DEFAULT_SCORE_PLUGIN.to_string()),
    })
}

// ── Database init ─────────────────────────────────────────────────────────

/// Initialize the database: open connection, enable WAL, run migrations.
pub fn init_db(data_dir: &Path) -> Result<Db> {
    std::fs::create_dir_all(data_dir)?;
    let db_path = data_dir.join("opensession.db");
    let conn = Connection::open(&db_path).context("opening SQLite database")?;

    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;

    run_migrations(&conn)?;

    Ok(Db {
        conn: Arc::new(Mutex::new(conn)),
        data_dir: data_dir.to_path_buf(),
    })
}

fn run_migrations(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _migrations (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )?;

    for (name, sql) in db::migrations::MIGRATIONS {
        let already_applied: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM _migrations WHERE name = ?1",
                [name],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if !already_applied {
            conn.execute_batch(sql)
                .with_context(|| format!("running migration {name}"))?;
            conn.execute("INSERT INTO _migrations (name) VALUES (?1)", [name])?;
            tracing::info!("Applied migration: {name}");
        }
    }

    if !oauth_provider_tokens_has_provider_host(conn)? {
        tracing::warn!(
            "rebuilding oauth_provider_tokens table for provider_host security upgrade (stored provider tokens will be removed)"
        );
        rebuild_oauth_provider_tokens_table(conn)?;
    }

    conn.execute_batch(
        r#"
CREATE TABLE IF NOT EXISTS git_credentials (
    id               TEXT PRIMARY KEY,
    user_id          TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    label            TEXT NOT NULL,
    host             TEXT NOT NULL,
    path_prefix      TEXT NOT NULL DEFAULT '',
    header_name      TEXT NOT NULL,
    header_value_enc TEXT NOT NULL,
    created_at       TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at       TEXT NOT NULL DEFAULT (datetime('now')),
    last_used_at     TEXT
);
CREATE INDEX IF NOT EXISTS idx_git_credentials_user_host ON git_credentials(user_id, host);
CREATE INDEX IF NOT EXISTS idx_git_credentials_user_host_prefix
ON git_credentials(user_id, host, path_prefix);

CREATE TABLE IF NOT EXISTS oauth_provider_tokens (
    id               TEXT PRIMARY KEY,
    user_id          TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider         TEXT NOT NULL,
    provider_host    TEXT NOT NULL,
    access_token_enc TEXT NOT NULL,
    expires_at       TEXT,
    created_at       TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at       TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (user_id, provider, provider_host)
);
DROP INDEX IF EXISTS idx_oauth_provider_tokens_user_provider;
CREATE INDEX IF NOT EXISTS idx_oauth_provider_tokens_user_provider_host
ON oauth_provider_tokens(user_id, provider, provider_host);
"#,
    )?;

    Ok(())
}

fn oauth_provider_tokens_has_provider_host(conn: &Connection) -> Result<bool> {
    let mut stmt = conn
        .prepare("PRAGMA table_info(oauth_provider_tokens)")
        .context("prepare oauth_provider_tokens schema inspection")?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .context("query oauth_provider_tokens schema inspection")?;
    for row in rows {
        if row.unwrap_or_default() == "provider_host" {
            return Ok(true);
        }
    }
    Ok(false)
}

fn rebuild_oauth_provider_tokens_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
DROP TABLE IF EXISTS oauth_provider_tokens;
CREATE TABLE oauth_provider_tokens (
    id               TEXT PRIMARY KEY,
    user_id          TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider         TEXT NOT NULL,
    provider_host    TEXT NOT NULL,
    access_token_enc TEXT NOT NULL,
    expires_at       TEXT,
    created_at       TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at       TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (user_id, provider, provider_host)
);
DROP INDEX IF EXISTS idx_oauth_provider_tokens_user_provider;
CREATE INDEX IF NOT EXISTS idx_oauth_provider_tokens_user_provider_host
ON oauth_provider_tokens(user_id, provider, provider_host);
"#,
    )
    .context("rebuild oauth_provider_tokens table")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_data_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "opensession-server-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&path).expect("create temp data dir");
        path
    }

    fn cleanup_dir(path: &Path) {
        std::fs::remove_dir_all(path).expect("remove temp data dir");
    }

    fn insert_test_user(db: &Db, user_id: &str, nickname: &str) {
        let conn = db.conn.lock().expect("db conn");
        sq_execute(
            &conn,
            db::users::insert_oauth(user_id, nickname, Some("test@example.com")),
        )
        .expect("insert test user");
    }

    fn insert_test_session(db: &Db, session_id: &str, user_id: &str, storage_key: &str) {
        let conn = db.conn.lock().expect("db conn");
        let params = db::sessions::InsertParams {
            id: session_id,
            user_id,
            team_id: "team-1",
            tool: "codex",
            agent_provider: "openai",
            agent_model: "gpt-5",
            title: "Test Session",
            description: "Description",
            tags: "test",
            created_at: "2026-03-09 12:00:00",
            message_count: 1,
            task_count: 1,
            event_count: 1,
            duration_seconds: 1,
            total_input_tokens: 1,
            total_output_tokens: 1,
            body_storage_key: storage_key,
            body_url: None,
            git_remote: Some("https://github.com/hwisu/opensession"),
            git_branch: Some("main"),
            git_commit: Some("abc123"),
            git_repo_name: Some("opensession"),
            pr_number: None,
            pr_url: None,
            working_directory: Some("/tmp"),
            files_modified: Some("src/lib.rs"),
            files_read: Some("src/main.rs"),
            has_errors: false,
            max_active_agents: 1,
            session_score: 42,
            score_plugin: "default",
        };
        sq_execute(&conn, db::sessions::insert(&params)).expect("insert test session");
    }

    #[tokio::test]
    async fn body_round_trip_uses_async_fs() {
        let data_dir = test_data_dir("body-round-trip");
        let db = init_db(&data_dir).expect("init db");

        let key = db
            .write_body("session-1", b"{\"type\":\"message\"}\n")
            .await
            .expect("write body");
        let body = db.read_body(&key).await.expect("read body");

        assert_eq!(key, "session-1.hail.jsonl");
        assert_eq!(body, b"{\"type\":\"message\"}\n");

        cleanup_dir(&data_dir);
    }

    #[tokio::test]
    async fn concurrent_session_reads_are_serialized_inside_storage() {
        let data_dir = test_data_dir("concurrent-session-reads");
        let db = init_db(&data_dir).expect("init db");
        insert_test_user(&db, "user-1", "tester");
        insert_test_session(&db, "session-1", "user-1", "session-1.hail.jsonl");

        let query = SessionListQuery {
            page: 1,
            per_page: 20,
            search: None,
            tool: None,
            git_repo_name: None,
            sort: None,
            time_range: None,
        };

        let (list_result, detail_result) =
            tokio::join!(db.list_sessions(&query), db.get_session_detail("session-1"));

        let list_result = list_result.expect("list sessions");
        let detail_result = detail_result.expect("session detail");
        assert_eq!(list_result.total, 1);
        assert_eq!(list_result.sessions.len(), 1);
        assert_eq!(detail_result.summary.id, "session-1");

        cleanup_dir(&data_dir);
    }
}
