pub mod git;

use anyhow::{Context, Result};
use opensession_api::db::migrations::{LOCAL_MIGRATIONS, MIGRATIONS};
use opensession_core::session::{is_auxiliary_session, working_directory};
use opensession_core::trace::Session;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::Mutex;

use git::{normalize_repo_name, GitContext};

const SUMMARY_WORKER_TITLE_PREFIX_LOWER: &str =
    "convert a real coding session into semantic compression.";

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

/// A lightweight local link row for session-to-session relationships.
#[derive(Debug, Clone)]
pub struct LocalSessionLink {
    pub session_id: String,
    pub linked_session_id: String,
    pub link_type: String,
    pub created_at: String,
}

/// Session-level semantic summary row persisted in local SQLite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSemanticSummaryRow {
    pub session_id: String,
    pub summary_json: String,
    pub generated_at: String,
    pub provider: String,
    pub model: Option<String>,
    pub source_kind: String,
    pub generation_kind: String,
    pub prompt_fingerprint: Option<String>,
    pub source_details_json: Option<String>,
    pub diff_tree_json: Option<String>,
    pub error: Option<String>,
    pub updated_at: String,
}

/// Upsert payload for session-level semantic summaries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSemanticSummaryUpsert<'a> {
    pub session_id: &'a str,
    pub summary_json: &'a str,
    pub generated_at: &'a str,
    pub provider: &'a str,
    pub model: Option<&'a str>,
    pub source_kind: &'a str,
    pub generation_kind: &'a str,
    pub prompt_fingerprint: Option<&'a str>,
    pub source_details_json: Option<&'a str>,
    pub diff_tree_json: Option<&'a str>,
    pub error: Option<&'a str>,
}

/// Vector chunk payload persisted per session.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorChunkUpsert {
    pub chunk_id: String,
    pub session_id: String,
    pub chunk_index: u32,
    pub start_line: u32,
    pub end_line: u32,
    pub line_count: u32,
    pub content: String,
    pub content_hash: String,
    pub embedding: Vec<f32>,
}

/// Candidate row used for local semantic vector ranking.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorChunkCandidateRow {
    pub chunk_id: String,
    pub session_id: String,
    pub start_line: u32,
    pub end_line: u32,
    pub content: String,
    pub embedding: Vec<f32>,
}

/// Vector indexing progress/status snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorIndexJobRow {
    pub status: String,
    pub processed_sessions: u32,
    pub total_sessions: u32,
    pub message: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

/// Summary batch generation progress/status snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryBatchJobRow {
    pub status: String,
    pub processed_sessions: u32,
    pub total_sessions: u32,
    pub failed_sessions: u32,
    pub message: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
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

fn normalize_non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn build_fts_query(raw: &str) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for token in raw.split_whitespace() {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            continue;
        }
        let escaped = trimmed.replace('"', "\"\"");
        parts.push(format!("\"{escaped}\""));
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join(" OR "))
}

fn json_object_string(value: &Value, keys: &[&str]) -> Option<String> {
    let obj = value.as_object()?;
    for key in keys {
        if let Some(found) = obj.get(*key).and_then(Value::as_str) {
            let normalized = found.trim();
            if !normalized.is_empty() {
                return Some(normalized.to_string());
            }
        }
    }
    None
}

fn git_context_from_session_attributes(session: &Session) -> GitContext {
    let attrs = &session.context.attributes;

    let mut remote = normalize_non_empty(attrs.get("git_remote").and_then(Value::as_str));
    let mut branch = normalize_non_empty(attrs.get("git_branch").and_then(Value::as_str));
    let mut commit = normalize_non_empty(attrs.get("git_commit").and_then(Value::as_str));
    let mut repo_name = normalize_non_empty(attrs.get("git_repo_name").and_then(Value::as_str));

    if let Some(git_value) = attrs.get("git") {
        if remote.is_none() {
            remote = json_object_string(
                git_value,
                &["remote", "repository_url", "repo_url", "origin", "url"],
            );
        }
        if branch.is_none() {
            branch = json_object_string(
                git_value,
                &["branch", "git_branch", "current_branch", "ref", "head"],
            );
        }
        if commit.is_none() {
            commit = json_object_string(git_value, &["commit", "commit_hash", "sha", "git_commit"]);
        }
        if repo_name.is_none() {
            repo_name = json_object_string(git_value, &["repo_name", "repository", "repo", "name"]);
        }
    }

    if repo_name.is_none() {
        repo_name = remote
            .as_deref()
            .and_then(normalize_repo_name)
            .map(ToOwned::to_owned);
    }

    GitContext {
        remote,
        branch,
        commit,
        repo_name,
    }
}

fn git_context_has_any_field(git: &GitContext) -> bool {
    git.remote.is_some() || git.branch.is_some() || git.commit.is_some() || git.repo_name.is_some()
}

fn merge_git_context(preferred: &GitContext, fallback: &GitContext) -> GitContext {
    GitContext {
        remote: preferred.remote.clone().or_else(|| fallback.remote.clone()),
        branch: preferred.branch.clone().or_else(|| fallback.branch.clone()),
        commit: preferred.commit.clone().or_else(|| fallback.commit.clone()),
        repo_name: preferred
            .repo_name
            .clone()
            .or_else(|| fallback.repo_name.clone()),
    }
}

/// Filter for listing sessions from the local DB.
#[derive(Debug, Clone)]
pub struct LocalSessionFilter {
    pub team_id: Option<String>,
    pub sync_status: Option<String>,
    pub git_repo_name: Option<String>,
    pub search: Option<String>,
    pub exclude_low_signal: bool,
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
            exclude_low_signal: false,
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
        let conn = open_connection_with_latest_schema(path)
            .with_context(|| format!("open local db {}", path.display()))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
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
        let git_from_session = git_context_from_session_attributes(session);
        let has_session_git = git_context_has_any_field(&git_from_session);
        let merged_git = merge_git_context(&git_from_session, git);

        let conn = self.conn();
        // Body contents are resolved via canonical body URLs and local body cache.
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
              git_remote=CASE WHEN ?26=1 THEN excluded.git_remote ELSE COALESCE(git_remote, excluded.git_remote) END, \
              git_branch=CASE WHEN ?26=1 THEN excluded.git_branch ELSE COALESCE(git_branch, excluded.git_branch) END, \
              git_commit=CASE WHEN ?26=1 THEN excluded.git_commit ELSE COALESCE(git_commit, excluded.git_commit) END, \
              git_repo_name=CASE WHEN ?26=1 THEN excluded.git_repo_name ELSE COALESCE(git_repo_name, excluded.git_repo_name) END, \
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
                &merged_git.remote,
                &merged_git.branch,
                &merged_git.commit,
                &merged_git.repo_name,
                &cwd,
                &files_modified,
                &files_read,
                has_errors,
                max_active_agents,
                is_auxiliary as i64,
                has_session_git as i64,
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
        // Body contents are resolved via canonical body URLs and local body cache.
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
            format!(
                "NOT (LOWER(COALESCE(s.tool, '')) = 'codex' \
                 AND LOWER(COALESCE(s.title, '')) LIKE '{}%')",
                SUMMARY_WORKER_TITLE_PREFIX_LOWER
            ),
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

        if filter.exclude_low_signal {
            where_clauses.push(
                "NOT (COALESCE(s.message_count, 0) = 0 \
                  AND COALESCE(s.user_message_count, 0) = 0 \
                  AND COALESCE(s.task_count, 0) = 0 \
                  AND COALESCE(s.event_count, 0) <= 2 \
                  AND (s.title IS NULL OR TRIM(s.title) = ''))"
                    .to_string(),
            );
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
            format!(
                "NOT (LOWER(COALESCE(s.tool, '')) = 'codex' \
                 AND LOWER(COALESCE(s.title, '')) LIKE '{}%')",
                SUMMARY_WORKER_TITLE_PREFIX_LOWER
            ),
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

        let _ = idx; // suppress unused warning

        let where_str = where_clauses.join(" AND ");
        let mut sql = format!(
            "SELECT {LOCAL_SESSION_COLUMNS} \
             {FROM_CLAUSE} WHERE {where_str} \
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

    /// List every session id with a non-empty source path from session_sync.
    pub fn list_session_source_paths(&self) -> Result<Vec<(String, String)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT session_id, source_path \
             FROM session_sync \
             WHERE source_path IS NOT NULL AND TRIM(source_path) != ''",
        )?;
        let rows = stmt.query_map([], |row| {
            let session_id: String = row.get(0)?;
            let source_path: String = row.get(1)?;
            Ok((session_id, source_path))
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Get a single session row by id.
    pub fn get_session_by_id(&self, session_id: &str) -> Result<Option<LocalSessionRow>> {
        let sql = format!(
            "SELECT {LOCAL_SESSION_COLUMNS} \
             {FROM_CLAUSE} WHERE s.id = ?1 LIMIT 1"
        );
        let conn = self.conn();
        let mut stmt = conn.prepare(&sql)?;
        let row = stmt
            .query_map(params![session_id], row_to_local_session)?
            .next()
            .transpose()?;
        Ok(row)
    }

    /// List links where the given session is the source session.
    pub fn list_session_links(&self, session_id: &str) -> Result<Vec<LocalSessionLink>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT session_id, linked_session_id, link_type, created_at \
             FROM session_links WHERE session_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok(LocalSessionLink {
                session_id: row.get(0)?,
                linked_session_id: row.get(1)?,
                link_type: row.get(2)?,
                created_at: row.get(3)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
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
            "DELETE FROM session_links WHERE session_id = ?1 OR linked_session_id = ?1",
            params![session_id],
        )?;
        conn.execute(
            "DELETE FROM vector_embeddings \
             WHERE chunk_id IN (SELECT id FROM vector_chunks WHERE session_id = ?1)",
            params![session_id],
        )?;
        conn.execute(
            "DELETE FROM vector_chunks_fts WHERE session_id = ?1",
            params![session_id],
        )?;
        conn.execute(
            "DELETE FROM vector_chunks WHERE session_id = ?1",
            params![session_id],
        )?;
        conn.execute(
            "DELETE FROM vector_index_sessions WHERE session_id = ?1",
            params![session_id],
        )?;
        conn.execute(
            "DELETE FROM session_semantic_summaries WHERE session_id = ?1",
            params![session_id],
        )?;
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

    // ── Semantic summary cache ───────────────────────────────────────────

    pub fn upsert_session_semantic_summary(
        &self,
        payload: &SessionSemanticSummaryUpsert<'_>,
    ) -> Result<()> {
        self.conn().execute(
            "INSERT INTO session_semantic_summaries (\
                session_id, summary_json, generated_at, provider, model, \
                source_kind, generation_kind, prompt_fingerprint, source_details_json, \
                diff_tree_json, error, updated_at\
             ) VALUES (\
                ?1, ?2, ?3, ?4, ?5, \
                ?6, ?7, ?8, ?9, \
                ?10, ?11, datetime('now')\
             ) \
             ON CONFLICT(session_id) DO UPDATE SET \
                summary_json=excluded.summary_json, \
                generated_at=excluded.generated_at, \
                provider=excluded.provider, \
                model=excluded.model, \
                source_kind=excluded.source_kind, \
                generation_kind=excluded.generation_kind, \
                prompt_fingerprint=excluded.prompt_fingerprint, \
                source_details_json=excluded.source_details_json, \
                diff_tree_json=excluded.diff_tree_json, \
                error=excluded.error, \
                updated_at=datetime('now')",
            params![
                payload.session_id,
                payload.summary_json,
                payload.generated_at,
                payload.provider,
                payload.model,
                payload.source_kind,
                payload.generation_kind,
                payload.prompt_fingerprint,
                payload.source_details_json,
                payload.diff_tree_json,
                payload.error,
            ],
        )?;
        Ok(())
    }

    pub fn list_expired_session_ids(&self, keep_days: u32) -> Result<Vec<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id FROM sessions \
             WHERE julianday(created_at) <= julianday('now') - ?1 \
             ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![keep_days as i64], |row| row.get(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn get_session_semantic_summary(
        &self,
        session_id: &str,
    ) -> Result<Option<SessionSemanticSummaryRow>> {
        let row = self
            .conn()
            .query_row(
                "SELECT session_id, summary_json, generated_at, provider, model, \
                        source_kind, generation_kind, prompt_fingerprint, source_details_json, \
                        diff_tree_json, error, updated_at \
                 FROM session_semantic_summaries WHERE session_id = ?1 LIMIT 1",
                params![session_id],
                |row| {
                    Ok(SessionSemanticSummaryRow {
                        session_id: row.get(0)?,
                        summary_json: row.get(1)?,
                        generated_at: row.get(2)?,
                        provider: row.get(3)?,
                        model: row.get(4)?,
                        source_kind: row.get(5)?,
                        generation_kind: row.get(6)?,
                        prompt_fingerprint: row.get(7)?,
                        source_details_json: row.get(8)?,
                        diff_tree_json: row.get(9)?,
                        error: row.get(10)?,
                        updated_at: row.get(11)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    pub fn delete_expired_session_summaries(&self, keep_days: u32) -> Result<u32> {
        let deleted = self.conn().execute(
            "DELETE FROM session_semantic_summaries \
             WHERE julianday(generated_at) <= julianday('now') - ?1",
            params![keep_days as i64],
        )?;
        Ok(deleted as u32)
    }

    // ── Semantic vector index cache ────────────────────────────────────

    pub fn vector_index_source_hash(&self, session_id: &str) -> Result<Option<String>> {
        let hash = self
            .conn()
            .query_row(
                "SELECT source_hash FROM vector_index_sessions WHERE session_id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(hash)
    }

    pub fn clear_vector_index(&self) -> Result<()> {
        let conn = self.conn();
        conn.execute("DELETE FROM vector_embeddings", [])?;
        conn.execute("DELETE FROM vector_chunks_fts", [])?;
        conn.execute("DELETE FROM vector_chunks", [])?;
        conn.execute("DELETE FROM vector_index_sessions", [])?;
        Ok(())
    }

    pub fn replace_session_vector_chunks(
        &self,
        session_id: &str,
        source_hash: &str,
        model: &str,
        chunks: &[VectorChunkUpsert],
    ) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;

        tx.execute(
            "DELETE FROM vector_embeddings \
             WHERE chunk_id IN (SELECT id FROM vector_chunks WHERE session_id = ?1)",
            params![session_id],
        )?;
        tx.execute(
            "DELETE FROM vector_chunks_fts WHERE session_id = ?1",
            params![session_id],
        )?;
        tx.execute(
            "DELETE FROM vector_chunks WHERE session_id = ?1",
            params![session_id],
        )?;

        for chunk in chunks {
            let embedding_json = serde_json::to_string(&chunk.embedding)
                .context("serialize vector embedding for local cache")?;
            tx.execute(
                "INSERT INTO vector_chunks \
                 (id, session_id, chunk_index, start_line, end_line, line_count, content, content_hash, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, datetime('now'), datetime('now'))",
                params![
                    &chunk.chunk_id,
                    &chunk.session_id,
                    chunk.chunk_index as i64,
                    chunk.start_line as i64,
                    chunk.end_line as i64,
                    chunk.line_count as i64,
                    &chunk.content,
                    &chunk.content_hash,
                ],
            )?;
            tx.execute(
                "INSERT INTO vector_embeddings \
                 (chunk_id, model, embedding_dim, embedding_json, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, datetime('now'))",
                params![
                    &chunk.chunk_id,
                    model,
                    chunk.embedding.len() as i64,
                    &embedding_json
                ],
            )?;
            tx.execute(
                "INSERT INTO vector_chunks_fts (chunk_id, session_id, content) VALUES (?1, ?2, ?3)",
                params![&chunk.chunk_id, &chunk.session_id, &chunk.content],
            )?;
        }

        tx.execute(
            "INSERT INTO vector_index_sessions \
             (session_id, source_hash, chunk_count, last_indexed_at, updated_at) \
             VALUES (?1, ?2, ?3, datetime('now'), datetime('now')) \
             ON CONFLICT(session_id) DO UPDATE SET \
             source_hash=excluded.source_hash, \
             chunk_count=excluded.chunk_count, \
             last_indexed_at=datetime('now'), \
             updated_at=datetime('now')",
            params![session_id, source_hash, chunks.len() as i64],
        )?;

        tx.commit()?;
        Ok(())
    }

    pub fn list_vector_chunk_candidates(
        &self,
        query: &str,
        model: &str,
        limit: u32,
    ) -> Result<Vec<VectorChunkCandidateRow>> {
        let Some(fts_query) = build_fts_query(query) else {
            return Ok(Vec::new());
        };
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT c.id, c.session_id, c.start_line, c.end_line, c.content, e.embedding_json \
             FROM vector_chunks_fts f \
             INNER JOIN vector_chunks c ON c.id = f.chunk_id \
             INNER JOIN vector_embeddings e ON e.chunk_id = c.id \
             WHERE f.content MATCH ?1 AND e.model = ?2 \
             ORDER BY bm25(vector_chunks_fts) ASC, c.updated_at DESC \
             LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![fts_query, model, limit as i64], |row| {
            let embedding_json: String = row.get(5)?;
            let embedding =
                serde_json::from_str::<Vec<f32>>(&embedding_json).unwrap_or_else(|_| Vec::new());
            Ok(VectorChunkCandidateRow {
                chunk_id: row.get(0)?,
                session_id: row.get(1)?,
                start_line: row.get::<_, i64>(2)?.max(0) as u32,
                end_line: row.get::<_, i64>(3)?.max(0) as u32,
                content: row.get(4)?,
                embedding,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn list_recent_vector_chunks_for_model(
        &self,
        model: &str,
        limit: u32,
    ) -> Result<Vec<VectorChunkCandidateRow>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT c.id, c.session_id, c.start_line, c.end_line, c.content, e.embedding_json \
             FROM vector_chunks c \
             INNER JOIN vector_embeddings e ON e.chunk_id = c.id \
             WHERE e.model = ?1 \
             ORDER BY c.updated_at DESC \
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![model, limit as i64], |row| {
            let embedding_json: String = row.get(5)?;
            let embedding =
                serde_json::from_str::<Vec<f32>>(&embedding_json).unwrap_or_else(|_| Vec::new());
            Ok(VectorChunkCandidateRow {
                chunk_id: row.get(0)?,
                session_id: row.get(1)?,
                start_line: row.get::<_, i64>(2)?.max(0) as u32,
                end_line: row.get::<_, i64>(3)?.max(0) as u32,
                content: row.get(4)?,
                embedding,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn set_vector_index_job(&self, payload: &VectorIndexJobRow) -> Result<()> {
        self.conn().execute(
            "INSERT INTO vector_index_jobs \
             (id, status, processed_sessions, total_sessions, message, started_at, finished_at, updated_at) \
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, datetime('now')) \
             ON CONFLICT(id) DO UPDATE SET \
             status=excluded.status, \
             processed_sessions=excluded.processed_sessions, \
             total_sessions=excluded.total_sessions, \
             message=excluded.message, \
             started_at=excluded.started_at, \
             finished_at=excluded.finished_at, \
             updated_at=datetime('now')",
            params![
                payload.status,
                payload.processed_sessions as i64,
                payload.total_sessions as i64,
                payload.message,
                payload.started_at,
                payload.finished_at,
            ],
        )?;
        Ok(())
    }

    pub fn get_vector_index_job(&self) -> Result<Option<VectorIndexJobRow>> {
        let row = self
            .conn()
            .query_row(
                "SELECT status, processed_sessions, total_sessions, message, started_at, finished_at \
                 FROM vector_index_jobs WHERE id = 1 LIMIT 1",
                [],
                |row| {
                    Ok(VectorIndexJobRow {
                        status: row.get(0)?,
                        processed_sessions: row.get::<_, i64>(1)?.max(0) as u32,
                        total_sessions: row.get::<_, i64>(2)?.max(0) as u32,
                        message: row.get(3)?,
                        started_at: row.get(4)?,
                        finished_at: row.get(5)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    pub fn set_summary_batch_job(&self, payload: &SummaryBatchJobRow) -> Result<()> {
        self.conn().execute(
            "INSERT INTO summary_batch_jobs \
             (id, status, processed_sessions, total_sessions, failed_sessions, message, started_at, finished_at, updated_at) \
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now')) \
             ON CONFLICT(id) DO UPDATE SET \
             status=excluded.status, \
             processed_sessions=excluded.processed_sessions, \
             total_sessions=excluded.total_sessions, \
             failed_sessions=excluded.failed_sessions, \
             message=excluded.message, \
             started_at=excluded.started_at, \
             finished_at=excluded.finished_at, \
             updated_at=datetime('now')",
            params![
                payload.status,
                payload.processed_sessions as i64,
                payload.total_sessions as i64,
                payload.failed_sessions as i64,
                payload.message,
                payload.started_at,
                payload.finished_at,
            ],
        )?;
        Ok(())
    }

    pub fn get_summary_batch_job(&self) -> Result<Option<SummaryBatchJobRow>> {
        let row = self
            .conn()
            .query_row(
                "SELECT status, processed_sessions, total_sessions, failed_sessions, message, started_at, finished_at \
                 FROM summary_batch_jobs WHERE id = 1 LIMIT 1",
                [],
                |row| {
                    Ok(SummaryBatchJobRow {
                        status: row.get(0)?,
                        processed_sessions: row.get::<_, i64>(1)?.max(0) as u32,
                        total_sessions: row.get::<_, i64>(2)?.max(0) as u32,
                        failed_sessions: row.get::<_, i64>(3)?.max(0) as u32,
                        message: row.get(4)?,
                        started_at: row.get(5)?,
                        finished_at: row.get(6)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
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

// ── Schema bootstrap ──────────────────────────────────────────────────

fn open_connection_with_latest_schema(path: &PathBuf) -> Result<Connection> {
    let conn = Connection::open(path).with_context(|| format!("open db {}", path.display()))?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;

    // Disable FK constraints for local DB (index/cache, not source of truth)
    conn.execute_batch("PRAGMA foreign_keys=OFF;")?;

    apply_local_migrations(&conn)?;
    repair_session_tools_from_source_path(&conn)?;
    repair_auxiliary_flags_from_source_path(&conn)?;
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

fn repair_auxiliary_flags_from_source_path(conn: &Connection) -> Result<()> {
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
    fn test_open_repairs_codex_auxiliary_flag_from_source_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("repair-auxiliary.db");
        let codex_dir = dir
            .path()
            .join(".codex")
            .join("sessions")
            .join("2026")
            .join("02")
            .join("20");
        create_dir_all(&codex_dir).unwrap();
        let source_path = codex_dir.join("rollout-subagent.jsonl");
        write(
            &source_path,
            r#"{"timestamp":"2026-02-20T00:00:00.000Z","type":"session_meta","payload":{"id":"rollout-subagent","timestamp":"2026-02-20T00:00:00.000Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.105.0","source":{"subagent":{"thread_spawn":{"parent_thread_id":"parent-session-id","depth":1,"agent_role":"awaiter"}}},"agent_role":"awaiter"}}\n"#,
        )
        .unwrap();

        {
            let _ = LocalDb::open_path(&path).unwrap();
        }

        {
            let conn = Connection::open(&path).unwrap();
            conn.execute(
                "INSERT INTO sessions (id, team_id, tool, created_at, body_storage_key, is_auxiliary) VALUES (?1, 'personal', 'codex', ?2, '', 0)",
                params!["rollout-subagent", "2026-02-20T00:00:00Z"],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO session_sync (session_id, source_path, sync_status) VALUES (?1, ?2, 'local_only')",
                params!["rollout-subagent", source_path.to_string_lossy().to_string()],
            )
            .unwrap();
        }

        let db = LocalDb::open_path(&path).unwrap();
        let rows = db.list_sessions(&LocalSessionFilter::default()).unwrap();
        assert!(
            rows.iter().all(|row| row.id != "rollout-subagent"),
            "auxiliary codex session should be hidden after repair"
        );
    }

    #[test]
    fn test_open_repairs_codex_auxiliary_flag_when_session_meta_is_not_first_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("repair-auxiliary-shifted.db");
        let codex_dir = dir
            .path()
            .join(".codex")
            .join("sessions")
            .join("2026")
            .join("03")
            .join("03");
        create_dir_all(&codex_dir).unwrap();
        let source_path = codex_dir.join("rollout-subagent-shifted.jsonl");
        write(
            &source_path,
            [
                r#"{"timestamp":"2026-03-03T00:00:00.010Z","type":"event_msg","payload":{"type":"agent_message","message":"bootstrap line"}}"#,
                r#"{"timestamp":"2026-03-03T00:00:00.020Z","type":"session_meta","payload":{"id":"rollout-subagent-shifted","timestamp":"2026-03-03T00:00:00.000Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.108.0","source":{"subagent":{"thread_spawn":{"parent_thread_id":"parent-session-id","depth":1,"agent_role":"worker"}}},"agent_role":"worker"}}"#,
            ]
            .join("\n"),
        )
        .unwrap();

        {
            let _ = LocalDb::open_path(&path).unwrap();
        }

        {
            let conn = Connection::open(&path).unwrap();
            conn.execute(
                "INSERT INTO sessions (id, team_id, tool, created_at, body_storage_key, is_auxiliary) VALUES (?1, 'personal', 'codex', ?2, '', 0)",
                params!["rollout-subagent-shifted", "2026-03-03T00:00:00Z"],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO session_sync (session_id, source_path, sync_status) VALUES (?1, ?2, 'local_only')",
                params!["rollout-subagent-shifted", source_path.to_string_lossy().to_string()],
            )
            .unwrap();
        }

        let db = LocalDb::open_path(&path).unwrap();
        let rows = db.list_sessions(&LocalSessionFilter::default()).unwrap();
        assert!(
            rows.iter().all(|row| row.id != "rollout-subagent-shifted"),
            "auxiliary codex session should be hidden after repair even if session_meta is not the first line"
        );
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
    fn test_upsert_local_session_preserves_existing_git_when_session_has_no_git_metadata() {
        let db = test_db();
        let mut session = Session::new(
            "preserve-git".to_string(),
            opensession_core::trace::Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );
        session.stats.event_count = 1;

        let first_git = crate::git::GitContext {
            remote: Some("https://github.com/acme/repo.git".to_string()),
            branch: Some("feature/original".to_string()),
            commit: Some("1111111".to_string()),
            repo_name: Some("acme/repo".to_string()),
        };
        db.upsert_local_session(
            &session,
            "/Users/test/.codex/sessions/2026/02/20/preserve-git.jsonl",
            &first_git,
        )
        .unwrap();

        let second_git = crate::git::GitContext {
            remote: Some("https://github.com/acme/repo.git".to_string()),
            branch: Some("feature/current-head".to_string()),
            commit: Some("2222222".to_string()),
            repo_name: Some("acme/repo".to_string()),
        };
        db.upsert_local_session(
            &session,
            "/Users/test/.codex/sessions/2026/02/20/preserve-git.jsonl",
            &second_git,
        )
        .unwrap();

        let rows = db.list_sessions(&LocalSessionFilter::default()).unwrap();
        let row = rows
            .iter()
            .find(|row| row.id == "preserve-git")
            .expect("row exists");
        assert_eq!(row.git_branch.as_deref(), Some("feature/original"));
        assert_eq!(row.git_commit.as_deref(), Some("1111111"));
    }

    #[test]
    fn test_upsert_local_session_prefers_git_branch_from_session_attributes() {
        let db = test_db();
        let mut session = Session::new(
            "session-git-branch".to_string(),
            opensession_core::trace::Agent {
                provider: "anthropic".to_string(),
                model: "claude-opus-4-6".to_string(),
                tool: "claude-code".to_string(),
                tool_version: None,
            },
        );
        session.stats.event_count = 1;
        session.context.attributes.insert(
            "git_branch".to_string(),
            serde_json::Value::String("from-session".to_string()),
        );

        let fallback_git = crate::git::GitContext {
            remote: Some("https://github.com/acme/repo.git".to_string()),
            branch: Some("fallback-branch".to_string()),
            commit: Some("aaaaaaaa".to_string()),
            repo_name: Some("acme/repo".to_string()),
        };
        db.upsert_local_session(
            &session,
            "/Users/test/.claude/projects/foo/session-git-branch.jsonl",
            &fallback_git,
        )
        .unwrap();

        session.context.attributes.insert(
            "git_branch".to_string(),
            serde_json::Value::String("from-session-updated".to_string()),
        );
        db.upsert_local_session(
            &session,
            "/Users/test/.claude/projects/foo/session-git-branch.jsonl",
            &fallback_git,
        )
        .unwrap();

        let rows = db.list_sessions(&LocalSessionFilter::default()).unwrap();
        let row = rows
            .iter()
            .find(|row| row.id == "session-git-branch")
            .expect("row exists");
        assert_eq!(row.git_branch.as_deref(), Some("from-session-updated"));
    }

    #[test]
    fn test_upsert_local_session_marks_parented_sessions_auxiliary() {
        let db = test_db();
        let mut session = Session::new(
            "aux-upsert".to_string(),
            opensession_core::trace::Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "opencode".to_string(),
                tool_version: None,
            },
        );
        session.stats.event_count = 1;
        session.context.attributes.insert(
            opensession_core::session::ATTR_PARENT_SESSION_ID.to_string(),
            serde_json::Value::String("parent-session".to_string()),
        );

        db.upsert_local_session(
            &session,
            "/Users/test/.opencode/storage/session/project/aux-upsert.json",
            &crate::git::GitContext::default(),
        )
        .unwrap();

        let is_auxiliary: i64 = db
            .conn()
            .query_row(
                "SELECT is_auxiliary FROM sessions WHERE id = ?1",
                params!["aux-upsert"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(is_auxiliary, 1);

        let rows = db.list_sessions(&LocalSessionFilter::default()).unwrap();
        assert!(
            rows.iter().all(|row| row.id != "aux-upsert"),
            "auxiliary sessions should be hidden from default listing"
        );
    }

    #[test]
    fn test_upsert_local_session_primary_role_overrides_parent_link() {
        let db = test_db();
        let mut session = Session::new(
            "primary-override".to_string(),
            opensession_core::trace::Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "opencode".to_string(),
                tool_version: None,
            },
        );
        session.stats.event_count = 1;
        session.context.attributes.insert(
            opensession_core::session::ATTR_PARENT_SESSION_ID.to_string(),
            serde_json::Value::String("parent-session".to_string()),
        );
        session.context.attributes.insert(
            opensession_core::session::ATTR_SESSION_ROLE.to_string(),
            serde_json::Value::String("primary".to_string()),
        );

        db.upsert_local_session(
            &session,
            "/Users/test/.opencode/storage/session/project/primary-override.json",
            &crate::git::GitContext::default(),
        )
        .unwrap();

        let rows = db.list_sessions(&LocalSessionFilter::default()).unwrap();
        let row = rows
            .iter()
            .find(|row| row.id == "primary-override")
            .expect("session with explicit primary role should stay visible");
        assert!(!row.is_auxiliary);
    }

    #[test]
    fn test_list_sessions_hides_codex_summary_worker_titles() {
        let db = test_db();
        let mut codex_summary_worker = Session::new(
            "codex-summary-worker".to_string(),
            opensession_core::trace::Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );
        codex_summary_worker.context.title = Some(
            "Convert a real coding session into semantic compression. Pipeline: ...".to_string(),
        );
        codex_summary_worker.stats.event_count = 2;
        codex_summary_worker.stats.message_count = 1;

        db.upsert_local_session(
            &codex_summary_worker,
            "/Users/test/.codex/sessions/2026/03/05/summary-worker.jsonl",
            &crate::git::GitContext::default(),
        )
        .expect("upsert codex summary worker session");

        let mut non_codex_same_title = Session::new(
            "claude-similar-title".to_string(),
            opensession_core::trace::Agent {
                provider: "anthropic".to_string(),
                model: "claude-opus-4-6".to_string(),
                tool: "claude-code".to_string(),
                tool_version: None,
            },
        );
        non_codex_same_title.context.title = Some(
            "Convert a real coding session into semantic compression. Pipeline: ...".to_string(),
        );
        non_codex_same_title.stats.event_count = 2;
        non_codex_same_title.stats.message_count = 1;

        db.upsert_local_session(
            &non_codex_same_title,
            "/Users/test/.claude/projects/p1/claude-similar-title.jsonl",
            &crate::git::GitContext::default(),
        )
        .expect("upsert non-codex session");

        let rows = db.list_sessions(&LocalSessionFilter::default()).unwrap();
        assert!(
            rows.iter().all(|row| row.id != "codex-summary-worker"),
            "codex summary worker sessions should be hidden from default listing"
        );
        assert!(
            rows.iter().any(|row| row.id == "claude-similar-title"),
            "non-codex sessions must remain visible even with similar title"
        );

        let count = db
            .count_sessions_filtered(&LocalSessionFilter::default())
            .unwrap();
        assert_eq!(count, 1);
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
    fn test_list_session_source_paths_returns_non_empty_paths_only() {
        let db = test_db();
        let mut s1 = Session::new(
            "source-path-1".to_string(),
            opensession_core::trace::Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );
        s1.stats.event_count = 1;
        db.upsert_local_session(
            &s1,
            "/tmp/source-path-1.jsonl",
            &crate::git::GitContext::default(),
        )
        .expect("upsert first session");

        let mut s2 = Session::new(
            "source-path-2".to_string(),
            opensession_core::trace::Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );
        s2.stats.event_count = 1;
        db.upsert_local_session(&s2, "", &crate::git::GitContext::default())
            .expect("upsert second session");

        let paths = db
            .list_session_source_paths()
            .expect("list source paths should work");
        assert!(paths
            .iter()
            .any(|(id, path)| id == "source-path-1" && path == "/tmp/source-path-1.jsonl"));
        assert!(paths.iter().all(|(id, _)| id != "source-path-2"));
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
    fn test_get_session_by_id_and_list_session_links() {
        let db = test_db();
        db.upsert_remote_session(&make_summary(
            "parent-session",
            "codex",
            "Parent session",
            "2024-01-01T00:00:00Z",
        ))
        .unwrap();
        db.upsert_remote_session(&make_summary(
            "child-session",
            "codex",
            "Child session",
            "2024-01-01T01:00:00Z",
        ))
        .unwrap();

        db.conn()
            .execute(
                "INSERT INTO session_links (session_id, linked_session_id, link_type, created_at) VALUES (?1, ?2, ?3, ?4)",
                params!["parent-session", "child-session", "handoff", "2024-01-01T01:00:00Z"],
            )
            .unwrap();

        let parent = db
            .get_session_by_id("parent-session")
            .unwrap()
            .expect("session should exist");
        assert_eq!(parent.id, "parent-session");
        assert_eq!(parent.title.as_deref(), Some("Parent session"));

        let links = db.list_session_links("parent-session").unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].session_id, "parent-session");
        assert_eq!(links[0].linked_session_id, "child-session");
        assert_eq!(links[0].link_type, "handoff");
    }

    #[test]
    fn test_local_migrations_are_loaded_from_api_crate() {
        let migration_names: Vec<&str> = super::LOCAL_MIGRATIONS
            .iter()
            .map(|(name, _)| *name)
            .collect();
        assert!(
            migration_names.contains(&"local_0001_schema"),
            "expected local_0001_schema migration from opensession-api"
        );
        assert!(
            migration_names.contains(&"local_0002_session_summaries"),
            "expected local_0002_session_summaries migration from opensession-api"
        );
        assert!(
            migration_names.contains(&"local_0003_vector_index"),
            "expected local_0003_vector_index migration from opensession-api"
        );
        assert!(
            migration_names.contains(&"local_0004_summary_batch_status"),
            "expected local_0004_summary_batch_status migration from opensession-api"
        );
        assert_eq!(
            migration_names.len(),
            4,
            "local schema should include baseline + summary cache + vector index + summary batch status steps"
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
    fn test_local_schema_bootstrap_includes_is_auxiliary_column() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("local.db");
        let db = LocalDb::open_path(&path).unwrap();
        let conn = db.conn();
        let mut stmt = conn.prepare("PRAGMA table_info(sessions)").unwrap();
        let columns = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert!(
            columns.iter().any(|name| name == "is_auxiliary"),
            "sessions schema must include is_auxiliary column in bootstrap migration"
        );
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
    fn test_exclude_low_signal_filter_hides_metadata_only_sessions() {
        let db = test_db();

        let mut low_signal = make_summary("meta-only", "claude-code", "", "2024-01-01T00:00:00Z");
        low_signal.title = None;
        low_signal.message_count = 0;
        low_signal.task_count = 0;
        low_signal.event_count = 2;
        low_signal.git_repo_name = Some("frontend/aviss-react-front".to_string());

        let mut normal = make_summary(
            "real-work",
            "opencode",
            "Socket.IO decision",
            "2024-01-02T00:00:00Z",
        );
        normal.message_count = 14;
        normal.task_count = 2;
        normal.event_count = 38;
        normal.git_repo_name = Some("frontend/aviss-react-front".to_string());

        db.upsert_remote_session(&low_signal).unwrap();
        db.upsert_remote_session(&normal).unwrap();

        let default_filter = LocalSessionFilter {
            git_repo_name: Some("frontend/aviss-react-front".to_string()),
            ..Default::default()
        };
        assert_eq!(db.list_sessions(&default_filter).unwrap().len(), 2);
        assert_eq!(db.count_sessions_filtered(&default_filter).unwrap(), 2);

        let repo_filter = LocalSessionFilter {
            git_repo_name: Some("frontend/aviss-react-front".to_string()),
            exclude_low_signal: true,
            ..Default::default()
        };
        let rows = db.list_sessions(&repo_filter).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "real-work");
        assert_eq!(db.count_sessions_filtered(&repo_filter).unwrap(), 1);
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
    fn test_upsert_and_get_session_semantic_summary() {
        let db = test_db();
        seed_sessions(&db);

        db.upsert_session_semantic_summary(&SessionSemanticSummaryUpsert {
            session_id: "s1",
            summary_json: r#"{"changes":"updated files","auth_security":"none detected","layer_file_changes":[]}"#,
            generated_at: "2026-03-04T10:00:00Z",
            provider: "codex_exec",
            model: Some("gpt-5"),
            source_kind: "session_signals",
            generation_kind: "provider",
            prompt_fingerprint: Some("abc123"),
            source_details_json: Some(r#"{"source":"session"}"#),
            diff_tree_json: Some(r#"[]"#),
            error: None,
        })
        .expect("upsert semantic summary");

        let row = db
            .get_session_semantic_summary("s1")
            .expect("query semantic summary")
            .expect("summary row exists");
        assert_eq!(row.session_id, "s1");
        assert_eq!(row.provider, "codex_exec");
        assert_eq!(row.model.as_deref(), Some("gpt-5"));
        assert_eq!(row.source_kind, "session_signals");
        assert_eq!(row.generation_kind, "provider");
        assert_eq!(row.prompt_fingerprint.as_deref(), Some("abc123"));
        assert!(row.error.is_none());
    }

    #[test]
    fn test_delete_session_removes_semantic_summary_row() {
        let db = test_db();
        seed_sessions(&db);

        db.upsert_session_semantic_summary(&SessionSemanticSummaryUpsert {
            session_id: "s1",
            summary_json: r#"{"changes":"updated files","auth_security":"none detected","layer_file_changes":[]}"#,
            generated_at: "2026-03-04T10:00:00Z",
            provider: "heuristic",
            model: None,
            source_kind: "heuristic",
            generation_kind: "heuristic_fallback",
            prompt_fingerprint: None,
            source_details_json: None,
            diff_tree_json: None,
            error: Some("provider disabled"),
        })
        .expect("upsert semantic summary");

        db.delete_session("s1").expect("delete session");

        let missing = db
            .get_session_semantic_summary("s1")
            .expect("query semantic summary");
        assert!(missing.is_none());
    }

    #[test]
    fn test_delete_session_removes_session_links_bidirectionally() {
        let db = test_db();
        seed_sessions(&db);

        db.conn()
            .execute(
                "INSERT INTO session_links (session_id, linked_session_id, link_type, created_at) \
                 VALUES (?1, ?2, 'handoff', datetime('now'))",
                params!["s1", "s2"],
            )
            .expect("insert forward link");
        db.conn()
            .execute(
                "INSERT INTO session_links (session_id, linked_session_id, link_type, created_at) \
                 VALUES (?1, ?2, 'related', datetime('now'))",
                params!["s3", "s1"],
            )
            .expect("insert reverse link");

        db.delete_session("s1").expect("delete root session");

        let remaining: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM session_links WHERE session_id = ?1 OR linked_session_id = ?1",
                params!["s1"],
                |row| row.get(0),
            )
            .expect("count linked rows");
        assert_eq!(remaining, 0);
    }

    #[test]
    fn test_delete_expired_session_summaries_uses_generated_at_ttl() {
        let db = test_db();
        seed_sessions(&db);

        db.upsert_session_semantic_summary(&SessionSemanticSummaryUpsert {
            session_id: "s1",
            summary_json: r#"{"changes":"old"}"#,
            generated_at: "2020-01-01T00:00:00Z",
            provider: "codex_exec",
            model: None,
            source_kind: "session_signals",
            generation_kind: "provider",
            prompt_fingerprint: None,
            source_details_json: None,
            diff_tree_json: None,
            error: None,
        })
        .expect("upsert old summary");
        db.upsert_session_semantic_summary(&SessionSemanticSummaryUpsert {
            session_id: "s2",
            summary_json: r#"{"changes":"new"}"#,
            generated_at: "2999-01-01T00:00:00Z",
            provider: "codex_exec",
            model: None,
            source_kind: "session_signals",
            generation_kind: "provider",
            prompt_fingerprint: None,
            source_details_json: None,
            diff_tree_json: None,
            error: None,
        })
        .expect("upsert new summary");

        let deleted = db
            .delete_expired_session_summaries(30)
            .expect("delete expired summaries");
        assert_eq!(deleted, 1);
        assert!(db
            .get_session_semantic_summary("s1")
            .expect("query old summary")
            .is_none());
        assert!(db
            .get_session_semantic_summary("s2")
            .expect("query new summary")
            .is_some());
    }

    #[test]
    fn test_list_expired_session_ids_uses_created_at_ttl() {
        let db = test_db();
        seed_sessions(&db);

        let expired = db
            .list_expired_session_ids(30)
            .expect("list expired sessions");
        assert!(
            expired.contains(&"s1".to_string()),
            "older seeded sessions should be expired for 30-day keep"
        );

        let none_expired = db
            .list_expired_session_ids(10_000)
            .expect("list non-expired sessions");
        assert!(
            none_expired.is_empty(),
            "seeded sessions should be retained with a large keep window"
        );
    }

    #[test]
    fn test_build_fts_query_quotes_tokens() {
        assert_eq!(
            build_fts_query("parser retry"),
            Some("\"parser\" OR \"retry\"".to_string())
        );
        assert!(build_fts_query("   ").is_none());
    }

    #[test]
    fn test_vector_chunk_replace_and_candidate_query() {
        let db = test_db();
        seed_sessions(&db);

        let chunks = vec![
            VectorChunkUpsert {
                chunk_id: "chunk-s1-0".to_string(),
                session_id: "s1".to_string(),
                chunk_index: 0,
                start_line: 1,
                end_line: 8,
                line_count: 8,
                content: "parser selection retry after auth error".to_string(),
                content_hash: "hash-0".to_string(),
                embedding: vec![0.1, 0.2, 0.3],
            },
            VectorChunkUpsert {
                chunk_id: "chunk-s1-1".to_string(),
                session_id: "s1".to_string(),
                chunk_index: 1,
                start_line: 9,
                end_line: 15,
                line_count: 7,
                content: "session list refresh control wired to runtime".to_string(),
                content_hash: "hash-1".to_string(),
                embedding: vec![0.3, 0.2, 0.1],
            },
        ];

        db.replace_session_vector_chunks("s1", "source-hash-s1", "bge-m3", &chunks)
            .expect("replace vector chunks");

        let source_hash = db
            .vector_index_source_hash("s1")
            .expect("read source hash")
            .expect("source hash should exist");
        assert_eq!(source_hash, "source-hash-s1");

        let matches = db
            .list_vector_chunk_candidates("parser retry", "bge-m3", 10)
            .expect("query vector chunk candidates");
        assert!(
            !matches.is_empty(),
            "vector FTS query should return at least one candidate"
        );
        assert_eq!(matches[0].session_id, "s1");
        assert!(matches[0].content.contains("parser"));
    }

    #[test]
    fn test_delete_session_removes_vector_index_rows() {
        let db = test_db();
        seed_sessions(&db);

        let chunks = vec![VectorChunkUpsert {
            chunk_id: "chunk-s1-delete".to_string(),
            session_id: "s1".to_string(),
            chunk_index: 0,
            start_line: 1,
            end_line: 2,
            line_count: 2,
            content: "delete me from vector cache".to_string(),
            content_hash: "hash-delete".to_string(),
            embedding: vec![0.7, 0.1, 0.2],
        }];
        db.replace_session_vector_chunks("s1", "delete-hash", "bge-m3", &chunks)
            .expect("insert vector chunk");

        db.delete_session("s1")
            .expect("delete session with vector rows");

        let candidates = db
            .list_vector_chunk_candidates("delete", "bge-m3", 10)
            .expect("query candidates after delete");
        assert!(
            candidates.iter().all(|row| row.session_id != "s1"),
            "vector rows for deleted session should be removed"
        );
    }

    #[test]
    fn test_vector_index_job_round_trip() {
        let db = test_db();
        let payload = VectorIndexJobRow {
            status: "running".to_string(),
            processed_sessions: 2,
            total_sessions: 10,
            message: Some("indexing".to_string()),
            started_at: Some("2026-03-05T10:00:00Z".to_string()),
            finished_at: None,
        };
        db.set_vector_index_job(&payload)
            .expect("set vector index job snapshot");

        let loaded = db
            .get_vector_index_job()
            .expect("read vector index job snapshot")
            .expect("vector index job row should exist");
        assert_eq!(loaded.status, "running");
        assert_eq!(loaded.processed_sessions, 2);
        assert_eq!(loaded.total_sessions, 10);
        assert_eq!(loaded.message.as_deref(), Some("indexing"));
    }

    #[test]
    fn test_summary_batch_job_round_trip() {
        let db = test_db();
        let payload = SummaryBatchJobRow {
            status: "running".to_string(),
            processed_sessions: 4,
            total_sessions: 12,
            failed_sessions: 1,
            message: Some("processing summaries".to_string()),
            started_at: Some("2026-03-05T10:00:00Z".to_string()),
            finished_at: None,
        };
        db.set_summary_batch_job(&payload)
            .expect("set summary batch job snapshot");

        let loaded = db
            .get_summary_batch_job()
            .expect("read summary batch job snapshot")
            .expect("summary batch job row should exist");
        assert_eq!(loaded.status, "running");
        assert_eq!(loaded.processed_sessions, 4);
        assert_eq!(loaded.total_sessions, 12);
        assert_eq!(loaded.failed_sessions, 1);
        assert_eq!(loaded.message.as_deref(), Some("processing summaries"));
    }

    #[test]
    fn test_session_count() {
        let db = test_db();
        assert_eq!(db.session_count().unwrap(), 0);
        seed_sessions(&db);
        assert_eq!(db.session_count().unwrap(), 5);
    }
}
