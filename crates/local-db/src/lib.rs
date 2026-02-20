pub mod git;

use anyhow::{Context, Result};
use opensession_api::db::migrations::{LOCAL_MIGRATIONS, MIGRATIONS};
use opensession_core::session::{is_auxiliary_session, working_directory};
use opensession_core::trace::Session;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use git::GitContext;

/// A local session row stored in the local SQLite index/cache database.
#[derive(Debug, Clone)]
pub struct LocalSessionRow {
    pub id: String,
    pub source_path: Option<String>,
    pub sync_status: String,
    pub last_synced_at: Option<String>,
    pub user_id: Option<String>,
    pub nickname: Option<String>,
    pub team_id: Option<String>,
    pub tool: String,
    pub agent_provider: Option<String>,
    pub agent_model: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub tags: Option<String>,
    pub created_at: String,
    pub uploaded_at: Option<String>,
    pub message_count: i64,
    pub user_message_count: i64,
    pub task_count: i64,
    pub event_count: i64,
    pub duration_seconds: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub git_remote: Option<String>,
    pub git_branch: Option<String>,
    pub git_commit: Option<String>,
    pub git_repo_name: Option<String>,
    pub pr_number: Option<i64>,
    pub pr_url: Option<String>,
    pub working_directory: Option<String>,
    pub files_modified: Option<String>,
    pub files_read: Option<String>,
    pub has_errors: bool,
    pub max_active_agents: i64,
    pub is_auxiliary: bool,
}

/// A link between a git commit and an AI session.
#[derive(Debug, Clone)]
pub struct CommitLink {
    pub commit_hash: String,
    pub session_id: String,
    pub repo_path: Option<String>,
    pub branch: Option<String>,
    pub created_at: String,
}

/// Return true when a cached row corresponds to an OpenCode child session.
pub fn is_opencode_child_session(row: &LocalSessionRow) -> bool {
    row.tool == "opencode" && row.is_auxiliary
}

/// Parse `parentID` / `parentId` from an OpenCode session JSON file.
#[deprecated(
    note = "Use parser/core canonical session role attributes instead of runtime file inspection"
)]
pub fn parse_opencode_parent_session_id(source_path: &str) -> Option<String> {
    let text = fs::read_to_string(source_path).ok()?;
    let json: Value = serde_json::from_str(&text).ok()?;
    lookup_parent_session_id(&json)
}

fn lookup_parent_session_id(value: &Value) -> Option<String> {
    match value {
        Value::Object(obj) => {
            for (key, value) in obj {
                if is_parent_id_key(key) {
                    if let Some(parent_id) = value.as_str() {
                        let parent_id = parent_id.trim();
                        if !parent_id.is_empty() {
                            return Some(parent_id.to_string());
                        }
                    }
                }
                if let Some(parent_id) = lookup_parent_session_id(value) {
                    return Some(parent_id);
                }
            }
            None
        }
        Value::Array(items) => items.iter().find_map(lookup_parent_session_id),
        _ => None,
    }
}

fn is_parent_id_key(key: &str) -> bool {
    let flat = key
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect::<String>();

    flat == "parentid"
        || flat == "parentuuid"
        || flat == "parentsessionid"
        || flat == "parentsessionuuid"
        || flat.ends_with("parentsessionid")
        || (flat.contains("parent") && flat.ends_with("id"))
        || (flat.contains("parent") && flat.ends_with("uuid"))
}

/// Remove OpenCode child sessions so only parent sessions remain visible.
pub fn hide_opencode_child_sessions(mut rows: Vec<LocalSessionRow>) -> Vec<LocalSessionRow> {
    rows.retain(|row| !row.is_auxiliary);
    rows
}

fn infer_tool_from_source_path(source_path: Option<&str>) -> Option<&'static str> {
    let source_path = source_path.map(|path| path.to_ascii_lowercase())?;

    if source_path.contains("/.codex/sessions/")
        || source_path.contains("\\.codex\\sessions\\")
        || source_path.contains("/codex/sessions/")
        || source_path.contains("\\codex\\sessions\\")
    {
        return Some("codex");
    }

    if source_path.contains("/.claude/projects/")
        || source_path.contains("\\.claude\\projects\\")
        || source_path.contains("/claude/projects/")
        || source_path.contains("\\claude\\projects\\")
    {
        return Some("claude-code");
    }

    None
}

fn normalize_tool_for_source_path(current_tool: &str, source_path: Option<&str>) -> String {
    infer_tool_from_source_path(source_path)
        .unwrap_or(current_tool)
        .to_string()
}

/// Filter for listing sessions from the local DB.
#[derive(Debug, Clone)]
pub struct LocalSessionFilter {
    pub team_id: Option<String>,
    pub sync_status: Option<String>,
    pub git_repo_name: Option<String>,
    pub search: Option<String>,
    pub tool: Option<String>,
    pub sort: LocalSortOrder,
    pub time_range: LocalTimeRange,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

impl Default for LocalSessionFilter {
    fn default() -> Self {
        Self {
            team_id: None,
            sync_status: None,
            git_repo_name: None,
            search: None,
            tool: None,
            sort: LocalSortOrder::Recent,
            time_range: LocalTimeRange::All,
            limit: None,
            offset: None,
        }
    }
}

/// Sort order for local session listing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum LocalSortOrder {
    #[default]
    Recent,
    Popular,
    Longest,
}

/// Time range filter for local session listing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum LocalTimeRange {
    Hours24,
    Days7,
    Days30,
    #[default]
    All,
}

/// Minimal remote session payload needed for local index/cache upsert.
#[derive(Debug, Clone)]
pub struct RemoteSessionSummary {
    pub id: String,
    pub user_id: Option<String>,
    pub nickname: Option<String>,
    pub team_id: String,
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
    pub git_remote: Option<String>,
    pub git_branch: Option<String>,
    pub git_commit: Option<String>,
    pub git_repo_name: Option<String>,
    pub pr_number: Option<i64>,
    pub pr_url: Option<String>,
    pub working_directory: Option<String>,
    pub files_modified: Option<String>,
    pub files_read: Option<String>,
    pub has_errors: bool,
    pub max_active_agents: i64,
}

/// Extended filter for the `log` command.
#[derive(Debug, Default)]
pub struct LogFilter {
    /// Filter by tool name (exact match).
    pub tool: Option<String>,
    /// Filter by model (glob-like, uses LIKE).
    pub model: Option<String>,
    /// Filter sessions created after this ISO8601 timestamp.
    pub since: Option<String>,
    /// Filter sessions created before this ISO8601 timestamp.
    pub before: Option<String>,
    /// Filter sessions that touched this file path (searches files_modified JSON).
    pub touches: Option<String>,
    /// Free-text search in title, description, tags.
    pub grep: Option<String>,
    /// Only sessions with errors.
    pub has_errors: Option<bool>,
    /// Filter by working directory (prefix match).
    pub working_directory: Option<String>,
    /// Filter by git repo name.
    pub git_repo_name: Option<String>,
    /// Filter sessions linked to this git commit hash.
    pub commit: Option<String>,
    /// Maximum number of results.
    pub limit: Option<u32>,
    /// Offset for pagination.
    pub offset: Option<u32>,
}

/// Base FROM clause for session list queries.
const FROM_CLAUSE: &str = "\
FROM sessions s \
LEFT JOIN session_sync ss ON ss.session_id = s.id \
LEFT JOIN users u ON u.id = s.user_id";

/// Local SQLite index/cache shared by TUI and Daemon.
/// This is not the source of truth for canonical session bodies.
/// Thread-safe: wraps the connection in a Mutex so it can be shared via `Arc<LocalDb>`.
pub struct LocalDb {
    conn: Mutex<Connection>,
}

impl LocalDb {
    /// Open (or create) the local database at the default path.
    /// `~/.local/share/opensession/local.db`
    pub fn open() -> Result<Self> {
        let path = default_db_path()?;
        Self::open_path(&path)
    }

    /// Open (or create) the local database at a specific path.
    pub fn open_path(path: &PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create dir for {}", path.display()))?;
        }
        match open_connection_with_latest_schema(path) {
            Ok(conn) => Ok(Self {
                conn: Mutex::new(conn),
            }),
            Err(err) => {
                if !is_schema_compat_error(&err) {
                    return Err(err);
                }

                // Local DB is a cache. If schema migration cannot safely reconcile
                // an incompatible/corrupted file, rotate it out and recreate latest schema.
                rotate_legacy_db(path)?;

                let conn = open_connection_with_latest_schema(path)
                    .with_context(|| format!("recreate db {}", path.display()))?;
                Ok(Self {
                    conn: Mutex::new(conn),
                })
            }
        }
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("local db mutex poisoned")
    }

    // ── Upsert local session (parsed from file) ────────────────────────

    pub fn upsert_local_session(
        &self,
        session: &Session,
        source_path: &str,
        git: &GitContext,
    ) -> Result<()> {
        let title = session.context.title.as_deref();
        let description = session.context.description.as_deref();
        let tags = if session.context.tags.is_empty() {
            None
        } else {
            Some(session.context.tags.join(","))
        };
        let created_at = session.context.created_at.to_rfc3339();
        let cwd = working_directory(session).map(String::from);
        let is_auxiliary = is_auxiliary_session(session);

        // Extract files_modified, files_read, and has_errors from events
        let (files_modified, files_read, has_errors) =
            opensession_core::extract::extract_file_metadata(session);
        let max_active_agents = opensession_core::agent_metrics::max_active_agents(session) as i64;
        let normalized_tool =
            normalize_tool_for_source_path(&session.agent.tool, Some(source_path));

        let conn = self.conn();
        // NOTE: `body_storage_key` is kept only for migration/schema parity.
        // Runtime lookup uses canonical body URLs and local body cache tables.
        conn.execute(
            "INSERT INTO sessions \
             (id, team_id, tool, agent_provider, agent_model, \
              title, description, tags, created_at, \
             message_count, user_message_count, task_count, event_count, duration_seconds, \
              total_input_tokens, total_output_tokens, body_storage_key, \
              git_remote, git_branch, git_commit, git_repo_name, working_directory, \
              files_modified, files_read, has_errors, max_active_agents, is_auxiliary) \
             VALUES (?1,'personal',?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,'',?16,?17,?18,?19,?20,?21,?22,?23,?24,?25) \
             ON CONFLICT(id) DO UPDATE SET \
              tool=excluded.tool, agent_provider=excluded.agent_provider, \
              agent_model=excluded.agent_model, \
              title=excluded.title, description=excluded.description, \
              tags=excluded.tags, \
              message_count=excluded.message_count, user_message_count=excluded.user_message_count, \
              task_count=excluded.task_count, \
              event_count=excluded.event_count, duration_seconds=excluded.duration_seconds, \
              total_input_tokens=excluded.total_input_tokens, \
              total_output_tokens=excluded.total_output_tokens, \
              git_remote=excluded.git_remote, git_branch=excluded.git_branch, \
              git_commit=excluded.git_commit, git_repo_name=excluded.git_repo_name, \
              working_directory=excluded.working_directory, \
              files_modified=excluded.files_modified, files_read=excluded.files_read, \
              has_errors=excluded.has_errors, \
              max_active_agents=excluded.max_active_agents, \
              is_auxiliary=excluded.is_auxiliary",
            params![
                &session.session_id,
                &normalized_tool,
                &session.agent.provider,
                &session.agent.model,
                title,
                description,
                &tags,
                &created_at,
                session.stats.message_count as i64,
                session.stats.user_message_count as i64,
                session.stats.task_count as i64,
                session.stats.event_count as i64,
                session.stats.duration_seconds as i64,
                session.stats.total_input_tokens as i64,
                session.stats.total_output_tokens as i64,
                &git.remote,
                &git.branch,
                &git.commit,
                &git.repo_name,
                &cwd,
                &files_modified,
                &files_read,
                has_errors,
                max_active_agents,
                is_auxiliary as i64,
            ],
        )?;

        conn.execute(
            "INSERT INTO session_sync (session_id, source_path, sync_status) \
             VALUES (?1, ?2, 'local_only') \
             ON CONFLICT(session_id) DO UPDATE SET source_path=excluded.source_path",
            params![&session.session_id, source_path],
        )?;
        Ok(())
    }

    // ── Upsert remote session (from server sync pull) ──────────────────

    pub fn upsert_remote_session(&self, summary: &RemoteSessionSummary) -> Result<()> {
        let conn = self.conn();
        // NOTE: `body_storage_key` is kept only for migration/schema parity.
        // Runtime lookup uses canonical body URLs and local body cache tables.
        conn.execute(
            "INSERT INTO sessions \
             (id, user_id, team_id, tool, agent_provider, agent_model, \
              title, description, tags, created_at, uploaded_at, \
              message_count, task_count, event_count, duration_seconds, \
              total_input_tokens, total_output_tokens, body_storage_key, \
              git_remote, git_branch, git_commit, git_repo_name, \
              pr_number, pr_url, working_directory, \
              files_modified, files_read, has_errors, max_active_agents, is_auxiliary) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,'',?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,0) \
             ON CONFLICT(id) DO UPDATE SET \
              title=excluded.title, description=excluded.description, \
              tags=excluded.tags, uploaded_at=excluded.uploaded_at, \
              message_count=excluded.message_count, task_count=excluded.task_count, \
              event_count=excluded.event_count, duration_seconds=excluded.duration_seconds, \
              total_input_tokens=excluded.total_input_tokens, \
              total_output_tokens=excluded.total_output_tokens, \
              git_remote=excluded.git_remote, git_branch=excluded.git_branch, \
              git_commit=excluded.git_commit, git_repo_name=excluded.git_repo_name, \
              pr_number=excluded.pr_number, pr_url=excluded.pr_url, \
              working_directory=excluded.working_directory, \
              files_modified=excluded.files_modified, files_read=excluded.files_read, \
              has_errors=excluded.has_errors, \
              max_active_agents=excluded.max_active_agents, \
              is_auxiliary=excluded.is_auxiliary",
            params![
                &summary.id,
                &summary.user_id,
                &summary.team_id,
                &summary.tool,
                &summary.agent_provider,
                &summary.agent_model,
                &summary.title,
                &summary.description,
                &summary.tags,
                &summary.created_at,
                &summary.uploaded_at,
                summary.message_count,
                summary.task_count,
                summary.event_count,
                summary.duration_seconds,
                summary.total_input_tokens,
                summary.total_output_tokens,
                &summary.git_remote,
                &summary.git_branch,
                &summary.git_commit,
                &summary.git_repo_name,
                summary.pr_number,
                &summary.pr_url,
                &summary.working_directory,
                &summary.files_modified,
                &summary.files_read,
                summary.has_errors,
                summary.max_active_agents,
            ],
        )?;

        conn.execute(
            "INSERT INTO session_sync (session_id, sync_status) \
             VALUES (?1, 'remote_only') \
             ON CONFLICT(session_id) DO UPDATE SET \
              sync_status = CASE WHEN session_sync.sync_status = 'local_only' THEN 'synced' ELSE session_sync.sync_status END",
            params![&summary.id],
        )?;
        Ok(())
    }

    // ── List sessions ──────────────────────────────────────────────────

    fn build_local_session_where_clause(
        filter: &LocalSessionFilter,
    ) -> (String, Vec<Box<dyn rusqlite::types::ToSql>>) {
        let mut where_clauses = vec![
            "1=1".to_string(),
            "COALESCE(s.is_auxiliary, 0) = 0".to_string(),
        ];
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let mut idx = 1u32;

        if let Some(ref team_id) = filter.team_id {
            where_clauses.push(format!("s.team_id = ?{idx}"));
            param_values.push(Box::new(team_id.clone()));
            idx += 1;
        }

        if let Some(ref sync_status) = filter.sync_status {
            where_clauses.push(format!("COALESCE(ss.sync_status, 'unknown') = ?{idx}"));
            param_values.push(Box::new(sync_status.clone()));
            idx += 1;
        }

        if let Some(ref repo) = filter.git_repo_name {
            where_clauses.push(format!("s.git_repo_name = ?{idx}"));
            param_values.push(Box::new(repo.clone()));
            idx += 1;
        }

        if let Some(ref tool) = filter.tool {
            where_clauses.push(format!("s.tool = ?{idx}"));
            param_values.push(Box::new(tool.clone()));
            idx += 1;
        }

        if let Some(ref search) = filter.search {
            let like = format!("%{search}%");
            where_clauses.push(format!(
                "(s.title LIKE ?{i1} OR s.description LIKE ?{i2} OR s.tags LIKE ?{i3})",
                i1 = idx,
                i2 = idx + 1,
                i3 = idx + 2,
            ));
            param_values.push(Box::new(like.clone()));
            param_values.push(Box::new(like.clone()));
            param_values.push(Box::new(like));
            idx += 3;
        }

        let interval = match filter.time_range {
            LocalTimeRange::Hours24 => Some("-1 day"),
            LocalTimeRange::Days7 => Some("-7 days"),
            LocalTimeRange::Days30 => Some("-30 days"),
            LocalTimeRange::All => None,
        };
        if let Some(interval) = interval {
            where_clauses.push(format!("datetime(s.created_at) >= datetime('now', ?{idx})"));
            param_values.push(Box::new(interval.to_string()));
        }

        (where_clauses.join(" AND "), param_values)
    }

    pub fn list_sessions(&self, filter: &LocalSessionFilter) -> Result<Vec<LocalSessionRow>> {
        let (where_str, mut param_values) = Self::build_local_session_where_clause(filter);
        let order_clause = match filter.sort {
            LocalSortOrder::Popular => "s.message_count DESC, s.created_at DESC",
            LocalSortOrder::Longest => "s.duration_seconds DESC, s.created_at DESC",
            LocalSortOrder::Recent => "s.created_at DESC",
        };

        let mut sql = format!(
            "SELECT {LOCAL_SESSION_COLUMNS} \
             {FROM_CLAUSE} WHERE {where_str} \
             ORDER BY {order_clause}"
        );

        if let Some(limit) = filter.limit {
            sql.push_str(" LIMIT ?");
            param_values.push(Box::new(limit));
            if let Some(offset) = filter.offset {
                sql.push_str(" OFFSET ?");
                param_values.push(Box::new(offset));
            }
        }

        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        let conn = self.conn();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(param_refs.as_slice(), row_to_local_session)?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }

        Ok(result)
    }

    /// Count sessions for a given list filter (before UI-level page slicing).
    pub fn count_sessions_filtered(&self, filter: &LocalSessionFilter) -> Result<i64> {
        let mut count_filter = filter.clone();
        count_filter.limit = None;
        count_filter.offset = None;
        let (where_str, param_values) = Self::build_local_session_where_clause(&count_filter);
        let sql = format!("SELECT COUNT(*) {FROM_CLAUSE} WHERE {where_str}");
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        let conn = self.conn();
        let count = conn.query_row(&sql, param_refs.as_slice(), |row| row.get(0))?;
        Ok(count)
    }

    /// List distinct tool names for the current list filter (ignores active tool filter).
    pub fn list_session_tools(&self, filter: &LocalSessionFilter) -> Result<Vec<String>> {
        let mut tool_filter = filter.clone();
        tool_filter.tool = None;
        tool_filter.limit = None;
        tool_filter.offset = None;
        let (where_str, param_values) = Self::build_local_session_where_clause(&tool_filter);
        let sql = format!(
            "SELECT DISTINCT s.tool \
             {FROM_CLAUSE} WHERE {where_str} \
             ORDER BY s.tool ASC"
        );
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        let conn = self.conn();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(param_refs.as_slice(), |row| row.get::<_, String>(0))?;

        let mut tools = Vec::new();
        for row in rows {
            let tool = row?;
            if !tool.trim().is_empty() {
                tools.push(tool);
            }
        }
        Ok(tools)
    }

    // ── Log query ─────────────────────────────────────────────────────

    /// Query sessions with extended filters for the `log` command.
    pub fn list_sessions_log(&self, filter: &LogFilter) -> Result<Vec<LocalSessionRow>> {
        let mut where_clauses = vec![
            "1=1".to_string(),
            "COALESCE(s.is_auxiliary, 0) = 0".to_string(),
        ];
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let mut idx = 1u32;

        if let Some(ref tool) = filter.tool {
            where_clauses.push(format!("s.tool = ?{idx}"));
            param_values.push(Box::new(tool.clone()));
            idx += 1;
        }

        if let Some(ref model) = filter.model {
            let like = model.replace('*', "%");
            where_clauses.push(format!("s.agent_model LIKE ?{idx}"));
            param_values.push(Box::new(like));
            idx += 1;
        }

        if let Some(ref since) = filter.since {
            where_clauses.push(format!("s.created_at >= ?{idx}"));
            param_values.push(Box::new(since.clone()));
            idx += 1;
        }

        if let Some(ref before) = filter.before {
            where_clauses.push(format!("s.created_at < ?{idx}"));
            param_values.push(Box::new(before.clone()));
            idx += 1;
        }

        if let Some(ref touches) = filter.touches {
            let like = format!("%\"{touches}\"%");
            where_clauses.push(format!("s.files_modified LIKE ?{idx}"));
            param_values.push(Box::new(like));
            idx += 1;
        }

        if let Some(ref grep) = filter.grep {
            let like = format!("%{grep}%");
            where_clauses.push(format!(
                "(s.title LIKE ?{i1} OR s.description LIKE ?{i2} OR s.tags LIKE ?{i3})",
                i1 = idx,
                i2 = idx + 1,
                i3 = idx + 2,
            ));
            param_values.push(Box::new(like.clone()));
            param_values.push(Box::new(like.clone()));
            param_values.push(Box::new(like));
            idx += 3;
        }

        if let Some(true) = filter.has_errors {
            where_clauses.push("s.has_errors = 1".to_string());
        }

        if let Some(ref wd) = filter.working_directory {
            where_clauses.push(format!("s.working_directory LIKE ?{idx}"));
            param_values.push(Box::new(format!("{wd}%")));
            idx += 1;
        }

        if let Some(ref repo) = filter.git_repo_name {
            where_clauses.push(format!("s.git_repo_name = ?{idx}"));
            param_values.push(Box::new(repo.clone()));
            idx += 1;
        }

        // Optional JOIN for commit hash filter
        let mut extra_join = String::new();
        if let Some(ref commit) = filter.commit {
            extra_join =
                " INNER JOIN commit_session_links csl ON csl.session_id = s.id".to_string();
            where_clauses.push(format!("csl.commit_hash = ?{idx}"));
            param_values.push(Box::new(commit.clone()));
            idx += 1;
        }

        let _ = idx; // suppress unused warning

        let where_str = where_clauses.join(" AND ");
        let mut sql = format!(
            "SELECT {LOCAL_SESSION_COLUMNS} \
             {FROM_CLAUSE}{extra_join} WHERE {where_str} \
             ORDER BY s.created_at DESC"
        );

        if let Some(limit) = filter.limit {
            sql.push_str(" LIMIT ?");
            param_values.push(Box::new(limit));
            if let Some(offset) = filter.offset {
                sql.push_str(" OFFSET ?");
                param_values.push(Box::new(offset));
            }
        }

        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        let conn = self.conn();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(param_refs.as_slice(), row_to_local_session)?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    /// Get the latest N sessions for a specific tool, ordered by created_at DESC.
    pub fn get_sessions_by_tool_latest(
        &self,
        tool: &str,
        count: u32,
    ) -> Result<Vec<LocalSessionRow>> {
        let sql = format!(
            "SELECT {LOCAL_SESSION_COLUMNS} \
             {FROM_CLAUSE} WHERE s.tool = ?1 AND COALESCE(s.is_auxiliary, 0) = 0 \
             ORDER BY s.created_at DESC"
        );
        let conn = self.conn();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![tool], row_to_local_session)?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }

        result.truncate(count as usize);
        Ok(result)
    }

    /// Get the latest N sessions across all tools, ordered by created_at DESC.
    pub fn get_sessions_latest(&self, count: u32) -> Result<Vec<LocalSessionRow>> {
        let sql = format!(
            "SELECT {LOCAL_SESSION_COLUMNS} \
             {FROM_CLAUSE} WHERE COALESCE(s.is_auxiliary, 0) = 0 \
             ORDER BY s.created_at DESC"
        );
        let conn = self.conn();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], row_to_local_session)?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }

        result.truncate(count as usize);
        Ok(result)
    }

    /// Get the Nth most recent session for a specific tool (0 = HEAD, 1 = HEAD~1, etc.).
    pub fn get_session_by_tool_offset(
        &self,
        tool: &str,
        offset: u32,
    ) -> Result<Option<LocalSessionRow>> {
        let sql = format!(
            "SELECT {LOCAL_SESSION_COLUMNS} \
             {FROM_CLAUSE} WHERE s.tool = ?1 AND COALESCE(s.is_auxiliary, 0) = 0 \
             ORDER BY s.created_at DESC"
        );
        let conn = self.conn();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![tool], row_to_local_session)?;
        let result = rows.collect::<Result<Vec<_>, _>>()?;
        Ok(result.into_iter().nth(offset as usize))
    }

    /// Get the Nth most recent session across all tools (0 = HEAD, 1 = HEAD~1, etc.).
    pub fn get_session_by_offset(&self, offset: u32) -> Result<Option<LocalSessionRow>> {
        let sql = format!(
            "SELECT {LOCAL_SESSION_COLUMNS} \
             {FROM_CLAUSE} WHERE COALESCE(s.is_auxiliary, 0) = 0 \
             ORDER BY s.created_at DESC"
        );
        let conn = self.conn();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], row_to_local_session)?;
        let result = rows.collect::<Result<Vec<_>, _>>()?;
        Ok(result.into_iter().nth(offset as usize))
    }

    /// Fetch the source path used when the session was last parsed/loaded.
    pub fn get_session_source_path(&self, session_id: &str) -> Result<Option<String>> {
        let conn = self.conn();
        let result = conn
            .query_row(
                "SELECT source_path FROM session_sync WHERE session_id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .optional()?;

        Ok(result)
    }

    /// Count total sessions in the local DB.
    pub fn session_count(&self) -> Result<i64> {
        let count = self
            .conn()
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))?;
        Ok(count)
    }

    // ── Delete session ─────────────────────────────────────────────────

    pub fn delete_session(&self, session_id: &str) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "DELETE FROM body_cache WHERE session_id = ?1",
            params![session_id],
        )?;
        conn.execute(
            "DELETE FROM session_sync WHERE session_id = ?1",
            params![session_id],
        )?;
        conn.execute("DELETE FROM sessions WHERE id = ?1", params![session_id])?;
        Ok(())
    }

    // ── Sync cursor ────────────────────────────────────────────────────

    pub fn get_sync_cursor(&self, team_id: &str) -> Result<Option<String>> {
        let cursor = self
            .conn()
            .query_row(
                "SELECT cursor FROM sync_cursors WHERE team_id = ?1",
                params![team_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(cursor)
    }

    pub fn set_sync_cursor(&self, team_id: &str, cursor: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO sync_cursors (team_id, cursor, updated_at) \
             VALUES (?1, ?2, datetime('now')) \
             ON CONFLICT(team_id) DO UPDATE SET cursor=excluded.cursor, updated_at=datetime('now')",
            params![team_id, cursor],
        )?;
        Ok(())
    }

    // ── Upload tracking ────────────────────────────────────────────────

    /// Get sessions that are local_only and need to be uploaded.
    pub fn pending_uploads(&self, team_id: &str) -> Result<Vec<LocalSessionRow>> {
        let sql = format!(
            "SELECT {LOCAL_SESSION_COLUMNS} \
             FROM sessions s \
             INNER JOIN session_sync ss ON ss.session_id = s.id \
             LEFT JOIN users u ON u.id = s.user_id \
             WHERE ss.sync_status = 'local_only' AND s.team_id = ?1 AND COALESCE(s.is_auxiliary, 0) = 0 \
             ORDER BY s.created_at ASC"
        );
        let conn = self.conn();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![team_id], row_to_local_session)?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    pub fn mark_synced(&self, session_id: &str) -> Result<()> {
        self.conn().execute(
            "UPDATE session_sync SET sync_status = 'synced', last_synced_at = datetime('now') \
             WHERE session_id = ?1",
            params![session_id],
        )?;
        Ok(())
    }

    /// Check if a session was already uploaded (synced or remote_only) since the given modification time.
    pub fn was_uploaded_after(
        &self,
        source_path: &str,
        modified: &chrono::DateTime<chrono::Utc>,
    ) -> Result<bool> {
        let result: Option<String> = self
            .conn()
            .query_row(
                "SELECT last_synced_at FROM session_sync \
                 WHERE source_path = ?1 AND sync_status = 'synced' AND last_synced_at IS NOT NULL",
                params![source_path],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(synced_at) = result {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&synced_at) {
                return Ok(dt >= *modified);
            }
        }
        Ok(false)
    }

    // ── Body cache (local read acceleration) ───────────────────────────

    pub fn cache_body(&self, session_id: &str, body: &[u8]) -> Result<()> {
        self.conn().execute(
            "INSERT INTO body_cache (session_id, body, cached_at) \
             VALUES (?1, ?2, datetime('now')) \
             ON CONFLICT(session_id) DO UPDATE SET body=excluded.body, cached_at=datetime('now')",
            params![session_id, body],
        )?;
        Ok(())
    }

    pub fn get_cached_body(&self, session_id: &str) -> Result<Option<Vec<u8>>> {
        let body = self
            .conn()
            .query_row(
                "SELECT body FROM body_cache WHERE session_id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(body)
    }

    // ── Migration helper ───────────────────────────────────────────────

    /// Migrate entries from the old state.json UploadState into the local DB.
    /// Marks them as `synced` with no metadata (we only know the file path was uploaded).
    pub fn migrate_from_state_json(
        &self,
        uploaded: &std::collections::HashMap<String, chrono::DateTime<chrono::Utc>>,
    ) -> Result<usize> {
        let mut count = 0;
        for (path, uploaded_at) in uploaded {
            let exists: bool = self
                .conn()
                .query_row(
                    "SELECT COUNT(*) > 0 FROM session_sync WHERE source_path = ?1",
                    params![path],
                    |row| row.get(0),
                )
                .unwrap_or(false);

            if exists {
                self.conn().execute(
                    "UPDATE session_sync SET sync_status = 'synced', last_synced_at = ?1 \
                     WHERE source_path = ?2 AND sync_status = 'local_only'",
                    params![uploaded_at.to_rfc3339(), path],
                )?;
                count += 1;
            }
        }
        Ok(count)
    }

    // ── Commit ↔ session linking ────────────────────────────────────

    /// Link a git commit to an AI session.
    pub fn link_commit_session(
        &self,
        commit_hash: &str,
        session_id: &str,
        repo_path: Option<&str>,
        branch: Option<&str>,
    ) -> Result<()> {
        self.conn().execute(
            "INSERT INTO commit_session_links (commit_hash, session_id, repo_path, branch) \
             VALUES (?1, ?2, ?3, ?4) \
             ON CONFLICT(commit_hash, session_id) DO NOTHING",
            params![commit_hash, session_id, repo_path, branch],
        )?;
        Ok(())
    }

    /// Get all sessions linked to a git commit.
    pub fn get_sessions_by_commit(&self, commit_hash: &str) -> Result<Vec<LocalSessionRow>> {
        let sql = format!(
            "SELECT {LOCAL_SESSION_COLUMNS} \
             {FROM_CLAUSE} \
             INNER JOIN commit_session_links csl ON csl.session_id = s.id \
             WHERE csl.commit_hash = ?1 AND COALESCE(s.is_auxiliary, 0) = 0 \
             ORDER BY s.created_at DESC"
        );
        let conn = self.conn();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![commit_hash], row_to_local_session)?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    /// Get all commits linked to a session.
    pub fn get_commits_by_session(&self, session_id: &str) -> Result<Vec<CommitLink>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT commit_hash, session_id, repo_path, branch, created_at \
             FROM commit_session_links WHERE session_id = ?1 \
             ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok(CommitLink {
                commit_hash: row.get(0)?,
                session_id: row.get(1)?,
                repo_path: row.get(2)?,
                branch: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    /// Find the most recently active session for a given repo path.
    /// "Active" means the session's working_directory matches the repo path
    /// and was created within the last `since_minutes` minutes.
    pub fn find_active_session_for_repo(
        &self,
        repo_path: &str,
        since_minutes: u32,
    ) -> Result<Option<LocalSessionRow>> {
        let sql = format!(
            "SELECT {LOCAL_SESSION_COLUMNS} \
             {FROM_CLAUSE} \
             WHERE s.working_directory LIKE ?1 \
             AND COALESCE(s.is_auxiliary, 0) = 0 \
             AND s.created_at >= datetime('now', ?2) \
             ORDER BY s.created_at DESC LIMIT 1"
        );
        let since = format!("-{since_minutes} minutes");
        let like = format!("{repo_path}%");
        let conn = self.conn();
        let mut stmt = conn.prepare(&sql)?;
        let row = stmt
            .query_map(params![like, since], row_to_local_session)?
            .next()
            .transpose()?;
        Ok(row)
    }

    /// Get all session IDs currently in the local DB.
    pub fn existing_session_ids(&self) -> std::collections::HashSet<String> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT id FROM sessions")
            .unwrap_or_else(|_| panic!("failed to prepare existing_session_ids query"));
        let rows = stmt.query_map([], |row| row.get::<_, String>(0));
        let mut set = std::collections::HashSet::new();
        if let Ok(rows) = rows {
            for row in rows.flatten() {
                set.insert(row);
            }
        }
        set
    }

    /// Update only stats fields for an existing session (no git context re-extraction).
    pub fn update_session_stats(&self, session: &Session) -> Result<()> {
        let title = session.context.title.as_deref();
        let description = session.context.description.as_deref();
        let (files_modified, files_read, has_errors) =
            opensession_core::extract::extract_file_metadata(session);
        let max_active_agents = opensession_core::agent_metrics::max_active_agents(session) as i64;
        let is_auxiliary = is_auxiliary_session(session);

        self.conn().execute(
            "UPDATE sessions SET \
             title=?2, description=?3, \
             message_count=?4, user_message_count=?5, task_count=?6, \
             event_count=?7, duration_seconds=?8, \
             total_input_tokens=?9, total_output_tokens=?10, \
              files_modified=?11, files_read=?12, has_errors=?13, \
             max_active_agents=?14, is_auxiliary=?15 \
             WHERE id=?1",
            params![
                &session.session_id,
                title,
                description,
                session.stats.message_count as i64,
                session.stats.user_message_count as i64,
                session.stats.task_count as i64,
                session.stats.event_count as i64,
                session.stats.duration_seconds as i64,
                session.stats.total_input_tokens as i64,
                session.stats.total_output_tokens as i64,
                &files_modified,
                &files_read,
                has_errors,
                max_active_agents,
                is_auxiliary as i64,
            ],
        )?;
        Ok(())
    }

    /// Update only sync metadata path for an existing session.
    pub fn set_session_sync_path(&self, session_id: &str, source_path: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO session_sync (session_id, source_path) \
             VALUES (?1, ?2) \
             ON CONFLICT(session_id) DO UPDATE SET source_path = excluded.source_path",
            params![session_id, source_path],
        )?;
        Ok(())
    }

    /// Get a list of distinct git repo names present in the DB.
    pub fn list_repos(&self) -> Result<Vec<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT git_repo_name FROM sessions \
             WHERE git_repo_name IS NOT NULL AND COALESCE(is_auxiliary, 0) = 0 \
             ORDER BY git_repo_name ASC",
        )?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    /// Get a list of distinct, non-empty working directories present in the DB.
    pub fn list_working_directories(&self) -> Result<Vec<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT working_directory FROM sessions \
             WHERE working_directory IS NOT NULL AND TRIM(working_directory) <> '' \
             AND COALESCE(is_auxiliary, 0) = 0 \
             ORDER BY working_directory ASC",
        )?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }
}

// ── Schema backfill for existing local DB files ───────────────────────

fn open_connection_with_latest_schema(path: &PathBuf) -> Result<Connection> {
    let conn = Connection::open(path).with_context(|| format!("open db {}", path.display()))?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;

    // Disable FK constraints for local DB (index/cache, not source of truth)
    conn.execute_batch("PRAGMA foreign_keys=OFF;")?;

    apply_local_migrations(&conn)?;

    // Backfill missing columns for existing local DB files where `sessions`
    // existed before newer fields were introduced.
    ensure_sessions_columns(&conn)?;
    repair_session_tools_from_source_path(&conn)?;
    validate_local_schema(&conn)?;

    Ok(conn)
}

fn apply_local_migrations(conn: &Connection) -> Result<()> {
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

        if let Err(e) = conn.execute_batch(sql) {
            let msg = e.to_string().to_ascii_lowercase();
            if !is_local_migration_compat_error(&msg) {
                return Err(e).with_context(|| format!("apply local migration {name}"));
            }
        }

        conn.execute(
            "INSERT OR IGNORE INTO _migrations (name) VALUES (?1)",
            [name],
        )
        .with_context(|| format!("record local migration {name}"))?;
    }

    Ok(())
}

fn is_local_migration_compat_error(msg: &str) -> bool {
    msg.contains("duplicate column name")
        || msg.contains("no such column")
        || msg.contains("already exists")
}

fn validate_local_schema(conn: &Connection) -> Result<()> {
    let sql = format!("SELECT {LOCAL_SESSION_COLUMNS} {FROM_CLAUSE} WHERE 1=0");
    conn.prepare(&sql)
        .map(|_| ())
        .context("validate local session schema")
}

fn repair_session_tools_from_source_path(conn: &Connection) -> Result<()> {
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

fn is_schema_compat_error(err: &anyhow::Error) -> bool {
    let msg = format!("{err:#}").to_ascii_lowercase();
    msg.contains("no such column")
        || msg.contains("no such table")
        || msg.contains("cannot add a column")
        || msg.contains("already exists")
        || msg.contains("views may not be indexed")
        || msg.contains("malformed database schema")
        || msg.contains("duplicate column name")
}

fn rotate_legacy_db(path: &PathBuf) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    let ts = chrono::Utc::now().format("%Y%m%d%H%M%S");
    let backup_name = format!(
        "{}.legacy-{}.bak",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("local.db"),
        ts
    );
    let backup_path = path.with_file_name(backup_name);
    std::fs::rename(path, &backup_path).with_context(|| {
        format!(
            "rotate local db backup {} -> {}",
            path.display(),
            backup_path.display()
        )
    })?;

    let wal = PathBuf::from(format!("{}-wal", path.display()));
    let shm = PathBuf::from(format!("{}-shm", path.display()));
    let _ = std::fs::remove_file(wal);
    let _ = std::fs::remove_file(shm);
    Ok(())
}

const REQUIRED_SESSION_COLUMNS: &[(&str, &str)] = &[
    ("user_id", "TEXT"),
    ("team_id", "TEXT DEFAULT 'personal'"),
    ("tool", "TEXT DEFAULT ''"),
    ("agent_provider", "TEXT"),
    ("agent_model", "TEXT"),
    ("title", "TEXT"),
    ("description", "TEXT"),
    ("tags", "TEXT"),
    ("created_at", "TEXT DEFAULT ''"),
    ("uploaded_at", "TEXT DEFAULT ''"),
    ("message_count", "INTEGER DEFAULT 0"),
    ("user_message_count", "INTEGER DEFAULT 0"),
    ("task_count", "INTEGER DEFAULT 0"),
    ("event_count", "INTEGER DEFAULT 0"),
    ("duration_seconds", "INTEGER DEFAULT 0"),
    ("total_input_tokens", "INTEGER DEFAULT 0"),
    ("total_output_tokens", "INTEGER DEFAULT 0"),
    // Migration-only compatibility column from pre git-native body storage.
    ("body_storage_key", "TEXT DEFAULT ''"),
    ("body_url", "TEXT"),
    ("git_remote", "TEXT"),
    ("git_branch", "TEXT"),
    ("git_commit", "TEXT"),
    ("git_repo_name", "TEXT"),
    ("pr_number", "INTEGER"),
    ("pr_url", "TEXT"),
    ("working_directory", "TEXT"),
    ("files_modified", "TEXT"),
    ("files_read", "TEXT"),
    ("has_errors", "BOOLEAN DEFAULT 0"),
    ("max_active_agents", "INTEGER DEFAULT 1"),
    ("is_auxiliary", "INTEGER NOT NULL DEFAULT 0"),
];

fn ensure_sessions_columns(conn: &Connection) -> Result<()> {
    let mut existing = HashSet::new();
    let mut stmt = conn.prepare("PRAGMA table_info(sessions)")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for row in rows {
        existing.insert(row?);
    }

    for (name, decl) in REQUIRED_SESSION_COLUMNS {
        if existing.contains(*name) {
            continue;
        }
        let sql = format!("ALTER TABLE sessions ADD COLUMN {name} {decl};");
        conn.execute_batch(&sql)
            .with_context(|| format!("add sessions column '{name}'"))?;
    }

    Ok(())
}

/// Column list for SELECT queries against sessions + session_sync + users.
pub const LOCAL_SESSION_COLUMNS: &str = "\
s.id, ss.source_path, COALESCE(ss.sync_status, 'unknown') AS sync_status, ss.last_synced_at, \
s.user_id, u.nickname, s.team_id, s.tool, s.agent_provider, s.agent_model, \
s.title, s.description, s.tags, s.created_at, s.uploaded_at, \
s.message_count, COALESCE(s.user_message_count, 0), s.task_count, s.event_count, s.duration_seconds, \
s.total_input_tokens, s.total_output_tokens, \
s.git_remote, s.git_branch, s.git_commit, s.git_repo_name, \
s.pr_number, s.pr_url, s.working_directory, \
s.files_modified, s.files_read, s.has_errors, COALESCE(s.max_active_agents, 1), COALESCE(s.is_auxiliary, 0)";

fn row_to_local_session(row: &rusqlite::Row) -> rusqlite::Result<LocalSessionRow> {
    let source_path: Option<String> = row.get(1)?;
    let tool: String = row.get(7)?;
    let normalized_tool = normalize_tool_for_source_path(&tool, source_path.as_deref());

    Ok(LocalSessionRow {
        id: row.get(0)?,
        source_path,
        sync_status: row.get(2)?,
        last_synced_at: row.get(3)?,
        user_id: row.get(4)?,
        nickname: row.get(5)?,
        team_id: row.get(6)?,
        tool: normalized_tool,
        agent_provider: row.get(8)?,
        agent_model: row.get(9)?,
        title: row.get(10)?,
        description: row.get(11)?,
        tags: row.get(12)?,
        created_at: row.get(13)?,
        uploaded_at: row.get(14)?,
        message_count: row.get(15)?,
        user_message_count: row.get(16)?,
        task_count: row.get(17)?,
        event_count: row.get(18)?,
        duration_seconds: row.get(19)?,
        total_input_tokens: row.get(20)?,
        total_output_tokens: row.get(21)?,
        git_remote: row.get(22)?,
        git_branch: row.get(23)?,
        git_commit: row.get(24)?,
        git_repo_name: row.get(25)?,
        pr_number: row.get(26)?,
        pr_url: row.get(27)?,
        working_directory: row.get(28)?,
        files_modified: row.get(29)?,
        files_read: row.get(30)?,
        has_errors: row.get::<_, i64>(31).unwrap_or(0) != 0,
        max_active_agents: row.get(32).unwrap_or(1),
        is_auxiliary: row.get::<_, i64>(33).unwrap_or(0) != 0,
    })
}

fn default_db_path() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .context("Could not determine home directory")?;
    Ok(PathBuf::from(home)
        .join(".local")
        .join("share")
        .join("opensession")
        .join("local.db"))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs::{create_dir_all, write};
    use tempfile::tempdir;

    fn test_db() -> LocalDb {
        let dir = tempdir().unwrap();
        let path = dir.keep().join("test.db");
        LocalDb::open_path(&path).unwrap()
    }

    fn temp_root() -> tempfile::TempDir {
        tempdir().unwrap()
    }

    fn make_row(id: &str, tool: &str, source_path: Option<&str>) -> LocalSessionRow {
        LocalSessionRow {
            id: id.to_string(),
            source_path: source_path.map(String::from),
            sync_status: "local_only".to_string(),
            last_synced_at: None,
            user_id: None,
            nickname: None,
            team_id: None,
            tool: tool.to_string(),
            agent_provider: None,
            agent_model: None,
            title: Some("test".to_string()),
            description: None,
            tags: None,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            uploaded_at: None,
            message_count: 0,
            user_message_count: 0,
            task_count: 0,
            event_count: 0,
            duration_seconds: 0,
            total_input_tokens: 0,
            total_output_tokens: 0,
            git_remote: None,
            git_branch: None,
            git_commit: None,
            git_repo_name: None,
            pr_number: None,
            pr_url: None,
            working_directory: None,
            files_modified: None,
            files_read: None,
            has_errors: false,
            max_active_agents: 1,
            is_auxiliary: false,
        }
    }

    #[test]
    fn test_open_and_schema() {
        let _db = test_db();
    }

    #[test]
    fn test_open_repairs_codex_tool_hint_from_source_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("repair.db");

        {
            let _ = LocalDb::open_path(&path).unwrap();
        }

        {
            let conn = Connection::open(&path).unwrap();
            conn.execute(
                "INSERT INTO sessions (id, team_id, tool, created_at, body_storage_key) VALUES (?1, 'personal', 'claude-code', ?2, '')",
                params!["rollout-repair", "2026-02-20T00:00:00Z"],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO session_sync (session_id, source_path, sync_status) VALUES (?1, ?2, 'local_only')",
                params!["rollout-repair", "/Users/test/.codex/sessions/2026/02/20/rollout-repair.jsonl"],
            )
            .unwrap();
        }

        let db = LocalDb::open_path(&path).unwrap();
        let rows = db.list_sessions(&LocalSessionFilter::default()).unwrap();
        let row = rows
            .iter()
            .find(|row| row.id == "rollout-repair")
            .expect("repaired row");
        assert_eq!(row.tool, "codex");
    }

    #[test]
    fn test_upsert_local_session_normalizes_tool_from_source_path() {
        let db = test_db();
        let mut session = Session::new(
            "rollout-upsert".to_string(),
            opensession_core::trace::Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "claude-code".to_string(),
                tool_version: None,
            },
        );
        session.stats.event_count = 1;

        db.upsert_local_session(
            &session,
            "/Users/test/.codex/sessions/2026/02/20/rollout-upsert.jsonl",
            &crate::git::GitContext::default(),
        )
        .unwrap();

        let rows = db.list_sessions(&LocalSessionFilter::default()).unwrap();
        let row = rows
            .iter()
            .find(|row| row.id == "rollout-upsert")
            .expect("upserted row");
        assert_eq!(row.tool, "codex");
    }

    #[test]
    fn test_open_backfills_legacy_sessions_columns() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE sessions (id TEXT PRIMARY KEY);
                 INSERT INTO sessions (id) VALUES ('legacy-1');",
            )
            .unwrap();
        }

        let db = LocalDb::open_path(&path).unwrap();
        let rows = db.list_sessions(&LocalSessionFilter::default()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "legacy-1");
        assert_eq!(rows[0].user_message_count, 0);
    }

    #[test]
    fn test_open_rotates_incompatible_legacy_schema() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("CREATE VIEW sessions AS SELECT 'x' AS id;")
                .unwrap();
        }

        let db = LocalDb::open_path(&path).unwrap();
        let rows = db.list_sessions(&LocalSessionFilter::default()).unwrap();
        assert!(rows.is_empty());

        let rotated = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                name.starts_with("broken.db.legacy-") && name.ends_with(".bak")
            });
        assert!(rotated, "expected rotated legacy backup file");
    }

    #[test]
    fn test_is_opencode_child_session() {
        let root = temp_root();
        let dir = root.path().join("sessions");
        create_dir_all(&dir).unwrap();
        let parent_session = dir.join("parent.json");
        write(
            &parent_session,
            r#"{"id":"ses_parent","time":{"created":1000,"updated":1000}}"#,
        )
        .unwrap();
        let child_session = dir.join("child.json");
        write(
            &child_session,
            r#"{"id":"ses_child","parentID":"ses_parent","time":{"created":1000,"updated":1000}}"#,
        )
        .unwrap();

        let parent = make_row(
            "ses_parent",
            "opencode",
            Some(parent_session.to_str().unwrap()),
        );
        let mut child = make_row(
            "ses_child",
            "opencode",
            Some(child_session.to_str().unwrap()),
        );
        child.is_auxiliary = true;
        let mut codex = make_row("ses_other", "codex", Some(child_session.to_str().unwrap()));
        codex.is_auxiliary = true;

        assert!(!is_opencode_child_session(&parent));
        assert!(is_opencode_child_session(&child));
        assert!(!is_opencode_child_session(&codex));
    }

    #[allow(deprecated)]
    #[test]
    fn test_parse_opencode_parent_session_id_aliases() {
        let root = temp_root();
        let dir = root.path().join("session-aliases");
        create_dir_all(&dir).unwrap();
        let child_session = dir.join("child.json");
        write(
            &child_session,
            r#"{"id":"ses_child","parentUUID":"ses_parent","time":{"created":1000,"updated":1000}}"#,
        )
        .unwrap();
        assert_eq!(
            parse_opencode_parent_session_id(child_session.to_str().unwrap()).as_deref(),
            Some("ses_parent")
        );
    }

    #[allow(deprecated)]
    #[test]
    fn test_parse_opencode_parent_session_id_nested_metadata() {
        let root = temp_root();
        let dir = root.path().join("session-nested");
        create_dir_all(&dir).unwrap();
        let child_session = dir.join("child.json");
        write(
            &child_session,
            r#"{"id":"ses_child","metadata":{"links":{"parentSessionId":"ses_parent","trace":"x"}}}"#,
        )
        .unwrap();
        assert_eq!(
            parse_opencode_parent_session_id(child_session.to_str().unwrap()).as_deref(),
            Some("ses_parent")
        );
    }

    #[test]
    fn test_hide_opencode_child_sessions() {
        let root = temp_root();
        let dir = root.path().join("sessions");
        create_dir_all(&dir).unwrap();
        let parent_session = dir.join("parent.json");
        let child_session = dir.join("child.json");
        let orphan_session = dir.join("orphan.json");

        write(
            &parent_session,
            r#"{"id":"ses_parent","time":{"created":1000,"updated":1000}}"#,
        )
        .unwrap();
        write(
            &child_session,
            r#"{"id":"ses_child","parentID":"ses_parent","time":{"created":1000,"updated":1000}}"#,
        )
        .unwrap();
        write(
            &orphan_session,
            r#"{"id":"ses_orphan","time":{"created":1000,"updated":1000}}"#,
        )
        .unwrap();

        let rows = vec![
            {
                let mut row = make_row(
                    "ses_child",
                    "opencode",
                    Some(child_session.to_str().unwrap()),
                );
                row.is_auxiliary = true;
                row
            },
            make_row(
                "ses_parent",
                "opencode",
                Some(parent_session.to_str().unwrap()),
            ),
            {
                let mut row = make_row("ses_other", "codex", None);
                row.user_message_count = 1;
                row
            },
            make_row(
                "ses_orphan",
                "opencode",
                Some(orphan_session.to_str().unwrap()),
            ),
        ];

        let filtered = hide_opencode_child_sessions(rows);
        assert_eq!(filtered.len(), 3);
        assert!(filtered.iter().all(|r| r.id != "ses_child"));
    }

    #[test]
    fn test_sync_cursor() {
        let db = test_db();
        assert_eq!(db.get_sync_cursor("team1").unwrap(), None);
        db.set_sync_cursor("team1", "2024-01-01T00:00:00Z").unwrap();
        assert_eq!(
            db.get_sync_cursor("team1").unwrap(),
            Some("2024-01-01T00:00:00Z".to_string())
        );
        // Update
        db.set_sync_cursor("team1", "2024-06-01T00:00:00Z").unwrap();
        assert_eq!(
            db.get_sync_cursor("team1").unwrap(),
            Some("2024-06-01T00:00:00Z".to_string())
        );
    }

    #[test]
    fn test_body_cache() {
        let db = test_db();
        assert_eq!(db.get_cached_body("s1").unwrap(), None);
        db.cache_body("s1", b"hello world").unwrap();
        assert_eq!(
            db.get_cached_body("s1").unwrap(),
            Some(b"hello world".to_vec())
        );
    }

    #[test]
    fn test_local_migrations_are_loaded_from_api_crate() {
        let migration_names: Vec<&str> = super::LOCAL_MIGRATIONS
            .iter()
            .map(|(name, _)| *name)
            .collect();
        assert!(
            migration_names.contains(&"local_0003_session_flags"),
            "expected local_0003_session_flags migration from opensession-api"
        );

        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let migrations_dir = manifest_dir.join("migrations");
        if migrations_dir.exists() {
            let sql_files = std::fs::read_dir(migrations_dir)
                .expect("read local-db migrations directory")
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().to_string())
                .filter(|name| name.ends_with(".sql"))
                .collect::<Vec<_>>();
            assert!(
                sql_files.is_empty(),
                "local-db must not ship duplicated migration SQL files"
            );
        }
    }

    #[test]
    fn test_local_0003_session_flags_backfills_known_auxiliary_rows() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("local-legacy.db");

        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS _migrations (
                    id INTEGER PRIMARY KEY,
                    name TEXT NOT NULL UNIQUE,
                    applied_at TEXT NOT NULL DEFAULT (datetime('now'))
                );",
            )
            .unwrap();

            for (name, sql) in super::MIGRATIONS.iter().chain(
                super::LOCAL_MIGRATIONS
                    .iter()
                    .filter(|(name, _)| *name != "local_0003_session_flags"),
            ) {
                conn.execute_batch(sql).unwrap();
                conn.execute(
                    "INSERT OR IGNORE INTO _migrations (name) VALUES (?1)",
                    params![name],
                )
                .unwrap();
            }

            conn.execute(
                "INSERT INTO sessions \
                 (id, team_id, tool, created_at, body_storage_key, message_count, user_message_count, task_count, event_count) \
                 VALUES (?1, 'personal', 'codex', '2026-02-20T00:00:00Z', '', 12, 6, 6, 20)",
                params!["primary-visible"],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO sessions \
                 (id, team_id, tool, created_at, body_storage_key, message_count, user_message_count, task_count, event_count) \
                 VALUES (?1, 'personal', 'opencode', '2026-02-20T00:00:00Z', '', 2, 0, 2, 6)",
                params!["opencode-heuristic-child"],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO sessions \
                 (id, team_id, tool, created_at, body_storage_key, message_count, user_message_count, task_count, event_count) \
                 VALUES (?1, 'personal', 'claude-code', '2026-02-20T00:00:00Z', '', 3, 1, 3, 12)",
                params!["claude-subagent-child"],
            )
            .unwrap();

            conn.execute(
                "INSERT INTO session_sync (session_id, source_path, sync_status) VALUES (?1, ?2, 'local_only')",
                params![
                    "opencode-heuristic-child",
                    "/Users/test/.opencode/sessions/opencode-heuristic-child.json"
                ],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO session_sync (session_id, source_path, sync_status) VALUES (?1, ?2, 'local_only')",
                params![
                    "claude-subagent-child",
                    "/Users/test/.claude/projects/foo/subagents/agent-1.jsonl"
                ],
            )
            .unwrap();
        }

        let db = LocalDb::open_path(&path).unwrap();

        let flags = {
            let conn = db.conn();
            let mut stmt = conn
                .prepare("SELECT id, COALESCE(is_auxiliary, 0) FROM sessions ORDER BY id")
                .unwrap();
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })
                .unwrap();
            rows.collect::<Result<Vec<_>, _>>().unwrap()
        };

        assert_eq!(
            flags,
            vec![
                ("claude-subagent-child".to_string(), 1),
                ("opencode-heuristic-child".to_string(), 1),
                ("primary-visible".to_string(), 0),
            ]
        );

        let visible = db.list_sessions(&LocalSessionFilter::default()).unwrap();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].id, "primary-visible");
    }

    #[test]
    fn test_upsert_remote_session() {
        let db = test_db();
        let summary = RemoteSessionSummary {
            id: "remote-1".to_string(),
            user_id: Some("u1".to_string()),
            nickname: Some("alice".to_string()),
            team_id: "t1".to_string(),
            tool: "claude-code".to_string(),
            agent_provider: None,
            agent_model: None,
            title: Some("Test session".to_string()),
            description: None,
            tags: None,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            uploaded_at: "2024-01-01T01:00:00Z".to_string(),
            message_count: 10,
            task_count: 2,
            event_count: 20,
            duration_seconds: 300,
            total_input_tokens: 1000,
            total_output_tokens: 500,
            git_remote: None,
            git_branch: None,
            git_commit: None,
            git_repo_name: None,
            pr_number: None,
            pr_url: None,
            working_directory: None,
            files_modified: None,
            files_read: None,
            has_errors: false,
            max_active_agents: 1,
        };
        db.upsert_remote_session(&summary).unwrap();

        let sessions = db.list_sessions(&LocalSessionFilter::default()).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "remote-1");
        assert_eq!(sessions[0].sync_status, "remote_only");
        assert_eq!(sessions[0].nickname, None); // no user in local users table
        assert!(!sessions[0].is_auxiliary);
    }

    #[test]
    fn test_list_filter_by_repo() {
        let db = test_db();
        // Insert a remote session with team_id
        let summary1 = RemoteSessionSummary {
            id: "s1".to_string(),
            user_id: None,
            nickname: None,
            team_id: "t1".to_string(),
            tool: "claude-code".to_string(),
            agent_provider: None,
            agent_model: None,
            title: Some("Session 1".to_string()),
            description: None,
            tags: None,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            uploaded_at: "2024-01-01T01:00:00Z".to_string(),
            message_count: 5,
            task_count: 0,
            event_count: 10,
            duration_seconds: 60,
            total_input_tokens: 100,
            total_output_tokens: 50,
            git_remote: None,
            git_branch: None,
            git_commit: None,
            git_repo_name: None,
            pr_number: None,
            pr_url: None,
            working_directory: None,
            files_modified: None,
            files_read: None,
            has_errors: false,
            max_active_agents: 1,
        };
        db.upsert_remote_session(&summary1).unwrap();

        // Filter by team
        let filter = LocalSessionFilter {
            team_id: Some("t1".to_string()),
            ..Default::default()
        };
        assert_eq!(db.list_sessions(&filter).unwrap().len(), 1);

        let filter = LocalSessionFilter {
            team_id: Some("t999".to_string()),
            ..Default::default()
        };
        assert_eq!(db.list_sessions(&filter).unwrap().len(), 0);
    }

    // ── Helpers for inserting test sessions ────────────────────────────

    fn make_summary(id: &str, tool: &str, title: &str, created_at: &str) -> RemoteSessionSummary {
        RemoteSessionSummary {
            id: id.to_string(),
            user_id: None,
            nickname: None,
            team_id: "t1".to_string(),
            tool: tool.to_string(),
            agent_provider: Some("anthropic".to_string()),
            agent_model: Some("claude-opus-4-6".to_string()),
            title: Some(title.to_string()),
            description: None,
            tags: None,
            created_at: created_at.to_string(),
            uploaded_at: created_at.to_string(),
            message_count: 5,
            task_count: 1,
            event_count: 10,
            duration_seconds: 300,
            total_input_tokens: 1000,
            total_output_tokens: 500,
            git_remote: None,
            git_branch: None,
            git_commit: None,
            git_repo_name: None,
            pr_number: None,
            pr_url: None,
            working_directory: None,
            files_modified: None,
            files_read: None,
            has_errors: false,
            max_active_agents: 1,
        }
    }

    fn seed_sessions(db: &LocalDb) {
        // Insert 5 sessions across two tools, ordered by created_at
        db.upsert_remote_session(&make_summary(
            "s1",
            "claude-code",
            "First session",
            "2024-01-01T00:00:00Z",
        ))
        .unwrap();
        db.upsert_remote_session(&make_summary(
            "s2",
            "claude-code",
            "JWT auth work",
            "2024-01-02T00:00:00Z",
        ))
        .unwrap();
        db.upsert_remote_session(&make_summary(
            "s3",
            "gemini",
            "Gemini test",
            "2024-01-03T00:00:00Z",
        ))
        .unwrap();
        db.upsert_remote_session(&make_summary(
            "s4",
            "claude-code",
            "Error handling",
            "2024-01-04T00:00:00Z",
        ))
        .unwrap();
        db.upsert_remote_session(&make_summary(
            "s5",
            "claude-code",
            "Final polish",
            "2024-01-05T00:00:00Z",
        ))
        .unwrap();
    }

    // ── list_sessions_log tests ────────────────────────────────────────

    #[test]
    fn test_log_no_filters() {
        let db = test_db();
        seed_sessions(&db);
        let filter = LogFilter::default();
        let results = db.list_sessions_log(&filter).unwrap();
        assert_eq!(results.len(), 5);
        // Should be ordered by created_at DESC
        assert_eq!(results[0].id, "s5");
        assert_eq!(results[4].id, "s1");
    }

    #[test]
    fn test_log_filter_by_tool() {
        let db = test_db();
        seed_sessions(&db);
        let filter = LogFilter {
            tool: Some("claude-code".to_string()),
            ..Default::default()
        };
        let results = db.list_sessions_log(&filter).unwrap();
        assert_eq!(results.len(), 4);
        assert!(results.iter().all(|s| s.tool == "claude-code"));
    }

    #[test]
    fn test_log_filter_by_model_wildcard() {
        let db = test_db();
        seed_sessions(&db);
        let filter = LogFilter {
            model: Some("claude*".to_string()),
            ..Default::default()
        };
        let results = db.list_sessions_log(&filter).unwrap();
        assert_eq!(results.len(), 5); // all have claude-opus model
    }

    #[test]
    fn test_log_filter_since() {
        let db = test_db();
        seed_sessions(&db);
        let filter = LogFilter {
            since: Some("2024-01-03T00:00:00Z".to_string()),
            ..Default::default()
        };
        let results = db.list_sessions_log(&filter).unwrap();
        assert_eq!(results.len(), 3); // s3, s4, s5
    }

    #[test]
    fn test_log_filter_before() {
        let db = test_db();
        seed_sessions(&db);
        let filter = LogFilter {
            before: Some("2024-01-03T00:00:00Z".to_string()),
            ..Default::default()
        };
        let results = db.list_sessions_log(&filter).unwrap();
        assert_eq!(results.len(), 2); // s1, s2
    }

    #[test]
    fn test_log_filter_since_and_before() {
        let db = test_db();
        seed_sessions(&db);
        let filter = LogFilter {
            since: Some("2024-01-02T00:00:00Z".to_string()),
            before: Some("2024-01-04T00:00:00Z".to_string()),
            ..Default::default()
        };
        let results = db.list_sessions_log(&filter).unwrap();
        assert_eq!(results.len(), 2); // s2, s3
    }

    #[test]
    fn test_log_filter_grep() {
        let db = test_db();
        seed_sessions(&db);
        let filter = LogFilter {
            grep: Some("JWT".to_string()),
            ..Default::default()
        };
        let results = db.list_sessions_log(&filter).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "s2");
    }

    #[test]
    fn test_log_limit_and_offset() {
        let db = test_db();
        seed_sessions(&db);
        let filter = LogFilter {
            limit: Some(2),
            offset: Some(1),
            ..Default::default()
        };
        let results = db.list_sessions_log(&filter).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, "s4"); // second most recent
        assert_eq!(results[1].id, "s3");
    }

    #[test]
    fn test_log_limit_only() {
        let db = test_db();
        seed_sessions(&db);
        let filter = LogFilter {
            limit: Some(3),
            ..Default::default()
        };
        let results = db.list_sessions_log(&filter).unwrap();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_list_sessions_limit_offset() {
        let db = test_db();
        seed_sessions(&db);
        let filter = LocalSessionFilter {
            limit: Some(2),
            offset: Some(1),
            ..Default::default()
        };
        let results = db.list_sessions(&filter).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, "s4");
        assert_eq!(results[1].id, "s3");
    }

    #[test]
    fn test_count_sessions_filtered() {
        let db = test_db();
        seed_sessions(&db);
        let count = db
            .count_sessions_filtered(&LocalSessionFilter::default())
            .unwrap();
        assert_eq!(count, 5);
    }

    #[test]
    fn test_list_and_count_filters_match_when_auxiliary_rows_exist() {
        let db = test_db();
        seed_sessions(&db);
        db.conn()
            .execute(
                "UPDATE sessions SET is_auxiliary = 1 WHERE id IN ('s2', 's3')",
                [],
            )
            .unwrap();

        let default_filter = LocalSessionFilter::default();
        let rows = db.list_sessions(&default_filter).unwrap();
        let count = db.count_sessions_filtered(&default_filter).unwrap();
        assert_eq!(rows.len() as i64, count);
        assert!(rows.iter().all(|row| !row.is_auxiliary));

        let gemini_filter = LocalSessionFilter {
            tool: Some("gemini".to_string()),
            ..Default::default()
        };
        let gemini_rows = db.list_sessions(&gemini_filter).unwrap();
        let gemini_count = db.count_sessions_filtered(&gemini_filter).unwrap();
        assert_eq!(gemini_rows.len() as i64, gemini_count);
        assert!(gemini_rows.is_empty());
        assert_eq!(gemini_count, 0);
    }

    #[test]
    fn test_list_working_directories_distinct_non_empty() {
        let db = test_db();

        let mut a = make_summary("wd-1", "claude-code", "One", "2024-01-01T00:00:00Z");
        a.working_directory = Some("/tmp/repo-a".to_string());
        let mut b = make_summary("wd-2", "claude-code", "Two", "2024-01-02T00:00:00Z");
        b.working_directory = Some("/tmp/repo-a".to_string());
        let mut c = make_summary("wd-3", "claude-code", "Three", "2024-01-03T00:00:00Z");
        c.working_directory = Some("/tmp/repo-b".to_string());
        let mut d = make_summary("wd-4", "claude-code", "Four", "2024-01-04T00:00:00Z");
        d.working_directory = Some("".to_string());

        db.upsert_remote_session(&a).unwrap();
        db.upsert_remote_session(&b).unwrap();
        db.upsert_remote_session(&c).unwrap();
        db.upsert_remote_session(&d).unwrap();

        let dirs = db.list_working_directories().unwrap();
        assert_eq!(
            dirs,
            vec!["/tmp/repo-a".to_string(), "/tmp/repo-b".to_string()]
        );
    }

    #[test]
    fn test_list_session_tools() {
        let db = test_db();
        seed_sessions(&db);
        let tools = db
            .list_session_tools(&LocalSessionFilter::default())
            .unwrap();
        assert_eq!(tools, vec!["claude-code".to_string(), "gemini".to_string()]);
    }

    #[test]
    fn test_log_combined_filters() {
        let db = test_db();
        seed_sessions(&db);
        let filter = LogFilter {
            tool: Some("claude-code".to_string()),
            since: Some("2024-01-03T00:00:00Z".to_string()),
            limit: Some(1),
            ..Default::default()
        };
        let results = db.list_sessions_log(&filter).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "s5"); // most recent claude-code after Jan 3
    }

    // ── Session offset/latest tests ────────────────────────────────────

    #[test]
    fn test_get_session_by_offset() {
        let db = test_db();
        seed_sessions(&db);
        let row = db.get_session_by_offset(0).unwrap().unwrap();
        assert_eq!(row.id, "s5"); // most recent
        let row = db.get_session_by_offset(2).unwrap().unwrap();
        assert_eq!(row.id, "s3");
        assert!(db.get_session_by_offset(10).unwrap().is_none());
    }

    #[test]
    fn test_get_session_by_tool_offset() {
        let db = test_db();
        seed_sessions(&db);
        let row = db
            .get_session_by_tool_offset("claude-code", 0)
            .unwrap()
            .unwrap();
        assert_eq!(row.id, "s5");
        let row = db
            .get_session_by_tool_offset("claude-code", 1)
            .unwrap()
            .unwrap();
        assert_eq!(row.id, "s4");
        let row = db.get_session_by_tool_offset("gemini", 0).unwrap().unwrap();
        assert_eq!(row.id, "s3");
        assert!(db
            .get_session_by_tool_offset("gemini", 1)
            .unwrap()
            .is_none());
    }

    #[test]
    fn test_get_sessions_latest() {
        let db = test_db();
        seed_sessions(&db);
        let rows = db.get_sessions_latest(3).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].id, "s5");
        assert_eq!(rows[1].id, "s4");
        assert_eq!(rows[2].id, "s3");
    }

    #[test]
    fn test_get_sessions_by_tool_latest() {
        let db = test_db();
        seed_sessions(&db);
        let rows = db.get_sessions_by_tool_latest("claude-code", 2).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, "s5");
        assert_eq!(rows[1].id, "s4");
    }

    #[test]
    fn test_get_sessions_latest_more_than_available() {
        let db = test_db();
        seed_sessions(&db);
        let rows = db.get_sessions_by_tool_latest("gemini", 10).unwrap();
        assert_eq!(rows.len(), 1); // only 1 gemini session
    }

    #[test]
    fn test_session_count() {
        let db = test_db();
        assert_eq!(db.session_count().unwrap(), 0);
        seed_sessions(&db);
        assert_eq!(db.session_count().unwrap(), 5);
    }

    // ── Commit link tests ─────────────────────────────────────────────

    #[test]
    fn test_link_commit_session() {
        let db = test_db();
        seed_sessions(&db);
        db.link_commit_session("abc123", "s1", Some("/tmp/repo"), Some("main"))
            .unwrap();

        let commits = db.get_commits_by_session("s1").unwrap();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].commit_hash, "abc123");
        assert_eq!(commits[0].session_id, "s1");
        assert_eq!(commits[0].repo_path.as_deref(), Some("/tmp/repo"));
        assert_eq!(commits[0].branch.as_deref(), Some("main"));

        let sessions = db.get_sessions_by_commit("abc123").unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "s1");
    }

    #[test]
    fn test_get_sessions_by_commit() {
        let db = test_db();
        seed_sessions(&db);
        // Link multiple sessions to the same commit
        db.link_commit_session("abc123", "s1", None, None).unwrap();
        db.link_commit_session("abc123", "s2", None, None).unwrap();
        db.link_commit_session("abc123", "s3", None, None).unwrap();

        let sessions = db.get_sessions_by_commit("abc123").unwrap();
        assert_eq!(sessions.len(), 3);
        // Ordered by created_at DESC
        assert_eq!(sessions[0].id, "s3");
        assert_eq!(sessions[1].id, "s2");
        assert_eq!(sessions[2].id, "s1");
    }

    #[test]
    fn test_get_commits_by_session() {
        let db = test_db();
        seed_sessions(&db);
        // Link multiple commits to the same session
        db.link_commit_session("aaa111", "s1", Some("/repo"), Some("main"))
            .unwrap();
        db.link_commit_session("bbb222", "s1", Some("/repo"), Some("main"))
            .unwrap();
        db.link_commit_session("ccc333", "s1", Some("/repo"), Some("feat"))
            .unwrap();

        let commits = db.get_commits_by_session("s1").unwrap();
        assert_eq!(commits.len(), 3);
        // All linked to s1
        assert!(commits.iter().all(|c| c.session_id == "s1"));
    }

    #[test]
    fn test_duplicate_link_ignored() {
        let db = test_db();
        seed_sessions(&db);
        db.link_commit_session("abc123", "s1", Some("/repo"), Some("main"))
            .unwrap();
        // Inserting the same link again should not error
        db.link_commit_session("abc123", "s1", Some("/repo"), Some("main"))
            .unwrap();

        let commits = db.get_commits_by_session("s1").unwrap();
        assert_eq!(commits.len(), 1);
    }

    #[test]
    fn test_log_filter_by_commit() {
        let db = test_db();
        seed_sessions(&db);
        db.link_commit_session("abc123", "s2", None, None).unwrap();
        db.link_commit_session("abc123", "s4", None, None).unwrap();

        let filter = LogFilter {
            commit: Some("abc123".to_string()),
            ..Default::default()
        };
        let results = db.list_sessions_log(&filter).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, "s4");
        assert_eq!(results[1].id, "s2");

        // Non-existent commit returns nothing
        let filter = LogFilter {
            commit: Some("nonexistent".to_string()),
            ..Default::default()
        };
        let results = db.list_sessions_log(&filter).unwrap();
        assert_eq!(results.len(), 0);
    }
}
