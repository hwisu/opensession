use opensession_api::crypto::CredentialKeyring;
use opensession_api::oauth::{self, OAuthProviderConfig};
use worker::Env;

#[derive(Clone, Debug)]
pub struct WorkerConfig {
    pub base_url: Option<String>,
    pub jwt_secret: String,
    pub oauth_providers: Vec<OAuthProviderConfig>,
    pub credential_keyring: Option<CredentialKeyring>,
}

impl WorkerConfig {
    pub fn from_env(env: &Env) -> Self {
        let base_url =
            env_trimmed(env, "BASE_URL").or_else(|| env_trimmed(env, "OPENSESSION_BASE_URL"));
        let jwt_secret = env_trimmed(env, "JWT_SECRET").unwrap_or_default();
        let oauth_providers = load_oauth_providers(env);
        let credential_keyring = load_credential_keyring(env);

        Self {
            base_url,
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
    [try_load_github(env)].into_iter().flatten().collect()
}

fn try_load_github(env: &Env) -> Option<OAuthProviderConfig> {
    let id = env_trimmed(env, "GITHUB_CLIENT_ID")?;
    let secret = env_trimmed(env, "GITHUB_CLIENT_SECRET")?;
    Some(oauth::github_preset(id, secret))
}
