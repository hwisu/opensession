use anyhow::{Context, Result};
use opensession_api::db::migrations::{
    JOB_CONTEXT_GUARD_COLUMN, JOB_CONTEXT_MIGRATION_NAME, LOCAL_MIGRATIONS, MIGRATIONS,
};
use rusqlite::{Connection, params};
use std::fs;
use std::io::{BufRead, BufReader};

use crate::session_store::{
    FROM_CLAUSE, LOCAL_SESSION_COLUMNS, infer_tool_from_source_path, normalize_tool_for_source_path,
};

pub(crate) fn apply_local_migrations(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _migrations (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )
    .context("create _migrations table for local db")?;

    for (name, sql) in MIGRATIONS.iter().chain(LOCAL_MIGRATIONS.iter()) {
        let already_applied: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM _migrations WHERE name = ?1",
                [name],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if already_applied {
            continue;
        }

        if *name == JOB_CONTEXT_MIGRATION_NAME
            && sqlite_table_has_column(conn, "sessions", JOB_CONTEXT_GUARD_COLUMN)?
        {
            conn.execute(
                "INSERT OR IGNORE INTO _migrations (name) VALUES (?1)",
                [name],
            )
            .with_context(|| format!("record skipped local migration {name}"))?;
            continue;
        }

        conn.execute_batch(sql)
            .with_context(|| format!("apply local migration {name}"))?;

        conn.execute(
            "INSERT OR IGNORE INTO _migrations (name) VALUES (?1)",
            [name],
        )
        .with_context(|| format!("record local migration {name}"))?;
    }

    Ok(())
}

fn sqlite_table_has_column(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let pragma = format!("PRAGMA table_info({table})");
    let mut stmt = conn
        .prepare(&pragma)
        .with_context(|| format!("prepare table info query for {table}"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for row in rows {
        if row?.eq_ignore_ascii_case(column) {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn validate_local_schema(conn: &Connection) -> Result<()> {
    let sql = format!("SELECT {LOCAL_SESSION_COLUMNS} {FROM_CLAUSE} WHERE 1=0");
    conn.prepare(&sql)
        .map(|_| ())
        .context("validate local session schema")
}

pub(crate) fn repair_session_tools_from_source_path(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.tool, ss.source_path \
         FROM sessions s \
         LEFT JOIN session_sync ss ON ss.session_id = s.id \
         WHERE ss.source_path IS NOT NULL",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
        ))
    })?;

    let mut updates: Vec<(String, String)> = Vec::new();
    for row in rows {
        let (id, current_tool, source_path) = row?;
        let normalized = normalize_tool_for_source_path(&current_tool, source_path.as_deref());
        if normalized != current_tool {
            updates.push((id, normalized));
        }
    }
    drop(stmt);

    for (id, tool) in updates {
        conn.execute(
            "UPDATE sessions SET tool = ?1 WHERE id = ?2",
            params![tool, id],
        )?;
    }

    Ok(())
}

pub(crate) fn repair_auxiliary_flags_from_source_path(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT s.id, ss.source_path \
         FROM sessions s \
         LEFT JOIN session_sync ss ON ss.session_id = s.id \
         WHERE ss.source_path IS NOT NULL \
         AND COALESCE(s.is_auxiliary, 0) = 0",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
    })?;

    let mut updates: Vec<String> = Vec::new();
    for row in rows {
        let (id, source_path) = row?;
        let Some(source_path) = source_path else {
            continue;
        };
        if infer_tool_from_source_path(Some(&source_path)) != Some("codex") {
            continue;
        }
        if is_codex_auxiliary_source_file(&source_path) {
            updates.push(id);
        }
    }
    drop(stmt);

    for id in updates {
        conn.execute(
            "UPDATE sessions SET is_auxiliary = 1 WHERE id = ?1",
            params![id],
        )?;
    }

    Ok(())
}

fn is_codex_auxiliary_source_file(source_path: &str) -> bool {
    let Ok(file) = fs::File::open(source_path) else {
        return false;
    };
    let reader = BufReader::new(file);
    for line in reader.lines().take(32) {
        let Ok(raw) = line else {
            continue;
        };
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }

        if line.contains("\"source\":{\"subagent\"")
            || line.contains("\"source\": {\"subagent\"")
            || line.contains("\"agent_role\":\"awaiter\"")
            || line.contains("\"agent_role\":\"worker\"")
            || line.contains("\"agent_role\":\"explorer\"")
            || line.contains("\"agent_role\":\"subagent\"")
        {
            return true;
        }

        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(line) {
            let is_session_meta =
                parsed.get("type").and_then(|v| v.as_str()) == Some("session_meta");
            let payload = if is_session_meta {
                parsed.get("payload")
            } else {
                Some(&parsed)
            };
            if let Some(payload) = payload {
                if payload.pointer("/source/subagent").is_some() {
                    return true;
                }
                let role = payload
                    .get("agent_role")
                    .and_then(|v| v.as_str())
                    .map(str::to_ascii_lowercase);
                if matches!(
                    role.as_deref(),
                    Some("awaiter") | Some("worker") | Some("explorer") | Some("subagent")
                ) {
                    return true;
                }
            }
        }
    }
    false
}
