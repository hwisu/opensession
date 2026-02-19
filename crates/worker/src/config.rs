use opensession_api::oauth::{self, OAuthProviderConfig};
use worker::Env;

#[derive(Clone, Debug)]
pub struct WorkerConfig {
    pub base_url: Option<String>,
    pub jwt_secret: String,
    pub oauth_providers: Vec<OAuthProviderConfig>,
}

impl WorkerConfig {
    pub fn from_env(env: &Env) -> Self {
        let base_url =
            env_trimmed(env, "BASE_URL").or_else(|| env_trimmed(env, "OPENSESSION_BASE_URL"));
        let jwt_secret = env_trimmed(env, "JWT_SECRET").unwrap_or_default();
        let oauth_providers = load_oauth_providers(env);

        Self {
            base_url,
            jwt_secret,
            oauth_providers,
        }
    }

    pub fn auth_enabled(&self) -> bool {
        !self.jwt_secret.is_empty()
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
