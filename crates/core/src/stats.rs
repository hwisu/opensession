use crate::{Event, EventType, Session, Stats};
use chrono::Utc;

/// Aggregate statistics computed from a collection of sessions.
///
/// All fields are `u64` for in-memory computation; convert to `i64` when
/// mapping to SQL-based API types via the `From` impls in `api-types`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionAggregate {
    pub session_count: u64,
    pub message_count: u64,
    pub event_count: u64,
    pub tool_call_count: u64,
    pub task_count: u64,
    pub duration_seconds: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub user_message_count: u64,
    pub files_changed: u64,
    pub lines_added: u64,
    pub lines_removed: u64,
}

impl SessionAggregate {
    fn add_session_stats(&mut self, stats: &Stats) {
        self.session_count += 1;
        self.message_count += stats.message_count;
        self.event_count += stats.event_count;
        self.tool_call_count += stats.tool_call_count;
        self.task_count += stats.task_count;
        self.duration_seconds += stats.duration_seconds;
        self.total_input_tokens += stats.total_input_tokens;
        self.total_output_tokens += stats.total_output_tokens;
        self.user_message_count += stats.user_message_count;
        self.files_changed += stats.files_changed;
        self.lines_added += stats.lines_added;
        self.lines_removed += stats.lines_removed;
    }
}

/// Aggregate pre-computed `stats` from every session in the slice.
pub fn aggregate(sessions: &[Session]) -> SessionAggregate {
    let mut agg = SessionAggregate::default();
    for s in sessions {
        agg.add_session_stats(&s.stats);
    }
    agg
}

/// Group sessions by `agent.tool` and aggregate each group.
pub fn aggregate_by_tool(sessions: &[Session]) -> Vec<(String, SessionAggregate)> {
    aggregate_by(sessions, |s| s.agent.tool.clone())
}

/// Group sessions by `agent.model` and aggregate each group.
pub fn aggregate_by_model(sessions: &[Session]) -> Vec<(String, SessionAggregate)> {
    aggregate_by(sessions, |s| s.agent.model.clone())
}

/// Generic group-by aggregation. Results sorted by session_count descending.
fn aggregate_by(
    sessions: &[Session],
    key_fn: impl Fn(&Session) -> String,
) -> Vec<(String, SessionAggregate)> {
    let mut map = std::collections::HashMap::<String, SessionAggregate>::new();
    for s in sessions {
        map.entry(key_fn(s))
            .or_default()
            .add_session_stats(&s.stats);
    }
    let mut result: Vec<_> = map.into_iter().collect();
    result.sort_by(|a, b| b.1.session_count.cmp(&a.1.session_count));
    result
}

/// Filter sessions by a time-range string relative to now.
///
/// Supported values: `"24h"`, `"7d"`, `"30d"`, `"all"` (or anything else → no filter).
pub fn filter_by_time_range<'a>(sessions: &'a [Session], range: &str) -> Vec<&'a Session> {
    let cutoff = match range {
        "24h" => Some(Utc::now() - chrono::Duration::days(1)),
        "7d" => Some(Utc::now() - chrono::Duration::days(7)),
        "30d" => Some(Utc::now() - chrono::Duration::days(30)),
        _ => None,
    };
    match cutoff {
        Some(c) => sessions
            .iter()
            .filter(|s| s.context.created_at >= c)
            .collect(),
        None => sessions.iter().collect(),
    }
}

/// Extract tool name from an event, if applicable.
fn extract_tool_name(event: &Event) -> Option<String> {
    match &event.event_type {
        EventType::ToolCall { name } => Some(name.clone()),
        EventType::FileRead { .. } => Some("FileRead".to_string()),
        EventType::CodeSearch { .. } => Some("CodeSearch".to_string()),
        EventType::FileSearch { .. } => Some("FileSearch".to_string()),
        EventType::FileEdit { .. } => Some("FileEdit".to_string()),
        EventType::FileCreate { .. } => Some("FileCreate".to_string()),
        EventType::FileDelete { .. } => Some("FileDelete".to_string()),
        EventType::ShellCommand { .. } => Some("ShellCommand".to_string()),
        EventType::WebSearch { .. } => Some("WebSearch".to_string()),
        EventType::WebFetch { .. } => Some("WebFetch".to_string()),
        _ => None,
    }
}

/// Count tool calls per tool name across all sessions, returning `(tool_name, count)` sorted descending.
pub fn count_tool_calls(sessions: &[Session]) -> Vec<(String, u64)> {
    let mut result: Vec<_> = sessions
        .iter()
        .flat_map(|s| &s.events)
        .filter_map(extract_tool_name)
        .fold(
            std::collections::HashMap::<String, u64>::new(),
            |mut m, n| {
                *m.entry(n).or_default() += 1;
                m
            },
        )
        .into_iter()
        .collect();
    result.sort_by(|a, b| b.1.cmp(&a.1));
    result
}

// ---------------------------------------------------------------------------
// SQL helpers — shared query strings for SQLite-backed servers
// ---------------------------------------------------------------------------

pub mod sql {
    /// Convert a time-range string to a SQL WHERE clause fragment.
    ///
    /// Returns an empty string for `"all"` or unknown values.
    pub fn time_range_filter(range: &str) -> &'static str {
        match range {
            "24h" => " AND s.created_at >= datetime('now', '-1 day')",
            "7d" => " AND s.created_at >= datetime('now', '-7 days')",
            "30d" => " AND s.created_at >= datetime('now', '-30 days')",
            _ => "",
        }
    }

    /// Build a totals query for sessions matching `team_id = ?1`.
    pub fn totals_query(time_filter: &str) -> String {
        format!(
            "SELECT \
                COUNT(*) as session_count, \
                COALESCE(SUM(s.message_count), 0) as message_count, \
                COALESCE(SUM(s.event_count), 0) as event_count, \
                COALESCE(SUM(s.duration_seconds), 0) as duration_seconds, \
                COALESCE(SUM(s.total_input_tokens), 0) as total_input_tokens, \
                COALESCE(SUM(s.total_output_tokens), 0) as total_output_tokens \
             FROM sessions s \
             WHERE s.team_id = ?1{time_filter}"
        )
    }

    /// Build a by-user grouped query (requires JOIN with `users`).
    pub fn by_user_query(time_filter: &str) -> String {
        format!(
            "SELECT \
                s.user_id as user_id, \
                COALESCE(u.nickname, 'unknown') as nickname, \
                COUNT(*) as session_count, \
                COALESCE(SUM(s.message_count), 0) as message_count, \
                COALESCE(SUM(s.event_count), 0) as event_count, \
                COALESCE(SUM(s.duration_seconds), 0) as duration_seconds, \
                COALESCE(SUM(s.total_input_tokens), 0) as total_input_tokens, \
                COALESCE(SUM(s.total_output_tokens), 0) as total_output_tokens \
             FROM sessions s \
             LEFT JOIN users u ON u.id = s.user_id \
             WHERE s.team_id = ?1{time_filter} \
             GROUP BY s.user_id \
             ORDER BY session_count DESC"
        )
    }

    /// Build a by-tool grouped query.
    pub fn by_tool_query(time_filter: &str) -> String {
        format!(
            "SELECT \
                s.tool as tool, \
                COUNT(*) as session_count, \
                COALESCE(SUM(s.message_count), 0) as message_count, \
                COALESCE(SUM(s.event_count), 0) as event_count, \
                COALESCE(SUM(s.duration_seconds), 0) as duration_seconds, \
                COALESCE(SUM(s.total_input_tokens), 0) as total_input_tokens, \
                COALESCE(SUM(s.total_output_tokens), 0) as total_output_tokens \
             FROM sessions s \
             WHERE s.team_id = ?1{time_filter} \
             GROUP BY s.tool \
             ORDER BY session_count DESC"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing;
    use crate::{Content, Event, Session, Stats};
    use chrono::{Duration, Utc};
    use std::collections::HashMap;

    fn make_session_with_stats(tool: &str, model: &str, stats: Stats) -> Session {
        let mut s = Session::new("s1".to_string(), testing::agent_with(tool, model));
        s.stats = stats;
        s
    }

    fn sample_stats(msg: u64, events: u64, tools: u64, dur: u64) -> Stats {
        Stats {
            event_count: events,
            message_count: msg,
            tool_call_count: tools,
            task_count: 1,
            duration_seconds: dur,
            total_input_tokens: 100,
            total_output_tokens: 200,
            ..Default::default()
        }
    }

    #[test]
    fn test_aggregate_empty() {
        let agg = aggregate(&[]);
        assert_eq!(agg, SessionAggregate::default());
    }

    #[test]
    fn test_aggregate_single() {
        let sessions = vec![make_session_with_stats(
            "claude-code",
            "opus",
            sample_stats(5, 10, 3, 60),
        )];
        let agg = aggregate(&sessions);
        assert_eq!(agg.session_count, 1);
        assert_eq!(agg.message_count, 5);
        assert_eq!(agg.event_count, 10);
        assert_eq!(agg.tool_call_count, 3);
        assert_eq!(agg.duration_seconds, 60);
        assert_eq!(agg.total_input_tokens, 100);
        assert_eq!(agg.total_output_tokens, 200);
    }

    #[test]
    fn test_aggregate_multiple() {
        let sessions = vec![
            make_session_with_stats("claude-code", "opus", sample_stats(5, 10, 3, 60)),
            make_session_with_stats("cursor", "gpt-4o", sample_stats(3, 6, 2, 30)),
        ];
        let agg = aggregate(&sessions);
        assert_eq!(agg.session_count, 2);
        assert_eq!(agg.message_count, 8);
        assert_eq!(agg.event_count, 16);
        assert_eq!(agg.tool_call_count, 5);
        assert_eq!(agg.duration_seconds, 90);
        assert_eq!(agg.total_input_tokens, 200);
        assert_eq!(agg.total_output_tokens, 400);
    }

    #[test]
    fn test_aggregate_by_tool() {
        let sessions = vec![
            make_session_with_stats("claude-code", "opus", sample_stats(5, 10, 3, 60)),
            make_session_with_stats("claude-code", "sonnet", sample_stats(3, 6, 2, 30)),
            make_session_with_stats("cursor", "gpt-4o", sample_stats(1, 2, 1, 10)),
        ];
        let by_tool = aggregate_by_tool(&sessions);
        assert_eq!(by_tool.len(), 2);
        // claude-code has 2 sessions → should be first
        assert_eq!(by_tool[0].0, "claude-code");
        assert_eq!(by_tool[0].1.session_count, 2);
        assert_eq!(by_tool[1].0, "cursor");
        assert_eq!(by_tool[1].1.session_count, 1);
    }

    #[test]
    fn test_aggregate_by_model() {
        let sessions = vec![
            make_session_with_stats("claude-code", "opus", sample_stats(5, 10, 3, 60)),
            make_session_with_stats("cursor", "opus", sample_stats(3, 6, 2, 30)),
            make_session_with_stats("cursor", "gpt-4o", sample_stats(1, 2, 1, 10)),
        ];
        let by_model = aggregate_by_model(&sessions);
        assert_eq!(by_model.len(), 2);
        assert_eq!(by_model[0].0, "opus");
        assert_eq!(by_model[0].1.session_count, 2);
    }

    #[test]
    fn test_filter_by_time_range_all() {
        let sessions = vec![make_session_with_stats(
            "cc",
            "opus",
            sample_stats(1, 1, 0, 10),
        )];
        let filtered = filter_by_time_range(&sessions, "all");
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn test_filter_by_time_range_24h() {
        let mut recent = make_session_with_stats("cc", "opus", sample_stats(1, 1, 0, 10));
        recent.context.created_at = Utc::now();

        let mut old = make_session_with_stats("cc", "opus", sample_stats(1, 1, 0, 10));
        old.context.created_at = Utc::now() - Duration::days(2);

        let sessions = vec![recent, old];
        let filtered = filter_by_time_range(&sessions, "24h");
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn test_count_tool_calls() {
        let mut session = Session::new("s1".to_string(), testing::agent_with("cc", "opus"));
        session.events.push(Event {
            event_id: "e1".to_string(),
            timestamp: Utc::now(),
            event_type: EventType::ToolCall {
                name: "Read".to_string(),
            },
            task_id: None,
            content: Content::empty(),
            duration_ms: None,
            attributes: HashMap::new(),
        });
        session.events.push(Event {
            event_id: "e2".to_string(),
            timestamp: Utc::now(),
            event_type: EventType::FileRead {
                path: "/tmp/a.rs".to_string(),
            },
            task_id: None,
            content: Content::empty(),
            duration_ms: None,
            attributes: HashMap::new(),
        });
        session.events.push(Event {
            event_id: "e3".to_string(),
            timestamp: Utc::now(),
            event_type: EventType::UserMessage,
            task_id: None,
            content: Content::text("hello"),
            duration_ms: None,
            attributes: HashMap::new(),
        });

        let counts = count_tool_calls(&[session]);
        assert_eq!(counts.len(), 2);
        // Both Read and FileRead should appear
        let names: Vec<&str> = counts.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"Read"));
        assert!(names.contains(&"FileRead"));
    }

    // --- SQL helper tests ---

    #[test]
    fn test_sql_time_range_filter() {
        assert_eq!(
            sql::time_range_filter("24h"),
            " AND s.created_at >= datetime('now', '-1 day')"
        );
        assert_eq!(
            sql::time_range_filter("7d"),
            " AND s.created_at >= datetime('now', '-7 days')"
        );
        assert_eq!(
            sql::time_range_filter("30d"),
            " AND s.created_at >= datetime('now', '-30 days')"
        );
        assert_eq!(sql::time_range_filter("all"), "");
        assert_eq!(sql::time_range_filter("unknown"), "");
    }

    #[test]
    fn test_sql_totals_query_contains_expected_fragments() {
        let q = sql::totals_query("");
        assert!(q.contains("COUNT(*) as session_count"));
        assert!(q.contains("SUM(s.message_count)"));
        assert!(q.contains("SUM(s.total_input_tokens)"));
        assert!(q.contains("WHERE s.team_id = ?1"));
    }

    #[test]
    fn test_sql_totals_query_with_time_filter() {
        let tf = sql::time_range_filter("24h");
        let q = sql::totals_query(tf);
        assert!(q.contains("datetime('now', '-1 day')"));
    }

    #[test]
    fn test_sql_by_user_query() {
        let q = sql::by_user_query("");
        assert!(q.contains("LEFT JOIN users u"));
        assert!(q.contains("GROUP BY s.user_id"));
        assert!(q.contains("ORDER BY session_count DESC"));
    }

    #[test]
    fn test_sql_by_tool_query() {
        let q = sql::by_tool_query("");
        assert!(q.contains("GROUP BY s.tool"));
        assert!(q.contains("ORDER BY session_count DESC"));
    }
}
