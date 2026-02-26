use opensession_api::crypto::CredentialKeyring;
use opensession_api::oauth::{self, OAuthProviderConfig};
use worker::{Env, Url};

#[derive(Clone, Debug)]
pub struct WorkerConfig {
    pub base_url: Option<String>,
    pub allowed_origins: Vec<String>,
    pub jwt_secret: String,
    pub oauth_providers: Vec<OAuthProviderConfig>,
    pub credential_keyring: Option<CredentialKeyring>,
}

impl WorkerConfig {
    pub fn from_env(env: &Env) -> Self {
        let base_url =
            env_trimmed(env, "BASE_URL").or_else(|| env_trimmed(env, "OPENSESSION_BASE_URL"));
        let allowed_origins = load_allowed_origins(env, base_url.as_deref());
        let jwt_secret = env_trimmed(env, "JWT_SECRET").unwrap_or_default();
        let oauth_providers = load_oauth_providers(env);
        let credential_keyring = load_credential_keyring(env);

        Self {
            base_url,
            allowed_origins,
            jwt_secret,
            oauth_providers,
            credential_keyring,
        }
    }

    pub fn auth_enabled(&self) -> bool {
        !self.jwt_secret.is_empty()
    }
}

fn load_credential_keyring(env: &Env) -> Option<CredentialKeyring> {
    let active = env_trimmed(env, "OPENSESSION_CREDENTIAL_ACTIVE_KID")?;
    let keyset = env_trimmed(env, "OPENSESSION_CREDENTIAL_KEYS")?;
    match CredentialKeyring::from_csv(&active, &keyset) {
        Ok(keyring) => Some(keyring),
        Err(err) => {
            worker::console_error!("invalid credential encryption config: {}", err.message());
            None
        }
    }
}

fn env_trimmed(env: &Env, name: &str) -> Option<String> {
    env.var(name)
        .ok()
        .and_then(|v| oauth::normalize_oauth_config_value(&v.to_string()))
}

fn load_oauth_providers(env: &Env) -> Vec<OAuthProviderConfig> {
    [try_load_github(env), try_load_gitlab(env)]
        .into_iter()
        .flatten()
        .collect()
}

fn origin_from_base_url(raw: &str) -> Option<String> {
    let url = Url::parse(raw).ok()?;
    let host = url.host_str()?;
    let mut origin = format!("{}://{host}", url.scheme());
    if let Some(port) = url.port() {
        origin.push(':');
        origin.push_str(&port.to_string());
    }
    Some(origin)
}

fn load_allowed_origins(env: &Env, base_url: Option<&str>) -> Vec<String> {
    if let Some(raw) = env_trimmed(env, "OPENSESSION_ALLOWED_ORIGINS") {
        let parsed = raw
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        if !parsed.is_empty() {
            return parsed;
        }
    }
    base_url
        .and_then(origin_from_base_url)
        .into_iter()
        .collect()
}

fn try_load_github(env: &Env) -> Option<OAuthProviderConfig> {
    let id = env_trimmed(env, "GITHUB_CLIENT_ID")?;
    let secret = env_trimmed(env, "GITHUB_CLIENT_SECRET")?;
    Some(oauth::github_preset(id, secret))
}

fn try_load_gitlab(env: &Env) -> Option<OAuthProviderConfig> {
    let url = env_trimmed(env, "GITLAB_URL")?;
    let id = env_trimmed(env, "GITLAB_CLIENT_ID")?;
    let secret = env_trimmed(env, "GITLAB_CLIENT_SECRET")?;
    let ext_url = env_trimmed(env, "GITLAB_EXTERNAL_URL");
    Some(oauth::gitlab_preset(url, ext_url, id, secret))
}
