use serde::{Deserialize, Serialize};
use worker::*;

// D1 + R2 storage layer for the Cloudflare Worker (public read-only surface).

const MIGRATIONS_TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS _migrations (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, applied_at TEXT NOT NULL DEFAULT (datetime('now')));";

#[derive(Debug, Deserialize)]
struct AppliedMigrationRow {
    #[allow(dead_code)]
    name: String,
}

#[derive(Debug, Deserialize)]
struct TableInfoRow {
    name: String,
}

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

fn split_migration_statements(sql: &str) -> Vec<String> {
    let mut normalized = String::new();
    for line in sql.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("--") {
            continue;
        }
        normalized.push_str(trimmed);
        normalized.push(' ');
    }

    normalized
        .split(';')
        .map(str::trim)
        .filter(|stmt| !stmt.is_empty())
        .map(|stmt| format!("{stmt};"))
        .collect()
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

/// Ensure shared schema migrations are applied in D1.
///
/// Cloudflare local dev can start with an empty D1 file; without this bootstrap
/// the first `/api/sessions` query fails with `no such table: sessions`.
pub async fn ensure_d1_schema(env: &Env) -> Result<()> {
    let d1 = get_d1(env)?;
    d1.exec(MIGRATIONS_TABLE_SQL).await?;

    for &(name, sql) in opensession_api::db::migrations::MIGRATIONS {
        let bind_name = worker::wasm_bindgen::JsValue::from_str(name);
        let existing = d1
            .prepare("SELECT name FROM _migrations WHERE name = ?1 LIMIT 1")
            .bind(&[bind_name])?
            .first::<AppliedMigrationRow>(None)
            .await?;

        if existing.is_some() {
            continue;
        }

        for statement in split_migration_statements(sql) {
            d1.exec(&statement).await?;
        }
        let insert_bind_name = worker::wasm_bindgen::JsValue::from_str(name);
        d1.prepare("INSERT INTO _migrations (name) VALUES (?1)")
            .bind(&[insert_bind_name])?
            .run()
            .await?;
    }

    let table_info = d1
        .prepare("PRAGMA table_info(oauth_provider_tokens)")
        .all()
        .await?;
    let table_info_rows = table_info.results::<TableInfoRow>().unwrap_or_default();
    let has_provider_host = table_info_rows
        .iter()
        .any(|row| row.name == "provider_host");
    if !has_provider_host {
        d1.exec("DROP TABLE IF EXISTS oauth_provider_tokens;")
            .await?;
        let rebuild_sql = r#"
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
CREATE INDEX IF NOT EXISTS idx_oauth_provider_tokens_user_provider_host
ON oauth_provider_tokens(user_id, provider, provider_host);
"#;
        for statement in split_migration_statements(rebuild_sql) {
            d1.exec(&statement).await?;
        }
    }

    // Keep bootstrap policy while ensuring post-bootstrap tables for existing DBs.
    let ensure_sql = r#"
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
"#;
    for statement in split_migration_statements(ensure_sql) {
        d1.exec(&statement).await?;
    }

    Ok(())
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

#[cfg(test)]
mod tests {
    use super::MIGRATIONS_TABLE_SQL;

    #[test]
    fn migrations_table_bootstrap_has_expected_shape() {
        assert!(MIGRATIONS_TABLE_SQL.contains("CREATE TABLE IF NOT EXISTS _migrations"));
        assert!(MIGRATIONS_TABLE_SQL.contains("name TEXT NOT NULL UNIQUE"));
        assert!(MIGRATIONS_TABLE_SQL.contains("applied_at TEXT NOT NULL"));
    }

    #[test]
    fn split_migration_statements_ignores_comments_and_blank_lines() {
        let sql = r#"
-- comment
CREATE TABLE foo (
    id INTEGER PRIMARY KEY
);

-- another comment
CREATE INDEX idx_foo_id ON foo(id);
"#;

        let statements = super::split_migration_statements(sql);
        assert_eq!(statements.len(), 2);
        assert_eq!(
            statements[0],
            "CREATE TABLE foo ( id INTEGER PRIMARY KEY );"
        );
        assert_eq!(statements[1], "CREATE INDEX idx_foo_id ON foo(id);");
    }
}
