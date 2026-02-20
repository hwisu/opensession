#![allow(dead_code)]

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use crossterm::event::{KeyCode, MouseEvent, MouseEventKind};
use opensession_api::{SortOrder, TimeRange, UserSettingsResponse};
use opensession_core::handoff_artifact::HandoffArtifact;
use opensession_core::trace::{ContentBlock, Event, EventType, Session};
use opensession_git_native::{load_handoff_artifact, ops};
use opensession_local_db::{
    LocalDb, LocalSessionFilter, LocalSessionRow, LocalSortOrder, LocalTimeRange,
};
use ratatui::widgets::ListState;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use crate::async_ops::{AsyncCommand, CommandResult};
use crate::config::{self, DaemonConfig, PublishMode, SettingField};
use crate::live::{
    DefaultLiveFeedProvider, FollowTailState, LiveFeedProvider, LiveSubscription, LiveUpdate,
    LiveUpdateBatch,
};
use crate::session_timeline::{build_lane_events_with_filter, LaneMarker};
pub use crate::views::modal::{ConfirmAction, Modal};

/// A display-level event for the timeline. Wraps real events with collapse/summary info.
#[derive(Debug, Clone)]
pub enum DisplayEvent<'a> {
    /// A single normal event.
    Single {
        event: &'a Event,
        source_index: usize,
        lane: usize,
        marker: LaneMarker,
        active_lanes: Vec<usize>,
    },
    /// A collapsed group of consecutive similar events.
    Collapsed {
        first: &'a Event,
        source_index: usize,
        count: u32,
        kind: String,
        lane: usize,
        marker: LaneMarker,
        active_lanes: Vec<usize>,
    },
}

impl<'a> DisplayEvent<'a> {
    pub fn event(&self) -> &'a Event {
        match self {
            DisplayEvent::Single { event, .. } => event,
            DisplayEvent::Collapsed { first, .. } => first,
        }
    }

    pub fn source_index(&self) -> usize {
        match self {
            DisplayEvent::Single { source_index, .. }
            | DisplayEvent::Collapsed { source_index, .. } => *source_index,
        }
    }

    pub fn lane(&self) -> usize {
        match self {
            DisplayEvent::Single { lane, .. } | DisplayEvent::Collapsed { lane, .. } => *lane,
        }
    }

    pub fn marker(&self) -> LaneMarker {
        match self {
            DisplayEvent::Single { marker, .. } | DisplayEvent::Collapsed { marker, .. } => *marker,
        }
    }

    pub fn active_lanes(&self) -> &[usize] {
        match self {
            DisplayEvent::Single { active_lanes, .. }
            | DisplayEvent::Collapsed { active_lanes, .. } => active_lanes,
        }
    }
}

/// Returns a grouping key for consecutive-collapse. Same key = can be grouped.
fn consecutive_group_key(event_type: &EventType) -> Option<String> {
    match event_type {
        EventType::FileRead { .. } => Some("FileRead".to_string()),
        EventType::CodeSearch { .. } => Some("CodeSearch".to_string()),
        EventType::FileSearch { .. } => Some("FileSearch".to_string()),
        EventType::WebSearch { .. } => Some("WebSearch".to_string()),
        EventType::WebFetch { .. } => Some("WebFetch".to_string()),
        EventType::ToolResult { .. } => Some("ToolResult".to_string()),
        _ => None,
    }
}

/// Which screen the user is viewing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    SessionList,
    SessionDetail,
    Setup,
    Settings,
    Handoff,
    Help,
}

/// Top-level tab navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Sessions,
    Handoff,
    Settings,
}

/// Settings sub-section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSection {
    Workspace,
    CaptureSync,
    StoragePrivacy,
    Git,
}

impl SettingsSection {
    pub const ORDER: [Self; 4] = [
        Self::CaptureSync,
        Self::StoragePrivacy,
        Self::Git,
        Self::Workspace,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Workspace => "Web Sync (Public)",
            Self::CaptureSync => "Capture Flow",
            Self::StoragePrivacy => "Privacy",
            Self::Git => "Git",
        }
    }

    pub fn panel_title(self) -> &'static str {
        match self {
            Self::Workspace => "Web Sync (Public)",
            Self::CaptureSync => "Capture Flow",
            Self::StoragePrivacy => "Privacy",
            Self::Git => "Git Explorer",
        }
    }

    pub fn group(self) -> Option<config::SettingsGroup> {
        Some(match self {
            Self::Workspace => config::SettingsGroup::Workspace,
            Self::CaptureSync => config::SettingsGroup::CaptureSync,
            Self::StoragePrivacy => config::SettingsGroup::StoragePrivacy,
            Self::Git => config::SettingsGroup::Git,
        })
    }

    pub fn next(self) -> Self {
        let idx = Self::ORDER
            .iter()
            .position(|section| *section == self)
            .unwrap_or(0);
        Self::ORDER[(idx + 1) % Self::ORDER.len()]
    }

    pub fn prev(self) -> Self {
        let idx = Self::ORDER
            .iter()
            .position(|section| *section == self)
            .unwrap_or(0);
        if idx == 0 {
            *Self::ORDER.last().unwrap_or(&Self::Workspace)
        } else {
            Self::ORDER[idx - 1]
        }
    }
}

/// Setup flow step: choose scenario first, then configure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupStep {
    Scenario,
    Configure,
}

/// First-run scenario choices used to prefill setup defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupScenario {
    Local,
    Public,
}

impl SetupScenario {
    pub const ALL: [Self; 2] = [Self::Local, Self::Public];
}

/// Active event type filter options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventFilter {
    All,
    User,
    Agent,
    Think,
    Tools,
    Files,
    Shell,
    Task,
    Web,
    Other,
}

/// Flash message severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlashLevel {
    Success,
    Error,
    Info,
}

/// Layout for the session list (single vs multi-column by active agent count).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListLayout {
    #[default]
    Single,
    ByUser,
}

/// How the session detail timeline is displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailViewMode {
    /// Events in chronological order (default).
    Linear,
    /// Side-by-side user/agent turn view.
    Turn,
}

/// A single conversational turn: user prompt + agent response.
pub struct Turn<'a> {
    pub turn_index: usize,
    pub start_display_index: usize,
    pub end_display_index: usize,
    pub anchor_source_index: usize,
    pub user_events: Vec<&'a Event>,
    pub agent_events: Vec<&'a Event>,
}

#[derive(Debug, Clone)]
pub struct HandoffCandidate {
    pub session_id: String,
    pub title: String,
    pub tool: String,
    pub model: String,
    pub created_at: DateTime<Utc>,
    pub event_count: usize,
    pub message_count: usize,
    pub source_path: Option<PathBuf>,
}

fn is_infra_warning_user_message(event: &Event) -> bool {
    if !matches!(event.event_type, EventType::UserMessage) {
        return false;
    }
    event_user_text(event)
        .map(|text| is_control_user_text(&text))
        .unwrap_or(false)
}

fn is_control_event(event: &Event) -> bool {
    if is_infra_warning_user_message(event) {
        return true;
    }
    matches!(
        &event.event_type,
        EventType::Custom { kind } if kind.eq_ignore_ascii_case("turn_aborted")
    )
}

fn event_user_text(event: &Event) -> Option<String> {
    if !matches!(event.event_type, EventType::UserMessage) {
        return None;
    }
    let mut text = String::new();
    for block in &event.content.blocks {
        for block_text in App::block_text_fragments(block) {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&block_text);
        }
    }
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn is_control_user_text(text: &str) -> bool {
    let lower = text.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return false;
    }
    if lower.contains("apply_patch was requested via exec_command")
        && lower.contains("use the apply_patch tool instead")
    {
        return true;
    }
    lower == "agents.md instructions"
        || lower.starts_with("# agents.md instructions")
        || lower.contains("<instructions>")
        || lower.contains("</instructions>")
        || lower.contains("<environment_context>")
        || lower.contains("</environment_context>")
        || lower.contains("<turn_aborted>")
        || lower.contains("</turn_aborted>")
}

/// Extract turns from visible events. Each UserMessage starts a new turn.
pub fn extract_turns<'a>(events: &[DisplayEvent<'a>]) -> Vec<Turn<'a>> {
    let mut turns = Vec::new();
    let mut current_user: Vec<&'a Event> = Vec::new();
    let mut current_agent: Vec<&'a Event> = Vec::new();
    let mut current_start_display = 0usize;
    let mut current_anchor_source = 0usize;
    let mut current_turn_index = 0usize;
    let mut seen_any = false;

    for (display_idx, de) in events.iter().enumerate() {
        let event = de.event();
        if is_control_event(event) {
            continue;
        }
        if !seen_any {
            current_start_display = display_idx;
            current_anchor_source = de.source_index();
            seen_any = true;
        }
        if matches!(event.event_type, EventType::UserMessage) {
            if !current_user.is_empty() || !current_agent.is_empty() {
                turns.push(Turn {
                    turn_index: current_turn_index,
                    start_display_index: current_start_display,
                    end_display_index: display_idx.saturating_sub(1),
                    anchor_source_index: current_anchor_source,
                    user_events: std::mem::take(&mut current_user),
                    agent_events: std::mem::take(&mut current_agent),
                });
                current_turn_index += 1;
                current_start_display = display_idx;
                current_anchor_source = de.source_index();
            }
            current_user.push(event);
        } else {
            current_agent.push(event);
        }
    }

    if !current_user.is_empty() || !current_agent.is_empty() {
        turns.push(Turn {
            turn_index: current_turn_index,
            start_display_index: current_start_display,
            end_display_index: events.len().saturating_sub(1),
            anchor_source_index: current_anchor_source,
            user_events: current_user,
            agent_events: current_agent,
        });
    }

    turns
}

fn turn_has_visible_prompt(turn: &Turn<'_>) -> bool {
    turn.user_events.iter().any(|event| {
        if is_control_event(event) {
            return false;
        }
        event_user_text(event)
            .map(|text| {
                text.lines().any(|line| {
                    let trimmed = line.trim();
                    !trimmed.is_empty() && !is_control_user_text(trimmed)
                })
            })
            .unwrap_or(true)
    })
}

pub fn extract_visible_turns<'a>(events: &[DisplayEvent<'a>]) -> Vec<Turn<'a>> {
    extract_turns(events)
        .into_iter()
        .filter(turn_has_visible_prompt)
        .collect()
}

#[cfg(test)]
mod turn_extract_tests {
    use super::*;
    use crate::live::{LiveUpdate, LiveUpdateBatch};
    use chrono::{Duration as ChronoDuration, Utc};
    use opensession_core::trace::{Agent, Content, Session};
    use serde_json::Value;

    fn make_event(event_id: &str, event_type: EventType, text: &str) -> Event {
        Event {
            event_id: event_id.to_string(),
            timestamp: Utc::now(),
            event_type,
            task_id: None,
            content: Content::text(text),
            duration_ms: None,
            attributes: HashMap::new(),
        }
    }

    fn make_event_with_task(
        event_id: &str,
        event_type: EventType,
        text: &str,
        task_id: &str,
        merged_subagent: bool,
    ) -> Event {
        let mut event = make_event(event_id, event_type, text);
        event.task_id = Some(task_id.to_string());
        if merged_subagent {
            event
                .attributes
                .insert("merged_subagent".to_string(), Value::Bool(true));
            event.attributes.insert(
                "subagent_id".to_string(),
                Value::String(task_id.to_string()),
            );
        }
        event
    }

    fn make_live_session(session_id: &str, event_count: usize) -> Session {
        let mut session = Session::new(
            session_id.to_string(),
            Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );

        for idx in 0..event_count {
            let event_type = if idx % 2 == 0 {
                EventType::UserMessage
            } else {
                EventType::AgentMessage
            };
            session.events.push(make_event(
                &format!("e-{idx}"),
                event_type,
                &format!("line-{idx}"),
            ));
        }
        session.recompute_stats();
        session
    }

    fn make_repo_db_row(id: &str, repo: &str) -> LocalSessionRow {
        LocalSessionRow {
            id: id.to_string(),
            source_path: None,
            sync_status: "synced".to_string(),
            last_synced_at: None,
            user_id: Some("user-1".to_string()),
            nickname: Some("alice".to_string()),
            team_id: None,
            tool: "codex".to_string(),
            agent_provider: Some("openai".to_string()),
            agent_model: Some("gpt-5".to_string()),
            title: Some(format!("{repo} candidate")),
            description: None,
            tags: None,
            created_at: "2026-02-20T00:00:00Z".to_string(),
            uploaded_at: None,
            message_count: 2,
            user_message_count: 1,
            task_count: 0,
            event_count: 2,
            duration_seconds: 1,
            total_input_tokens: 0,
            total_output_tokens: 0,
            git_remote: None,
            git_branch: None,
            git_commit: None,
            git_repo_name: Some(repo.to_string()),
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
    fn settings_sections_expose_web_capture_storage_git() {
        assert_eq!(SettingsSection::ORDER.len(), 4);
        assert_eq!(SettingsSection::Workspace.label(), "Web Sync (Public)");
        assert_eq!(SettingsSection::CaptureSync.label(), "Capture Flow");
        assert_eq!(SettingsSection::Git.label(), "Git");
        assert_eq!(
            SettingsSection::Workspace.panel_title(),
            "Web Sync (Public)"
        );
    }

    #[test]
    fn global_tab_shortcut_opens_handoff_menu() {
        let mut app = App::new(Vec::new());
        assert_eq!(app.active_tab, Tab::Sessions);
        assert_eq!(app.view, View::SessionList);

        app.handle_key(KeyCode::Char('2'));
        assert_eq!(app.active_tab, Tab::Handoff);
        assert_eq!(app.view, View::Handoff);

        app.handle_key(KeyCode::Esc);
        assert_eq!(app.active_tab, Tab::Sessions);
        assert_eq!(app.view, View::SessionList);
    }

    #[test]
    fn global_tab_shortcut_three_opens_settings() {
        let mut app = App::new(Vec::new());
        app.handle_key(KeyCode::Char('3'));
        assert_eq!(app.active_tab, Tab::Settings);
        assert_eq!(app.view, View::Settings);
    }

    #[test]
    fn extract_turns_ignores_control_messages() {
        let events = vec![
            make_event(
                "e1",
                EventType::UserMessage,
                "Warning: apply_patch was requested via exec_command. Use the apply_patch tool instead of exec_command.",
            ),
            make_event(
                "e2",
                EventType::UserMessage,
                "<turn_aborted>Request interrupted by user for tool use</turn_aborted>",
            ),
            make_event(
                "e2b",
                EventType::UserMessage,
                "<instructions>system control message</instructions>",
            ),
            make_event(
                "e3",
                EventType::Custom {
                    kind: "turn_aborted".to_string(),
                },
                "turn aborted",
            ),
            make_event("e4", EventType::UserMessage, "real user prompt"),
            make_event("e5", EventType::AgentMessage, "assistant response"),
        ];
        let display = vec![
            DisplayEvent::Single {
                event: &events[0],
                source_index: 0,
                lane: 0,
                marker: LaneMarker::None,
                active_lanes: vec![0],
            },
            DisplayEvent::Single {
                event: &events[1],
                source_index: 1,
                lane: 0,
                marker: LaneMarker::None,
                active_lanes: vec![0],
            },
            DisplayEvent::Single {
                event: &events[2],
                source_index: 2,
                lane: 0,
                marker: LaneMarker::None,
                active_lanes: vec![0],
            },
            DisplayEvent::Single {
                event: &events[3],
                source_index: 3,
                lane: 0,
                marker: LaneMarker::None,
                active_lanes: vec![0],
            },
            DisplayEvent::Single {
                event: &events[4],
                source_index: 4,
                lane: 0,
                marker: LaneMarker::None,
                active_lanes: vec![0],
            },
            DisplayEvent::Single {
                event: &events[5],
                source_index: 5,
                lane: 0,
                marker: LaneMarker::None,
                active_lanes: vec![0],
            },
        ];

        let turns = extract_turns(&display);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].user_events.len(), 1);
        let user_text = turns[0].user_events[0]
            .content
            .blocks
            .iter()
            .find_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .unwrap_or("");
        assert_eq!(user_text, "real user prompt");
    }

    #[test]
    fn extract_visible_turns_hides_no_prompt_turns() {
        let events = vec![
            make_event("e1", EventType::AgentMessage, "assistant-only turn"),
            make_event("e2", EventType::UserMessage, "real prompt"),
            make_event("e3", EventType::AgentMessage, "assistant response"),
        ];
        let display = vec![
            DisplayEvent::Single {
                event: &events[0],
                source_index: 0,
                lane: 0,
                marker: LaneMarker::None,
                active_lanes: vec![0],
            },
            DisplayEvent::Single {
                event: &events[1],
                source_index: 1,
                lane: 0,
                marker: LaneMarker::None,
                active_lanes: vec![0],
            },
            DisplayEvent::Single {
                event: &events[2],
                source_index: 2,
                lane: 0,
                marker: LaneMarker::None,
                active_lanes: vec![0],
            },
        ];

        let turns = extract_visible_turns(&display);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].user_events.len(), 1);
    }

    #[test]
    fn rebuild_columns_groups_local_sessions_by_agent_count() {
        let mut app = App::new(vec![
            Session::new(
                "s1".to_string(),
                Agent {
                    provider: "anthropic".to_string(),
                    model: "claude".to_string(),
                    tool: "claude-code".to_string(),
                    tool_version: None,
                },
            ),
            Session::new(
                "s2".to_string(),
                Agent {
                    provider: "openai".to_string(),
                    model: "gpt-5".to_string(),
                    tool: "codex".to_string(),
                    tool_version: None,
                },
            ),
            Session::new(
                "s3".to_string(),
                Agent {
                    provider: "openai".to_string(),
                    model: "gpt-5".to_string(),
                    tool: "codex".to_string(),
                    tool_version: None,
                },
            ),
        ]);
        app.filtered_sessions = vec![0, 1, 2];
        app.session_max_active_agents.insert("s1".to_string(), 1);
        app.session_max_active_agents.insert("s2".to_string(), 3);
        app.session_max_active_agents.insert("s3".to_string(), 3);
        app.list_layout = ListLayout::ByUser;

        app.rebuild_columns();

        assert_eq!(
            app.column_users,
            vec!["3 agents".to_string(), "1 agent".to_string()]
        );
        assert_eq!(app.column_session_indices("3 agents"), vec![1, 2]);
        assert_eq!(app.column_session_indices("1 agent"), vec![0]);
    }

    #[test]
    fn list_navigation_crosses_page_boundary() {
        let mut app = App::new(vec![
            Session::new(
                "s1".to_string(),
                Agent {
                    provider: "anthropic".to_string(),
                    model: "claude".to_string(),
                    tool: "claude-code".to_string(),
                    tool_version: None,
                },
            ),
            Session::new(
                "s2".to_string(),
                Agent {
                    provider: "openai".to_string(),
                    model: "gpt-5".to_string(),
                    tool: "codex".to_string(),
                    tool_version: None,
                },
            ),
            Session::new(
                "s3".to_string(),
                Agent {
                    provider: "openai".to_string(),
                    model: "gpt-5".to_string(),
                    tool: "codex".to_string(),
                    tool_version: None,
                },
            ),
        ]);
        app.per_page = 2;
        app.page = 0;
        app.list_state.select(Some(1));

        app.list_next();
        assert_eq!(app.page, 1);
        assert_eq!(app.list_state.selected(), Some(0));

        app.list_prev();
        assert_eq!(app.page, 0);
        assert_eq!(app.list_state.selected(), Some(1));
    }

    #[test]
    fn get_base_visible_events_keeps_claude_merged_subagent_events() {
        let mut session = Session::new(
            "s-hidden".to_string(),
            Agent {
                provider: "anthropic".to_string(),
                model: "claude-sonnet".to_string(),
                tool: "claude-code".to_string(),
                tool_version: None,
            },
        );
        session.events = vec![
            make_event("u1", EventType::UserMessage, "prompt"),
            make_event_with_task(
                "s-start",
                EventType::TaskStart {
                    title: Some("subagent".to_string()),
                },
                "",
                "agent-123",
                true,
            ),
            make_event_with_task(
                "s-msg",
                EventType::AgentMessage,
                "subagent response",
                "agent-123",
                true,
            ),
            make_event_with_task(
                "s-end",
                EventType::TaskEnd {
                    summary: Some("done".to_string()),
                },
                "",
                "agent-123",
                true,
            ),
            make_event("a1", EventType::AgentMessage, "main response"),
        ];
        session.recompute_stats();

        let app = App::new(vec![session]);
        let visible = app.get_base_visible_events(&app.sessions[0]);

        assert_eq!(visible.len(), 5);
        assert!(visible
            .iter()
            .any(|row| row.event().task_id.as_deref() == Some("agent-123")));
        assert_eq!(app.session_max_active_agents.get("s-hidden"), Some(&2usize));
    }

    #[test]
    fn selected_session_actor_label_uses_local_nickname_attribute() {
        let mut session = Session::new(
            "s-local".to_string(),
            Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );
        session
            .context
            .attributes
            .insert("nickname".to_string(), Value::String("alice".to_string()));

        let app = App::new(vec![session]);
        assert_eq!(
            app.selected_session_actor_label().as_deref(),
            Some("@alice")
        );
    }

    #[test]
    fn selected_session_actor_label_falls_back_to_db_user_id() {
        let mut app = App::new(vec![]);
        app.view_mode = ViewMode::Repo("repo-1".to_string());
        app.db_sessions = vec![LocalSessionRow {
            id: "db-1".to_string(),
            source_path: None,
            sync_status: "synced".to_string(),
            last_synced_at: None,
            user_id: Some("0123456789abcdef".to_string()),
            nickname: None,
            team_id: None,
            tool: "codex".to_string(),
            agent_provider: None,
            agent_model: None,
            title: None,
            description: None,
            tags: None,
            created_at: "2026-02-14T00:00:00Z".to_string(),
            uploaded_at: None,
            message_count: 1,
            user_message_count: 1,
            task_count: 0,
            event_count: 1,
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
        }];
        app.list_state.select(Some(0));

        assert_eq!(
            app.selected_session_actor_label().as_deref(),
            Some("id:0123456789")
        );
    }

    #[test]
    fn list_repo_picker_opens_and_selects_repo() {
        let mut app = App::new(vec![]);
        app.repos = vec!["alpha/repo".to_string(), "beta/repo".to_string()];

        app.handle_list_key(KeyCode::Char('R'));
        assert!(app.repo_picker_open);

        app.handle_repo_picker_key(KeyCode::Char('b'));
        app.handle_repo_picker_key(KeyCode::Enter);

        assert!(!app.repo_picker_open);
        assert_eq!(app.view_mode, ViewMode::Repo("beta/repo".to_string()));
    }

    #[test]
    fn list_a_key_cycles_agent_filter() {
        let mut app = App::new(vec![
            make_live_session("agent-a", 2),
            make_live_session("agent-b", 2),
        ]);
        app.sessions[0].agent.tool = "codex".to_string();
        app.sessions[1].agent.tool = "claude-code".to_string();
        app.rebuild_available_tools();
        app.apply_filter();

        assert_eq!(app.active_agent_filter(), None);
        app.handle_list_key(KeyCode::Char('a'));
        assert!(app.active_agent_filter().is_some());
    }

    #[test]
    fn list_tab_cycles_repo_view_even_if_active_tab_drifted() {
        let mut app = App::new(vec![]);
        app.repos = vec!["alpha/repo".to_string()];
        app.active_tab = Tab::Handoff;
        app.view = View::SessionList;
        app.view_mode = ViewMode::Local;

        app.handle_key(KeyCode::Tab);

        assert_eq!(app.view_mode, ViewMode::Repo("alpha/repo".to_string()));
    }

    #[test]
    fn list_bracket_keys_do_not_change_page_anymore() {
        let mut app = App::new(vec![
            Session::new(
                "s1".to_string(),
                Agent {
                    provider: "openai".to_string(),
                    model: "gpt-5".to_string(),
                    tool: "codex".to_string(),
                    tool_version: None,
                },
            ),
            Session::new(
                "s2".to_string(),
                Agent {
                    provider: "openai".to_string(),
                    model: "gpt-5".to_string(),
                    tool: "codex".to_string(),
                    tool_version: None,
                },
            ),
        ]);
        app.per_page = 1;
        app.page = 0;
        app.apply_filter();

        app.handle_list_key(KeyCode::Char(']'));
        assert_eq!(app.page, 0);

        app.handle_list_key(KeyCode::Char('['));
        assert_eq!(app.page, 0);
    }

    #[test]
    fn list_right_key_does_not_open_session_detail() {
        let mut app = App::new(vec![Session::new(
            "s1".to_string(),
            Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        )]);
        assert_eq!(app.view, View::SessionList);

        app.handle_list_key(KeyCode::Right);

        assert_eq!(app.view, View::SessionList);
    }

    #[test]
    fn live_detail_enters_at_tail_in_linear_and_turn_modes() {
        let session = make_live_session("live-tail", 4);

        let mut linear_app = App::new(vec![session.clone()]);
        linear_app.enter_detail();
        linear_app.detail_view_mode = DetailViewMode::Linear;
        linear_app.jump_to_latest_linear();
        assert!(linear_app.live_mode);
        assert_eq!(linear_app.detail_event_index, 3);

        let mut turn_app = App::new(vec![session]);
        turn_app.focus_detail_view = true;
        turn_app.enter_detail();
        turn_app.detail_view_mode = DetailViewMode::Turn;
        turn_app.jump_to_latest_turn();
        assert!(turn_app.live_mode);
        assert_eq!(turn_app.turn_index, 1);
    }

    #[test]
    fn refresh_live_mode_turns_on_when_source_was_modified_recently() {
        let unique = format!(
            "ops-live-source-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        );
        let dir = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("session.jsonl");
        let ts = Utc::now().to_rfc3339();
        std::fs::write(&path, format!(r#"{{"timestamp":"{ts}"}}"#)).expect("write file");

        let session = make_live_session("live-source-recent", 2);
        let mut app = App::new(vec![session]);
        app.enter_detail();
        app.detail_source_path = Some(path.clone());
        app.live_subscription = None;
        app.live_mode = false;
        app.refresh_live_mode();

        assert!(app.live_mode);

        std::fs::remove_file(&path).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn refresh_live_mode_ignores_recent_mtime_when_source_events_are_stale() {
        let unique = format!(
            "ops-live-source-stale-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        );
        let dir = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("session.jsonl");
        let stale_ts = (Utc::now() - ChronoDuration::hours(12)).to_rfc3339();
        std::fs::write(&path, format!(r#"{{"timestamp":"{stale_ts}"}}"#)).expect("write file");

        let mut session = make_live_session("live-source-stale", 2);
        let stale_event = Utc::now() - ChronoDuration::hours(10);
        session.context.created_at = stale_event;
        session.context.updated_at = stale_event;
        for event in &mut session.events {
            event.timestamp = stale_event;
        }
        let mut app = App::new(vec![session]);
        app.enter_detail();
        app.detail_source_path = Some(path.clone());
        app.live_subscription = None;
        app.live_mode = false;
        app.refresh_live_mode();

        assert!(!app.live_mode);

        std::fs::remove_file(&path).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn focus_detail_view_v_does_not_toggle_detail_mode() {
        let session = make_live_session("focus-v-toggle", 4);
        let mut app = App::new(vec![session]);
        app.focus_detail_view = true;
        app.enter_detail();
        assert_eq!(app.detail_view_mode, DetailViewMode::Linear);

        app.handle_detail_key(KeyCode::Char('v'));
        assert_eq!(app.detail_view_mode, DetailViewMode::Linear);
    }

    #[test]
    fn detail_esc_leaves_detail_view() {
        let session = make_live_session("turn-esc-back", 4);
        let mut app = App::new(vec![session]);
        app.enter_detail();
        assert_eq!(app.view, View::SessionDetail);

        let should_quit = app.handle_detail_key(KeyCode::Esc);

        assert!(!should_quit);
        assert_eq!(app.view, View::SessionList);
    }

    #[test]
    fn detail_left_scrolls_horizontal_timeline() {
        let session = make_live_session("turn-left-stays-turn", 4);
        let mut app = App::new(vec![session]);
        app.enter_detail();
        app.detail_h_scroll = 8;

        app.handle_detail_key(KeyCode::Left);

        assert_eq!(app.detail_view_mode, DetailViewMode::Linear);
        assert_eq!(app.detail_h_scroll, 4);
    }

    #[test]
    fn detail_p_key_is_noop_in_linear_mode() {
        let session = make_live_session("turn-prompt-toggle", 4);
        let mut app = App::new(vec![session]);
        app.enter_detail();
        app.turn_index = 1;

        assert!(!app.turn_prompt_expanded.contains(&1));
        app.handle_detail_key(KeyCode::Char('p'));
        assert!(!app.turn_prompt_expanded.contains(&1));
    }

    #[test]
    fn detail_numeric_keys_toggle_event_filters() {
        let session = make_live_session("turn-filter-toggle", 4);
        let mut app = App::new(vec![session]);
        app.enter_detail();

        assert!(app.event_filters.contains(&EventFilter::All));
        app.handle_detail_key(KeyCode::Char('3'));
        assert!(app.event_filters.contains(&EventFilter::Agent));
        assert!(!app.event_filters.contains(&EventFilter::All));

        app.handle_detail_key(KeyCode::Char('9'));
        assert!(app.event_filters.contains(&EventFilter::Web));

        app.handle_detail_key(KeyCode::Char('0'));
        assert!(app.event_filters.contains(&EventFilter::Other));

        app.handle_detail_key(KeyCode::Char('1'));
        assert!(app.event_filters.contains(&EventFilter::All));
        assert_eq!(app.event_filters.len(), 1);
    }

    #[test]
    fn turn_numeric_keys_toggle_event_filters_to_nine() {
        let session = make_live_session("turn-filter-toggle-turn", 4);
        let mut app = App::new(vec![session]);
        app.enter_detail();
        app.detail_view_mode = DetailViewMode::Turn;

        app.handle_turn_key(KeyCode::Char('8'));
        assert!(app.event_filters.contains(&EventFilter::Task));
        assert!(!app.event_filters.contains(&EventFilter::All));

        app.handle_turn_key(KeyCode::Char('9'));
        assert!(app.event_filters.contains(&EventFilter::Web));

        app.handle_turn_key(KeyCode::Char('0'));
        assert!(app.event_filters.contains(&EventFilter::Other));
    }

    #[test]
    fn detail_d_key_toggles_diff_expansion_only() {
        let session = make_live_session("turn-diff-toggle", 4);
        let mut app = App::new(vec![session]);
        app.enter_detail();
        app.detail_event_index = 2;

        assert!(!app.expanded_diff_events.contains(&2));

        app.handle_detail_key(KeyCode::Char('d'));
        assert!(app.expanded_diff_events.contains(&2));

        app.handle_detail_key(KeyCode::Char('d'));
        assert!(!app.expanded_diff_events.contains(&2));
    }

    #[test]
    fn matches_event_filter_uses_expanded_bucket_mapping() {
        let mut app = App::new(Vec::new());

        app.event_filters = HashSet::from([EventFilter::User]);
        assert!(app.matches_event_filter(&EventType::UserMessage));
        assert!(app.matches_event_filter(&EventType::SystemMessage));
        assert!(!app.matches_event_filter(&EventType::AgentMessage));

        app.event_filters = HashSet::from([EventFilter::Agent]);
        assert!(app.matches_event_filter(&EventType::AgentMessage));
        assert!(!app.matches_event_filter(&EventType::UserMessage));

        app.event_filters = HashSet::from([EventFilter::Think]);
        assert!(app.matches_event_filter(&EventType::Thinking));

        app.event_filters = HashSet::from([EventFilter::Tools]);
        assert!(app.matches_event_filter(&EventType::ToolCall {
            name: "x".to_string()
        }));
        assert!(app.matches_event_filter(&EventType::ToolResult {
            name: "x".to_string(),
            is_error: false,
            call_id: None
        }));

        app.event_filters = HashSet::from([EventFilter::Files]);
        assert!(app.matches_event_filter(&EventType::FileRead {
            path: "a".to_string()
        }));
        assert!(app.matches_event_filter(&EventType::CodeSearch {
            query: "q".to_string()
        }));

        app.event_filters = HashSet::from([EventFilter::Shell]);
        assert!(app.matches_event_filter(&EventType::ShellCommand {
            command: "echo hi".to_string(),
            exit_code: Some(0)
        }));

        app.event_filters = HashSet::from([EventFilter::Task]);
        assert!(app.matches_event_filter(&EventType::TaskStart { title: None }));
        assert!(app.matches_event_filter(&EventType::TaskEnd { summary: None }));

        app.event_filters = HashSet::from([EventFilter::Web]);
        assert!(app.matches_event_filter(&EventType::WebSearch {
            query: "rust".to_string()
        }));
        assert!(app.matches_event_filter(&EventType::WebFetch {
            url: "https://example.com".to_string()
        }));
        assert!(!app.matches_event_filter(&EventType::Custom {
            kind: "x".to_string()
        }));

        app.event_filters = HashSet::from([EventFilter::Other]);
        assert!(app.matches_event_filter(&EventType::Custom {
            kind: "x".to_string()
        }));
        assert!(app.matches_event_filter(&EventType::ImageGenerate {
            prompt: "draw".to_string()
        }));
    }

    #[test]
    fn apply_discovered_sessions_keeps_selected_session_when_present() {
        let sessions = vec![
            make_live_session("session-a", 2),
            make_live_session("session-b", 2),
            make_live_session("session-c", 2),
        ];
        let mut app = App::new(sessions);
        assert!(app.select_session_by_id("session-c"));

        let next = vec![
            make_live_session("session-z", 2),
            make_live_session("session-c", 3),
            make_live_session("session-q", 2),
        ];
        app.apply_discovered_sessions(next);

        let selected_id = app
            .selected_session()
            .map(|session| session.session_id.clone());
        assert_eq!(selected_id.as_deref(), Some("session-c"));
    }

    #[test]
    fn handoff_tab_supports_picker_navigation_and_enter() {
        let sessions = vec![
            make_live_session("handoff-a", 2),
            make_live_session("handoff-b", 2),
        ];
        let mut app = App::new(sessions);
        app.handle_key(KeyCode::Char('2'));
        assert_eq!(app.view, View::Handoff);
        let before = app
            .selected_handoff_candidate()
            .map(|candidate| candidate.session_id)
            .expect("initial handoff candidate");

        app.handle_key(KeyCode::Char('j'));
        let selected_after = app
            .selected_handoff_candidate()
            .map(|candidate| candidate.session_id)
            .expect("moved handoff candidate");
        assert_ne!(selected_after, before);

        app.handle_key(KeyCode::Enter);
        assert_eq!(app.view, View::Handoff);
        assert_eq!(app.active_tab, Tab::Handoff);
        let message = app
            .flash_message
            .as_ref()
            .map(|(msg, _)| msg.clone())
            .unwrap_or_default();
        assert!(message.to_ascii_lowercase().contains("preview"));
    }

    #[test]
    fn handoff_picker_space_supports_multi_selection() {
        let sessions = vec![
            make_live_session("handoff-a", 2),
            make_live_session("handoff-b", 2),
            make_live_session("handoff-c", 2),
        ];
        let mut app = App::new(sessions);
        app.handle_key(KeyCode::Char('2'));
        assert_eq!(app.view, View::Handoff);
        let candidate_order = app
            .handoff_candidates()
            .into_iter()
            .map(|candidate| candidate.session_id)
            .collect::<Vec<_>>();
        assert_eq!(candidate_order.len(), 3);

        app.handle_key(KeyCode::Char(' ')); // select a
        app.handle_key(KeyCode::Char('j'));
        app.handle_key(KeyCode::Char(' ')); // select b
        app.handle_key(KeyCode::Char('j'));
        app.handle_key(KeyCode::Char(' ')); // select c
        assert_eq!(app.handoff_selected_session_ids, candidate_order);

        let selected = app
            .handoff_selected_candidates()
            .into_iter()
            .map(|candidate| candidate.session_id)
            .collect::<Vec<_>>();
        assert_eq!(selected, app.handoff_selected_session_ids);

        app.handle_key(KeyCode::Char('j'));
        app.handle_key(KeyCode::Char(' ')); // deselect c
        assert_eq!(
            app.handoff_selected_session_ids,
            vec![selected[0].clone(), selected[1].clone()]
        );
    }

    #[test]
    fn handoff_generate_without_candidates_shows_info() {
        let mut app = App::new(vec![]);
        app.handle_key(KeyCode::Char('2'));
        assert_eq!(app.view, View::Handoff);

        app.handle_key(KeyCode::Char('g'));
        let message = app
            .flash_message
            .as_ref()
            .map(|(msg, _)| msg.clone())
            .unwrap_or_default();
        assert!(message
            .to_ascii_lowercase()
            .contains("no handoff candidate"));
    }

    #[test]
    fn handoff_generate_requires_local_source_files() {
        let sessions = vec![make_live_session("handoff-a", 2)];
        let mut app = App::new(sessions);
        app.handle_key(KeyCode::Char('2'));
        assert_eq!(app.view, View::Handoff);

        app.handle_key(KeyCode::Char('g'));
        let message = app
            .flash_message
            .as_ref()
            .map(|(msg, _)| msg.clone())
            .unwrap_or_default();
        assert!(message.to_ascii_lowercase().contains("local source file"));
    }

    #[test]
    fn handoff_enter_preserves_repo_scope_selection() {
        let sessions = vec![make_live_session("handoff-db", 2)];
        let mut app = App::new(sessions);
        app.search_query = "repo-only".to_string();
        app.view_mode = ViewMode::Repo("repo-1".to_string());
        app.db_sessions = vec![make_repo_db_row("handoff-db", "repo-1")];
        app.list_state.select(Some(0));

        app.handle_key(KeyCode::Char('2'));
        assert_eq!(app.view, View::Handoff);
        assert_eq!(app.active_tab, Tab::Handoff);
        assert_eq!(app.view_mode, ViewMode::Repo("repo-1".to_string()));

        app.handle_key(KeyCode::Enter);

        assert_eq!(app.active_tab, Tab::Handoff);
        assert_eq!(app.view, View::Handoff);
        assert_eq!(app.view_mode, ViewMode::Repo("repo-1".to_string()));
        let message = app
            .flash_message
            .as_ref()
            .map(|(msg, _)| msg.clone())
            .unwrap_or_default();
        assert!(message.to_ascii_lowercase().contains("preview"));
    }

    #[test]
    fn handoff_esc_preserves_repo_scope() {
        let sessions = vec![make_live_session("handoff-db", 2)];
        let mut app = App::new(sessions);
        app.view_mode = ViewMode::Repo("repo-1".to_string());
        app.db_sessions = vec![make_repo_db_row("handoff-db", "repo-1")];
        app.list_state.select(Some(0));

        app.handle_key(KeyCode::Char('2'));
        assert_eq!(app.view, View::Handoff);
        assert_eq!(app.active_tab, Tab::Handoff);

        app.handle_key(KeyCode::Esc);

        assert_eq!(app.active_tab, Tab::Sessions);
        assert_eq!(app.view, View::SessionList);
        assert_eq!(app.view_mode, ViewMode::Repo("repo-1".to_string()));
    }

    #[test]
    fn handoff_enter_remote_only_candidate_stays_on_handoff() {
        let mut app = App::new(vec![]);
        app.view_mode = ViewMode::Repo("repo-1".to_string());
        app.db_sessions = vec![make_repo_db_row("remote-only", "repo-1")];
        app.list_state.select(Some(0));
        app.daemon_config.server.url = "https://opensession.test".to_string();

        app.handle_key(KeyCode::Char('2'));
        assert_eq!(app.view, View::Handoff);
        assert_eq!(app.active_tab, Tab::Handoff);

        app.handle_key(KeyCode::Enter);

        assert_eq!(app.view, View::Handoff);
        assert_eq!(app.active_tab, Tab::Handoff);
        let message = app
            .flash_message
            .as_ref()
            .map(|(msg, _)| msg.clone())
            .unwrap_or_default();
        assert!(message.to_ascii_lowercase().contains("preview"));
    }

    #[test]
    fn handoff_tab_seeds_selection_from_multi_column_focus() {
        let mut app = App::new(vec![
            make_live_session("s1", 2),
            make_live_session("s2", 2),
            make_live_session("s3", 2),
        ]);
        app.filtered_sessions = vec![0, 1, 2];
        app.session_max_active_agents.insert("s1".to_string(), 1);
        app.session_max_active_agents.insert("s2".to_string(), 3);
        app.session_max_active_agents.insert("s3".to_string(), 3);
        app.list_layout = ListLayout::ByUser;
        app.rebuild_columns();

        app.list_state.select(Some(1));
        app.column_focus = 1;
        if let Some(state) = app.column_list_states.get_mut(1) {
            state.select(Some(0));
        }

        app.handle_key(KeyCode::Char('2'));
        assert_eq!(app.view, View::Handoff);
        assert_eq!(app.active_tab, Tab::Handoff);

        let selected = app
            .selected_handoff_candidate()
            .map(|candidate| candidate.session_id)
            .expect("handoff candidate");
        assert_eq!(selected, "s1");
    }

    #[test]
    fn get_base_visible_events_hides_write_stdin_polling_noise() {
        let mut session = Session::new(
            "stdin-noise".to_string(),
            Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );
        session.events = vec![
            make_event("u1", EventType::UserMessage, "run command"),
            make_event(
                "c1",
                EventType::ToolCall {
                    name: "write_stdin".to_string(),
                },
                "",
            ),
            make_event(
                "r1",
                EventType::ToolResult {
                    name: "write_stdin".to_string(),
                    is_error: false,
                    call_id: None,
                },
                "Process running with session ID 1915",
            ),
            make_event("a1", EventType::AgentMessage, "done"),
        ];
        session.recompute_stats();

        let app = App::new(vec![session]);
        let visible = app.get_base_visible_events(&app.sessions[0]);
        assert_eq!(visible.len(), 2);
        assert!(visible.iter().all(|event| {
            !matches!(
                event.event().event_type,
                EventType::ToolCall { ref name } if name == "write_stdin"
            )
        }));
    }

    #[test]
    fn get_base_visible_events_hides_markdown_progress_thinking_noise() {
        let mut session = Session::new(
            "thinking-noise".to_string(),
            Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );
        session.events = vec![
            make_event("u1", EventType::UserMessage, "prompt"),
            make_event(
                "t1",
                EventType::Thinking,
                "**Adjusting command to capture all results**",
            ),
            make_event("a1", EventType::AgentMessage, "done"),
        ];
        session.recompute_stats();

        let app = App::new(vec![session]);
        let visible = app.get_base_visible_events(&app.sessions[0]);
        assert_eq!(visible.len(), 2);
        assert!(visible
            .iter()
            .all(|event| !matches!(event.event().event_type, EventType::Thinking)));
    }

    #[test]
    fn get_base_visible_events_never_drops_user_or_agent_messages() {
        let mut session = Session::new(
            "message-preserve".to_string(),
            Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );
        session.events = vec![
            make_event(
                "u1",
                EventType::UserMessage,
                "**Adjusting command to capture all results**",
            ),
            make_event(
                "a1",
                EventType::AgentMessage,
                "**Summarizing final commit details**",
            ),
        ];
        session.recompute_stats();

        let app = App::new(vec![session]);
        let visible = app.get_base_visible_events(&app.sessions[0]);
        assert_eq!(visible.len(), 2);
        assert!(matches!(
            visible[0].event().event_type,
            EventType::UserMessage
        ));
        assert!(matches!(
            visible[1].event().event_type,
            EventType::AgentMessage
        ));
    }

    #[test]
    fn source_error_hint_detects_recent_json_error() {
        let unique = format!(
            "ops-source-error-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        );
        let dir = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("session.log");
        std::fs::write(
            &path,
            "ok line\n{\"level\":\"error\",\"message\":\"parse exploded\"}\n",
        )
        .expect("write");

        let hint = App::source_error_hint(&path).expect("error hint");
        assert!(hint.contains("parse exploded"));

        std::fs::remove_file(&path).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn enter_detail_marks_zero_event_issue() {
        let session = Session::new(
            "zero-event".to_string(),
            Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );

        let mut app = App::new(vec![session]);
        app.enter_detail();

        let sid = app.sessions[0].session_id.clone();
        let issue = app
            .detail_issue_for_session(&sid)
            .expect("detail issue should be recorded");
        assert!(issue.contains("No parsed events"));
    }

    #[test]
    fn jump_to_latest_turn_uses_tail_scroll_anchor() {
        let session = make_live_session("turn-tail-anchor", 6);
        let mut app = App::new(vec![session]);
        app.enter_detail();
        app.detail_view_mode = DetailViewMode::Turn;
        app.jump_to_latest_turn();

        assert_eq!(app.turn_agent_scroll, u16::MAX);
    }

    #[test]
    fn live_reload_assigns_lane_for_spawn_task_without_explicit_task_start() {
        let mut session = Session::new(
            "live-spawn-lane".to_string(),
            Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );
        session
            .events
            .push(make_event("seed", EventType::UserMessage, "seed"));
        session.recompute_stats();

        let mut app = App::new(vec![session.clone()]);
        app.enter_detail();

        let mut reloaded = session.clone();
        let mut spawned = make_event(
            "spawn-msg",
            EventType::AgentMessage,
            "spawned worker output",
        );
        spawned.task_id = Some("spawn-task-1".to_string());
        reloaded.events.push(spawned.clone());
        reloaded.recompute_stats();

        let batch = LiveUpdateBatch {
            updates: vec![
                LiveUpdate::SessionReloaded(Box::new(reloaded.clone())),
                LiveUpdate::EventsAppended(vec![spawned]),
            ],
            cursor: Some(reloaded.events.len() as u64),
            source_offset: Some(10),
            last_event_at: reloaded.events.last().map(|event| event.timestamp),
            active: true,
        };
        app.apply_live_update_batch(batch);

        let visible = app.get_base_visible_events(&app.sessions[0]);
        let lane = visible
            .iter()
            .find_map(|row| match row {
                DisplayEvent::Single { event, lane, .. } if event.event_id == "spawn-msg" => {
                    Some(*lane)
                }
                _ => None,
            })
            .unwrap_or(0);
        assert!(lane > 0);
    }

    #[test]
    fn live_follow_detach_then_reattach_controls_auto_jump() {
        let session = make_live_session("live-follow", 4);
        let mut app = App::new(vec![session.clone()]);
        app.enter_detail();
        app.detail_view_mode = DetailViewMode::Linear;
        app.jump_to_latest_linear();
        assert_eq!(app.detail_event_index, 3);
        assert_eq!(app.detail_follow_status_label(), "ON");

        app.handle_detail_key(KeyCode::Up);
        assert_eq!(app.detail_event_index, 2);
        assert_eq!(app.detail_follow_status_label(), "OFF");

        let mut reloaded = session.clone();
        reloaded.events.push(make_event(
            "e-4",
            EventType::AgentMessage,
            "appended-after-detach",
        ));
        reloaded.recompute_stats();

        let detached_batch = LiveUpdateBatch {
            updates: vec![
                LiveUpdate::SessionReloaded(Box::new(reloaded.clone())),
                LiveUpdate::EventsAppended(vec![reloaded.events.last().cloned().expect("event")]),
            ],
            cursor: Some(reloaded.events.len() as u64),
            source_offset: Some(1),
            last_event_at: reloaded.events.last().map(|event| event.timestamp),
            active: true,
        };
        app.apply_live_update_batch(detached_batch);
        assert_eq!(app.detail_event_index, 2);

        app.handle_detail_key(KeyCode::End);
        assert_eq!(app.detail_follow_status_label(), "ON");

        let mut reloaded2 = reloaded.clone();
        reloaded2.events.push(make_event(
            "e-5",
            EventType::AgentMessage,
            "appended-after-reattach",
        ));
        reloaded2.recompute_stats();

        let attached_batch = LiveUpdateBatch {
            updates: vec![
                LiveUpdate::SessionReloaded(Box::new(reloaded2.clone())),
                LiveUpdate::EventsAppended(vec![reloaded2.events.last().cloned().expect("event")]),
            ],
            cursor: Some(reloaded2.events.len() as u64),
            source_offset: Some(2),
            last_event_at: reloaded2.events.last().map(|event| event.timestamp),
            active: true,
        };
        app.apply_live_update_batch(attached_batch);
        assert_eq!(app.detail_event_index, reloaded2.events.len() - 1);
    }
}

#[derive(Debug, Clone)]
pub struct ServerInfo {
    pub url: String,
    pub status: ServerStatus,
    pub last_upload: Option<String>,
}

#[derive(Debug, Clone)]
pub enum ServerStatus {
    Online(String), // version
    Offline,
    Unknown,
}

/// Connection context — determines the badge and available features.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionContext {
    /// No server configured — local-only usage.
    Local,
    /// Connected to a local/self-hosted server.
    Server { url: String },
    /// Connected to opensession.io (or cloud), personal mode.
    CloudPersonal,
}

/// View mode selector — what set of sessions to display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewMode {
    /// Show local sessions only (file-parsed, original behaviour).
    Local,
    /// Show sessions grouped by a specific git repo name.
    Repo(String),
}

impl std::fmt::Display for ViewMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ViewMode::Local => write!(f, "Local"),
            ViewMode::Repo(r) => write!(f, "Repo: {r}"),
        }
    }
}

/// Startup status information shown in the header.
#[derive(Debug, Clone, Default)]
pub struct StartupStatus {
    pub sessions_cached: usize,
    pub repos_detected: usize,
    pub daemon_pid: Option<u32>,
    pub config_exists: bool,
}

pub struct App {
    // Sessions loaded from file parsing (original mode)
    pub sessions: Vec<Session>,
    pub filtered_sessions: Vec<usize>,
    pub view: View,

    // Session list state
    pub list_state: ListState,
    pub search_query: String,
    pub searching: bool,

    // Session detail state
    pub detail_scroll: u16,
    pub detail_event_index: usize,
    pub event_filters: HashSet<EventFilter>,
    pub expanded_diff_events: HashSet<usize>,
    pub detail_view_mode: DetailViewMode,
    pub focus_detail_view: bool,
    pub detail_h_scroll: u16,
    pub detail_viewport_height: u16,
    pub detail_selected_event_id: Option<String>,
    pub detail_source_path: Option<PathBuf>,
    pub detail_source_mtime: Option<SystemTime>,
    pub detail_hydrate_pending: bool,
    pub session_detail_issues: HashMap<String, String>,
    pub realtime_preview_enabled: bool,
    pub live_mode: bool,
    pub follow_tail_linear: FollowTailState,
    pub follow_tail_turn: FollowTailState,
    pub live_last_event_at: Option<DateTime<Utc>>,
    pub live_subscription: Option<LiveSubscription>,
    pub detail_entered_at: Instant,
    pub session_max_active_agents: HashMap<String, usize>,
    pub turn_index: usize,
    pub turn_agent_scroll: u16,
    pub turn_h_scroll: u16,
    pub turn_line_offsets: Vec<u16>,
    pub turn_raw_overrides: HashSet<usize>,
    pub turn_prompt_expanded: HashSet<usize>,

    // Server connection info
    pub server_info: Option<ServerInfo>,

    // ── Local DB + view mode ──────────────────────────────────────
    pub db: Option<Arc<LocalDb>>,
    pub view_mode: ViewMode,
    /// DB-backed session list (for repo view).
    pub db_sessions: Vec<LocalSessionRow>,
    /// Total DB-backed rows for the active filter (across all pages).
    pub db_total_sessions: usize,
    /// Available repos for Repo view cycling.
    pub repos: Vec<String>,
    /// Current repo index when cycling.
    pub repo_index: usize,
    /// Repo picker popup state (`R` in session list).
    pub repo_picker_open: bool,
    pub repo_picker_query: String,
    pub repo_picker_index: usize,

    // ── Tool filter ──────────────────────────────────────────────
    pub tool_filter: Option<String>,
    pub available_tools: Vec<String>,
    pub session_time_range: TimeRange,

    // ── Pagination ───────────────────────────────────────────────
    pub page: usize,
    pub per_page: usize,

    // ── Multi-column layout ──────────────────────────────────────
    pub list_layout: ListLayout,
    pub column_focus: usize,
    pub column_list_states: Vec<ListState>,
    pub column_users: Vec<String>,
    pub column_group_indices: Vec<Vec<usize>>,

    // ── Connection context ────────────────────────────────────────
    pub connection_ctx: ConnectionContext,

    // ── Config + Settings ─────────────────────────────────────────
    pub daemon_config: DaemonConfig,
    pub startup_status: StartupStatus,
    /// Index of selected field in settings/setup (among selectable items).
    pub settings_index: usize,
    /// Whether we're editing a text/number field inline.
    pub editing_field: bool,
    /// Buffer for inline text editing.
    pub edit_buffer: String,
    /// Whether settings have unsaved changes.
    pub config_dirty: bool,
    /// Transient message shown after save, etc.
    pub flash_message: Option<(String, FlashLevel)>,
    /// Active timeline preset slot for save/load shortcuts (1..=5).
    pub timeline_preset_slot: u8,
    /// Timeline preset slots that currently have saved values.
    pub timeline_preset_slots_filled: Vec<u8>,

    // ── Setup ────────────────────────────────────────────────────
    pub setup_step: SetupStep,
    pub setup_scenario_index: usize,
    pub setup_scenario: Option<SetupScenario>,

    // ── Upload popup / Modal ────────────────────────────────────
    pub upload_popup: Option<UploadPopup>,
    pub modal: Option<Modal>,

    // ── Tab navigation ───────────────────────────────────────────
    pub active_tab: Tab,
    pub handoff_selected_session_id: Option<String>,
    pub handoff_selected_session_ids: Vec<String>,
    pub handoff_last_artifact_id: Option<String>,
    pub pending_command: Option<AsyncCommand>,

    // ── Profile / Web Sync (Settings enhancement) ───────────────
    pub settings_section: SettingsSection,
    pub profile: Option<UserSettingsResponse>,
    pub profile_loading: bool,
    pub profile_error: Option<String>,

    // ── Deferred health check ──────────────────────────────────
    pub health_check_done: bool,

    // ── Background loading ───────────────────────────────────
    pub loading_sessions: bool,
}

/// State for the upload popup.
pub struct UploadPopup {
    pub target_name: String,
    pub status: Option<String>,
    pub phase: UploadPhase,
    pub results: Vec<(String, Result<String, String>)>,
}

pub enum UploadPhase {
    Uploading,
    Done,
}

impl App {
    pub fn is_local_mode(&self) -> bool {
        matches!(self.connection_ctx, ConnectionContext::Local)
    }

    pub(crate) fn block_text_fragments(block: &ContentBlock) -> Vec<String> {
        match block {
            ContentBlock::Text { text } => vec![text.clone()],
            ContentBlock::Code { code, .. } => vec![code.clone()],
            ContentBlock::Json { data } => vec![data.to_string()],
            ContentBlock::File { path, content } => {
                let mut out = vec![path.clone()];
                if let Some(value) = content {
                    out.push(value.clone());
                }
                out
            }
            ContentBlock::Reference { uri, media_type } => vec![uri.clone(), media_type.clone()],
            ContentBlock::Image { url, alt, mime } => {
                let mut out = vec![url.clone(), mime.clone()];
                if let Some(alt) = alt.clone() {
                    out.push(alt);
                }
                out
            }
            ContentBlock::Video { url, mime } | ContentBlock::Audio { url, mime } => {
                vec![url.clone(), mime.clone()]
            }
            _ => Vec::new(),
        }
    }

    fn can_use_collab_tabs(&self) -> bool {
        true
    }

    fn apply_session_view_mode(&mut self, mode: ViewMode) {
        self.view_mode = mode;
        self.tool_filter = None;
        self.page = 0;
        self.apply_filter();
        self.rebuild_available_tools();
        if self.list_layout == ListLayout::ByUser {
            self.rebuild_columns();
        }
    }

    pub fn new(sessions: Vec<Session>) -> Self {
        let session_max_active_agents: HashMap<String, usize> = sessions
            .iter()
            .map(|session| {
                (
                    session.session_id.clone(),
                    Self::compute_session_max_active_agents(session),
                )
            })
            .collect();
        let filtered: Vec<usize> = (0..sessions.len()).collect();
        let mut local_tools: Vec<String> = sessions
            .iter()
            .map(|s| s.agent.tool.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        local_tools.sort();
        let mut list_state = ListState::default();
        if !filtered.is_empty() {
            list_state.select(Some(0));
        }

        let mut app = Self {
            sessions,
            filtered_sessions: filtered,
            view: View::SessionList,
            list_state,
            search_query: String::new(),
            searching: false,
            detail_scroll: 0,
            detail_event_index: 0,
            event_filters: HashSet::from([EventFilter::All]),
            expanded_diff_events: HashSet::new(),
            detail_view_mode: DetailViewMode::Linear,
            focus_detail_view: false,
            detail_h_scroll: 0,
            detail_viewport_height: 24,
            detail_selected_event_id: None,
            detail_source_path: None,
            detail_source_mtime: None,
            detail_hydrate_pending: false,
            session_detail_issues: HashMap::new(),
            realtime_preview_enabled: false,
            live_mode: false,
            follow_tail_linear: FollowTailState::default(),
            follow_tail_turn: FollowTailState::default(),
            live_last_event_at: None,
            live_subscription: None,
            detail_entered_at: Instant::now(),
            session_max_active_agents,
            turn_index: 0,
            turn_agent_scroll: 0,
            turn_h_scroll: 0,
            turn_line_offsets: Vec::new(),
            turn_raw_overrides: HashSet::new(),
            turn_prompt_expanded: HashSet::new(),
            server_info: None,
            db: None,
            view_mode: ViewMode::Local,
            db_sessions: Vec::new(),
            db_total_sessions: 0,
            repos: Vec::new(),
            repo_index: 0,
            repo_picker_open: false,
            repo_picker_query: String::new(),
            repo_picker_index: 0,
            tool_filter: None,
            available_tools: local_tools,
            session_time_range: TimeRange::All,
            page: 0,
            per_page: 50,
            list_layout: ListLayout::default(),
            column_focus: 0,
            column_list_states: Vec::new(),
            column_users: Vec::new(),
            column_group_indices: Vec::new(),
            connection_ctx: ConnectionContext::Local,
            daemon_config: DaemonConfig::default(),
            startup_status: StartupStatus::default(),
            settings_index: 0,
            editing_field: false,
            edit_buffer: String::new(),
            config_dirty: false,
            flash_message: None,
            timeline_preset_slot: config::TIMELINE_PRESET_SLOT_MIN,
            timeline_preset_slots_filled: Vec::new(),
            setup_step: SetupStep::Scenario,
            setup_scenario_index: 0,
            setup_scenario: None,
            upload_popup: None,
            modal: None,
            active_tab: Tab::Sessions,
            handoff_selected_session_id: None,
            handoff_selected_session_ids: Vec::new(),
            handoff_last_artifact_id: None,
            pending_command: None,
            settings_section: SettingsSection::Workspace,
            profile: None,
            profile_loading: false,
            profile_error: None,
            health_check_done: false,
            loading_sessions: false,
        };
        app.apply_filter();
        app
    }

    /// Returns true if the app should quit.
    pub fn handle_key(&mut self, key: KeyCode) -> bool {
        // Clear flash message on any key press
        self.flash_message = None;

        // Modal intercepts all keys when active
        if self.modal.is_some() {
            return self.handle_modal_key(key);
        }

        // Upload popup intercepts keys when active
        if self.upload_popup.is_some() {
            return self.handle_upload_popup_key(key);
        }

        if self.repo_picker_open {
            return self.handle_repo_picker_key(key);
        }

        if self.searching {
            return self.handle_search_key(key);
        }

        // Help overlay — `?` from any non-editing state
        if matches!(key, KeyCode::Char('?'))
            && !self.editing_field
            && !self.searching
            && !matches!(self.view, View::Setup)
        {
            if self.view == View::Help {
                if self.focus_detail_view {
                    self.view = View::SessionDetail;
                } else {
                    self.view = View::SessionList;
                    self.active_tab = Tab::Sessions;
                }
            } else {
                self.view = View::Help;
            }
            return false;
        }

        // Global tab switching (only when not in detail/setup/editing/searching)
        if !matches!(self.view, View::SessionDetail | View::Setup | View::Help)
            && !self.editing_field
        {
            match key {
                KeyCode::Char('1') => {
                    self.switch_tab(Tab::Sessions);
                    return false;
                }
                KeyCode::Char('2') => {
                    self.switch_tab(Tab::Handoff);
                    return false;
                }
                KeyCode::Char('3') => {
                    self.switch_tab(Tab::Settings);
                    return false;
                }
                _ => {}
            }
        }

        match self.view {
            View::SessionList => self.handle_list_key(key),
            View::SessionDetail => self.handle_detail_key(key),
            View::Setup => self.handle_setup_key(key),
            View::Settings => self.handle_settings_key(key),
            View::Handoff => self.handle_handoff_key(key),
            View::Help => {
                // Any key exits help
                if self.focus_detail_view {
                    self.view = View::SessionDetail;
                } else {
                    self.view = View::SessionList;
                    self.active_tab = Tab::Sessions;
                }
                false
            }
        }
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent) -> bool {
        if self.modal.is_some() || self.upload_popup.is_some() || self.repo_picker_open {
            return false;
        }
        match mouse.kind {
            MouseEventKind::ScrollUp => match self.view {
                View::SessionList => self.list_prev(),
                View::SessionDetail => {
                    self.detail_event_index = self.detail_event_index.saturating_sub(2);
                    self.detach_live_follow_linear();
                    self.update_detail_selection_anchor();
                }
                _ => {}
            },
            MouseEventKind::ScrollDown => match self.view {
                View::SessionList => self.list_next(),
                View::SessionDetail => {
                    if let Some(session) = self.selected_session() {
                        let visible = self.visible_event_count(session);
                        if visible > 0 {
                            self.detail_event_index =
                                (self.detail_event_index + 2).min(visible - 1);
                            self.update_detail_selection_anchor();
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        }
        false
    }

    fn switch_tab(&mut self, tab: Tab) {
        if self.active_tab == tab {
            return;
        }
        self.active_tab = tab;
        match tab {
            Tab::Sessions => {
                self.view = View::SessionList;
                self.apply_session_view_mode(ViewMode::Local);
            }
            Tab::Handoff => {
                self.view = View::Handoff;
                self.handoff_selected_session_id = self.current_scope_handoff_seed_session_id();
                self.handoff_selected_session_ids.clear();
            }
            Tab::Settings => {
                self.view = View::Settings;
                self.settings_index = 0;
                self.editing_field = false;
                if !self.is_local_mode()
                    && !self.daemon_config.server.api_key.is_empty()
                    && self.profile.is_none()
                    && !self.profile_loading
                {
                    self.profile_loading = true;
                    self.pending_command = Some(AsyncCommand::FetchProfile);
                }
            }
        }
    }

    fn current_scope_handoff_seed_session_id(&self) -> Option<String> {
        if let Some(abs_index) = self.focused_column_absolute_index() {
            if self.is_db_view() {
                if let Some(row) = self.db_sessions.get(abs_index) {
                    return Some(row.id.clone());
                }
            } else if let Some(session_index) = self.filtered_sessions.get(abs_index).copied() {
                if let Some(session) = self.sessions.get(session_index) {
                    return Some(session.session_id.clone());
                }
            }
        }

        self.selected_session()
            .map(|session| session.session_id.clone())
            .or_else(|| self.selected_db_session().map(|row| row.id.clone()))
    }

    fn focused_column_absolute_index(&self) -> Option<usize> {
        if self.list_layout != ListLayout::ByUser {
            return None;
        }
        let label = self.column_users.get(self.column_focus)?;
        let indices = self.column_session_indices(label);
        let selected = self.column_list_states.get(self.column_focus)?.selected()?;
        indices.get(selected).copied()
    }

    fn handle_search_key(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Esc => {
                self.searching = false;
                self.search_query.clear();
                self.apply_filter();
            }
            KeyCode::Enter => {
                self.searching = false;
                self.apply_filter();
            }
            KeyCode::Backspace => {
                self.search_query.pop();
                self.apply_filter();
            }
            KeyCode::Char(c) => {
                self.search_query.push(c);
                self.apply_filter();
            }
            _ => {}
        }
        false
    }

    fn open_repo_picker(&mut self) {
        if self.repos.is_empty() {
            self.flash_info("No repositories available");
            return;
        }
        self.repo_picker_open = true;
        self.repo_picker_query.clear();
        self.repo_picker_index = self.repo_index.min(self.repos.len().saturating_sub(1));
    }

    fn repo_picker_filtered_indices(&self) -> Vec<usize> {
        let query = self.repo_picker_query.trim().to_ascii_lowercase();
        self.repos
            .iter()
            .enumerate()
            .filter(|(_, repo)| {
                query.is_empty() || repo.to_ascii_lowercase().contains(query.as_str())
            })
            .map(|(idx, _)| idx)
            .collect()
    }

    pub fn repo_picker_entries(&self) -> Vec<String> {
        self.repo_picker_filtered_indices()
            .into_iter()
            .filter_map(|idx| self.repos.get(idx).cloned())
            .collect()
    }

    pub fn repo_picker_selected_index(&self) -> usize {
        let len = self.repo_picker_filtered_indices().len();
        if len == 0 {
            0
        } else {
            self.repo_picker_index.min(len - 1)
        }
    }

    fn handle_repo_picker_key(&mut self, key: KeyCode) -> bool {
        let filtered = self.repo_picker_filtered_indices();
        match key {
            KeyCode::Esc => {
                self.repo_picker_open = false;
                self.repo_picker_query.clear();
                self.repo_picker_index = 0;
            }
            KeyCode::Enter => {
                if filtered.is_empty() {
                    self.flash_info("No repository matches search");
                    return false;
                }
                let selected = self.repo_picker_index.min(filtered.len() - 1);
                let repo_idx = filtered[selected];
                if let Some(repo) = self.repos.get(repo_idx).cloned() {
                    self.repo_index = repo_idx;
                    self.apply_session_view_mode(ViewMode::Repo(repo));
                }
                self.repo_picker_open = false;
                self.repo_picker_query.clear();
                self.repo_picker_index = 0;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if !filtered.is_empty() {
                    self.repo_picker_index = (self.repo_picker_index + 1).min(filtered.len() - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.repo_picker_index = self.repo_picker_index.saturating_sub(1);
            }
            KeyCode::Backspace => {
                self.repo_picker_query.pop();
                self.repo_picker_index = 0;
            }
            KeyCode::Char(c) => {
                if !c.is_control() {
                    self.repo_picker_query.push(c);
                    self.repo_picker_index = 0;
                }
            }
            _ => {}
        }
        false
    }

    fn handle_list_key(&mut self, key: KeyCode) -> bool {
        // Agent-count multi-column mode
        if self.list_layout == ListLayout::ByUser {
            return self.handle_multi_column_key(key);
        }

        match key {
            KeyCode::Char('q') => return true,
            KeyCode::Char('j') | KeyCode::Down => self.list_next(),
            KeyCode::Char('k') | KeyCode::Up => self.list_prev(),
            KeyCode::PageDown => self.list_page_down(),
            KeyCode::PageUp => self.list_page_up(),
            KeyCode::Char('G') | KeyCode::End => self.list_end(),
            KeyCode::Char('g') | KeyCode::Home => self.list_start(),
            KeyCode::Enter => self.enter_detail(),
            KeyCode::Char('/') => {
                self.searching = true;
            }
            KeyCode::Tab => {
                self.cycle_view_mode();
            }
            KeyCode::Char('m') => self.toggle_list_layout(),
            KeyCode::Char('a') => {
                self.cycle_tool_filter();
            }
            KeyCode::Char('t') => {
                self.cycle_tool_filter();
            }
            KeyCode::Char('r') => {
                self.cycle_session_time_range();
            }
            KeyCode::Char('R') => self.open_repo_picker(),
            KeyCode::Char('p') => {
                // Open upload popup — only when connected to a server
                if matches!(self.connection_ctx, ConnectionContext::Local) {
                    self.flash_info("No server configured");
                } else if self.list_state.selected().is_some() {
                    self.upload_popup = Some(UploadPopup {
                        target_name: "Personal (Public)".to_string(),
                        status: Some("Uploading...".to_string()),
                        phase: UploadPhase::Uploading,
                        results: Vec::new(),
                    });
                }
            }
            KeyCode::Char('f') => {
                if self.is_db_view() {
                    self.cycle_tool_filter();
                }
            }
            KeyCode::Char('d') => {
                if self.is_db_view() {
                    if let Some(row) = self.selected_db_session().cloned() {
                        if row.sync_status == "local_only" {
                            self.flash_info("Local-only session — delete from filesystem");
                        } else {
                            self.modal = Some(Modal::Confirm {
                                title: "Delete Session".to_string(),
                                message: format!(
                                    "Delete \"{}\"?",
                                    row.title.as_deref().unwrap_or(&row.id)
                                ),
                                action: ConfirmAction::DeleteSession {
                                    session_id: row.id.clone(),
                                },
                            });
                        }
                    }
                }
            }
            _ => {}
        }
        false
    }

    fn handle_multi_column_key(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Char('q') => return true,
            KeyCode::Char('h') | KeyCode::Left => {
                if self.column_focus > 0 {
                    self.column_focus -= 1;
                }
            }
            KeyCode::Char('l') | KeyCode::Right => {
                if self.column_focus + 1 < self.column_users.len() {
                    self.column_focus += 1;
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(label) = self.column_users.get(self.column_focus).cloned() {
                    let count = self.column_session_indices(&label).len();
                    if let Some(state) = self.column_list_states.get_mut(self.column_focus) {
                        if count > 0 {
                            let current = state.selected().unwrap_or(0);
                            state.select(Some((current + 1).min(count - 1)));
                        }
                    }
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(state) = self.column_list_states.get_mut(self.column_focus) {
                    let current = state.selected().unwrap_or(0);
                    state.select(Some(current.saturating_sub(1)));
                }
            }
            KeyCode::PageDown => {
                if self.is_db_view() {
                    self.list_page_down();
                }
            }
            KeyCode::PageUp => {
                if self.is_db_view() {
                    self.list_page_up();
                }
            }
            KeyCode::Enter => {
                // Open the selected session from the focused column
                if let Some(label) = self.column_users.get(self.column_focus).cloned() {
                    let indices = self.column_session_indices(&label);
                    if let Some(state) = self.column_list_states.get(self.column_focus) {
                        if let Some(sel) = state.selected() {
                            if let Some(&abs_idx) = indices.get(sel) {
                                // Sync main list selection from absolute index so enter_detail works
                                if self.is_db_view() {
                                    self.list_state.select(Some(abs_idx));
                                } else {
                                    let per_page = self.per_page.max(1);
                                    self.page = abs_idx / per_page;
                                    self.list_state.select(Some(abs_idx % per_page));
                                }
                                self.enter_detail();
                            }
                        }
                    }
                }
            }
            KeyCode::Char('m') => self.toggle_list_layout(),
            KeyCode::Char('a') => {
                self.cycle_tool_filter();
            }
            KeyCode::Char('t') => {
                self.cycle_tool_filter();
            }
            KeyCode::Char('r') => {
                self.cycle_session_time_range();
            }
            KeyCode::Char('R') => self.open_repo_picker(),
            KeyCode::Char('f') => {
                // Compatibility alias in DB multi-column view.
                self.cycle_tool_filter();
            }
            KeyCode::Tab => {
                self.cycle_view_mode();
            }
            _ => {}
        }
        false
    }

    fn handle_detail_key(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Char('q') | KeyCode::Esc | KeyCode::Backspace => {
                if self.focus_detail_view {
                    return true;
                }
                self.leave_detail_view();
            }
            KeyCode::Char('h') | KeyCode::Left => {
                self.detail_h_scroll = self.detail_h_scroll.saturating_sub(4);
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.detail_h_scroll = self.detail_h_scroll.saturating_add(4);
            }
            KeyCode::Char('j') | KeyCode::Down => self.detail_next_event(),
            KeyCode::Char('k') | KeyCode::Up => self.detail_prev_event(),
            KeyCode::Char('G') | KeyCode::End => self.detail_end(),
            KeyCode::Char('g') | KeyCode::Home => {
                self.detail_event_index = 0;
                self.detail_scroll = 0;
                self.detail_h_scroll = 0;
                self.detach_live_follow_linear();
            }
            KeyCode::PageDown => self.detail_page_down(),
            KeyCode::PageUp => self.detail_page_up(),
            KeyCode::Char('d') => self.toggle_diff_expanded(),
            KeyCode::Char('u') => self.jump_to_next_user_message(),
            KeyCode::Char('U') => self.jump_to_prev_user_message(),
            KeyCode::Char('n') => self.jump_to_next_same_type(),
            KeyCode::Char('N') => self.jump_to_prev_same_type(),
            KeyCode::Char('1') => self.toggle_event_filter(EventFilter::All),
            KeyCode::Char('2') => self.toggle_event_filter(EventFilter::User),
            KeyCode::Char('3') => self.toggle_event_filter(EventFilter::Agent),
            KeyCode::Char('4') => self.toggle_event_filter(EventFilter::Think),
            KeyCode::Char('5') => self.toggle_event_filter(EventFilter::Tools),
            KeyCode::Char('6') => self.toggle_event_filter(EventFilter::Files),
            KeyCode::Char('7') => self.toggle_event_filter(EventFilter::Shell),
            KeyCode::Char('8') => self.toggle_event_filter(EventFilter::Task),
            KeyCode::Char('9') => self.toggle_event_filter(EventFilter::Web),
            KeyCode::Char('0') => self.toggle_event_filter(EventFilter::Other),
            _ => {}
        }
        self.update_detail_selection_anchor();
        false
    }

    // ── Setup key handler ─────────────────────────────────────────────

    fn handle_setup_key(&mut self, key: KeyCode) -> bool {
        if self.setup_step == SetupStep::Scenario {
            return self.handle_setup_scenario_key(key);
        }
        self.handle_setup_apikey_key(key)
    }

    fn handle_setup_scenario_key(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Char('j') | KeyCode::Down => {
                if self.setup_scenario_index + 1 < SetupScenario::ALL.len() {
                    self.setup_scenario_index += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.setup_scenario_index = self.setup_scenario_index.saturating_sub(1);
            }
            KeyCode::Enter => {
                if let Some(scenario) = SetupScenario::ALL.get(self.setup_scenario_index).copied() {
                    self.apply_setup_scenario(scenario);
                    self.setup_scenario = Some(scenario);
                    match scenario {
                        SetupScenario::Local => {
                            // Persist local mode selection so startup no longer re-enters setup
                            // on every launch when no API key is configured yet.
                            self.save_config();
                            self.view = View::SessionList;
                            self.active_tab = Tab::Sessions;
                            if self.startup_status.config_exists {
                                self.flash_info(
                                    "Local mode enabled. Configure cloud sync later in Settings > Web Sync (Public)",
                                );
                            }
                        }
                        SetupScenario::Public => {
                            self.setup_step = SetupStep::Configure;
                            self.settings_index = 0;
                            self.editing_field = false;
                            self.edit_buffer.clear();
                            if scenario == SetupScenario::Public {
                                self.flash_info(
                                    "Public mode uses personal upload. Git setup is required for public uploads.",
                                );
                            }
                        }
                    }
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                self.view = View::SessionList;
                self.active_tab = Tab::Sessions;
                self.flash_info(
                    "You can configure this later in Settings > Web Sync (Public) (~/.config/opensession/opensession.toml)",
                );
            }
            _ => {}
        }
        false
    }

    fn apply_setup_scenario(&mut self, scenario: SetupScenario) {
        match scenario {
            SetupScenario::Local => {
                self.daemon_config.daemon.auto_publish = false;
                self.daemon_config.daemon.publish_on = PublishMode::Manual;
            }
            SetupScenario::Public => {
                self.daemon_config.daemon.auto_publish = true;
                self.daemon_config.daemon.publish_on = PublishMode::SessionEnd;
            }
        }
    }

    fn handle_setup_apikey_key(&mut self, key: KeyCode) -> bool {
        const SETUP_FIELDS: [SettingField; 3] = [
            SettingField::ServerUrl,
            SettingField::ApiKey,
            SettingField::Nickname,
        ];
        let setup_fields: &[SettingField] = &SETUP_FIELDS;
        let setup_field_count = setup_fields.len();

        if self.editing_field {
            match key {
                KeyCode::Esc => {
                    self.editing_field = false;
                    self.edit_buffer.clear();
                }
                KeyCode::Enter => {
                    if let Some(&field) = setup_fields.get(self.settings_index) {
                        field.set_value(&mut self.daemon_config, &self.edit_buffer);
                    }
                    self.editing_field = false;
                    self.edit_buffer.clear();
                }
                KeyCode::Backspace => {
                    self.edit_buffer.pop();
                }
                KeyCode::Char(c) => {
                    self.edit_buffer.push(c);
                }
                _ => {}
            }
            return false;
        }

        match key {
            KeyCode::Char('q') | KeyCode::Esc => {
                // Auto-save if API key has been set
                if !self.daemon_config.server.api_key.is_empty() {
                    self.save_config();
                    self.connection_ctx = Self::derive_connection_ctx(&self.daemon_config);
                }
                self.view = View::SessionList;
                self.active_tab = Tab::Sessions;
                if !self.startup_status.config_exists {
                    self.flash_info(
                        "You can configure this later in Settings > Web Sync (Public) (~/.config/opensession/opensession.toml)",
                    );
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if self.settings_index + 1 < setup_field_count {
                    self.settings_index += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.settings_index = self.settings_index.saturating_sub(1);
            }
            KeyCode::Enter => {
                if let Some(&field) = setup_fields.get(self.settings_index) {
                    self.edit_buffer = field.raw_value(&self.daemon_config);
                    self.editing_field = true;
                }
            }
            KeyCode::Char('s') => {
                self.save_config();
                self.view = View::SessionList;
                self.active_tab = Tab::Sessions;
            }
            _ => {}
        }
        false
    }

    // ── Upload popup key handler ─────────────────────────────────────

    fn handle_upload_popup_key(&mut self, key: KeyCode) -> bool {
        let popup = self.upload_popup.as_mut().unwrap();
        match &popup.phase {
            UploadPhase::Uploading => {
                // Only allow escape while loading
                if matches!(key, KeyCode::Esc) {
                    self.upload_popup = None;
                }
            }
            UploadPhase::Done => {
                // Any key dismisses
                self.upload_popup = None;
            }
        }
        false
    }

    // ── Settings key handler ──────────────────────────────────────────

    fn settings_group(&self) -> Option<config::SettingsGroup> {
        self.settings_section.group()
    }

    fn settings_field_count(&self) -> usize {
        self.settings_group()
            .map(config::selectable_field_count)
            .unwrap_or(0)
    }

    fn nth_settings_field(&self, index: usize) -> Option<SettingField> {
        self.settings_group()
            .and_then(|section| config::nth_selectable_field(section, index))
    }

    pub fn refresh_timeline_preset_slots(&mut self) {
        self.timeline_preset_slots_filled =
            config::list_timeline_preset_slots().unwrap_or_default();
        self.timeline_preset_slots_filled.sort_unstable();
        self.timeline_preset_slots_filled.dedup();
        if !(config::TIMELINE_PRESET_SLOT_MIN..=config::TIMELINE_PRESET_SLOT_MAX)
            .contains(&self.timeline_preset_slot)
        {
            self.timeline_preset_slot = config::TIMELINE_PRESET_SLOT_MIN;
        }
    }

    fn cycle_timeline_preset_slot(&mut self, forward: bool) {
        let min = config::TIMELINE_PRESET_SLOT_MIN;
        let max = config::TIMELINE_PRESET_SLOT_MAX;
        self.timeline_preset_slot = if forward {
            if self.timeline_preset_slot >= max {
                min
            } else {
                self.timeline_preset_slot + 1
            }
        } else if self.timeline_preset_slot <= min {
            max
        } else {
            self.timeline_preset_slot - 1
        };
        let slot = self.timeline_preset_slot;
        let state = if self.timeline_preset_slots_filled.contains(&slot) {
            "saved"
        } else {
            "empty"
        };
        self.flash_info(format!("Timeline preset slot #{} ({})", slot, state));
    }

    fn save_timeline_preset_slot(&mut self) {
        let slot = self.timeline_preset_slot;
        match config::save_timeline_preset(slot, &self.daemon_config) {
            Ok(()) => {
                if !self.timeline_preset_slots_filled.contains(&slot) {
                    self.timeline_preset_slots_filled.push(slot);
                    self.timeline_preset_slots_filled.sort_unstable();
                }
                self.flash_success(format!("Timeline preset slot #{} saved", slot));
            }
            Err(err) => {
                self.flash_error(format!("Timeline preset save failed: {}", err));
            }
        }
    }

    fn load_timeline_preset_slot(&mut self) {
        let slot = self.timeline_preset_slot;
        match config::load_timeline_preset(slot) {
            Ok(Some(preset)) => {
                preset.apply_to_config(&mut self.daemon_config);
                self.config_dirty = true;
                self.flash_success(format!("Timeline preset slot #{} loaded", slot));
            }
            Ok(None) => {
                self.flash_info(format!(
                    "Timeline preset slot #{} is empty (save with Shift+S)",
                    slot
                ));
            }
            Err(err) => {
                self.flash_error(format!("Timeline preset load failed: {}", err));
            }
        }
    }

    fn handle_settings_key(&mut self, key: KeyCode) -> bool {
        // DaemonConfig field editing
        if self.editing_field {
            match key {
                KeyCode::Esc => {
                    self.editing_field = false;
                    self.edit_buffer.clear();
                }
                KeyCode::Enter => {
                    if let Some(field) = self.nth_settings_field(self.settings_index) {
                        field.set_value(&mut self.daemon_config, &self.edit_buffer);
                        self.config_dirty = true;
                    }
                    self.editing_field = false;
                    self.edit_buffer.clear();
                }
                KeyCode::Backspace => {
                    self.edit_buffer.pop();
                }
                KeyCode::Char(c) => {
                    self.edit_buffer.push(c);
                }
                _ => {}
            }
            return false;
        }

        match key {
            KeyCode::Char('q') | KeyCode::Esc => {
                if self.config_dirty {
                    self.modal = Some(Modal::Confirm {
                        title: "Unsaved Changes".to_string(),
                        message: "You have unsaved changes. Save before leaving?".to_string(),
                        action: ConfirmAction::SaveChanges,
                    });
                } else {
                    self.view = View::SessionList;
                    self.active_tab = Tab::Sessions;
                }
            }
            KeyCode::Char(']') => {
                // Next settings section
                self.settings_section = self.settings_section.next();
                self.settings_index = 0;
            }
            KeyCode::Char('[') => {
                // Previous settings section
                self.settings_section = self.settings_section.prev();
                self.settings_index = 0;
            }
            _ => {
                // Delegate to section-specific handling
                match self.settings_section {
                    SettingsSection::Workspace
                    | SettingsSection::CaptureSync
                    | SettingsSection::StoragePrivacy
                    | SettingsSection::Git => {
                        self.handle_daemon_config_key(key);
                    }
                }
            }
        }
        false
    }

    fn handle_handoff_key(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.active_tab = Tab::Sessions;
                self.view = View::SessionList;
            }
            KeyCode::Char('j') | KeyCode::Down => self.move_handoff_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_handoff_selection(-1),
            KeyCode::Char(' ') => self.toggle_handoff_selection(),
            KeyCode::Enter | KeyCode::Char('l') => {
                if self.handoff_effective_candidates().is_empty() {
                    self.flash_info("No handoff candidate in current scope");
                } else {
                    self.flash_success("Handoff preview refreshed");
                }
            }
            KeyCode::Char('g') => self.generate_handoff_via_cli(),
            KeyCode::Char('s') => self.save_handoff_artifact_via_cli(),
            KeyCode::Char('r') => self.refresh_handoff_artifact_via_cli(),
            _ => {}
        }
        false
    }

    fn handle_daemon_config_key(&mut self, key: KeyCode) {
        let field_count = self.settings_field_count();

        match key {
            KeyCode::Char('d') if self.settings_section == SettingsSection::CaptureSync => {
                self.toggle_daemon();
            }
            KeyCode::Char('r') if self.settings_section == SettingsSection::Workspace => {
                if self.daemon_config.server.api_key.is_empty() {
                    self.flash_info("Set API key in Web Sync (Public) first");
                } else {
                    self.profile_loading = true;
                    self.pending_command = Some(AsyncCommand::FetchProfile);
                }
            }
            KeyCode::Char('g') if self.settings_section == SettingsSection::Workspace => {
                self.modal = Some(Modal::Confirm {
                    title: "Regenerate API Key".to_string(),
                    message: "This will invalidate your current API key.".to_string(),
                    action: ConfirmAction::RegenerateApiKey,
                });
            }
            KeyCode::Char('r') if self.settings_section == SettingsSection::CaptureSync => {
                self.startup_status.daemon_pid = config::daemon_pid();
                self.sync_daemon_publish_policy_from_runtime();
                self.flash_info("Capture Flow status refreshed");
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if self.settings_index + 1 < field_count {
                    self.settings_index += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.settings_index = self.settings_index.saturating_sub(1);
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(field) = self.nth_settings_field(self.settings_index) {
                    if field == SettingField::AutoPublish {
                        self.toggle_daemon();
                        return;
                    }
                    if let Some(reason) = self.daemon_config_field_block_reason(field) {
                        self.flash_info(reason);
                        return;
                    }
                    if field.is_toggle() {
                        field.toggle(&mut self.daemon_config);
                        self.config_dirty = true;
                    } else if field.is_enum() {
                        field.cycle_enum(&mut self.daemon_config);
                        self.config_dirty = true;
                    } else {
                        // Text or number — enter edit mode
                        self.edit_buffer = field.raw_value(&self.daemon_config);
                        self.editing_field = true;
                    }
                }
            }
            KeyCode::Char('s') => {
                self.save_config();
            }
            _ => {}
        }
    }

    pub fn daemon_config_field_block_reason(&self, _field: SettingField) -> Option<&'static str> {
        None
    }

    fn toggle_daemon(&mut self) {
        if self.startup_status.daemon_pid.is_some() {
            self.stop_daemon();
        } else {
            self.start_daemon();
        }
    }

    fn apply_daemon_publish_policy(config: &mut DaemonConfig, daemon_running: bool) {
        if daemon_running {
            config.daemon.auto_publish = true;
            config.daemon.publish_on = PublishMode::SessionEnd;
        } else {
            config.daemon.auto_publish = false;
            config.daemon.publish_on = PublishMode::Manual;
        }
    }

    pub(crate) fn sync_daemon_publish_policy_from_runtime(&mut self) {
        let daemon_running = self.startup_status.daemon_pid.is_some();
        Self::apply_daemon_publish_policy(&mut self.daemon_config, daemon_running);
    }

    fn persist_daemon_publish_policy(&mut self, daemon_running: bool) -> Result<(), String> {
        let was_dirty = self.config_dirty;
        Self::apply_daemon_publish_policy(&mut self.daemon_config, daemon_running);
        let mut persisted = config::load_daemon_config();
        Self::apply_daemon_publish_policy(&mut persisted, daemon_running);
        config::save_daemon_config(&persisted)
            .map_err(|err| format!("Failed to save daemon publish policy: {err}"))?;
        self.config_dirty = was_dirty;
        Ok(())
    }

    fn find_daemon_binary() -> Option<std::path::PathBuf> {
        // Look next to our own binary first
        if let Ok(exe) = std::env::current_exe() {
            let dir = exe.parent().unwrap_or(std::path::Path::new("."));
            let candidate = dir.join("opensession-daemon");
            if candidate.is_file() {
                return Some(candidate);
            }
            // Try with .exe on Windows
            let candidate = dir.join("opensession-daemon.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        // Try PATH without spawning an external command to avoid UI stalls.
        if let Some(path) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&path) {
                let candidate = dir.join("opensession-daemon");
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
        None
    }

    fn start_daemon(&mut self) {
        if let Err(err) = self.persist_daemon_publish_policy(true) {
            self.flash_error(err);
            return;
        }
        let bin = match Self::find_daemon_binary() {
            Some(b) => b,
            None => {
                self.flash_error(
                    "opensession-daemon not found. Install: cargo install opensession-daemon",
                );
                return;
            }
        };
        match std::process::Command::new(bin)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(_) => {
                std::thread::sleep(std::time::Duration::from_millis(500));
                self.startup_status.daemon_pid = config::daemon_pid();
                if self.startup_status.daemon_pid.is_some() {
                    self.flash_success("Daemon started");
                } else {
                    self.flash_error("Daemon started but PID not found");
                }
            }
            Err(e) => {
                let _ = self.persist_daemon_publish_policy(false);
                self.flash_error(format!("Failed to start daemon: {e}"));
            }
        }
    }

    fn stop_daemon(&mut self) {
        if let Some(pid) = self.startup_status.daemon_pid {
            let _ = Self::send_signal(pid, "TERM");
            for _ in 0..6 {
                std::thread::sleep(std::time::Duration::from_millis(120));
                self.startup_status.daemon_pid = config::daemon_pid();
                if self.startup_status.daemon_pid.is_none() {
                    break;
                }
            }
            if self.startup_status.daemon_pid.is_none() {
                if let Err(err) = self.persist_daemon_publish_policy(false) {
                    self.flash_error(err);
                } else {
                    self.flash_success("Daemon stopped");
                }
            } else {
                self.flash_error("Daemon may still be running");
            }
        }
    }

    fn send_signal(pid: u32, signal: &str) -> bool {
        std::process::Command::new("kill")
            .arg(format!("-{signal}"))
            .arg(pid.to_string())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    // ── Modal key handler ────────────────────────────────────────────

    fn handle_modal_key(&mut self, key: KeyCode) -> bool {
        let modal = match self.modal.take() {
            Some(m) => m,
            None => return false,
        };

        match modal {
            Modal::Confirm {
                title,
                message,
                action,
            } => match key {
                KeyCode::Char('y') | KeyCode::Enter => {
                    // Execute the confirmed action
                    match action {
                        ConfirmAction::RegenerateApiKey => {
                            self.pending_command = Some(AsyncCommand::RegenerateApiKey);
                        }
                        ConfirmAction::DeleteSession { session_id } => {
                            self.pending_command = Some(AsyncCommand::DeleteSession { session_id });
                        }
                        ConfirmAction::SaveChanges => {
                            self.save_config();
                            self.view = View::SessionList;
                            self.active_tab = Tab::Sessions;
                        }
                    }
                    // modal already taken
                }
                KeyCode::Char('n') => {
                    // For SaveChanges: discard changes and exit
                    if matches!(action, ConfirmAction::SaveChanges) {
                        self.daemon_config = config::load_daemon_config();
                        self.config_dirty = false;
                        self.view = View::SessionList;
                        self.active_tab = Tab::Sessions;
                    }
                    // For other modals: same as cancel
                }
                KeyCode::Esc => {
                    // Cancel — modal already removed, stay in current view
                }
                _ => {
                    // Put modal back
                    self.modal = Some(Modal::Confirm {
                        title,
                        message,
                        action,
                    });
                }
            },
        }
        false
    }

    // ── Apply async command result ────────────────────────────────────

    pub fn apply_command_result(&mut self, result: CommandResult) {
        match result {
            CommandResult::UploadDone(result) => {
                if result.is_ok() {
                    // Mark synced on any successful upload
                    if let Some(session) = self.selected_session() {
                        let sid = session.session_id.clone();
                        if let Some(ref db) = self.db {
                            let _ = db.mark_synced(&sid);
                        }
                    }
                }

                if let Some(ref mut popup) = self.upload_popup {
                    match result {
                        Ok((target_name, url)) => {
                            popup.results.push((target_name, Ok(url)));
                        }
                        Err((target_name, e)) => {
                            popup.results.push((target_name, Err(e)));
                        }
                    }
                    popup.phase = UploadPhase::Done;
                    popup.status = None;
                }
            }

            CommandResult::Profile(Ok(profile)) => {
                self.profile_loading = false;
                self.profile = Some(profile);
                self.profile_error = None;
            }
            CommandResult::Profile(Err(e)) => {
                self.profile_loading = false;
                self.profile_error = Some(e.clone());
                self.flash_error(format!("Error: {e}"));
            }

            CommandResult::ApiKey(Ok(resp)) => {
                self.daemon_config.server.api_key = resp.api_key;
                self.config_dirty = true;
                self.save_config();
                self.flash_success("API key regenerated and saved");
            }
            CommandResult::ApiKey(Err(e)) => {
                self.flash_error(format!("Error: {e}"));
            }

            CommandResult::ServerSessions(Ok(resp)) => {
                self.flash_success(format!(
                    "Loaded {} sessions from server",
                    resp.sessions.len()
                ));
            }
            CommandResult::ServerSessions(Err(e)) => {
                self.flash_error(format!("Error: {e}"));
            }

            CommandResult::DeleteSession(Ok(session_id)) => {
                if let Some(ref db) = self.db {
                    let _ = db.delete_session(&session_id);
                }
                self.sessions.retain(|s| s.session_id != session_id);
                if self.is_db_view() {
                    let selected = self.list_state.selected().unwrap_or(0);
                    self.reload_db_sessions();
                    if self.list_layout == ListLayout::ByUser {
                        self.rebuild_columns();
                    }
                    let count = self.page_count();
                    if count == 0 {
                        self.list_state.select(None);
                    } else {
                        self.list_state.select(Some(selected.min(count - 1)));
                    }
                } else {
                    self.db_sessions.retain(|r| r.id != session_id);
                    // Fix selection
                    let count = self.page_count();
                    if count == 0 {
                        self.list_state.select(None);
                    } else if let Some(sel) = self.list_state.selected() {
                        if sel >= count {
                            self.list_state.select(Some(count - 1));
                        }
                    }
                }
                self.flash_success("Session deleted");
            }
            CommandResult::DeleteSession(Err(e)) => {
                self.flash_error(format!("Delete failed: {e}"));
            }
        }
    }

    pub fn flash_success(&mut self, msg: impl Into<String>) {
        self.flash_message = Some((msg.into(), FlashLevel::Success));
    }

    pub fn flash_error(&mut self, msg: impl Into<String>) {
        self.flash_message = Some((msg.into(), FlashLevel::Error));
    }

    pub fn flash_info(&mut self, msg: impl Into<String>) {
        self.flash_message = Some((msg.into(), FlashLevel::Info));
    }

    pub fn save_config(&mut self) {
        match config::save_daemon_config(&self.daemon_config) {
            Ok(()) => {
                self.config_dirty = false;
                self.startup_status.config_exists = true;
                self.flash_success("Config saved to opensession.toml");
                // Re-derive connection context
                self.connection_ctx = Self::derive_connection_ctx(&self.daemon_config);
            }
            Err(e) => {
                self.flash_error(format!("Save failed: {e}"));
            }
        }
    }

    /// Derive the connection context from the current daemon config.
    pub fn derive_connection_ctx(config: &DaemonConfig) -> ConnectionContext {
        if config.server.api_key.is_empty() {
            return ConnectionContext::Local;
        }
        let url = config.server.url.to_lowercase();
        let is_local = url.contains("localhost")
            || url.contains("127.0.0.1")
            || url.contains("192.168.")
            || url.contains("10.")
            || url.contains("172.16.");
        if is_local {
            return ConnectionContext::Server {
                url: config.server.url.clone(),
            };
        }
        ConnectionContext::CloudPersonal
    }

    fn toggle_event_filter(&mut self, filter: EventFilter) {
        if filter == EventFilter::All {
            // "All" resets to show everything
            self.event_filters.clear();
            self.event_filters.insert(EventFilter::All);
        } else {
            // Remove "All" when toggling a specific filter
            self.event_filters.remove(&EventFilter::All);
            if self.event_filters.contains(&filter) {
                self.event_filters.remove(&filter);
            } else {
                self.event_filters.insert(filter);
            }
            // If nothing selected, fall back to All
            if self.event_filters.is_empty() {
                self.event_filters.insert(EventFilter::All);
            }
        }
        self.detail_event_index = 0;
    }

    // ── View mode cycling ──────────────────────────────────────────

    fn cycle_view_mode(&mut self) {
        let next = match &self.view_mode {
            ViewMode::Local => {
                if !self.repos.is_empty() {
                    self.repo_index = 0;
                    ViewMode::Repo(self.repos[0].clone())
                } else {
                    return; // nothing to cycle to
                }
            }
            ViewMode::Repo(_) => {
                // Cycle through repos, then back to Local
                if self.repo_index + 1 < self.repos.len() {
                    self.repo_index += 1;
                    ViewMode::Repo(self.repos[self.repo_index].clone())
                } else {
                    ViewMode::Local
                }
            }
        };
        self.view_mode = next;
        self.tool_filter = None;
        self.page = 0;
        self.apply_filter();
        self.rebuild_available_tools();
        if self.list_layout == ListLayout::ByUser {
            self.rebuild_columns();
        }
    }

    /// Toggle between Single and agent-count multi-column list layout.
    fn toggle_list_layout(&mut self) {
        match self.list_layout {
            ListLayout::Single => {
                self.list_layout = ListLayout::ByUser;
                self.rebuild_columns();
            }
            ListLayout::ByUser => {
                self.list_layout = ListLayout::Single;
            }
        }
    }

    /// Group current list by max active agent count for multi-column view.
    fn rebuild_columns(&mut self) {
        use std::collections::BTreeMap;
        let mut by_agents: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        match self.view_mode {
            ViewMode::Local => {
                for (abs_idx, &session_idx) in self.filtered_sessions.iter().enumerate() {
                    let session = &self.sessions[session_idx];
                    let fallback = if session.events.is_empty() { 0 } else { 1 };
                    let agent_count = self
                        .session_max_active_agents
                        .get(&session.session_id)
                        .copied()
                        .unwrap_or(fallback)
                        .max(1);
                    by_agents.entry(agent_count).or_default().push(abs_idx);
                }
            }
            ViewMode::Repo(_) => {
                for (abs_idx, row) in self.db_sessions.iter().enumerate() {
                    let agent_count = self
                        .session_max_active_agents
                        .get(&row.id)
                        .copied()
                        .unwrap_or_else(|| row.max_active_agents.max(1) as usize)
                        .max(1);
                    by_agents.entry(agent_count).or_default().push(abs_idx);
                }
            }
        }

        let mut grouped: Vec<(usize, Vec<usize>)> = by_agents.into_iter().collect();
        grouped.sort_by(|(a, _), (b, _)| b.cmp(a));
        self.column_users = grouped
            .iter()
            .map(|(count, _)| {
                if *count == 1 {
                    "1 agent".to_string()
                } else {
                    format!("{count} agents")
                }
            })
            .collect();
        self.column_group_indices = grouped.into_iter().map(|(_, indices)| indices).collect();

        self.column_list_states = vec![ListState::default(); self.column_users.len()];
        for (state, indices) in self
            .column_list_states
            .iter_mut()
            .zip(self.column_group_indices.iter())
        {
            state.select(if indices.is_empty() { None } else { Some(0) });
        }
        self.column_focus = 0;
    }

    /// Get the indices of db_sessions for a given column user.
    pub fn column_session_indices(&self, user: &str) -> Vec<usize> {
        self.column_users
            .iter()
            .position(|column| column == user)
            .and_then(|idx| self.column_group_indices.get(idx).cloned())
            .unwrap_or_default()
    }

    /// Reload db_sessions for the current view_mode.
    pub fn reload_db_sessions(&mut self) {
        let Some(db) = self.db.clone() else { return };
        let search = self.normalized_search_query();
        let base_filter = match &self.view_mode {
            ViewMode::Local => return, // Local mode uses self.sessions
            ViewMode::Repo(repo) => LocalSessionFilter {
                git_repo_name: Some(repo.clone()),
                tool: self.tool_filter.clone(),
                search,
                sort: LocalSortOrder::Recent,
                time_range: self.local_session_time_range(),
                ..Default::default()
            },
        };

        self.db_total_sessions = match db.count_sessions_filtered(&base_filter) {
            Ok(count) => count.max(0) as usize,
            Err(e) => {
                eprintln!("DB count error: {e}");
                self.db_sessions.clear();
                self.column_users.clear();
                self.column_group_indices.clear();
                0
            }
        };

        let per_page = self.per_page.max(1);
        let total_pages = if self.db_total_sessions == 0 {
            1
        } else {
            self.db_total_sessions.div_ceil(per_page)
        };
        if self.page >= total_pages {
            self.page = total_pages.saturating_sub(1);
        }

        let mut page_filter = base_filter.clone();
        page_filter.limit = Some(per_page.min(u32::MAX as usize) as u32);
        page_filter.offset = Some(self.page.saturating_mul(per_page).min(u32::MAX as usize) as u32);

        match db.list_sessions(&page_filter) {
            Ok(rows) => {
                self.db_sessions = rows;
                self.rebuild_available_tools();
            }
            Err(e) => {
                eprintln!("DB error: {e}");
                self.db_sessions.clear();
                self.db_total_sessions = 0;
                self.column_users.clear();
                self.column_group_indices.clear();
            }
        }
    }

    /// Total visible session count for current view mode.
    pub fn session_count(&self) -> usize {
        match &self.view_mode {
            ViewMode::Local => self.filtered_sessions.len(),
            _ => self.db_total_sessions,
        }
    }

    /// Returns true if the detail view should use DB data (no parsed Session available).
    pub fn is_db_view(&self) -> bool {
        !matches!(self.view_mode, ViewMode::Local)
    }

    // ── Pagination ──────────────────────────────────────────────────────

    /// Total pages for current session count.
    pub fn total_pages(&self) -> usize {
        let total = self.session_count();
        if total == 0 {
            1
        } else {
            total.div_ceil(self.per_page)
        }
    }

    /// Index range for the current page.
    pub fn page_range(&self) -> std::ops::Range<usize> {
        if self.is_db_view() {
            return 0..self.db_sessions.len();
        }
        let total = self.session_count();
        let start = self.page * self.per_page;
        let end = (start + self.per_page).min(total);
        start..end
    }

    /// Number of items on the current page.
    pub fn page_count(&self) -> usize {
        self.page_range().len()
    }

    fn next_page(&mut self) {
        if self.page + 1 < self.total_pages() {
            self.page += 1;
            if self.is_db_view() {
                self.reload_db_sessions();
                if self.list_layout == ListLayout::ByUser {
                    self.rebuild_columns();
                }
            }
            self.list_state.select(Some(0));
        }
    }

    fn prev_page(&mut self) {
        if self.page > 0 {
            self.page -= 1;
            if self.is_db_view() {
                self.reload_db_sessions();
                if self.list_layout == ListLayout::ByUser {
                    self.rebuild_columns();
                }
            }
            self.list_state.select(Some(0));
        }
    }

    // ── Tool filter ─────────────────────────────────────────────────────

    /// Rebuild the list of available tools from the current db_sessions.
    /// Only updates when no tool filter is active (to keep the list stable while cycling).
    pub fn rebuild_available_tools(&mut self) {
        if self.tool_filter.is_some() {
            return; // Keep existing list while filtering
        }
        let mut tools: Vec<String> = match self.view_mode.clone() {
            ViewMode::Local => self
                .sessions
                .iter()
                .map(|s| s.agent.tool.clone())
                .collect::<HashSet<_>>()
                .into_iter()
                .collect(),
            ViewMode::Repo(repo) => {
                let search = self.normalized_search_query();
                let filter = LocalSessionFilter {
                    git_repo_name: Some(repo),
                    tool: None,
                    search,
                    sort: LocalSortOrder::Recent,
                    time_range: self.local_session_time_range(),
                    ..Default::default()
                };
                if let Some(db) = self.db.as_ref() {
                    db.list_session_tools(&filter).unwrap_or_default()
                } else {
                    Vec::new()
                }
            }
        };
        tools.sort();
        tools.dedup();
        self.available_tools = tools;
    }

    fn cycle_tool_filter(&mut self) {
        if self.available_tools.is_empty() {
            self.tool_filter = None;
            return;
        }
        self.tool_filter = match &self.tool_filter {
            None => Some(self.available_tools[0].clone()),
            Some(current) => {
                if let Some(pos) = self.available_tools.iter().position(|t| t == current) {
                    if pos + 1 < self.available_tools.len() {
                        Some(self.available_tools[pos + 1].clone())
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        };
        self.page = 0;
        self.apply_filter();
    }

    pub fn session_time_range_label(&self) -> &'static str {
        match self.session_time_range {
            TimeRange::All => "all",
            TimeRange::Hours24 => "24h",
            TimeRange::Days7 => "7d",
            TimeRange::Days30 => "30d",
        }
    }

    pub fn is_default_time_range(&self) -> bool {
        matches!(self.session_time_range, TimeRange::All)
    }

    pub fn active_tool_filter(&self) -> Option<&str> {
        self.tool_filter
            .as_deref()
            .filter(|tool| !tool.trim().is_empty())
    }

    pub fn active_agent_filter(&self) -> Option<&str> {
        self.active_tool_filter()
    }

    pub fn has_active_session_filters(&self) -> bool {
        self.active_agent_filter().is_some()
            || !self.search_query.trim().is_empty()
            || !self.is_default_time_range()
    }

    fn normalized_search_query(&self) -> Option<String> {
        let query = self.search_query.trim();
        if query.is_empty() {
            None
        } else {
            Some(query.to_string())
        }
    }

    fn local_time_cutoff(&self) -> Option<DateTime<Utc>> {
        match self.session_time_range {
            TimeRange::All => None,
            TimeRange::Hours24 => Some(Utc::now() - ChronoDuration::hours(24)),
            TimeRange::Days7 => Some(Utc::now() - ChronoDuration::days(7)),
            TimeRange::Days30 => Some(Utc::now() - ChronoDuration::days(30)),
        }
    }

    fn local_session_time_range(&self) -> LocalTimeRange {
        match self.session_time_range {
            TimeRange::All => LocalTimeRange::All,
            TimeRange::Hours24 => LocalTimeRange::Hours24,
            TimeRange::Days7 => LocalTimeRange::Days7,
            TimeRange::Days30 => LocalTimeRange::Days30,
        }
    }

    fn local_session_matches_search(session: &Session, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }

        let title = session
            .context
            .title
            .as_deref()
            .unwrap_or("")
            .to_ascii_lowercase();
        let tool = session.agent.tool.to_ascii_lowercase();
        let model = session.agent.model.to_ascii_lowercase();
        let sid = session.session_id.to_ascii_lowercase();
        let tags = session.context.tags.join(" ").to_ascii_lowercase();

        title.contains(query)
            || tool.contains(query)
            || model.contains(query)
            || sid.contains(query)
            || tags.contains(query)
    }

    fn local_session_matches_filters(
        &self,
        session: &Session,
        search_query: &str,
        cutoff: Option<&DateTime<Utc>>,
        required_tool: Option<&str>,
    ) -> bool {
        if let Some(cutoff) = cutoff {
            if session.context.created_at < *cutoff {
                return false;
            }
        }

        if let Some(required_tool) = required_tool {
            if !session.agent.tool.eq_ignore_ascii_case(required_tool) {
                return false;
            }
        }

        Self::local_session_matches_search(session, search_query)
    }

    fn compare_local_sessions_for_sort(
        sort_order: &SortOrder,
        lhs: &Session,
        rhs: &Session,
    ) -> Ordering {
        match sort_order {
            SortOrder::Recent => rhs
                .context
                .created_at
                .cmp(&lhs.context.created_at)
                .then_with(|| rhs.session_id.cmp(&lhs.session_id)),
            SortOrder::Popular => rhs
                .stats
                .message_count
                .cmp(&lhs.stats.message_count)
                .then_with(|| rhs.context.created_at.cmp(&lhs.context.created_at))
                .then_with(|| rhs.session_id.cmp(&lhs.session_id)),
            SortOrder::Longest => rhs
                .stats
                .duration_seconds
                .cmp(&lhs.stats.duration_seconds)
                .then_with(|| rhs.context.created_at.cmp(&lhs.context.created_at))
                .then_with(|| rhs.session_id.cmp(&lhs.session_id)),
        }
    }

    fn cycle_session_time_range(&mut self) {
        self.session_time_range = match self.session_time_range {
            TimeRange::All => TimeRange::Hours24,
            TimeRange::Hours24 => TimeRange::Days7,
            TimeRange::Days7 => TimeRange::Days30,
            TimeRange::Days30 => TimeRange::All,
        };
        self.page = 0;
        self.apply_filter();
    }

    // ── List navigation ─────────────────────────────────────────────────

    fn list_next(&mut self) {
        let count = self.page_count();
        if count == 0 {
            return;
        }
        let selected = self.list_state.selected().unwrap_or(0);
        if selected + 1 < count {
            self.list_state.select(Some(selected + 1));
            return;
        }
        if self.page + 1 < self.total_pages() {
            self.next_page();
        }
    }

    fn list_prev(&mut self) {
        let count = self.page_count();
        if count == 0 {
            return;
        }
        let selected = self.list_state.selected().unwrap_or(0);
        if selected > 0 {
            self.list_state.select(Some(selected - 1));
            return;
        }
        if self.page > 0 {
            self.prev_page();
            self.list_end();
        }
    }

    fn list_page_down(&mut self) {
        if self.page + 1 < self.total_pages() {
            self.next_page();
        } else {
            self.list_end();
        }
    }

    fn list_page_up(&mut self) {
        if self.page > 0 {
            self.prev_page();
        } else {
            self.list_start();
        }
    }

    fn list_end(&mut self) {
        let count = self.page_count();
        if count > 0 {
            self.list_state.select(Some(count - 1));
        }
    }

    fn list_start(&mut self) {
        if self.page_count() > 0 {
            self.list_state.select(Some(0));
        }
    }

    fn enter_detail(&mut self) {
        if let Some(selected) = self.list_state.selected() {
            if selected < self.page_count() {
                // For DB views, only enter if we have a matching parsed session
                if self.is_db_view() && self.selected_session().is_none() {
                    let url = &self.daemon_config.server.url;
                    if let Some(row) = self.db_sessions.get(selected) {
                        self.flash_info(format!(
                            "Remote-only session — view at {}/sessions/{}",
                            url, row.id
                        ));
                    } else {
                        self.flash_info("Remote-only session — not available locally");
                    }
                    return;
                }
                self.view = View::SessionDetail;
                self.detail_scroll = 0;
                self.detail_event_index = 0;
                self.detail_h_scroll = 0;
                self.event_filters = HashSet::from([EventFilter::All]);
                self.expanded_diff_events.clear();
                self.turn_raw_overrides.clear();
                self.turn_prompt_expanded.clear();
                self.detail_view_mode = DetailViewMode::Linear;
                self.detail_selected_event_id = None;
                self.turn_index = 0;
                self.turn_agent_scroll = 0;
                self.turn_h_scroll = 0;
                self.live_mode = false;
                self.live_last_event_at = None;
                self.live_subscription = None;
                self.follow_tail_linear.reset();
                self.follow_tail_turn.reset();
                self.detail_source_path = self.resolve_selected_source_path();
                self.detail_source_mtime = self
                    .detail_source_path
                    .as_ref()
                    .and_then(|p| std::fs::metadata(p).ok())
                    .and_then(|m| m.modified().ok());
                self.detail_hydrate_pending = self
                    .selected_session()
                    .map(|session| session.events.is_empty() && session.stats.event_count > 0)
                    .unwrap_or(false);
                self.detail_entered_at = Instant::now();
                self.realtime_preview_enabled = false;
                if let Some(session) = self.selected_session().cloned() {
                    if session.events.is_empty() && session.stats.event_count == 0 {
                        self.set_session_detail_issue(
                            session.session_id.clone(),
                            self.build_zero_event_detail_issue(),
                        );
                    } else {
                        self.clear_session_detail_issue(&session.session_id);
                    }
                    self.live_last_event_at = session.events.last().map(|event| event.timestamp);
                }
                self.refresh_live_subscription();
                self.refresh_live_mode();
                if self.live_mode {
                    self.jump_to_latest_linear();
                }
                self.update_detail_selection_anchor();
            }
        }
    }

    pub(crate) fn enter_detail_for_startup(&mut self) {
        self.enter_detail();
    }

    // ── Detail navigation ───────────────────────────────────────────────

    fn detail_next_event(&mut self) {
        if let Some(session) = self.selected_session() {
            let visible = self.visible_event_count(session);
            if visible > 0 && self.detail_event_index < visible - 1 {
                self.detail_event_index += 1;
            }
        }
        self.update_detail_selection_anchor();
    }

    fn detail_prev_event(&mut self) {
        self.detail_event_index = self.detail_event_index.saturating_sub(1);
        self.detach_live_follow_linear();
        self.update_detail_selection_anchor();
    }

    fn detail_end(&mut self) {
        if let Some(session) = self.selected_session() {
            let visible = self.visible_event_count(session);
            if visible > 0 {
                self.detail_event_index = visible - 1;
            }
        }
        self.reattach_live_follow_linear();
        self.update_detail_selection_anchor();
    }

    fn detail_page_down(&mut self) {
        if let Some(session) = self.selected_session() {
            let visible = self.visible_event_count(session);
            if visible > 0 {
                self.detail_event_index = (self.detail_event_index + 10).min(visible - 1);
            }
        }
        self.update_detail_selection_anchor();
    }

    fn detail_page_up(&mut self) {
        self.detail_event_index = self.detail_event_index.saturating_sub(10);
        self.detach_live_follow_linear();
        self.update_detail_selection_anchor();
    }

    fn toggle_diff_expanded(&mut self) {
        let idx = self.detail_event_index;
        if self.expanded_diff_events.contains(&idx) {
            self.expanded_diff_events.remove(&idx);
        } else {
            self.expanded_diff_events.insert(idx);
        }
    }

    fn handle_turn_key(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Char('q') => {
                if self.focus_detail_view {
                    return true;
                }
                self.leave_detail_view();
            }
            KeyCode::Esc => {
                if self.focus_detail_view {
                    return true;
                }
                self.leave_detail_view();
            }
            KeyCode::Char('v') => {
                self.detail_view_mode = DetailViewMode::Linear;
                self.sync_turn_to_linear();
                self.update_detail_selection_anchor();
            }
            KeyCode::Char('h') | KeyCode::Left => {
                self.turn_h_scroll = self.turn_h_scroll.saturating_sub(4);
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.turn_h_scroll = self.turn_h_scroll.saturating_add(4);
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.turn_agent_scroll = self.turn_agent_scroll.saturating_add(1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.turn_agent_scroll = self.turn_agent_scroll.saturating_sub(1);
                self.detach_live_follow_turn();
            }
            KeyCode::Char('J') | KeyCode::Char('n') => self.turn_next(),
            KeyCode::Char('K') | KeyCode::Char('N') => self.turn_prev(),
            KeyCode::PageUp => {
                self.turn_agent_scroll = self.turn_agent_scroll.saturating_sub(10);
                self.detach_live_follow_turn();
            }
            KeyCode::PageDown => {
                self.turn_agent_scroll = self.turn_agent_scroll.saturating_add(10);
            }
            KeyCode::Char('g') | KeyCode::Home => {
                self.turn_index = 0;
                self.turn_agent_scroll = 0;
                self.turn_h_scroll = 0;
                self.detach_live_follow_turn();
            }
            KeyCode::Char('G') | KeyCode::End => {
                self.jump_to_latest_turn();
            }
            KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Char('a') => {
                self.toggle_turn_raw_override();
            }
            KeyCode::Char('p') => self.toggle_turn_prompt_expanded(),
            KeyCode::Char('1') => self.toggle_event_filter(EventFilter::All),
            KeyCode::Char('2') => self.toggle_event_filter(EventFilter::User),
            KeyCode::Char('3') => self.toggle_event_filter(EventFilter::Agent),
            KeyCode::Char('4') => self.toggle_event_filter(EventFilter::Think),
            KeyCode::Char('5') => self.toggle_event_filter(EventFilter::Tools),
            KeyCode::Char('6') => self.toggle_event_filter(EventFilter::Files),
            KeyCode::Char('7') => self.toggle_event_filter(EventFilter::Shell),
            KeyCode::Char('8') => self.toggle_event_filter(EventFilter::Task),
            KeyCode::Char('9') => self.toggle_event_filter(EventFilter::Web),
            KeyCode::Char('0') => self.toggle_event_filter(EventFilter::Other),
            _ => {}
        }
        false
    }

    fn toggle_turn_raw_override(&mut self) {
        if let Some(session) = self.selected_session().cloned() {
            let visible = self.get_visible_events(&session);
            let turns = extract_visible_turns(&visible);
            if turns.get(self.turn_index).is_some() {
                let idx = self.turn_index;
                if self.turn_raw_overrides.contains(&idx) {
                    self.turn_raw_overrides.remove(&idx);
                } else {
                    self.turn_raw_overrides.insert(idx);
                }
            }
        }
    }

    fn toggle_turn_prompt_expanded(&mut self) {
        let idx = self.turn_index;
        if self.turn_prompt_expanded.contains(&idx) {
            self.turn_prompt_expanded.remove(&idx);
        } else {
            self.turn_prompt_expanded.insert(idx);
        }
    }

    fn turn_next(&mut self) {
        if let Some(session) = self.selected_session().cloned() {
            let visible = self.get_visible_events(&session);
            let turns = extract_visible_turns(&visible);
            if self.turn_index + 1 < turns.len() {
                self.turn_index += 1;
                if let Some(&offset) = self.turn_line_offsets.get(self.turn_index) {
                    self.turn_agent_scroll = offset;
                } else {
                    self.turn_agent_scroll = 0;
                }
            }
        }
    }

    fn turn_prev(&mut self) {
        if self.turn_index > 0 {
            self.turn_index -= 1;
            if let Some(&offset) = self.turn_line_offsets.get(self.turn_index) {
                self.turn_agent_scroll = offset;
            } else {
                self.turn_agent_scroll = 0;
            }
            self.detach_live_follow_turn();
        }
    }

    fn sync_linear_to_turn(&mut self) {
        if let Some(session) = self.selected_session().cloned() {
            let visible = self.get_visible_events(&session);
            let turns = extract_visible_turns(&visible);
            let mut event_count = 0;
            for (ti, turn) in turns.iter().enumerate() {
                let turn_size = turn.user_events.len() + turn.agent_events.len();
                if event_count + turn_size > self.detail_event_index {
                    self.turn_index = ti;
                    self.turn_agent_scroll = 0;
                    self.turn_h_scroll = 0;
                    return;
                }
                event_count += turn_size;
            }
            self.turn_index = turns.len().saturating_sub(1);
            self.turn_agent_scroll = 0;
            self.turn_h_scroll = 0;
        }
    }

    fn sync_turn_to_linear(&mut self) {
        if let Some(session) = self.selected_session().cloned() {
            let visible = self.get_visible_events(&session);
            let turns = extract_visible_turns(&visible);
            let mut event_count = 0;
            for (ti, turn) in turns.iter().enumerate() {
                if ti == self.turn_index {
                    self.detail_event_index = event_count;
                    return;
                }
                event_count += turn.user_events.len() + turn.agent_events.len();
            }
        }
    }

    fn jump_to_next_same_type(&mut self) {
        if let Some(session) = self.selected_session().cloned() {
            let visible = self.get_visible_events(&session);
            if visible.is_empty() {
                return;
            }
            let current = self.detail_event_index.min(visible.len() - 1);
            let target_disc = std::mem::discriminant(&visible[current].event().event_type);
            for (i, de) in visible.iter().enumerate().skip(current + 1) {
                if std::mem::discriminant(&de.event().event_type) == target_disc {
                    self.detail_event_index = i;
                    return;
                }
            }
        }
    }

    fn jump_to_prev_same_type(&mut self) {
        if let Some(session) = self.selected_session().cloned() {
            let visible = self.get_visible_events(&session);
            if visible.is_empty() {
                return;
            }
            let current = self.detail_event_index.min(visible.len() - 1);
            if current == 0 {
                return;
            }
            let target_disc = std::mem::discriminant(&visible[current].event().event_type);
            for (i, de) in visible.iter().enumerate().take(current).rev() {
                if std::mem::discriminant(&de.event().event_type) == target_disc {
                    self.detail_event_index = i;
                    return;
                }
            }
        }
    }

    fn jump_to_next_user_message(&mut self) {
        if let Some(session) = self.selected_session().cloned() {
            let visible = self.get_visible_events(&session);
            let start = self.detail_event_index + 1;
            for (offset, de) in visible.iter().skip(start).enumerate() {
                if matches!(de.event().event_type, EventType::UserMessage) {
                    self.detail_event_index = start + offset;
                    return;
                }
            }
        }
    }

    fn jump_to_prev_user_message(&mut self) {
        if self.detail_event_index == 0 {
            return;
        }
        if let Some(session) = self.selected_session().cloned() {
            let visible = self.get_visible_events(&session);
            for (i, de) in visible
                .iter()
                .enumerate()
                .take(self.detail_event_index)
                .rev()
            {
                if matches!(de.event().event_type, EventType::UserMessage) {
                    self.detail_event_index = i;
                    return;
                }
            }
        }
    }

    // ── Helpers ─────────────────────────────────────────────────────────

    pub fn selected_session(&self) -> Option<&Session> {
        if self.is_db_view() {
            // In DB view, match by session_id against parsed sessions
            let idx = self.list_state.selected()?;
            let db_row = self.db_sessions.get(idx)?;
            self.sessions.iter().find(|s| s.session_id == db_row.id)
        } else {
            let abs_idx = self
                .list_state
                .selected()
                .map(|i| self.page * self.per_page + i)?;
            self.filtered_sessions
                .get(abs_idx)
                .and_then(|&idx| self.sessions.get(idx))
        }
    }

    pub fn rebuild_session_agent_metrics(&mut self) {
        self.session_max_active_agents = self
            .sessions
            .iter()
            .map(|session| {
                (
                    session.session_id.clone(),
                    Self::compute_session_max_active_agents(session),
                )
            })
            .collect();
    }

    fn short_user_id(user_id: &str) -> String {
        user_id.chars().take(10).collect()
    }

    fn actor_label_from_session(session: &Session) -> Option<String> {
        let attrs = &session.context.attributes;
        if let Some(nickname) = attrs
            .get("nickname")
            .or_else(|| attrs.get("user_nickname"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Some(format!("@{nickname}"));
        }
        if let Some(user_id) = attrs
            .get("user_id")
            .or_else(|| attrs.get("uid"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Some(format!("id:{}", Self::short_user_id(user_id)));
        }
        attrs
            .get("originator")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
    }

    /// Actor label for the currently selected session (nickname or user-id fallback).
    pub fn selected_session_actor_label(&self) -> Option<String> {
        if self.is_db_view() {
            let row = self.selected_db_session()?;
            if let Some(nickname) = row
                .nickname
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                return Some(format!("@{nickname}"));
            }
            if let Some(user_id) = row
                .user_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                return Some(format!("id:{}", Self::short_user_id(user_id)));
            }
        }
        self.selected_session()
            .and_then(Self::actor_label_from_session)
    }

    /// Get the selected DB session row (for repo view).
    pub fn selected_db_session(&self) -> Option<&LocalSessionRow> {
        let idx = self.list_state.selected()?;
        self.db_sessions.get(idx)
    }

    fn source_path_from_attrs(
        attrs: Option<&HashMap<String, serde_json::Value>>,
    ) -> Option<PathBuf> {
        let attrs = attrs?;
        for key in ["source_path", "source_file", "session_path", "path"] {
            let maybe = attrs
                .get(key)
                .and_then(|v| v.as_str())
                .map(PathBuf::from)
                .filter(|path| path.exists());
            if let Some(path) = maybe {
                return Some(path);
            }
        }
        None
    }

    pub fn handoff_candidates(&self) -> Vec<HandoffCandidate> {
        if self.is_db_view() {
            return self
                .db_sessions
                .iter()
                .map(|row| {
                    let source_path = row
                        .source_path
                        .as_ref()
                        .map(PathBuf::from)
                        .filter(|path| path.exists());
                    HandoffCandidate {
                        session_id: row.id.clone(),
                        title: row.title.clone().unwrap_or_else(|| row.id.clone()),
                        tool: row.tool.clone(),
                        model: row
                            .agent_model
                            .clone()
                            .unwrap_or_else(|| "unknown".to_string()),
                        created_at: DateTime::parse_from_rfc3339(&row.created_at)
                            .map(|value| value.with_timezone(&Utc))
                            .unwrap_or_else(|_| Utc::now()),
                        event_count: row.event_count.max(0) as usize,
                        message_count: row.message_count.max(0) as usize,
                        source_path,
                    }
                })
                .collect();
        }

        self.filtered_sessions
            .iter()
            .filter_map(|idx| self.sessions.get(*idx))
            .map(|session| HandoffCandidate {
                session_id: session.session_id.clone(),
                title: session
                    .context
                    .title
                    .clone()
                    .unwrap_or_else(|| session.session_id.clone()),
                tool: session.agent.tool.clone(),
                model: session.agent.model.clone(),
                created_at: session.context.created_at,
                event_count: if session.events.is_empty() {
                    session.stats.event_count as usize
                } else {
                    session.events.len()
                },
                message_count: session.stats.message_count as usize,
                source_path: Self::source_path_from_attrs(Some(&session.context.attributes)),
            })
            .collect()
    }

    pub fn selected_handoff_candidate(&self) -> Option<HandoffCandidate> {
        let candidates = self.handoff_candidates();
        let index = self.handoff_selected_index_for(&candidates)?;
        candidates.get(index).cloned()
    }

    fn handoff_selected_index_for(&self, candidates: &[HandoffCandidate]) -> Option<usize> {
        if candidates.is_empty() {
            return None;
        }
        if let Some(selected_id) = self.handoff_selected_session_id.as_deref() {
            if let Some(index) = candidates
                .iter()
                .position(|candidate| candidate.session_id == selected_id)
            {
                return Some(index);
            }
        }
        Some(0)
    }

    fn move_handoff_selection(&mut self, step: isize) {
        let candidates = self.handoff_candidates();
        if candidates.is_empty() {
            self.handoff_selected_session_id = None;
            return;
        }
        let current = self.handoff_selected_index_for(&candidates).unwrap_or(0) as isize;
        let max = candidates.len().saturating_sub(1) as isize;
        let next = (current + step).clamp(0, max) as usize;
        self.handoff_selected_session_id = Some(candidates[next].session_id.clone());
    }

    fn toggle_handoff_selection(&mut self) {
        let Some(candidate) = self.selected_handoff_candidate() else {
            self.flash_info("No handoff candidate in current scope");
            return;
        };
        let session_id = candidate.session_id;

        if let Some(idx) = self
            .handoff_selected_session_ids
            .iter()
            .position(|id| id == &session_id)
        {
            self.handoff_selected_session_ids.remove(idx);
            return;
        }

        self.handoff_selected_session_ids.push(session_id);
    }

    pub fn handoff_selected_candidates(&self) -> Vec<HandoffCandidate> {
        let candidates = self.handoff_candidates();
        self.handoff_selected_session_ids
            .iter()
            .filter_map(|session_id| {
                candidates
                    .iter()
                    .find(|candidate| &candidate.session_id == session_id)
                    .cloned()
            })
            .collect()
    }

    pub fn handoff_effective_candidates(&self) -> Vec<HandoffCandidate> {
        let selected = self.handoff_selected_candidates();
        if !selected.is_empty() {
            return selected;
        }
        self.selected_handoff_candidate()
            .into_iter()
            .collect::<Vec<_>>()
    }

    pub fn handoff_last_artifact_status(&self) -> Option<(String, bool, Vec<String>)> {
        let artifact_id = self.handoff_last_artifact_id.clone()?;
        let cwd = std::env::current_dir().ok()?;
        let repo_root = ops::find_repo_root(&cwd)?;
        let bytes = load_handoff_artifact(&repo_root, &artifact_id).ok()?;
        let artifact: HandoffArtifact = serde_json::from_slice(&bytes).ok()?;
        let stale_reasons = artifact
            .stale_reasons()
            .into_iter()
            .map(|reason| format!("{}: {}", reason.session_id, reason.reason))
            .collect::<Vec<_>>();
        let stale = !stale_reasons.is_empty();
        Some((artifact.artifact_id, stale, stale_reasons))
    }

    fn save_handoff_artifact_via_cli(&mut self) {
        let candidates = self.handoff_effective_candidates();
        if candidates.is_empty() {
            self.flash_info("No handoff candidate in current scope");
            return;
        }
        let source_paths = candidates
            .iter()
            .filter_map(|candidate| candidate.source_path.clone())
            .collect::<Vec<_>>();
        if source_paths.len() != candidates.len() {
            self.flash_info("Some selected sessions have no local source file");
            return;
        }

        let mut cmd = std::process::Command::new("opensession");
        cmd.arg("session").arg("handoff").arg("save");
        for path in &source_paths {
            cmd.arg(path);
        }

        let output = match cmd.output() {
            Ok(output) => output,
            Err(err) => {
                self.flash_error(format!("Failed to run opensession CLI: {err}"));
                return;
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if stderr.is_empty() {
                self.flash_error("Handoff save command failed");
            } else {
                self.flash_error(stderr);
            }
            return;
        }

        let parsed = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok();
        if let Some(artifact_id) = parsed
            .as_ref()
            .and_then(|value| value.get("artifact_id"))
            .and_then(|value| value.as_str())
        {
            self.handoff_last_artifact_id = Some(artifact_id.to_string());
            self.flash_success(format!("Saved handoff artifact: {artifact_id}"));
            return;
        }

        self.flash_success("Saved handoff artifact");
    }

    fn generate_handoff_via_cli(&mut self) {
        let candidates = self.handoff_effective_candidates();
        if candidates.is_empty() {
            self.flash_info("No handoff candidate in current scope");
            return;
        }
        let source_paths = candidates
            .iter()
            .filter_map(|candidate| candidate.source_path.clone())
            .collect::<Vec<_>>();
        if source_paths.len() != candidates.len() {
            self.flash_info("Some selected sessions have no local source file");
            return;
        }

        let output_path = std::env::current_dir()
            .map(|cwd| cwd.join("HANDOFF.md"))
            .unwrap_or_else(|_| std::path::PathBuf::from("HANDOFF.md"));

        let mut cmd = std::process::Command::new("opensession");
        cmd.arg("session")
            .arg("handoff")
            .arg("--format")
            .arg("markdown")
            .arg("--output")
            .arg(&output_path);
        for path in &source_paths {
            cmd.arg(path);
        }

        let output = match cmd.output() {
            Ok(output) => output,
            Err(err) => {
                self.flash_error(format!("Failed to run opensession CLI: {err}"));
                return;
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if stderr.is_empty() {
                self.flash_error("Handoff generate command failed");
            } else {
                self.flash_error(stderr);
            }
            return;
        }

        self.flash_success(format!("Generated handoff: {}", output_path.display()));
    }

    fn refresh_handoff_artifact_via_cli(&mut self) {
        let Some(artifact_id) = self.handoff_last_artifact_id.clone() else {
            self.flash_info("No saved handoff artifact yet");
            return;
        };

        let output = match std::process::Command::new("opensession")
            .arg("session")
            .arg("handoff")
            .arg("artifact")
            .arg("refresh")
            .arg(&artifact_id)
            .output()
        {
            Ok(output) => output,
            Err(err) => {
                self.flash_error(format!("Failed to run opensession CLI: {err}"));
                return;
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if stderr.is_empty() {
                self.flash_error("Handoff artifact refresh failed");
            } else {
                self.flash_error(stderr);
            }
            return;
        }

        self.flash_success(format!("Refreshed handoff artifact: {artifact_id}"));
    }

    fn select_session_by_id(&mut self, session_id: &str) -> bool {
        if self.is_db_view() {
            if let Some(index) = self.db_sessions.iter().position(|row| row.id == session_id) {
                self.list_state.select(Some(index));
                return true;
            }
            return false;
        }

        let Some(abs_index) = self.filtered_sessions.iter().position(|idx| {
            self.sessions
                .get(*idx)
                .is_some_and(|session| session.session_id == session_id)
        }) else {
            return false;
        };

        let per_page = self.per_page.max(1);
        self.page = abs_index / per_page;
        self.list_state.select(Some(abs_index % per_page));
        true
    }

    pub fn apply_discovered_sessions(&mut self, sessions: Vec<Session>) {
        let selected_session_id = self
            .selected_session()
            .map(|session| session.session_id.clone());
        let selected_local_abs = if self.is_db_view() {
            None
        } else {
            self.list_state
                .selected()
                .map(|selected| self.page.saturating_mul(self.per_page.max(1)) + selected)
        };

        self.sessions = sessions;
        self.rebuild_session_agent_metrics();
        self.filtered_sessions = (0..self.sessions.len()).collect();
        self.rebuild_available_tools();
        if self.list_layout == ListLayout::ByUser {
            self.rebuild_columns();
        }

        if self.sessions.is_empty() {
            self.list_state.select(None);
            self.handoff_selected_session_id = None;
            return;
        }

        if let Some(session_id) = selected_session_id {
            if self.select_session_by_id(&session_id) {
                self.handoff_selected_session_id = Some(session_id);
                return;
            }
        }

        if !self.is_db_view() {
            if let Some(abs_index) =
                selected_local_abs.filter(|idx| *idx < self.filtered_sessions.len())
            {
                let per_page = self.per_page.max(1);
                self.page = abs_index / per_page;
                self.list_state.select(Some(abs_index % per_page));
            } else {
                self.page = 0;
                self.list_state.select(Some(0));
            }
            self.handoff_selected_session_id = self
                .selected_session()
                .map(|session| session.session_id.clone());
        }

        if self.view == View::SessionDetail {
            self.remap_detail_selection_by_event_id();
        }
    }

    /// Check if the selected session's repo allows public upload.
    /// Returns `false` only if the repo has `.opensession/config.toml` with `allow_public = false`.
    pub fn selected_session_allows_public(&self) -> bool {
        let session = match self.selected_session() {
            Some(s) => s,
            None => return true,
        };
        let cwd = session
            .context
            .attributes
            .get("cwd")
            .or_else(|| session.context.attributes.get("working_directory"))
            .and_then(|v| v.as_str());
        let cwd = match cwd {
            Some(c) => c,
            None => return true,
        };
        let repo_root = match opensession_git_native::ops::find_repo_root(std::path::Path::new(cwd))
        {
            Some(r) => r,
            None => return true,
        };

        // Read allow_public from per-repo config (shared + local)
        for name in &["config.toml", "config.local.toml"] {
            let path = repo_root.join(".opensession").join(name);
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(val) = toml::from_str::<toml::Value>(&content) {
                    if let Some(false) = val.get("allow_public").and_then(|v| v.as_bool()) {
                        return false;
                    }
                }
            }
        }
        true
    }

    pub fn matches_event_filter(&self, event_type: &EventType) -> bool {
        if self.event_filters.contains(&EventFilter::All) {
            return true;
        }
        for f in &self.event_filters {
            let matches = match f {
                EventFilter::All => true,
                EventFilter::User => {
                    matches!(
                        event_type,
                        EventType::UserMessage | EventType::SystemMessage
                    )
                }
                EventFilter::Agent => matches!(event_type, EventType::AgentMessage),
                EventFilter::Think => matches!(event_type, EventType::Thinking),
                EventFilter::Tools => matches!(
                    event_type,
                    EventType::ToolCall { .. } | EventType::ToolResult { .. }
                ),
                EventFilter::Files => matches!(
                    event_type,
                    EventType::FileEdit { .. }
                        | EventType::FileCreate { .. }
                        | EventType::FileDelete { .. }
                        | EventType::FileRead { .. }
                        | EventType::CodeSearch { .. }
                        | EventType::FileSearch { .. }
                ),
                EventFilter::Shell => matches!(event_type, EventType::ShellCommand { .. }),
                EventFilter::Task => matches!(
                    event_type,
                    EventType::TaskStart { .. } | EventType::TaskEnd { .. }
                ),
                EventFilter::Web => matches!(
                    event_type,
                    EventType::WebSearch { .. } | EventType::WebFetch { .. }
                ),
                EventFilter::Other => matches!(
                    event_type,
                    EventType::Custom { .. }
                        | EventType::ImageGenerate { .. }
                        | EventType::VideoGenerate { .. }
                        | EventType::AudioGenerate { .. }
                ),
            };
            if matches {
                return true;
            }
        }
        false
    }

    pub fn get_visible_events<'a>(&self, session: &'a Session) -> Vec<DisplayEvent<'a>> {
        self.get_base_visible_events(session)
    }

    pub fn get_base_visible_events<'a>(&self, session: &'a Session) -> Vec<DisplayEvent<'a>> {
        let mut after_task: Vec<DisplayEvent<'a>> = build_lane_events_with_filter(
            session,
            |_| true,
            |event_type| self.matches_event_filter(event_type),
        )
        .into_iter()
        .map(|lane_event| DisplayEvent::Single {
            event: lane_event.event,
            source_index: lane_event.source_index,
            lane: lane_event.lane,
            marker: lane_event.marker,
            active_lanes: lane_event.active_lanes,
        })
        .collect();
        after_task.retain(|event| !Self::is_boilerplate_detail_event(event.event()));
        after_task
    }

    fn is_boilerplate_detail_event(event: &Event) -> bool {
        match &event.event_type {
            EventType::ToolCall { name } => name.eq_ignore_ascii_case("write_stdin"),
            EventType::ToolResult { name, .. } => {
                if name.eq_ignore_ascii_case("write_stdin") {
                    return Self::is_running_session_status_line(
                        Self::first_event_text_line(event).as_deref(),
                    );
                }
                if matches!(
                    name.to_ascii_lowercase().as_str(),
                    "exec_command" | "shell" | "bash" | "execute_command" | "spawn_process"
                ) {
                    return Self::is_running_session_status_line(
                        Self::first_event_text_line(event).as_deref(),
                    );
                }
                false
            }
            EventType::Thinking => Self::first_event_text_line(event)
                .as_deref()
                .is_some_and(Self::is_markdown_progress_line),
            _ => false,
        }
    }

    fn first_event_text_line(event: &Event) -> Option<String> {
        for block in &event.content.blocks {
            for fragment in Self::block_text_fragments(block) {
                for line in fragment.lines() {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
        }
        None
    }

    fn is_running_session_status_line(line: Option<&str>) -> bool {
        let Some(line) = line else {
            return true;
        };
        let lowered = line.trim().to_ascii_lowercase();
        lowered.is_empty()
            || lowered.contains("process running with session id")
            || lowered == "ok"
            || lowered == "output:"
    }

    fn is_markdown_progress_line(line: &str) -> bool {
        let trimmed = line.trim();
        if !(trimmed.starts_with("**") && trimmed.ends_with("**") && trimmed.len() > 4) {
            return false;
        }
        let lowered = trimmed.to_ascii_lowercase();
        lowered.contains("evaluating")
            || lowered.contains("planning")
            || lowered.contains("adjusting")
            || lowered.contains("confirming")
            || lowered.contains("summarizing")
    }

    fn collapse_consecutive_events<'a>(events: Vec<DisplayEvent<'a>>) -> Vec<DisplayEvent<'a>> {
        let mut result: Vec<DisplayEvent<'a>> = Vec::new();
        let mut i = 0;
        while i < events.len() {
            let group_seed = match &events[i] {
                DisplayEvent::Single { event, lane, .. } => {
                    consecutive_group_key(&event.event_type).map(|kind| (kind, *lane))
                }
                _ => None,
            };

            if let Some((kind, lane)) = group_seed {
                let start = i;
                let mut items: Vec<&DisplayEvent<'a>> = Vec::new();
                while i < events.len() {
                    if let DisplayEvent::Single {
                        event,
                        lane: current_lane,
                        ..
                    } = &events[i]
                    {
                        if *current_lane == lane
                            && consecutive_group_key(&event.event_type).as_deref() == Some(&kind)
                        {
                            items.push(&events[i]);
                            i += 1;
                            continue;
                        }
                    }
                    break;
                }

                if items.len() > 1 {
                    if let DisplayEvent::Single {
                        event,
                        source_index,
                        lane,
                        marker,
                        active_lanes,
                    } = items[0]
                    {
                        result.push(DisplayEvent::Collapsed {
                            first: event,
                            source_index: *source_index,
                            count: items.len() as u32,
                            kind: kind.clone(),
                            lane: *lane,
                            marker: *marker,
                            active_lanes: active_lanes.clone(),
                        });
                    }
                } else {
                    result.push(events[start].clone());
                }
            } else {
                result.push(events[i].clone());
                i += 1;
            }
        }
        result
    }

    fn compact_event_line(event: &Event) -> String {
        match &event.event_type {
            EventType::UserMessage | EventType::AgentMessage | EventType::SystemMessage => {
                let line = Self::first_text_block_line(&event.content.blocks, 96);
                if line.is_empty() {
                    "(message)".to_string()
                } else {
                    line
                }
            }
            EventType::Thinking => "thinking".to_string(),
            EventType::ToolCall { name } => format!("tool call: {name}"),
            EventType::ToolResult {
                name,
                is_error,
                call_id: _,
            } => {
                if *is_error {
                    format!("tool error: {name}")
                } else {
                    format!("tool ok: {name}")
                }
            }
            EventType::FileRead { path }
            | EventType::FileCreate { path }
            | EventType::FileDelete { path } => path.clone(),
            EventType::FileEdit { path, .. } => format!("edit {path}"),
            EventType::CodeSearch { query } | EventType::WebSearch { query } => query.clone(),
            EventType::FileSearch { pattern } => pattern.clone(),
            EventType::ShellCommand { command, exit_code } => match exit_code {
                Some(code) => format!("{command} => {code}"),
                None => command.clone(),
            },
            EventType::WebFetch { url } => url.clone(),
            EventType::ImageGenerate { prompt }
            | EventType::VideoGenerate { prompt }
            | EventType::AudioGenerate { prompt } => prompt.clone(),
            EventType::TaskStart { title } => {
                title.clone().unwrap_or_else(|| "task start".to_string())
            }
            EventType::TaskEnd { summary } => {
                summary.clone().unwrap_or_else(|| "task end".to_string())
            }
            EventType::Custom { kind } => kind.clone(),
            _ => String::new(),
        }
    }

    fn first_text_block_line(blocks: &[ContentBlock], max_len: usize) -> String {
        for block in blocks {
            if let ContentBlock::Text { text } = block {
                if let Some(line) = text.lines().next() {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        if trimmed.chars().count() <= max_len {
                            return trimmed.to_string();
                        }
                        let mut out = String::new();
                        for ch in trimmed.chars().take(max_len.saturating_sub(1)) {
                            out.push(ch);
                        }
                        out.push('…');
                        return out;
                    }
                }
            }
        }
        String::new()
    }

    fn event_kind_label(event_type: &EventType) -> &'static str {
        match event_type {
            EventType::UserMessage => "user",
            EventType::AgentMessage => "agent",
            EventType::SystemMessage => "system",
            EventType::Thinking => "thinking",
            EventType::ToolCall { .. } => "tool_call",
            EventType::ToolResult { .. } => "tool_result",
            EventType::FileRead { .. } => "file_read",
            EventType::CodeSearch { .. } => "code_search",
            EventType::FileSearch { .. } => "file_search",
            EventType::FileEdit { .. } => "file_edit",
            EventType::FileCreate { .. } => "file_create",
            EventType::FileDelete { .. } => "file_delete",
            EventType::ShellCommand { .. } => "shell",
            EventType::WebSearch { .. } => "web_search",
            EventType::WebFetch { .. } => "web_fetch",
            EventType::ImageGenerate { .. } => "image",
            EventType::VideoGenerate { .. } => "video",
            EventType::AudioGenerate { .. } => "audio",
            EventType::TaskStart { .. } => "task_start",
            EventType::TaskEnd { .. } => "task_end",
            EventType::Custom { .. } => "custom",
            _ => "other",
        }
    }

    fn set_session_detail_issue(&mut self, session_id: String, message: String) {
        self.session_detail_issues.insert(session_id, message);
    }

    fn clear_session_detail_issue(&mut self, session_id: &str) {
        self.session_detail_issues.remove(session_id);
    }

    pub fn detail_issue_for_session(&self, session_id: &str) -> Option<&str> {
        self.session_detail_issues
            .get(session_id)
            .map(String::as_str)
    }

    pub fn record_selected_session_detail_issue(&mut self, message: String) {
        let Some(session_id) = self
            .selected_session()
            .map(|session| session.session_id.clone())
        else {
            return;
        };
        self.set_session_detail_issue(session_id, message);
    }

    fn build_zero_event_detail_issue(&self) -> String {
        if let Some(path) = self.detail_source_path.as_deref() {
            if let Some(err) = Self::source_error_hint(path) {
                return format!("No parsed events. detected source error: {err}");
            }
            return format!("No parsed events from source log: {}", path.display());
        }
        "No parsed events for this session.".to_string()
    }

    pub(crate) fn source_error_hint(path: &Path) -> Option<String> {
        let raw = std::fs::read_to_string(path).ok()?;
        let mut lines: Vec<&str> = raw.lines().collect();
        if lines.len() > 400 {
            lines = lines.split_off(lines.len() - 400);
        }
        for line in lines.into_iter().rev() {
            if let Some(err) = Self::source_line_error_hint(line) {
                return Some(err);
            }
        }
        None
    }

    fn source_line_error_hint(line: &str) -> Option<String> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }

        if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if !Self::json_looks_error(&value) {
                return None;
            }
            if let Some(msg) = Self::json_error_message(&value) {
                return Some(msg);
            }
            return Some(Self::clip_plain(trimmed, 220));
        }

        let lower = trimmed.to_ascii_lowercase();
        let plain_error = lower.starts_with("error")
            || lower.contains(" error:")
            || lower.contains(" failed")
            || lower.contains(" exception")
            || lower.contains(" panic")
            || lower.contains(" traceback");
        if plain_error {
            return Some(Self::clip_plain(trimmed, 220));
        }
        None
    }

    fn json_looks_error(value: &serde_json::Value) -> bool {
        let Some(obj) = value.as_object() else {
            return false;
        };

        let bool_error = obj
            .get("is_error")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if bool_error {
            return true;
        }

        let level_error = obj
            .get("level")
            .and_then(|v| v.as_str())
            .is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "error" | "fatal"));
        if level_error {
            return true;
        }

        let status_error = obj
            .get("status")
            .and_then(|v| v.as_str())
            .is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "error" | "failed"));
        if status_error {
            return true;
        }

        obj.contains_key("error") || obj.contains_key("exception") || obj.contains_key("stderr")
    }

    fn json_error_message(value: &serde_json::Value) -> Option<String> {
        let obj = value.as_object()?;

        for key in [
            "error",
            "exception",
            "message",
            "detail",
            "stderr",
            "summary",
            "text",
            "output",
        ] {
            let Some(field) = obj.get(key) else {
                continue;
            };
            if let Some(msg) = field.as_str() {
                let msg = msg.trim();
                if !msg.is_empty() {
                    return Some(Self::clip_plain(msg, 220));
                }
            }
            if let Some(inner) = field
                .as_object()
                .and_then(|inner| inner.get("message"))
                .and_then(|v| v.as_str())
            {
                let msg = inner.trim();
                if !msg.is_empty() {
                    return Some(Self::clip_plain(msg, 220));
                }
            }
        }
        None
    }

    fn clip_plain(text: &str, max_chars: usize) -> String {
        let trimmed = text.trim();
        if trimmed.chars().count() <= max_chars {
            return trimmed.to_string();
        }
        let mut out = String::new();
        for ch in trimmed.chars().take(max_chars.saturating_sub(1)) {
            out.push(ch);
        }
        out.push('…');
        out
    }

    fn leave_detail_view(&mut self) {
        self.view = View::SessionList;
        self.detail_scroll = 0;
        self.detail_event_index = 0;
        self.detail_h_scroll = 0;
        self.detail_view_mode = DetailViewMode::Linear;
        self.detail_hydrate_pending = false;
        self.live_mode = false;
        self.live_last_event_at = None;
        self.live_subscription = None;
        self.follow_tail_linear.reset();
        self.follow_tail_turn.reset();
    }

    fn live_recent_cutoff() -> ChronoDuration {
        ChronoDuration::minutes(5)
    }

    fn selected_session_last_event_at(&self) -> Option<DateTime<Utc>> {
        self.selected_session()
            .and_then(|session| session.events.last().map(|event| event.timestamp))
    }

    fn parse_datetime_utc(value: &str) -> Option<DateTime<Utc>> {
        if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
            return Some(dt.with_timezone(&Utc));
        }
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f") {
            return Some(dt.and_utc());
        }
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S") {
            return Some(dt.and_utc());
        }
        None
    }

    fn parse_unix_timestamp_utc(seconds: i64) -> Option<DateTime<Utc>> {
        DateTime::<Utc>::from_timestamp(seconds, 0)
    }

    fn json_value_timestamp(value: &serde_json::Value) -> Option<DateTime<Utc>> {
        match value {
            serde_json::Value::String(raw) => Self::parse_datetime_utc(raw),
            serde_json::Value::Number(num) => num
                .as_i64()
                .or_else(|| num.as_u64().and_then(|v| i64::try_from(v).ok()))
                .and_then(Self::parse_unix_timestamp_utc),
            serde_json::Value::Object(map) => {
                for key in ["timestamp", "created_at", "updated_at", "time", "ts"] {
                    if let Some(ts) = map.get(key).and_then(Self::json_value_timestamp) {
                        return Some(ts);
                    }
                }
                for nested in map.values() {
                    if let Some(ts) = Self::json_value_timestamp(nested) {
                        return Some(ts);
                    }
                }
                None
            }
            serde_json::Value::Array(values) => {
                for nested in values.iter().rev() {
                    if let Some(ts) = Self::json_value_timestamp(nested) {
                        return Some(ts);
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn source_path_last_event_at(path: &Path) -> Option<DateTime<Utc>> {
        let raw = std::fs::read_to_string(path).ok()?;

        for line in raw.lines().rev().take(400) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
                continue;
            };
            if let Some(ts) = Self::json_value_timestamp(&value) {
                return Some(ts);
            }
        }

        let value = serde_json::from_str::<serde_json::Value>(&raw).ok()?;
        Self::json_value_timestamp(&value)
    }

    fn is_source_path_recently_modified(&self) -> bool {
        let Some(path) = self.detail_source_path.as_ref() else {
            return false;
        };
        let Ok(meta) = std::fs::metadata(path) else {
            return false;
        };
        let Ok(modified) = meta.modified() else {
            return false;
        };
        let modified_at: DateTime<Utc> = modified.into();
        if Utc::now().signed_duration_since(modified_at) > Self::live_recent_cutoff() {
            return false;
        }

        // Guard against stale sessions that happen to have a recently touched source file.
        Self::source_path_last_event_at(path)
            .map(|ts| Utc::now().signed_duration_since(ts) <= Self::live_recent_cutoff())
            .unwrap_or(false)
    }

    fn is_selected_session_recently_live(&self) -> bool {
        self.selected_session_last_event_at()
            .map(|ts| Utc::now().signed_duration_since(ts) <= Self::live_recent_cutoff())
            .unwrap_or(false)
    }

    fn refresh_live_subscription(&mut self) {
        self.live_subscription = None;
        if self.view != View::SessionDetail {
            return;
        }
        if !self.daemon_config.daemon.detail_realtime_preview_enabled
            || !self.realtime_preview_enabled
        {
            return;
        }

        let Some(path) = self.detail_source_path.clone() else {
            return;
        };
        let Some(session) = self.selected_session().cloned() else {
            return;
        };
        let debounce_ms = self.daemon_config.daemon.realtime_debounce_ms.max(300);
        let provider = DefaultLiveFeedProvider;
        self.live_subscription =
            provider.subscribe(&path, &session, Duration::from_millis(debounce_ms));
    }

    fn refresh_live_mode(&mut self) {
        let has_subscription = self.live_subscription.is_some();
        let provider_active = self
            .live_subscription
            .as_ref()
            .is_some_and(LiveSubscription::is_active);
        let source_recent = self.is_source_path_recently_modified();
        let event_recent = self.is_selected_session_recently_live();
        self.live_mode = if has_subscription {
            provider_active || source_recent || event_recent
        } else {
            source_recent || event_recent
        };
    }

    fn observe_live_tail_proximity(&mut self) {
        let Some(session) = self.selected_session().cloned() else {
            self.follow_tail_linear.mark_before_update(true);
            self.follow_tail_turn.mark_before_update(true);
            return;
        };
        let visible = self.get_visible_events(&session);
        self.observe_linear_tail_proximity(visible.len());
        let turns = extract_visible_turns(&visible);
        self.observe_turn_tail_proximity(turns.len());
    }

    pub fn observe_linear_tail_proximity(&mut self, visible_event_count: usize) {
        let threshold = self.follow_tail_linear.auto_follow_threshold_rows;
        let remaining_rows = visible_event_count.saturating_sub(self.detail_event_index + 1);
        self.follow_tail_linear
            .mark_before_update(remaining_rows <= threshold);
    }

    pub fn observe_turn_tail_proximity(&mut self, turn_count: usize) {
        let threshold = self.follow_tail_turn.auto_follow_threshold_rows;
        let remaining_rows = turn_count.saturating_sub(self.turn_index + 1);
        self.follow_tail_turn
            .mark_before_update(remaining_rows <= threshold);
    }

    fn detach_live_follow_linear(&mut self) {
        self.follow_tail_linear.detach();
    }

    fn reattach_live_follow_linear(&mut self) {
        self.follow_tail_linear.reattach();
    }

    fn detach_live_follow_turn(&mut self) {
        self.follow_tail_turn.detach();
    }

    fn reattach_live_follow_turn(&mut self) {
        self.follow_tail_turn.reattach();
    }

    pub fn detail_follow_state(&self) -> &FollowTailState {
        if self.detail_view_mode == DetailViewMode::Turn {
            &self.follow_tail_turn
        } else {
            &self.follow_tail_linear
        }
    }

    pub fn detail_follow_status_label(&self) -> &'static str {
        if self.detail_follow_state().is_following {
            "ON"
        } else {
            "OFF"
        }
    }

    pub fn jump_to_latest_linear(&mut self) {
        if let Some(session) = self.selected_session() {
            let visible = self.visible_event_count(session);
            if visible > 0 {
                self.detail_event_index = visible - 1;
            } else {
                self.detail_event_index = 0;
            }
        }
        self.reattach_live_follow_linear();
        self.update_detail_selection_anchor();
    }

    pub fn jump_to_latest_turn(&mut self) {
        if let Some(session) = self.selected_session().cloned() {
            let visible = self.get_visible_events(&session);
            let turns = extract_visible_turns(&visible);
            self.turn_index = turns.len().saturating_sub(1);
            self.turn_agent_scroll = u16::MAX;
        }
        self.reattach_live_follow_turn();
    }

    pub fn poll_live_update_batch(&mut self) -> Option<LiveUpdateBatch> {
        if self.view != View::SessionDetail {
            return None;
        }
        let batch = self
            .live_subscription
            .as_mut()
            .and_then(LiveSubscription::poll_update);
        if let Some(ref batch) = batch {
            if !batch.has_updates() && !batch.active {
                self.refresh_live_mode();
                return None;
            }
            if let Some(last_event_at) = batch.last_event_at {
                self.live_last_event_at = Some(last_event_at);
            }
            if batch.active || batch.cursor.is_some() || batch.source_offset.is_some() {
                self.live_mode = true;
            }
        }
        self.refresh_live_mode();
        batch
    }

    pub fn apply_live_update_batch(&mut self, batch: LiveUpdateBatch) {
        self.observe_live_tail_proximity();
        let should_follow_linear = self.follow_tail_linear.should_follow_after_update();
        let should_follow_turn = self.follow_tail_turn.should_follow_after_update();
        let mut applied_reload = false;

        for update in batch.updates {
            match update {
                LiveUpdate::SessionReloaded(session) => {
                    self.apply_reloaded_session(*session);
                    applied_reload = true;
                }
                LiveUpdate::EventsAppended(events) => {
                    if let Some(last_event) = events.last() {
                        self.live_last_event_at = Some(last_event.timestamp);
                    }
                }
            }
        }

        if let Some(last_event_at) = batch.last_event_at {
            self.live_last_event_at = Some(last_event_at);
        }
        if !applied_reload {
            self.refresh_live_mode();
            return;
        }

        if should_follow_linear {
            self.jump_to_latest_linear();
        }
        if should_follow_turn {
            self.jump_to_latest_turn();
        }
        self.refresh_live_mode();
        self.update_detail_selection_anchor();
    }

    pub(crate) fn resolve_selected_source_path(&self) -> Option<PathBuf> {
        if let Some(row) = self.selected_db_session() {
            if let Some(path) = row.source_path.as_ref().map(PathBuf::from) {
                if path.exists() {
                    return Some(path);
                }
            }
        }

        if let (Some(db), Some(session)) = (&self.db, self.selected_session()) {
            if let Ok(Some(path)) = db.get_session_source_path(&session.session_id) {
                let path = PathBuf::from(path);
                if path.exists() {
                    return Some(path);
                }
            }
        }

        let session = self.selected_session()?;
        for key in ["source_path", "source_file", "session_path", "path"] {
            let maybe = session
                .context
                .attributes
                .get(key)
                .and_then(|v| v.as_str())
                .map(PathBuf::from);
            if let Some(path) = maybe {
                if path.exists() {
                    return Some(path);
                }
            }
        }
        None
    }

    pub fn update_detail_selection_anchor(&mut self) {
        if self.view != View::SessionDetail {
            return;
        }
        let Some(session) = self.selected_session().cloned() else {
            return;
        };
        let visible = self.get_visible_events(&session);
        if visible.is_empty() {
            self.detail_selected_event_id = None;
            self.detail_event_index = 0;
            return;
        }
        self.detail_event_index = self.detail_event_index.min(visible.len() - 1);
        self.detail_selected_event_id = visible
            .get(self.detail_event_index)
            .map(|de| de.event().event_id.clone());
    }

    pub fn remap_detail_selection_by_event_id(&mut self) {
        if self.view != View::SessionDetail {
            return;
        }
        let Some(session) = self.selected_session().cloned() else {
            return;
        };
        let visible = self.get_visible_events(&session);
        if visible.is_empty() {
            self.detail_event_index = 0;
            self.detail_selected_event_id = None;
            return;
        }

        if let Some(anchor) = self.detail_selected_event_id.clone() {
            if let Some(idx) = visible.iter().position(|de| de.event().event_id == anchor) {
                self.detail_event_index = idx;
                return;
            }
        }
        self.detail_event_index = self.detail_event_index.min(visible.len() - 1);
        self.detail_selected_event_id = visible
            .get(self.detail_event_index)
            .map(|de| de.event().event_id.clone());
    }

    pub fn should_skip_realtime_for_selected(&self) -> bool {
        self.selected_session().is_none()
    }

    pub fn take_detail_hydrate_path(&mut self) -> Option<PathBuf> {
        if self.view != View::SessionDetail || !self.detail_hydrate_pending {
            return None;
        }
        self.detail_hydrate_pending = false;

        let session = self.selected_session().cloned()?;
        if !session.events.is_empty() {
            self.clear_session_detail_issue(&session.session_id);
            return None;
        }
        if session.stats.event_count == 0 {
            self.set_session_detail_issue(session.session_id, self.build_zero_event_detail_issue());
            return None;
        }
        self.clear_session_detail_issue(&session.session_id);
        self.detail_source_path.clone()
    }

    pub fn apply_reloaded_session(&mut self, reloaded: Session) {
        let sid = reloaded.session_id.clone();
        let max_agents = Self::compute_session_max_active_agents(&reloaded);
        if let Some(existing) = self.sessions.iter_mut().find(|s| s.session_id == sid) {
            *existing = reloaded;
        } else {
            self.sessions.push(reloaded);
        }
        self.session_max_active_agents
            .insert(sid.clone(), max_agents);
        self.session_detail_issues.remove(&sid);
        self.detail_hydrate_pending = false;
        if let Some(session) = self.selected_session() {
            self.live_last_event_at = session.events.last().map(|event| event.timestamp);
        }
        self.remap_detail_selection_by_event_id();
        self.refresh_live_mode();
    }

    fn compute_session_max_active_agents(session: &Session) -> usize {
        opensession_core::agent_metrics::max_active_agents(session)
    }

    fn visible_event_count(&self, session: &Session) -> usize {
        self.get_visible_events(session).len()
    }

    fn apply_filter(&mut self) {
        let search_query = self
            .normalized_search_query()
            .unwrap_or_default()
            .to_ascii_lowercase();
        self.page = 0;

        match self.view_mode.clone() {
            ViewMode::Local => {
                let required_tool = self.active_tool_filter();
                let cutoff = self.local_time_cutoff();

                self.filtered_sessions = self
                    .sessions
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| {
                        self.local_session_matches_filters(
                            s,
                            &search_query,
                            cutoff.as_ref(),
                            required_tool,
                        )
                    })
                    .map(|(i, _)| i)
                    .collect();

                let sort_order = SortOrder::Recent;
                self.filtered_sessions.sort_by(|a, b| {
                    let lhs = &self.sessions[*a];
                    let rhs = &self.sessions[*b];
                    Self::compare_local_sessions_for_sort(&sort_order, lhs, rhs)
                });

                self.list_state
                    .select(if self.filtered_sessions.is_empty() {
                        None
                    } else {
                        Some(0)
                    });
                if self.list_layout == ListLayout::ByUser {
                    self.rebuild_columns();
                }
            }
            ViewMode::Repo(_) => {
                self.reload_db_sessions();
                if self.list_layout == ListLayout::ByUser {
                    self.rebuild_columns();
                }
                self.list_state.select(if self.db_sessions.is_empty() {
                    None
                } else {
                    Some(0)
                });
            }
        }
    }
}
