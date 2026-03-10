use anyhow::Result;
use opensession_core::session::{is_auxiliary_session, working_directory};
use opensession_core::trace::Session;
use rusqlite::params;
use serde_json::Value;
use std::collections::HashSet;

use crate::connection::LocalDb;
use crate::git::{GitContext, normalize_repo_name};

pub(crate) const SUMMARY_WORKER_TITLE_PREFIX_LOWER: &str =
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

pub(crate) fn infer_tool_from_source_path(source_path: Option<&str>) -> Option<&'static str> {
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

pub(crate) fn normalize_tool_for_source_path(
    current_tool: &str,
    source_path: Option<&str>,
) -> String {
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
pub(crate) const FROM_CLAUSE: &str = "\
FROM sessions s \
LEFT JOIN session_sync ss ON ss.session_id = s.id \
LEFT JOIN users u ON u.id = s.user_id";

pub(crate) const LOCAL_SESSION_COLUMNS: &str = "\
s.id, ss.source_path, COALESCE(ss.sync_status, 'unknown') AS sync_status, ss.last_synced_at, \
s.user_id, u.nickname, s.team_id, s.tool, s.agent_provider, s.agent_model, \
s.title, s.description, s.tags, s.created_at, s.uploaded_at, \
s.message_count, COALESCE(s.user_message_count, 0), s.task_count, s.event_count, s.duration_seconds, \
s.total_input_tokens, s.total_output_tokens, \
s.git_remote, s.git_branch, s.git_commit, s.git_repo_name, \
s.pr_number, s.pr_url, s.working_directory, \
s.files_modified, s.files_read, s.has_errors, COALESCE(s.max_active_agents, 1), COALESCE(s.is_auxiliary, 0)";

pub(crate) fn row_to_local_session(row: &rusqlite::Row) -> rusqlite::Result<LocalSessionRow> {
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

impl LocalDb {
    pub(crate) fn build_local_session_where_clause(
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

    pub fn upsert_local_session(
        &self,
        session: &Session,
        source_path: &str,
        git: &GitContext,
    ) -> Result<()> {
        let is_empty_signal = session.stats.event_count == 0
            && session.stats.message_count == 0
            && session.stats.user_message_count == 0
            && session.stats.task_count == 0;
        if is_empty_signal {
            self.delete_session(&session.session_id)?;
            return Ok(());
        }

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

        let (files_modified, files_read, has_errors) =
            opensession_core::extract::extract_file_metadata(session);
        let max_active_agents = opensession_core::agent_metrics::max_active_agents(session) as i64;
        let normalized_tool =
            normalize_tool_for_source_path(&session.agent.tool, Some(source_path));
        let git_from_session = git_context_from_session_attributes(session);
        let has_session_git = git_context_has_any_field(&git_from_session);
        let merged_git = merge_git_context(&git_from_session, git);

        let conn = self.conn();
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

    pub fn upsert_remote_session(&self, summary: &RemoteSessionSummary) -> Result<()> {
        let conn = self.conn();
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

        let _ = idx;

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

    pub fn session_count(&self) -> Result<i64> {
        let count = self
            .conn()
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))?;
        Ok(count)
    }

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

    pub fn existing_session_ids(&self) -> HashSet<String> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT id FROM sessions")
            .unwrap_or_else(|_| panic!("failed to prepare existing_session_ids query"));
        let rows = stmt.query_map([], |row| row.get::<_, String>(0));
        let mut set = HashSet::new();
        if let Ok(rows) = rows {
            for row in rows.flatten() {
                set.insert(row);
            }
        }
        set
    }

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
}
