use std::path::PathBuf;

use opensession_api::crypto::CredentialKeyring;
use opensession_api::oauth::{self, OAuthProviderConfig};

#[derive(Clone)]
pub struct AppConfig {
    pub base_url: String,
    pub allowed_origins: Vec<String>,
    pub oauth_use_request_host: bool,
    pub jwt_secret: String,
    pub admin_key: String,
    pub oauth_providers: Vec<OAuthProviderConfig>,
    pub public_feed_enabled: bool,
    pub local_review_root: Option<PathBuf>,
    pub credential_keyring: Option<CredentialKeyring>,
}

pub struct ServerBootstrap {
    pub data_dir: PathBuf,
    pub web_dir: PathBuf,
    pub port: String,
    pub config: AppConfig,
}

pub fn load_server_bootstrap() -> ServerBootstrap {
    let data_dir = std::env::var("OPENSESSION_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("data"));
    let web_dir = std::env::var("OPENSESSION_WEB_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("web/build"));
    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());

    let base_url_env = env_trimmed("BASE_URL").or_else(|| env_trimmed("OPENSESSION_BASE_URL"));
    let base_url = base_url_env
        .clone()
        .unwrap_or_else(|| "http://localhost:3000".to_string());
    let public_feed_enabled_raw =
        std::env::var(opensession_api::deploy::ENV_PUBLIC_FEED_ENABLED).ok();

    ServerBootstrap {
        data_dir,
        web_dir,
        port,
        config: AppConfig {
            base_url: base_url.clone(),
            allowed_origins: load_allowed_origins(&base_url),
            oauth_use_request_host: base_url_env.is_none(),
            jwt_secret: env_trimmed("JWT_SECRET").unwrap_or_default(),
            admin_key: env_trimmed("OPENSESSION_ADMIN_KEY").unwrap_or_default(),
            oauth_providers: load_oauth_providers(),
            public_feed_enabled: opensession_api::deploy::parse_bool_flag(
                public_feed_enabled_raw.as_deref(),
                true,
            ),
            local_review_root: std::env::var("OPENSESSION_LOCAL_REVIEW_ROOT")
                .ok()
                .map(PathBuf::from),
            credential_keyring: load_credential_keyring(),
        },
    }
}

fn origin_from_base_url(raw: &str) -> Option<String> {
    let url = reqwest::Url::parse(raw).ok()?;
    let host = url.host_str()?;
    let mut origin = format!("{}://{host}", url.scheme());
    if let Some(port) = url.port() {
        origin.push(':');
        origin.push_str(&port.to_string());
    }
    Some(origin)
}

fn load_allowed_origins(base_url: &str) -> Vec<String> {
    let configured = std::env::var("OPENSESSION_ALLOWED_ORIGINS")
        .ok()
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !configured.is_empty() {
        return configured;
    }
    origin_from_base_url(base_url).into_iter().collect()
}

fn load_oauth_providers() -> Vec<OAuthProviderConfig> {
    [try_load_github(), try_load_gitlab()]
        .into_iter()
        .flatten()
        .collect()
}

fn env_trimmed(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .and_then(|value| oauth::normalize_oauth_config_value(&value))
}

fn try_load_github() -> Option<OAuthProviderConfig> {
    let id = env_trimmed("GITHUB_CLIENT_ID")?;
    let secret = env_trimmed("GITHUB_CLIENT_SECRET")?;
    tracing::info!("OAuth provider enabled: GitHub");
    Some(oauth::github_preset(id, secret))
}

fn try_load_gitlab() -> Option<OAuthProviderConfig> {
    let url = env_trimmed("GITLAB_URL")?;
    let id = env_trimmed("GITLAB_CLIENT_ID")?;
    let secret = env_trimmed("GITLAB_CLIENT_SECRET")?;
    let ext_url = env_trimmed("GITLAB_EXTERNAL_URL");
    tracing::info!("OAuth provider enabled: GitLab ({})", url);
    Some(oauth::gitlab_preset(url, ext_url, id, secret))
}

fn load_credential_keyring() -> Option<CredentialKeyring> {
    let active = env_trimmed("OPENSESSION_CREDENTIAL_ACTIVE_KID")?;
    let keyset = env_trimmed("OPENSESSION_CREDENTIAL_KEYS")?;
    match CredentialKeyring::from_csv(&active, &keyset) {
        Ok(keyring) => Some(keyring),
        Err(err) => {
            tracing::error!("invalid credential encryption config: {}", err.message());
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::load_server_bootstrap;
    use std::sync::{LazyLock, Mutex, MutexGuard};

    static TEST_ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }

        fn clear(key: &'static str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::remove_var(key);
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(previous) = self.previous.take() {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn lock_env() -> MutexGuard<'static, ()> {
        TEST_ENV_LOCK.lock().expect("test env lock")
    }

    #[test]
    fn bootstrap_uses_explicit_base_url_and_allowed_origins() {
        let _lock = lock_env();
        let _guards = [
            EnvVarGuard::set("BASE_URL", "https://api.example.test"),
            EnvVarGuard::set("OPENSESSION_BASE_URL", "https://ignored.example.test"),
            EnvVarGuard::set(
                "OPENSESSION_ALLOWED_ORIGINS",
                " https://app.example.test , https://ops.example.test ",
            ),
            EnvVarGuard::clear("OPENSESSION_DATA_DIR"),
            EnvVarGuard::clear("OPENSESSION_WEB_DIR"),
            EnvVarGuard::clear("PORT"),
            EnvVarGuard::clear("JWT_SECRET"),
            EnvVarGuard::clear("OPENSESSION_ADMIN_KEY"),
            EnvVarGuard::clear(opensession_api::deploy::ENV_PUBLIC_FEED_ENABLED),
            EnvVarGuard::clear("OPENSESSION_LOCAL_REVIEW_ROOT"),
            EnvVarGuard::clear("OPENSESSION_CREDENTIAL_ACTIVE_KID"),
            EnvVarGuard::clear("OPENSESSION_CREDENTIAL_KEYS"),
            EnvVarGuard::clear("GITHUB_CLIENT_ID"),
            EnvVarGuard::clear("GITHUB_CLIENT_SECRET"),
            EnvVarGuard::clear("GITLAB_URL"),
            EnvVarGuard::clear("GITLAB_CLIENT_ID"),
            EnvVarGuard::clear("GITLAB_CLIENT_SECRET"),
            EnvVarGuard::clear("GITLAB_EXTERNAL_URL"),
        ];

        let bootstrap = load_server_bootstrap();

        assert_eq!(bootstrap.config.base_url, "https://api.example.test");
        assert_eq!(
            bootstrap.config.allowed_origins,
            vec![
                "https://app.example.test".to_string(),
                "https://ops.example.test".to_string()
            ]
        );
        assert!(!bootstrap.config.oauth_use_request_host);
        assert_eq!(bootstrap.data_dir, std::path::PathBuf::from("data"));
        assert_eq!(bootstrap.web_dir, std::path::PathBuf::from("web/build"));
        assert_eq!(bootstrap.port, "3000");
    }

    #[test]
    fn bootstrap_derives_origin_and_request_host_mode_when_base_url_is_missing() {
        let _lock = lock_env();
        let _guards = [
            EnvVarGuard::clear("BASE_URL"),
            EnvVarGuard::clear("OPENSESSION_BASE_URL"),
            EnvVarGuard::clear("OPENSESSION_ALLOWED_ORIGINS"),
            EnvVarGuard::clear("OPENSESSION_CREDENTIAL_ACTIVE_KID"),
            EnvVarGuard::clear("OPENSESSION_CREDENTIAL_KEYS"),
            EnvVarGuard::clear("GITHUB_CLIENT_ID"),
            EnvVarGuard::clear("GITHUB_CLIENT_SECRET"),
            EnvVarGuard::clear("GITLAB_URL"),
            EnvVarGuard::clear("GITLAB_CLIENT_ID"),
            EnvVarGuard::clear("GITLAB_CLIENT_SECRET"),
            EnvVarGuard::clear("GITLAB_EXTERNAL_URL"),
        ];

        let bootstrap = load_server_bootstrap();

        assert_eq!(bootstrap.config.base_url, "http://localhost:3000");
        assert_eq!(
            bootstrap.config.allowed_origins,
            vec!["http://localhost:3000".to_string()]
        );
        assert!(bootstrap.config.oauth_use_request_host);
    }

    #[test]
    fn bootstrap_ignores_invalid_credential_keyring_config() {
        let _lock = lock_env();
        let _guards = [
            EnvVarGuard::clear("BASE_URL"),
            EnvVarGuard::clear("OPENSESSION_BASE_URL"),
            EnvVarGuard::clear("OPENSESSION_ALLOWED_ORIGINS"),
            EnvVarGuard::set("OPENSESSION_CREDENTIAL_ACTIVE_KID", "kid-1"),
            EnvVarGuard::set("OPENSESSION_CREDENTIAL_KEYS", "not-a-valid-keyset"),
            EnvVarGuard::clear("GITHUB_CLIENT_ID"),
            EnvVarGuard::clear("GITHUB_CLIENT_SECRET"),
            EnvVarGuard::clear("GITLAB_URL"),
            EnvVarGuard::clear("GITLAB_CLIENT_ID"),
            EnvVarGuard::clear("GITLAB_CLIENT_SECRET"),
            EnvVarGuard::clear("GITLAB_EXTERNAL_URL"),
        ];

        let bootstrap = load_server_bootstrap();

        assert!(bootstrap.config.credential_keyring.is_none());
    }
}
