use serde::{Deserialize, Serialize};
use worker::*;

// D1 + R2 storage layer for the Cloudflare Worker (public read-only surface).

// ── D1 bool helper ─────────────────────────────────────────────────────────

/// D1 returns booleans as floats (0.0 / 1.0). This deserializer handles both.
pub fn bool_from_d1<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<bool, D::Error> {
    use serde::de;

    struct BoolVisitor;

    impl<'de> de::Visitor<'de> for BoolVisitor {
        type Value = bool;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a boolean or number")
        }

        fn visit_bool<E: de::Error>(self, v: bool) -> std::result::Result<bool, E> {
            Ok(v)
        }

        fn visit_f64<E: de::Error>(self, v: f64) -> std::result::Result<bool, E> {
            Ok(v != 0.0)
        }

        fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<bool, E> {
            Ok(v != 0)
        }

        fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<bool, E> {
            Ok(v != 0)
        }
    }

    d.deserialize_any(BoolVisitor)
}

// ── Row types ───────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionRow {
    pub id: String,
    pub user_id: Option<String>,
    pub nickname: Option<String>,
    pub tool: String,
    pub agent_provider: Option<String>,
    pub agent_model: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub tags: Option<String>,
    pub created_at: String,
    pub uploaded_at: String,
    pub message_count: i64,
    pub task_count: i64,
    pub event_count: i64,
    pub duration_seconds: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    #[serde(default)]
    pub git_remote: Option<String>,
    #[serde(default)]
    pub git_branch: Option<String>,
    #[serde(default)]
    pub git_commit: Option<String>,
    #[serde(default)]
    pub git_repo_name: Option<String>,
    #[serde(default)]
    pub pr_number: Option<i64>,
    #[serde(default)]
    pub pr_url: Option<String>,
    #[serde(default)]
    pub working_directory: Option<String>,
    #[serde(default)]
    pub files_modified: Option<String>,
    #[serde(default)]
    pub files_read: Option<String>,
    #[serde(default, deserialize_with = "bool_from_d1")]
    pub has_errors: bool,
    #[serde(default = "default_max_active_agents")]
    pub max_active_agents: i64,
    #[serde(default)]
    pub session_score: i64,
    #[serde(default = "default_score_plugin")]
    pub score_plugin: String,
}

fn default_max_active_agents() -> i64 {
    1
}

fn default_score_plugin() -> String {
    opensession_core::scoring::DEFAULT_SCORE_PLUGIN.to_string()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CountRow {
    pub count: i64,
}

#[derive(Debug, Deserialize)]
pub struct StorageInfoRow {
    pub body_storage_key: String,
    pub body_url: Option<String>,
}

// ── D1 accessor ─────────────────────────────────────────────────────────────

pub fn get_d1(env: &Env) -> Result<D1Database> {
    env.d1("DB")
}

// ── R2 accessor ─────────────────────────────────────────────────────────────

pub fn get_r2(env: &Env) -> Result<Bucket> {
    env.bucket("SESSIONS")
}

// ── R2 convenience functions ────────────────────────────────────────────────

/// Retrieve a raw session body from R2.
pub async fn get_session_body(env: &Env, key: &str) -> Result<Option<Vec<u8>>> {
    let bucket = get_r2(env)?;
    match bucket.get(key).execute().await? {
        Some(object) => {
            let bytes = object.body().unwrap().bytes().await?;
            Ok(Some(bytes))
        }
        None => Ok(None),
    }
}

/// Store a raw session body to R2 at the given key.
pub async fn put_session_body(env: &Env, key: &str, bytes: &[u8]) -> Result<()> {
    let bucket = get_r2(env)?;
    bucket.put(key, bytes.to_vec()).execute().await?;
    Ok(())
}
