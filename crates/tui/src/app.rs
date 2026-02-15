use chrono::{DateTime, Duration as ChronoDuration, Utc};
use crossterm::event::{KeyCode, MouseEvent, MouseEventKind};
use opensession_api::{
    InvitationResponse, MemberResponse, SortOrder, TeamDetailResponse, TeamResponse, TimeRange,
    UserSettingsResponse,
};
use opensession_core::trace::{ContentBlock, Event, EventType, Session};
use opensession_local_db::{LocalDb, LocalSessionFilter, LocalSessionRow};
use ratatui::widgets::ListState;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use crate::async_ops::{AsyncCommand, CommandResult};
use crate::config::{self, DaemonConfig, GitStorageMethod, PublishMode, SettingField};
use crate::live::{
    DefaultLiveFeedProvider, FollowTailState, LiveFeedProvider, LiveSubscription, LiveUpdate,
    LiveUpdateBatch,
};
use crate::session_timeline::{build_lane_events_with_filter, LaneMarker};
use crate::timeline_summary::{
    describe_summary_engine, parse_timeline_summary_output, SummaryRuntimeConfig,
    TimelineSummaryCacheEntry, TimelineSummaryPayload, TimelineSummaryWindowKey,
    TimelineSummaryWindowRequest,
};
pub use crate::views::modal::{ConfirmAction, InputAction, Modal};

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
    /// A semantic summary row inserted after key task/checkpoint events.
    SummaryRow {
        event: &'a Event,
        source_index: usize,
        window_id: u64,
        summary: String,
        lane: usize,
        active_lanes: Vec<usize>,
    },
}

impl<'a> DisplayEvent<'a> {
    pub fn event(&self) -> &'a Event {
        match self {
            DisplayEvent::Single { event, .. } => event,
            DisplayEvent::Collapsed { first, .. } => first,
            DisplayEvent::SummaryRow { event, .. } => event,
        }
    }

    pub fn source_index(&self) -> usize {
        match self {
            DisplayEvent::Single { source_index, .. }
            | DisplayEvent::Collapsed { source_index, .. }
            | DisplayEvent::SummaryRow { source_index, .. } => *source_index,
        }
    }

    pub fn lane(&self) -> usize {
        match self {
            DisplayEvent::Single { lane, .. }
            | DisplayEvent::Collapsed { lane, .. }
            | DisplayEvent::SummaryRow { lane, .. } => *lane,
        }
    }

    pub fn marker(&self) -> LaneMarker {
        match self {
            DisplayEvent::Single { marker, .. } | DisplayEvent::Collapsed { marker, .. } => *marker,
            DisplayEvent::SummaryRow { .. } => LaneMarker::None,
        }
    }

    pub fn active_lanes(&self) -> &[usize] {
        match self {
            DisplayEvent::Single { active_lanes, .. }
            | DisplayEvent::Collapsed { active_lanes, .. }
            | DisplayEvent::SummaryRow { active_lanes, .. } => active_lanes,
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

/// High-signal action events used for chronicle/window summary anchors.
fn is_action_summary_event(event_type: &EventType) -> bool {
    matches!(
        event_type,
        EventType::TaskStart { .. }
            | EventType::TaskEnd { .. }
            | EventType::ToolResult { .. }
            | EventType::ShellCommand { .. }
            | EventType::FileEdit { .. }
            | EventType::FileCreate { .. }
            | EventType::FileDelete { .. }
    )
}

#[derive(Debug, Clone)]
struct SummaryAnchor<'a> {
    scope: SummaryScope,
    key: TimelineSummaryWindowKey,
    anchor_event: &'a Event,
    anchor_source_index: usize,
    display_index: usize,
    start_display_index: usize,
    end_display_index: usize,
    lane: usize,
    active_lanes: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SummaryScope {
    Window,
    Turn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedTimelineSummaryRow {
    lookup_key: String,
    compact: String,
    payload: TimelineSummaryPayload,
    raw: String,
    saved_at_unix: u64,
}

/// Which screen the user is viewing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    SessionList,
    SessionDetail,
    Setup,
    Settings,
    Operations,
    Teams,
    TeamDetail,
    #[allow(dead_code)]
    Invitations,
    Help,
}

/// Top-level tab navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Sessions,
    Collaboration,
    Operations,
    Settings,
}

/// Focus section within TeamDetail view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamDetailFocus {
    Info,
    Members,
    Invite,
}

/// Settings sub-section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSection {
    Workspace,
    CaptureSync,
    TimelineIntelligence,
    StoragePrivacy,
    Account,
}

impl SettingsSection {
    pub const ORDER: [Self; 5] = [
        Self::Workspace,
        Self::CaptureSync,
        Self::TimelineIntelligence,
        Self::StoragePrivacy,
        Self::Account,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Workspace => "Workspace",
            Self::CaptureSync => "Capture & Sync",
            Self::TimelineIntelligence => "Timeline Intel",
            Self::StoragePrivacy => "Storage & Privacy",
            Self::Account => "Account",
        }
    }

    pub fn panel_title(self) -> &'static str {
        match self {
            Self::Workspace => "Workspace",
            Self::CaptureSync => "Capture & Sync",
            Self::TimelineIntelligence => "Timeline Intelligence",
            Self::StoragePrivacy => "Storage & Privacy",
            Self::Account => "Account",
        }
    }

    pub fn group(self) -> Option<config::SettingsGroup> {
        match self {
            Self::Workspace => Some(config::SettingsGroup::Workspace),
            Self::CaptureSync => Some(config::SettingsGroup::CaptureSync),
            Self::TimelineIntelligence => Some(config::SettingsGroup::TimelineIntelligence),
            Self::StoragePrivacy => Some(config::SettingsGroup::StoragePrivacy),
            Self::Account => None,
        }
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

/// Password change form state.
#[derive(Default)]
pub struct PasswordForm {
    pub field_index: usize, // 0=current, 1=new, 2=confirm
    pub current: String,
    pub new_password: String,
    pub confirm: String,
    pub editing: bool,
}

/// Setup sub-mode: API key entry vs email/password login.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupMode {
    ApiKey,
    Login,
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
    Team,
    Public,
}

impl SetupScenario {
    pub const ALL: [Self; 3] = [Self::Local, Self::Team, Self::Public];
}

/// State for the email/password login form.
#[derive(Default)]
pub struct LoginState {
    pub field_index: usize, // 0=email, 1=password
    pub email: String,
    pub password: String,
    pub editing: bool,
    pub status: Option<String>,
    pub loading: bool,
}

/// Active event type filter options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventFilter {
    Messages,
    ToolCalls,
    Thinking,
    FileOps,
    Shell,
    All,
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

fn is_infra_warning_user_message(event: &Event) -> bool {
    if !matches!(event.event_type, EventType::UserMessage) {
        return false;
    }
    if App::is_internal_summary_user_event(event) {
        return true;
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
    if App::is_internal_summary_title(&lower) {
        return true;
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
        if is_control_event(event) || App::is_internal_summary_user_event(event) {
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
    use chrono::Utc;
    use opensession_core::trace::{Agent, Content, Session};
    use serde_json::Value;
    use std::time::{Duration, Instant};

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
                "You are generating a turn-summary payload. Return JSON only.",
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
    fn internal_summary_title_matches_turn_summary_prompt() {
        assert!(App::is_internal_summary_title(
            "You are generating a turn-summary payload."
        ));
        assert!(App::is_internal_summary_title(
            "Return JSON only (no markdown, no prose) with keys"
        ));
        assert!(App::is_internal_summary_title(
            "\"kind\":\"turn-summary\",\"version\":\"2.0\""
        ));
        assert!(!App::is_internal_summary_title(
            "Rules: Preserve evidence and keep factual and concise."
        ));
        assert!(App::is_internal_summary_title(
            "\"agent_quotes\":[\"...\"], \"modified_files\":[{\"path\":\"...\"}]"
        ));
        assert!(!App::is_internal_summary_title("Rules:"));
        assert!(!App::is_internal_summary_title(
            "summary_scope: 1라인 보다는 전체 이벤트 내용을 읽고 서머리가 되어야해"
        ));
    }

    #[test]
    fn internal_summary_row_filters_json_schema_description() {
        let row = LocalSessionRow {
            id: "row-1".to_string(),
            source_path: None,
            sync_status: "local".to_string(),
            last_synced_at: None,
            user_id: None,
            nickname: None,
            team_id: None,
            agent_provider: None,
            agent_model: None,
            tool: String::new(),
            title: None,
            description: Some("\"kind\":\"turn-summary\"".to_string()),
            tags: None,
            created_at: "2026-02-14T00:00:00Z".to_string(),
            uploaded_at: None,
            message_count: 1,
            user_message_count: 1,
            task_count: 0,
            event_count: 3,
            duration_seconds: 1,
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
        };

        assert!(App::is_internal_summary_row(&row));
    }

    #[test]
    fn internal_summary_row_keeps_codex_without_user_messages() {
        let row = LocalSessionRow {
            id: "row-2".to_string(),
            source_path: None,
            sync_status: "local".to_string(),
            last_synced_at: None,
            user_id: None,
            nickname: None,
            team_id: None,
            agent_provider: None,
            agent_model: None,
            tool: "codex".to_string(),
            title: None,
            description: None,
            tags: None,
            created_at: "2026-02-14T00:00:00Z".to_string(),
            uploaded_at: None,
            message_count: 1,
            user_message_count: 0,
            task_count: 0,
            event_count: 3,
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
        };

        assert!(!App::is_internal_summary_row(&row));
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
    fn get_base_visible_events_hides_claude_merged_subagent_events() {
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

        assert_eq!(visible.len(), 2);
        assert!(visible
            .iter()
            .all(|row| row.event().task_id.as_deref() != Some("agent-123")));
        assert_eq!(app.session_max_active_agents.get("s-hidden"), Some(&1usize));
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
        app.view_mode = ViewMode::Team("team-1".to_string());
        app.db_sessions = vec![LocalSessionRow {
            id: "db-1".to_string(),
            source_path: None,
            sync_status: "synced".to_string(),
            last_synced_at: None,
            user_id: Some("0123456789abcdef".to_string()),
            nickname: None,
            team_id: Some("team-1".to_string()),
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
    fn live_summary_jobs_are_blocked() {
        let session = make_live_session("live-summary-off", 4);
        let mut app = App::new(vec![session]);
        app.enter_detail();
        app.live_mode = true;
        app.detail_entered_at = Instant::now() - Duration::from_secs(2);

        assert!(app.schedule_detail_summary_jobs().is_none());
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
        std::fs::write(&path, "{}\n").expect("write file");

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
    fn focus_detail_view_disables_summary_status_and_queue() {
        let session = make_live_session("focus-no-summary", 4);
        let mut app = App::new(vec![session]);
        app.enter_detail();
        app.daemon_config.daemon.summary_enabled = true;
        app.detail_entered_at = Instant::now() - Duration::from_secs(2);

        app.focus_detail_view = true;
        assert_eq!(app.llm_summary_status_label(), "off");
        assert!(app.schedule_detail_summary_jobs().is_none());
    }

    #[test]
    fn focus_detail_view_v_toggles_turn_and_linear() {
        let session = make_live_session("focus-v-toggle", 4);
        let mut app = App::new(vec![session]);
        app.focus_detail_view = true;
        app.enter_detail();
        assert_eq!(app.detail_view_mode, DetailViewMode::Turn);

        app.handle_detail_key(KeyCode::Char('v'));
        assert_eq!(app.detail_view_mode, DetailViewMode::Linear);

        app.handle_detail_key(KeyCode::Char('v'));
        assert_eq!(app.detail_view_mode, DetailViewMode::Turn);
    }

    #[test]
    fn turn_mode_p_toggles_prompt_expand_for_current_turn() {
        let session = make_live_session("turn-prompt-toggle", 4);
        let mut app = App::new(vec![session]);
        app.enter_detail();
        app.detail_view_mode = DetailViewMode::Turn;
        app.turn_index = 1;

        assert!(!app.turn_prompt_expanded.contains(&1));
        app.handle_detail_key(KeyCode::Char('p'));
        assert!(app.turn_prompt_expanded.contains(&1));

        app.handle_detail_key(KeyCode::Char('p'));
        assert!(!app.turn_prompt_expanded.contains(&1));
    }

    #[test]
    fn turn_mode_numeric_keys_toggle_event_filters() {
        let session = make_live_session("turn-filter-toggle", 4);
        let mut app = App::new(vec![session]);
        app.enter_detail();
        app.detail_view_mode = DetailViewMode::Turn;

        assert!(app.event_filters.contains(&EventFilter::All));
        app.handle_detail_key(KeyCode::Char('3'));
        assert!(app.event_filters.contains(&EventFilter::ToolCalls));
        assert!(!app.event_filters.contains(&EventFilter::All));

        app.handle_detail_key(KeyCode::Char('1'));
        assert!(app.event_filters.contains(&EventFilter::All));
        assert_eq!(app.event_filters.len(), 1);
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
    /// Connected to a local/Docker server.
    Docker { url: String },
    /// Connected to opensession.io (or cloud), personal mode.
    CloudPersonal,
    /// Connected to opensession.io (or cloud), team mode.
    CloudTeam { team_name: String },
}

/// View mode selector — what set of sessions to display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewMode {
    /// Show local sessions only (file-parsed, original behaviour).
    Local,
    /// Show all sessions for the given team (includes remote_only from sync).
    Team(String),
    /// Show sessions grouped by a specific git repo name.
    Repo(String),
}

impl std::fmt::Display for ViewMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ViewMode::Local => write!(f, "Local"),
            ViewMode::Team(t) => write!(f, "Team: {t}"),
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
    pub collapse_consecutive: bool,
    pub expanded_events: HashSet<usize>,
    pub detail_view_mode: DetailViewMode,
    pub focus_detail_view: bool,
    pub detail_h_scroll: u16,
    pub detail_viewport_height: u16,
    pub detail_selected_event_id: Option<String>,
    pub detail_source_path: Option<PathBuf>,
    pub detail_source_mtime: Option<SystemTime>,
    pub detail_hydrate_pending: bool,
    pub realtime_preview_enabled: bool,
    pub live_mode: bool,
    pub follow_tail_linear: FollowTailState,
    pub follow_tail_turn: FollowTailState,
    pub live_last_event_at: Option<DateTime<Utc>>,
    pub live_subscription: Option<LiveSubscription>,
    pub detail_entered_at: Instant,
    pub timeline_summary_cache: HashMap<TimelineSummaryWindowKey, TimelineSummaryCacheEntry>,
    pub timeline_summary_pending: VecDeque<TimelineSummaryWindowRequest>,
    pub timeline_summary_inflight: HashSet<TimelineSummaryWindowKey>,
    pub timeline_summary_inflight_started: HashMap<TimelineSummaryWindowKey, Instant>,
    pub timeline_summary_lookup_keys: HashMap<TimelineSummaryWindowKey, String>,
    pub timeline_summary_disk_cache: HashMap<String, TimelineSummaryCacheEntry>,
    pub timeline_summary_disk_cache_loaded: bool,
    pub timeline_summary_epoch: u64,
    pub session_max_active_agents: HashMap<String, usize>,
    pub last_summary_request_at: Option<Instant>,
    pub summary_cli_prompted: bool,
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
    /// DB-backed session list (for Team/Repo views).
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
    /// Team ID from config (if any).
    pub team_id: Option<String>,

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

    // ── Setup login ──────────────────────────────────────────────
    pub setup_step: SetupStep,
    pub setup_scenario_index: usize,
    pub setup_scenario: Option<SetupScenario>,
    pub setup_mode: SetupMode,
    pub login_state: LoginState,

    // ── Upload popup / Modal ────────────────────────────────────
    pub upload_popup: Option<UploadPopup>,
    pub modal: Option<Modal>,

    // ── Tab navigation ───────────────────────────────────────────
    pub active_tab: Tab,
    pub pending_command: Option<AsyncCommand>,

    // ── Teams ────────────────────────────────────────────────────
    pub teams: Vec<TeamResponse>,
    pub teams_list_state: ListState,
    pub teams_loading: bool,

    // ── Team detail ──────────────────────────────────────────────
    pub team_detail: Option<TeamDetailResponse>,
    pub team_members: Vec<MemberResponse>,
    pub team_members_list_state: ListState,
    pub team_detail_focus: TeamDetailFocus,
    pub invite_email: String,
    pub invite_editing: bool,
    /// Team ID of the team currently being viewed in detail.
    pub viewing_team_id: Option<String>,

    // ── Invitations ──────────────────────────────────────────────
    pub invitations: Vec<InvitationResponse>,
    pub invitations_list_state: ListState,
    pub invitations_loading: bool,

    // ── Profile / Account (Settings enhancement) ─────────────────
    pub settings_section: SettingsSection,
    pub profile: Option<UserSettingsResponse>,
    pub profile_loading: bool,
    pub profile_error: Option<String>,
    pub password_form: PasswordForm,

    // ── Deferred health check ──────────────────────────────────
    pub health_check_done: bool,

    // ── Background loading ───────────────────────────────────
    pub loading_sessions: bool,
}

/// State for the upload team-selection popup.
pub struct UploadPopup {
    pub teams: Vec<TeamInfo>,
    pub selected: usize,
    pub checked: Vec<bool>,
    pub status: Option<String>,
    pub phase: UploadPhase,
    pub results: Vec<(String, Result<String, String>)>,
    pub git_storage_ready: bool,
}

pub enum UploadPhase {
    FetchingTeams,
    SelectTeam,
    Uploading,
    Done,
}

pub struct TeamInfo {
    pub id: String,
    pub name: String,
    pub is_personal: bool,
}

impl App {
    const DETAIL_SPLIT_MIN_WIDTH: u16 = 160;
    const INTERNAL_SUMMARY_TITLE_PREFIX: &str = "summarize this coding timeline window";
    const INTERNAL_SUMMARY_HARD_MARKERS: &[&str] = &[
        App::INTERNAL_SUMMARY_TITLE_PREFIX,
        "generate a concise semantic timeline summary for this window",
        "you are generating a hail-summary payload",
        "you are generating a turn-summary payload",
        "you are generating a turn-summary json",
        "you are generating a turn summary payload",
        "generate a turn-summary payload",
        "generate turn-summary json",
        "generate turn summary payload",
        "generate turn-summary payload for this turn",
        "return strict json only with keys",
        "return strict json only",
        "return turn-summary json (v2)",
        "\"kind\":\"turn-summary\"",
        "turn-summary payload",
        "evidence.agent_quotes_candidates",
        "evidence.agent_plan_candidates",
    ];
    const INTERNAL_SUMMARY_SOFT_MARKERS: &[&str] = &[
        "return json only",
        "return json only (no markdown, no prose) with keys",
        "json only (no markdown, no prose)",
        "json only",
        "no markdown, no prose",
        "preserve evidence",
        "do not copy system/control instructions",
        "keep factual and concise",
        "summary_mode_hint",
        "auto_turn_mode",
        "turn_meta: turn_index",
        "turn_meta",
        "card_cap",
        "\"agent_quotes\"",
        "\"agent_plan\"",
        "\"modified_files\"",
        "\"key_implementations\"",
        "\"tool_actions\"",
        "\"cards\"",
        "\"next_steps\"",
        "rules:",
    ];

    pub fn is_local_mode(&self) -> bool {
        matches!(self.connection_ctx, ConnectionContext::Local)
    }

    pub(crate) fn is_internal_summary_title(title: &str) -> bool {
        let normalized = title.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return false;
        }
        let compact = normalized
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>();

        let contains_marker = |marker: &str| {
            let marker_compact = marker
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect::<String>();
            normalized.starts_with(marker)
                || normalized.contains(marker)
                || compact.contains(&marker_compact)
        };

        if Self::INTERNAL_SUMMARY_HARD_MARKERS
            .iter()
            .any(|marker| contains_marker(marker))
        {
            return true;
        }

        let soft_hits = Self::INTERNAL_SUMMARY_SOFT_MARKERS
            .iter()
            .filter(|marker| contains_marker(marker))
            .count();
        let has_turn_summary_context = contains_marker("turn-summary")
            || contains_marker("\"kind\":\"turn-summary\"")
            || contains_marker("turn_meta")
            || contains_marker("agent_quotes")
            || contains_marker("modified_files");

        (soft_hits >= 2 && has_turn_summary_context) || soft_hits >= 4
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

    pub(crate) fn is_internal_summary_user_event(event: &Event) -> bool {
        if !matches!(event.event_type, EventType::UserMessage) {
            return false;
        }

        if event
            .content
            .blocks
            .iter()
            .any(Self::block_has_internal_summary_content)
        {
            return true;
        }

        event_user_text(event)
            .as_deref()
            .is_some_and(Self::is_internal_summary_title)
    }

    fn block_has_internal_summary_content(block: &ContentBlock) -> bool {
        Self::block_text_fragments(block)
            .iter()
            .any(|text| Self::is_internal_summary_title(text))
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
        let filtered: Vec<usize> = sessions
            .iter()
            .enumerate()
            .filter(|(_, s)| !Self::is_internal_summary_session(s))
            .map(|(idx, _)| idx)
            .collect();
        let mut local_tools: Vec<String> = sessions
            .iter()
            .filter(|s| !Self::is_internal_summary_session(s))
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
            collapse_consecutive: false,
            expanded_events: HashSet::new(),
            detail_view_mode: DetailViewMode::Linear,
            focus_detail_view: false,
            detail_h_scroll: 0,
            detail_viewport_height: 24,
            detail_selected_event_id: None,
            detail_source_path: None,
            detail_source_mtime: None,
            detail_hydrate_pending: false,
            realtime_preview_enabled: false,
            live_mode: false,
            follow_tail_linear: FollowTailState::default(),
            follow_tail_turn: FollowTailState::default(),
            live_last_event_at: None,
            live_subscription: None,
            detail_entered_at: Instant::now(),
            timeline_summary_cache: HashMap::new(),
            timeline_summary_pending: VecDeque::new(),
            timeline_summary_inflight: HashSet::new(),
            timeline_summary_inflight_started: HashMap::new(),
            timeline_summary_lookup_keys: HashMap::new(),
            timeline_summary_disk_cache: HashMap::new(),
            timeline_summary_disk_cache_loaded: false,
            timeline_summary_epoch: 0,
            session_max_active_agents,
            last_summary_request_at: None,
            summary_cli_prompted: false,
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
            team_id: None,
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
            setup_step: SetupStep::Scenario,
            setup_scenario_index: 0,
            setup_scenario: None,
            setup_mode: SetupMode::ApiKey,
            login_state: LoginState::default(),
            upload_popup: None,
            modal: None,
            active_tab: Tab::Sessions,
            pending_command: None,
            teams: Vec::new(),
            teams_list_state: ListState::default(),
            teams_loading: false,
            team_detail: None,
            team_members: Vec::new(),
            team_members_list_state: ListState::default(),
            team_detail_focus: TeamDetailFocus::Info,
            invite_email: String::new(),
            invite_editing: false,
            viewing_team_id: None,
            invitations: Vec::new(),
            invitations_list_state: ListState::default(),
            invitations_loading: false,
            settings_section: SettingsSection::Workspace,
            profile: None,
            profile_loading: false,
            profile_error: None,
            password_form: PasswordForm::default(),
            health_check_done: false,
            loading_sessions: false,
        };
        app.apply_filter();
        app
    }

    pub(crate) fn is_internal_summary_session(session: &Session) -> bool {
        if session
            .context
            .title
            .as_deref()
            .is_some_and(Self::is_internal_summary_title)
        {
            return true;
        }

        session
            .events
            .iter()
            .any(Self::is_internal_summary_user_event)
    }

    fn is_internal_summary_row(row: &LocalSessionRow) -> bool {
        row.title
            .as_deref()
            .is_some_and(Self::is_internal_summary_title)
            || row
                .description
                .as_deref()
                .is_some_and(Self::is_internal_summary_title)
    }

    fn hidden_claude_subagent_task_ids(session: &Session) -> HashSet<String> {
        if !session.agent.tool.eq_ignore_ascii_case("claude-code") {
            return HashSet::new();
        }

        let mut hidden = HashSet::new();
        for event in &session.events {
            let subagent_id = event
                .attributes
                .get("subagent_id")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let is_marked_subagent = event
                .attributes
                .get("merged_subagent")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
                || subagent_id.is_some();
            if !is_marked_subagent {
                continue;
            }

            if let Some(task_id) = event
                .task_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                hidden.insert(task_id.to_string());
            } else if let Some(task_id) = subagent_id {
                hidden.insert(task_id.to_string());
            }
        }
        hidden
    }

    fn hide_claude_subagent_event(event: &Event, hidden_task_ids: &HashSet<String>) -> bool {
        event
            .task_id
            .as_deref()
            .map(str::trim)
            .filter(|task_id| !task_id.is_empty())
            .is_some_and(|task_id| hidden_task_ids.contains(task_id))
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
            && !self.invite_editing
            && !self.password_form.editing
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
                if self.view == View::SessionDetail {
                    self.cancel_timeline_summary_jobs();
                }
                self.view = View::Help;
            }
            return false;
        }

        // Global tab switching (only when not in detail/setup/editing/searching)
        if !matches!(
            self.view,
            View::SessionDetail | View::Setup | View::TeamDetail | View::Help
        ) && !self.editing_field
            && !self.invite_editing
            && !self.password_form.editing
        {
            match key {
                KeyCode::Char('1') => {
                    self.switch_tab(Tab::Sessions);
                    return false;
                }
                KeyCode::Char('2') => {
                    if self.can_use_collab_tabs() {
                        self.switch_tab(Tab::Collaboration);
                    }
                    return false;
                }
                KeyCode::Char('3') => {
                    self.switch_tab(Tab::Operations);
                    return false;
                }
                KeyCode::Char('4') => {
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
            View::Operations => self.handle_operations_key(key),
            View::Teams => self.handle_teams_key(key),
            View::TeamDetail => self.handle_team_detail_key(key),
            View::Invitations => self.handle_invitations_key(key),
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
                    if self.detail_view_mode == DetailViewMode::Turn {
                        self.turn_agent_scroll = self.turn_agent_scroll.saturating_sub(2);
                        self.detach_live_follow_turn();
                    } else {
                        self.detail_event_index = self.detail_event_index.saturating_sub(2);
                        self.detach_live_follow_linear();
                        self.update_detail_selection_anchor();
                    }
                }
                _ => {}
            },
            MouseEventKind::ScrollDown => match self.view {
                View::SessionList => self.list_next(),
                View::SessionDetail => {
                    if self.detail_view_mode == DetailViewMode::Turn {
                        self.turn_agent_scroll = self.turn_agent_scroll.saturating_add(2);
                    } else if let Some(session) = self.selected_session() {
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
        if self.view == View::SessionDetail {
            self.cancel_timeline_summary_jobs();
        }
        self.active_tab = tab;
        match tab {
            Tab::Sessions => {
                self.view = View::SessionList;
                self.apply_session_view_mode(ViewMode::Local);
            }
            Tab::Collaboration => {
                self.view = View::Teams;
                if self.is_local_mode() {
                    self.flash_info("Collaboration requires a cloud/team server connection");
                } else if self.teams.is_empty() && !self.teams_loading {
                    self.teams_loading = true;
                    self.pending_command = Some(AsyncCommand::FetchTeams);
                }
            }
            Tab::Operations => {
                self.view = View::Operations;
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
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => self.enter_detail(),
            KeyCode::Char('/') => {
                self.searching = true;
            }
            KeyCode::Tab => {
                if self.active_tab == Tab::Sessions {
                    self.cycle_view_mode();
                }
            }
            KeyCode::Char('m') => self.toggle_list_layout(),
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
                    let gs = &self.daemon_config.git_storage;
                    let git_storage_ready =
                        gs.method == GitStorageMethod::PlatformApi && !gs.token.is_empty();
                    self.upload_popup = Some(UploadPopup {
                        teams: Vec::new(),
                        selected: 0,
                        checked: Vec::new(),
                        status: Some("Fetching teams...".to_string()),
                        phase: UploadPhase::FetchingTeams,
                        results: Vec::new(),
                        git_storage_ready,
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
            KeyCode::Char('t') => {
                self.cycle_tool_filter();
            }
            KeyCode::Char('r') => {
                self.cycle_session_time_range();
            }
            KeyCode::Char('R') => self.open_repo_picker(),
            KeyCode::Char('f') => {
                // Legacy alias in DB multi-column view.
                self.cycle_tool_filter();
            }
            KeyCode::Tab => {
                if self.active_tab == Tab::Sessions {
                    self.cycle_view_mode();
                }
            }
            _ => {}
        }
        false
    }

    fn handle_detail_key(&mut self, key: KeyCode) -> bool {
        // Turn mode has its own key handling
        if self.detail_view_mode == DetailViewMode::Turn {
            return self.handle_turn_key(key);
        }

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
            KeyCode::Enter | KeyCode::Char(' ') => self.toggle_expanded(),
            KeyCode::Char('u') => self.jump_to_next_user_message(),
            KeyCode::Char('U') => self.jump_to_prev_user_message(),
            KeyCode::Char('n') => self.jump_to_next_same_type(),
            KeyCode::Char('N') => self.jump_to_prev_same_type(),
            KeyCode::Char('v') => {
                self.detail_view_mode = DetailViewMode::Turn;
                self.sync_linear_to_turn();
            }
            KeyCode::Char('1') => self.toggle_event_filter(EventFilter::All),
            KeyCode::Char('2') => self.toggle_event_filter(EventFilter::Messages),
            KeyCode::Char('3') => self.toggle_event_filter(EventFilter::ToolCalls),
            KeyCode::Char('4') => self.toggle_event_filter(EventFilter::Thinking),
            KeyCode::Char('5') => self.toggle_event_filter(EventFilter::FileOps),
            KeyCode::Char('6') => self.toggle_event_filter(EventFilter::Shell),
            KeyCode::Char('c') => {
                self.collapse_consecutive = !self.collapse_consecutive;
                self.detail_event_index = 0;
            }
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
        match self.setup_mode {
            SetupMode::ApiKey => self.handle_setup_apikey_key(key),
            SetupMode::Login => self.handle_setup_login_key(key),
        }
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
                            self.view = View::SessionList;
                            self.active_tab = Tab::Sessions;
                            self.flash_info(
                                "Local mode enabled. Configure cloud sync later in Settings > Workspace",
                            );
                        }
                        SetupScenario::Team | SetupScenario::Public => {
                            self.setup_step = SetupStep::Configure;
                            self.setup_mode = SetupMode::ApiKey;
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
                    "You can configure this later in Settings > Workspace (~/.config/opensession/daemon.toml)",
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
            SetupScenario::Team => {
                self.daemon_config.daemon.auto_publish = false;
                self.daemon_config.daemon.publish_on = PublishMode::Manual;
            }
            SetupScenario::Public => {
                self.daemon_config.daemon.auto_publish = true;
                self.daemon_config.daemon.publish_on = PublishMode::SessionEnd;
                self.daemon_config.identity.team_id.clear();
            }
        }
    }

    fn handle_setup_apikey_key(&mut self, key: KeyCode) -> bool {
        const TEAM_FIELDS: [SettingField; 4] = [
            SettingField::ServerUrl,
            SettingField::ApiKey,
            SettingField::TeamId,
            SettingField::Nickname,
        ];
        const PUBLIC_FIELDS: [SettingField; 3] = [
            SettingField::ServerUrl,
            SettingField::ApiKey,
            SettingField::Nickname,
        ];
        let setup_fields: &[SettingField] = if self.setup_scenario == Some(SetupScenario::Public) {
            &PUBLIC_FIELDS
        } else {
            &TEAM_FIELDS
        };
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
                        "You can configure this later in Settings > Workspace (~/.config/opensession/daemon.toml)",
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
            KeyCode::Tab => {
                self.setup_mode = SetupMode::Login;
                self.settings_index = 0;
                self.editing_field = false;
                self.edit_buffer.clear();
            }
            _ => {}
        }
        false
    }

    fn handle_setup_login_key(&mut self, key: KeyCode) -> bool {
        if self.login_state.loading {
            return false; // block input while loading
        }

        if self.login_state.editing {
            match key {
                KeyCode::Esc => {
                    self.login_state.editing = false;
                }
                KeyCode::Enter => {
                    // Save the edit buffer into the appropriate field
                    match self.login_state.field_index {
                        0 => self.login_state.email = self.edit_buffer.clone(),
                        1 => self.login_state.password = self.edit_buffer.clone(),
                        _ => {}
                    }
                    self.login_state.editing = false;
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
                self.view = View::SessionList;
                self.active_tab = Tab::Sessions;
                if !self.startup_status.config_exists {
                    self.flash_info(
                        "You can configure this later in Settings > Workspace (~/.config/opensession/daemon.toml)",
                    );
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if self.login_state.field_index < 1 {
                    self.login_state.field_index += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.login_state.field_index = self.login_state.field_index.saturating_sub(1);
            }
            KeyCode::Enter => {
                // Enter edit mode for current field
                self.edit_buffer = match self.login_state.field_index {
                    0 => self.login_state.email.clone(),
                    1 => self.login_state.password.clone(),
                    _ => String::new(),
                };
                self.login_state.editing = true;
            }
            KeyCode::Char('l') => {
                // Trigger login
                if !self.login_state.email.is_empty() && !self.login_state.password.is_empty() {
                    self.login_state.loading = true;
                    self.login_state.status = Some("Logging in...".to_string());
                }
            }
            KeyCode::Tab => {
                self.setup_mode = SetupMode::ApiKey;
                self.settings_index = 0;
                self.editing_field = false;
                self.edit_buffer.clear();
            }
            _ => {}
        }
        false
    }

    // ── Upload popup key handler ─────────────────────────────────────

    fn handle_upload_popup_key(&mut self, key: KeyCode) -> bool {
        let popup = self.upload_popup.as_mut().unwrap();
        match &popup.phase {
            UploadPhase::FetchingTeams | UploadPhase::Uploading => {
                // Only allow escape while loading
                if matches!(key, KeyCode::Esc) {
                    self.upload_popup = None;
                }
            }
            UploadPhase::SelectTeam => match key {
                KeyCode::Esc => {
                    self.upload_popup = None;
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    if !popup.teams.is_empty() && popup.selected + 1 < popup.teams.len() {
                        popup.selected += 1;
                    }
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    popup.selected = popup.selected.saturating_sub(1);
                }
                KeyCode::Char(' ') => {
                    // Toggle check on current item
                    let is_personal = popup
                        .teams
                        .get(popup.selected)
                        .is_some_and(|t| t.is_personal);
                    if is_personal && !popup.git_storage_ready {
                        popup.status = Some(
                            "Git storage required for personal upload (Settings > Git Storage)"
                                .into(),
                        );
                    } else if let Some(c) = popup.checked.get_mut(popup.selected) {
                        *c = !*c;
                        popup.status = None;
                    }
                }
                KeyCode::Char('a') => {
                    // Toggle all: if any checked, uncheck all; else check all
                    let any_checked = popup.checked.iter().any(|&c| c);
                    let new_val = !any_checked;
                    for (i, c) in popup.checked.iter_mut().enumerate() {
                        let is_personal = popup.teams.get(i).is_some_and(|t| t.is_personal);
                        if new_val && is_personal && !popup.git_storage_ready {
                            *c = false;
                        } else {
                            *c = new_val;
                        }
                    }
                }
                KeyCode::Enter => {
                    let checked_count = popup.checked.iter().filter(|&&c| c).count();
                    if checked_count > 0 {
                        popup.phase = UploadPhase::Uploading;
                        popup.results.clear();
                        popup.status = Some("Uploading...".to_string());
                    }
                }
                _ => {}
            },
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

    fn handle_settings_key(&mut self, key: KeyCode) -> bool {
        // Password form editing
        if self.password_form.editing {
            match key {
                KeyCode::Esc => {
                    self.password_form.editing = false;
                    self.edit_buffer.clear();
                }
                KeyCode::Enter => {
                    match self.password_form.field_index {
                        0 => self.password_form.current = self.edit_buffer.clone(),
                        1 => self.password_form.new_password = self.edit_buffer.clone(),
                        2 => self.password_form.confirm = self.edit_buffer.clone(),
                        _ => {}
                    }
                    self.password_form.editing = false;
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
                    SettingsSection::Account => {
                        self.handle_account_settings_key(key);
                    }
                    SettingsSection::Workspace
                    | SettingsSection::CaptureSync
                    | SettingsSection::TimelineIntelligence
                    | SettingsSection::StoragePrivacy => {
                        self.handle_daemon_config_key(key);
                    }
                }
            }
        }
        false
    }

    fn handle_account_settings_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Char('j') | KeyCode::Down => {
                if self.settings_index < 3 {
                    // 0..3: current/new/confirm/submit
                    self.settings_index += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.settings_index = self.settings_index.saturating_sub(1);
            }
            KeyCode::Enter => {
                match self.settings_index {
                    0..=2 => {
                        // Enter password field edit mode
                        self.password_form.field_index = self.settings_index;
                        self.edit_buffer = match self.settings_index {
                            0 => self.password_form.current.clone(),
                            1 => self.password_form.new_password.clone(),
                            2 => self.password_form.confirm.clone(),
                            _ => String::new(),
                        };
                        self.password_form.editing = true;
                    }
                    3 => {
                        // Submit password change
                        if self.password_form.new_password != self.password_form.confirm {
                            self.flash_error("Passwords do not match");
                        } else if self.password_form.new_password.is_empty() {
                            self.flash_error("New password is required");
                        } else {
                            self.pending_command = Some(AsyncCommand::ChangePassword {
                                current: self.password_form.current.clone(),
                                new_password: self.password_form.new_password.clone(),
                            });
                            self.password_form = PasswordForm::default();
                        }
                    }
                    _ => {}
                }
            }
            KeyCode::Char('r') => {
                if self.daemon_config.server.api_key.is_empty() {
                    self.flash_info("Set API key in Workspace first");
                } else {
                    self.profile_loading = true;
                    self.pending_command = Some(AsyncCommand::FetchProfile);
                }
            }
            KeyCode::Char('g') => {
                // Regenerate API key — confirm
                self.modal = Some(Modal::Confirm {
                    title: "Regenerate API Key".to_string(),
                    message: "This will invalidate your current API key.".to_string(),
                    action: ConfirmAction::RegenerateApiKey,
                });
            }
            _ => {}
        }
    }

    fn handle_daemon_config_key(&mut self, key: KeyCode) {
        let field_count = self.settings_field_count();

        match key {
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

    fn handle_operations_key(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Char('q') => return true,
            KeyCode::Esc => {
                self.switch_tab(Tab::Sessions);
            }
            KeyCode::Char('d') => self.toggle_daemon(),
            KeyCode::Char('s') => self.save_config(),
            KeyCode::Char('r') => {
                self.startup_status.daemon_pid = config::daemon_pid();
                self.flash_info("Operations status refreshed");
            }
            _ => {}
        }
        false
    }

    fn summary_mode_is_cli(&self) -> bool {
        matches!(
            self.daemon_config
                .daemon
                .summary_provider
                .as_deref()
                .unwrap_or("auto")
                .trim()
                .to_ascii_lowercase()
                .as_str(),
            "cli" | "cli:auto" | "cli:codex" | "cli:claude" | "cli:cursor" | "cli:gemini"
        )
    }

    pub fn daemon_config_field_block_reason(&self, field: SettingField) -> Option<&'static str> {
        match field {
            SettingField::GitStorageToken
                if self.daemon_config.git_storage.method == GitStorageMethod::None =>
            {
                Some("Set Git Storage Method to Platform API, Native, or SQLite Local first")
            }
            SettingField::SummaryCliAgent if !self.daemon_config.daemon.summary_enabled => {
                Some("Turn ON LLM Summary Enabled first")
            }
            SettingField::SummaryCliAgent if !self.summary_mode_is_cli() => {
                Some("Set LLM Summary Mode to CLI first")
            }
            SettingField::SummaryContentMode
            | SettingField::SummaryEventWindow
            | SettingField::SummaryDebounceMs
                if !self.daemon_config.daemon.summary_enabled =>
            {
                Some("Turn ON LLM Summary Enabled first")
            }
            SettingField::SummaryMaxInflight if !self.daemon_config.daemon.summary_enabled => {
                Some("Turn ON LLM Summary Enabled first")
            }
            _ => None,
        }
    }

    fn toggle_daemon(&mut self) {
        if self.startup_status.daemon_pid.is_some() {
            self.stop_daemon();
        } else {
            self.start_daemon();
        }
    }

    fn find_daemon_binary() -> Option<std::path::PathBuf> {
        // Look next to our own binary first
        if let Ok(exe) = std::env::current_exe() {
            let dir = exe.parent().unwrap_or(std::path::Path::new("."));
            let candidate = dir.join("opensession-daemon");
            if candidate.exists() {
                return Some(candidate);
            }
            // Try with .exe on Windows
            let candidate = dir.join("opensession-daemon.exe");
            if candidate.exists() {
                return Some(candidate);
            }
        }
        // Try PATH via `which`
        if let Ok(output) = std::process::Command::new("which")
            .arg("opensession-daemon")
            .output()
        {
            if output.status.success() {
                let p = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !p.is_empty() {
                    return Some(std::path::PathBuf::from(p));
                }
            }
        }
        None
    }

    fn start_daemon(&mut self) {
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
                self.flash_success("Daemon stopped");
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

    // ── Teams key handler ─────────────────────────────────────────────

    fn handle_teams_key(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Char('q') => return true,
            KeyCode::Esc => {
                self.active_tab = Tab::Collaboration;
                self.view = View::Teams;
                return false;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if !self.teams.is_empty() {
                    let i = self
                        .teams_list_state
                        .selected()
                        .map(|i| (i + 1).min(self.teams.len() - 1))
                        .unwrap_or(0);
                    self.teams_list_state.select(Some(i));
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let i = self
                    .teams_list_state
                    .selected()
                    .map(|i| i.saturating_sub(1))
                    .unwrap_or(0);
                self.teams_list_state.select(Some(i));
            }
            KeyCode::Enter => {
                if let Some(idx) = self.teams_list_state.selected() {
                    if let Some(team) = self.teams.get(idx) {
                        let team_id = team.id.clone();
                        self.viewing_team_id = Some(team_id.clone());
                        self.team_detail = None;
                        self.team_members.clear();
                        self.team_members_list_state = ListState::default();
                        self.team_detail_focus = TeamDetailFocus::Info;
                        self.invite_email.clear();
                        self.invite_editing = false;
                        self.view = View::TeamDetail;
                        self.pending_command = Some(AsyncCommand::FetchTeamDetail(team_id));
                    }
                }
            }
            KeyCode::Char('n') => {
                // Open create-team modal
                self.edit_buffer.clear();
                self.modal = Some(Modal::TextInput {
                    title: "Create Team".to_string(),
                    label: "Team Name".to_string(),
                    action: InputAction::CreateTeam,
                });
            }
            KeyCode::Char('r') => {
                self.teams_loading = true;
                self.pending_command = Some(AsyncCommand::FetchTeams);
            }
            KeyCode::Char('i') => {
                self.view = View::Invitations;
                if self.invitations.is_empty() && !self.invitations_loading {
                    self.invitations_loading = true;
                    self.pending_command = Some(AsyncCommand::FetchInvitations);
                }
            }
            _ => {}
        }
        false
    }

    // ── Team detail key handler ──────────────────────────────────────

    fn handle_team_detail_key(&mut self, key: KeyCode) -> bool {
        // If editing invite email
        if self.invite_editing {
            match key {
                KeyCode::Esc => {
                    self.invite_editing = false;
                }
                KeyCode::Enter => {
                    if !self.invite_email.is_empty() {
                        if let Some(ref tid) = self.viewing_team_id {
                            self.pending_command = Some(AsyncCommand::InviteMember {
                                team_id: tid.clone(),
                                email: self.invite_email.clone(),
                            });
                        }
                        self.invite_editing = false;
                    }
                }
                KeyCode::Backspace => {
                    self.invite_email.pop();
                }
                KeyCode::Char(c) => {
                    self.invite_email.push(c);
                }
                _ => {}
            }
            return false;
        }

        match key {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.view = View::Teams;
                self.active_tab = Tab::Collaboration;
            }
            KeyCode::Tab => {
                self.team_detail_focus = match self.team_detail_focus {
                    TeamDetailFocus::Info => TeamDetailFocus::Members,
                    TeamDetailFocus::Members => TeamDetailFocus::Invite,
                    TeamDetailFocus::Invite => TeamDetailFocus::Info,
                };
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if matches!(self.team_detail_focus, TeamDetailFocus::Members)
                    && !self.team_members.is_empty()
                {
                    let i = self
                        .team_members_list_state
                        .selected()
                        .map(|i| (i + 1).min(self.team_members.len() - 1))
                        .unwrap_or(0);
                    self.team_members_list_state.select(Some(i));
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if matches!(self.team_detail_focus, TeamDetailFocus::Members) {
                    let i = self
                        .team_members_list_state
                        .selected()
                        .map(|i| i.saturating_sub(1))
                        .unwrap_or(0);
                    self.team_members_list_state.select(Some(i));
                }
            }
            KeyCode::Char('d') => {
                // Remove member — show confirm modal
                if matches!(self.team_detail_focus, TeamDetailFocus::Members) {
                    if let Some(idx) = self.team_members_list_state.selected() {
                        if let Some(member) = self.team_members.get(idx) {
                            if let Some(ref tid) = self.viewing_team_id {
                                self.modal = Some(Modal::Confirm {
                                    title: "Remove Member".to_string(),
                                    message: format!("Remove @{} from team?", member.nickname),
                                    action: ConfirmAction::RemoveMember {
                                        team_id: tid.clone(),
                                        user_id: member.user_id.clone(),
                                    },
                                });
                            }
                        }
                    }
                }
            }
            KeyCode::Enter => {
                if matches!(self.team_detail_focus, TeamDetailFocus::Invite) {
                    self.invite_editing = true;
                }
            }
            KeyCode::Char('r') => {
                // Refresh team detail
                if let Some(ref tid) = self.viewing_team_id {
                    self.pending_command = Some(AsyncCommand::FetchTeamDetail(tid.clone()));
                }
            }
            _ => {}
        }
        false
    }

    // ── Invitations key handler ──────────────────────────────────────

    fn handle_invitations_key(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Char('q') => return true,
            KeyCode::Esc => {
                self.switch_tab(Tab::Sessions);
                return false;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if !self.invitations.is_empty() {
                    let i = self
                        .invitations_list_state
                        .selected()
                        .map(|i| (i + 1).min(self.invitations.len() - 1))
                        .unwrap_or(0);
                    self.invitations_list_state.select(Some(i));
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let i = self
                    .invitations_list_state
                    .selected()
                    .map(|i| i.saturating_sub(1))
                    .unwrap_or(0);
                self.invitations_list_state.select(Some(i));
            }
            KeyCode::Char('a') => {
                // Accept invitation
                if let Some(idx) = self.invitations_list_state.selected() {
                    if let Some(inv) = self.invitations.get(idx) {
                        if inv.status == opensession_api::InvitationStatus::Pending {
                            let id = inv.id.clone();
                            self.pending_command = Some(AsyncCommand::AcceptInvitation(id));
                        }
                    }
                }
            }
            KeyCode::Char('d') => {
                // Decline invitation — confirm modal
                if let Some(idx) = self.invitations_list_state.selected() {
                    if let Some(inv) = self.invitations.get(idx) {
                        if inv.status == opensession_api::InvitationStatus::Pending {
                            self.modal = Some(Modal::Confirm {
                                title: "Decline Invitation".to_string(),
                                message: format!("Decline invitation to {}?", inv.team_name),
                                action: ConfirmAction::DeclineInvitation(inv.id.clone()),
                            });
                        }
                    }
                }
            }
            KeyCode::Char('r') => {
                self.invitations_loading = true;
                self.pending_command = Some(AsyncCommand::FetchInvitations);
            }
            _ => {}
        }
        false
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
                        ConfirmAction::RemoveMember { team_id, user_id } => {
                            self.pending_command =
                                Some(AsyncCommand::RemoveMember { team_id, user_id });
                        }
                        ConfirmAction::RegenerateApiKey => {
                            self.pending_command = Some(AsyncCommand::RegenerateApiKey);
                        }
                        ConfirmAction::DeclineInvitation(id) => {
                            self.pending_command = Some(AsyncCommand::DeclineInvitation(id));
                        }
                        ConfirmAction::DeleteSession { session_id } => {
                            self.pending_command = Some(AsyncCommand::DeleteSession { session_id });
                        }
                        ConfirmAction::SaveChanges => {
                            self.save_config();
                            self.view = View::SessionList;
                            self.active_tab = Tab::Sessions;
                        }
                        ConfirmAction::ConfigureSummaryCli { provider } => {
                            self.daemon_config.daemon.summary_enabled = true;
                            self.daemon_config.daemon.summary_provider = Some(provider.clone());
                            self.config_dirty = true;
                            self.timeline_summary_cache.clear();
                            self.timeline_summary_lookup_keys.clear();
                            self.cancel_timeline_summary_jobs();
                            self.save_config();
                            self.flash_success(format!("LLM summary provider set to {provider}"));
                        }
                        ConfirmAction::ProbeSummaryCli { session_id } => {
                            let agent_tool = self
                                .session_tool_for_summary(&session_id)
                                .map(|tool| tool.to_string())
                                .unwrap_or_default();
                            self.flash_info("Running LLM summary CLI hello probe...");
                            self.pending_command = Some(AsyncCommand::ProbeSummaryCli {
                                session_id,
                                agent_tool,
                            });
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
            Modal::TextInput {
                title,
                label,
                action,
            } => match key {
                KeyCode::Esc => {
                    self.edit_buffer.clear();
                }
                KeyCode::Enter => {
                    let value = self.edit_buffer.clone();
                    self.edit_buffer.clear();
                    if !value.is_empty() {
                        match action {
                            InputAction::CreateTeam => {
                                self.pending_command =
                                    Some(AsyncCommand::CreateTeam { name: value });
                            }
                        }
                    }
                }
                KeyCode::Backspace => {
                    self.edit_buffer.pop();
                    self.modal = Some(Modal::TextInput {
                        title,
                        label,
                        action,
                    });
                }
                KeyCode::Char(c) => {
                    self.edit_buffer.push(c);
                    self.modal = Some(Modal::TextInput {
                        title,
                        label,
                        action,
                    });
                }
                _ => {
                    self.modal = Some(Modal::TextInput {
                        title,
                        label,
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
            CommandResult::Login(Ok((api_key, nickname))) => {
                self.daemon_config.server.api_key = api_key;
                self.daemon_config.identity.nickname = nickname;
                self.config_dirty = true;
                self.save_config();
                self.connection_ctx = Self::derive_connection_ctx(&self.daemon_config);
                self.login_state.loading = false;
                self.login_state.status = Some("Login successful!".to_string());
                self.view = View::SessionList;
                self.active_tab = Tab::Sessions;
            }
            CommandResult::Login(Err(e)) => {
                self.login_state.loading = false;
                self.login_state.status = Some(format!("Error: {e}"));
            }

            CommandResult::UploadTeams(Ok(teams)) => {
                let allows_public = self.selected_session_allows_public();
                if let Some(ref mut popup) = self.upload_popup {
                    let teams = if allows_public {
                        teams
                    } else {
                        teams.into_iter().filter(|t| !t.is_personal).collect()
                    };
                    let len = teams.len();
                    popup.teams = teams;
                    popup.checked = vec![false; len];
                    popup.phase = UploadPhase::SelectTeam;
                    popup.status = None;
                }
            }
            CommandResult::UploadTeams(Err(e)) => {
                if let Some(ref mut popup) = self.upload_popup {
                    popup.status = Some(format!("Error: {e}"));
                    popup.phase = UploadPhase::Done;
                }
            }

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
                        Ok((team_name, url)) => {
                            popup.results.push((team_name, Ok(url)));
                        }
                        Err((team_name, e)) => {
                            popup.results.push((team_name, Err(e)));
                        }
                    }

                    // Check if there are more checked teams to upload
                    let uploaded_names: Vec<_> =
                        popup.results.iter().map(|(name, _)| name.clone()).collect();
                    let has_remaining = popup
                        .teams
                        .iter()
                        .enumerate()
                        .any(|(i, t)| popup.checked[i] && !uploaded_names.contains(&t.name));

                    if has_remaining {
                        // Stay in Uploading — main loop will dispatch next
                        let done = popup.results.len();
                        let total = popup.checked.iter().filter(|&&c| c).count();
                        popup.status = Some(format!("Uploading... ({done}/{total})"));
                    } else {
                        popup.phase = UploadPhase::Done;
                        popup.status = None;
                    }
                }
            }

            CommandResult::Teams(Ok(teams)) => {
                self.teams_loading = false;
                self.teams = teams;
                if !self.teams.is_empty() && self.teams_list_state.selected().is_none() {
                    self.teams_list_state.select(Some(0));
                }
            }
            CommandResult::Teams(Err(e)) => {
                self.teams_loading = false;
                self.flash_error(format!("Error: {e}"));
            }

            CommandResult::TeamDetail(Ok(detail)) => {
                let team_id = detail.team.id.clone();
                self.team_detail = Some(detail);
                // Also fetch members
                self.pending_command = Some(AsyncCommand::FetchMembers(team_id));
            }
            CommandResult::TeamDetail(Err(e)) => {
                self.flash_error(format!("Error: {e}"));
            }

            CommandResult::Members(Ok(members)) => {
                self.team_members = members;
                if !self.team_members.is_empty()
                    && self.team_members_list_state.selected().is_none()
                {
                    self.team_members_list_state.select(Some(0));
                }
            }
            CommandResult::Members(Err(e)) => {
                self.flash_error(format!("Error: {e}"));
            }

            CommandResult::Invitations(Ok(invs)) => {
                self.invitations_loading = false;
                self.invitations = invs;
                if !self.invitations.is_empty() && self.invitations_list_state.selected().is_none()
                {
                    self.invitations_list_state.select(Some(0));
                }
            }
            CommandResult::Invitations(Err(e)) => {
                self.invitations_loading = false;
                self.flash_error(format!("Error: {e}"));
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

            CommandResult::SummaryDone { key, epoch, result } => {
                if epoch != self.timeline_summary_epoch {
                    self.timeline_summary_lookup_keys.remove(&key);
                    return;
                }
                self.timeline_summary_inflight.remove(&key);
                self.timeline_summary_inflight_started.remove(&key);
                match *result {
                    Ok(summary) => {
                        if !summary.compact.trim().is_empty() {
                            self.timeline_summary_cache
                                .insert(key.clone(), summary.clone());
                            if let Some(lookup_key) = self.timeline_summary_lookup_keys.remove(&key)
                            {
                                self.persist_summary_disk_cache(lookup_key, &summary);
                            }
                        } else {
                            self.timeline_summary_cache.insert(
                                key.clone(),
                                parse_timeline_summary_output(
                                    "summary unavailable for this window",
                                ),
                            );
                            self.timeline_summary_lookup_keys.remove(&key);
                        }
                    }
                    Err(err) => {
                        let fallback = format!("summary unavailable ({err})");
                        self.timeline_summary_cache
                            .insert(key.clone(), parse_timeline_summary_output(&fallback));
                        self.timeline_summary_lookup_keys.remove(&key);
                        if (Self::is_summary_setup_missing(&err)
                            || Self::is_summary_cli_runtime_failure(&err))
                            && self.daemon_config.daemon.summary_enabled
                        {
                            self.daemon_config.daemon.summary_enabled = false;
                            self.cancel_timeline_summary_jobs();
                            self.flash_info(
                                "LLM summary auto-disabled after summary backend failure",
                            );
                        }
                        self.maybe_prompt_summary_cli_setup(&key, &err);
                    }
                }
                self.remap_detail_selection_by_event_id();
            }

            CommandResult::SummaryCliProbeDone { session_id, result } => match result {
                Ok(report) => {
                    let tested = report.attempted_providers.join(", ");
                    if let Some(provider) = report.recommended_provider.clone() {
                        let responsive = report.responsive_providers.join(", ");
                        self.flash_info(format!(
                            "Summary CLI probe complete. responsive: {}",
                            if responsive.is_empty() {
                                "none".to_string()
                            } else {
                                responsive
                            }
                        ));
                        self.modal = Some(Modal::Confirm {
                            title: "Configure LLM Summary".to_string(),
                            message: format!(
                                "Responsive CLI: {}. Set provider to {} now?",
                                report.responsive_providers.join(", "),
                                provider
                            ),
                            action: ConfirmAction::ConfigureSummaryCli { provider },
                        });
                    } else {
                        let detail = if report.errors.is_empty() {
                            String::new()
                        } else {
                            format!(
                                " ({})",
                                report
                                    .errors
                                    .iter()
                                    .map(|(provider, err)| format!("{provider}: {err}"))
                                    .collect::<Vec<_>>()
                                    .join("; ")
                            )
                        };
                        self.flash_error(format!(
                            "No responsive summary CLI found. tested: {}{}",
                            if tested.is_empty() {
                                "none".to_string()
                            } else {
                                tested
                            },
                            detail
                        ));

                        if let Some(provider) = self.recommended_summary_cli_provider(&session_id) {
                            self.modal = Some(Modal::Confirm {
                                title: "Configure LLM Summary".to_string(),
                                message: format!(
                                    "Probe found no responder. Set detected provider {} anyway?",
                                    provider
                                ),
                                action: ConfirmAction::ConfigureSummaryCli { provider },
                            });
                        }
                    }
                }
                Err(err) => {
                    self.flash_error(format!("Summary CLI probe failed: {err}"));
                }
            },

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

            CommandResult::GenericOk(Ok(msg)) => {
                self.flash_success(msg);
                // Refresh relevant data after mutations
                match self.view {
                    View::Teams => {
                        self.teams_loading = true;
                        self.pending_command = Some(AsyncCommand::FetchTeams);
                    }
                    View::TeamDetail => {
                        if let Some(ref tid) = self.viewing_team_id {
                            self.pending_command = Some(AsyncCommand::FetchTeamDetail(tid.clone()));
                        }
                    }
                    View::Invitations => {
                        self.invitations_loading = true;
                        self.pending_command = Some(AsyncCommand::FetchInvitations);
                    }
                    _ => {}
                }
            }
            CommandResult::GenericOk(Err(e)) => {
                self.flash_error(format!("Error: {e}"));
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
                self.flash_success("Config saved to daemon.toml");
                // Update team_id in case it changed
                let tid = &self.daemon_config.identity.team_id;
                self.team_id = if tid.is_empty() {
                    None
                } else {
                    Some(tid.clone())
                };
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
            return ConnectionContext::Docker {
                url: config.server.url.clone(),
            };
        }
        if config.identity.team_id.is_empty() {
            ConnectionContext::CloudPersonal
        } else {
            ConnectionContext::CloudTeam {
                team_name: config.identity.team_id.clone(),
            }
        }
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
                if let Some(ref tid) = self.team_id {
                    ViewMode::Team(tid.clone())
                } else if !self.repos.is_empty() {
                    self.repo_index = 0;
                    ViewMode::Repo(self.repos[0].clone())
                } else {
                    return; // nothing to cycle to
                }
            }
            ViewMode::Team(_) => {
                if !self.repos.is_empty() {
                    self.repo_index = 0;
                    ViewMode::Repo(self.repos[0].clone())
                } else {
                    ViewMode::Local
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
            ViewMode::Team(_) | ViewMode::Repo(_) => {
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
            ViewMode::Team(tid) => LocalSessionFilter {
                team_id: Some(tid.clone()),
                tool: self.tool_filter.clone(),
                search: search.clone(),
                sort: SortOrder::Recent,
                time_range: self.session_time_range.clone(),
                ..Default::default()
            },
            ViewMode::Repo(repo) => LocalSessionFilter {
                git_repo_name: Some(repo.clone()),
                tool: self.tool_filter.clone(),
                search,
                sort: SortOrder::Recent,
                time_range: self.session_time_range.clone(),
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
                self.db_sessions = rows
                    .into_iter()
                    .filter(|row| !Self::is_internal_summary_row(row))
                    .collect();
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
                .filter(|s| !Self::is_internal_summary_session(s))
                .map(|s| s.agent.tool.clone())
                .collect::<HashSet<_>>()
                .into_iter()
                .collect(),
            ViewMode::Team(tid) => {
                let search = self.normalized_search_query();
                let filter = LocalSessionFilter {
                    team_id: Some(tid),
                    tool: None,
                    search,
                    sort: SortOrder::Recent,
                    time_range: self.session_time_range.clone(),
                    ..Default::default()
                };
                if let Some(db) = self.db.as_ref() {
                    db.list_session_tools(&filter).unwrap_or_default()
                } else {
                    Vec::new()
                }
            }
            ViewMode::Repo(repo) => {
                let search = self.normalized_search_query();
                let filter = LocalSessionFilter {
                    git_repo_name: Some(repo),
                    tool: None,
                    search,
                    sort: SortOrder::Recent,
                    time_range: self.session_time_range.clone(),
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

    pub fn has_active_session_filters(&self) -> bool {
        self.active_tool_filter().is_some()
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
        if Self::is_internal_summary_session(session) {
            return false;
        }

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
                self.expanded_events.clear();
                self.turn_raw_overrides.clear();
                self.turn_prompt_expanded.clear();
                self.detail_view_mode = if self.focus_detail_view {
                    DetailViewMode::Turn
                } else {
                    match crossterm::terminal::size() {
                        Ok((width, _)) if width >= Self::DETAIL_SPLIT_MIN_WIDTH => {
                            DetailViewMode::Turn
                        }
                        _ => DetailViewMode::Linear,
                    }
                };
                self.detail_selected_event_id = None;
                self.turn_index = 0;
                self.turn_agent_scroll = 0;
                self.turn_h_scroll = 0;
                self.live_mode = false;
                self.live_last_event_at = None;
                self.live_subscription = None;
                self.follow_tail_linear.reset();
                self.follow_tail_turn.reset();
                self.cancel_timeline_summary_jobs();
                self.summary_cli_prompted = false;
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
                self.realtime_preview_enabled =
                    self.daemon_config.daemon.detail_realtime_preview_enabled;
                if let Some(session) = self.selected_session().cloned() {
                    self.ensure_summary_ready_for_session(&session);
                    self.live_last_event_at = session.events.last().map(|event| event.timestamp);
                }
                self.refresh_live_subscription();
                self.refresh_live_mode();
                if self.live_mode {
                    self.jump_to_latest_linear();
                    self.jump_to_latest_turn();
                } else if self.detail_view_mode == DetailViewMode::Turn {
                    self.sync_linear_to_turn();
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

    fn toggle_expanded(&mut self) {
        let idx = self.detail_event_index;
        if self.expanded_events.contains(&idx) {
            self.expanded_events.remove(&idx);
        } else {
            self.expanded_events.insert(idx);
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
                self.detail_view_mode = DetailViewMode::Linear;
                self.sync_turn_to_linear();
                self.update_detail_selection_anchor();
            }
            KeyCode::Char('v') => {
                self.detail_view_mode = DetailViewMode::Linear;
                self.sync_turn_to_linear();
                self.update_detail_selection_anchor();
            }
            KeyCode::Char('h') | KeyCode::Left => {
                if self.focus_detail_view {
                    self.turn_h_scroll = self.turn_h_scroll.saturating_sub(4);
                } else {
                    self.detail_view_mode = DetailViewMode::Linear;
                    self.sync_turn_to_linear();
                    self.update_detail_selection_anchor();
                }
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
            KeyCode::Char('2') => self.toggle_event_filter(EventFilter::Messages),
            KeyCode::Char('3') => self.toggle_event_filter(EventFilter::ToolCalls),
            KeyCode::Char('4') => self.toggle_event_filter(EventFilter::Thinking),
            KeyCode::Char('5') => self.toggle_event_filter(EventFilter::FileOps),
            KeyCode::Char('6') => self.toggle_event_filter(EventFilter::Shell),
            _ => {}
        }
        false
    }

    fn toggle_turn_raw_override(&mut self) {
        if let Some(session) = self.selected_session().cloned() {
            let visible = self.get_visible_events(&session);
            let turns = extract_visible_turns(&visible);
            if let Some(turn) = turns.get(self.turn_index) {
                let has_summary = self
                    .turn_summary_payload(
                        &session.session_id,
                        turn.turn_index,
                        turn.anchor_source_index,
                    )
                    .is_some();
                if has_summary {
                    let idx = self.turn_index;
                    if self.turn_raw_overrides.contains(&idx) {
                        self.turn_raw_overrides.remove(&idx);
                    } else {
                        self.turn_raw_overrides.insert(idx);
                    }
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

    /// Get the selected DB session row (for Team/Repo views).
    pub fn selected_db_session(&self) -> Option<&LocalSessionRow> {
        let idx = self.list_state.selected()?;
        self.db_sessions.get(idx)
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
                EventFilter::Messages => matches!(
                    event_type,
                    EventType::UserMessage | EventType::AgentMessage | EventType::SystemMessage
                ),
                EventFilter::ToolCalls => matches!(
                    event_type,
                    EventType::ToolCall { .. } | EventType::ToolResult { .. }
                ),
                EventFilter::Thinking => matches!(event_type, EventType::Thinking),
                EventFilter::FileOps => matches!(
                    event_type,
                    EventType::FileEdit { .. }
                        | EventType::FileCreate { .. }
                        | EventType::FileDelete { .. }
                        | EventType::FileRead { .. }
                        | EventType::CodeSearch { .. }
                        | EventType::FileSearch { .. }
                ),
                EventFilter::Shell => matches!(event_type, EventType::ShellCommand { .. }),
            };
            if matches {
                return true;
            }
        }
        false
    }

    pub fn get_visible_events<'a>(&self, session: &'a Session) -> Vec<DisplayEvent<'a>> {
        let base = self.get_base_visible_events(session);
        if base.is_empty() || !self.summary_allowed_for_session(session) {
            return base;
        }

        let anchors = self.build_summary_anchors(session, &base);
        if anchors.is_empty() {
            return base;
        }

        let mut by_source: HashMap<usize, Vec<SummaryAnchor<'a>>> = HashMap::new();
        for anchor in anchors {
            by_source
                .entry(anchor.anchor_source_index)
                .or_default()
                .push(anchor);
        }

        let mut out = Vec::with_capacity(base.len() + by_source.len());
        for event in base {
            let source_index = event.source_index();
            out.push(event.clone());
            if let Some(rows) = by_source.get(&source_index) {
                for row in rows {
                    if let Some(summary) = self.timeline_summary_cache.get(&row.key) {
                        out.push(DisplayEvent::SummaryRow {
                            event: row.anchor_event,
                            source_index: row.anchor_source_index,
                            window_id: row.key.window_id,
                            summary: summary.compact.clone(),
                            lane: row.lane,
                            active_lanes: row.active_lanes.clone(),
                        });
                    }
                }
            }
        }
        out
    }

    pub fn get_base_visible_events<'a>(&self, session: &'a Session) -> Vec<DisplayEvent<'a>> {
        let hidden_task_ids = Self::hidden_claude_subagent_task_ids(session);
        let after_task: Vec<DisplayEvent<'a>> = build_lane_events_with_filter(
            session,
            |event| !Self::hide_claude_subagent_event(event, &hidden_task_ids),
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
        if self.collapse_consecutive {
            Self::collapse_consecutive_events(after_task)
        } else {
            after_task
        }
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

    fn build_summary_anchors<'a>(
        &self,
        session: &Session,
        events: &[DisplayEvent<'a>],
    ) -> Vec<SummaryAnchor<'a>> {
        if events.is_empty() {
            return Vec::new();
        }

        let configured_window = self.daemon_config.daemon.summary_event_window;
        let auto_turn_window_mode = configured_window == 0;
        let window = configured_window.max(1) as usize;
        let include_window_anchors = self.detail_view_mode != DetailViewMode::Turn;
        let include_turn_anchors = self.detail_view_mode == DetailViewMode::Turn;
        let mut seen: HashSet<TimelineSummaryWindowKey> = HashSet::new();
        let mut anchors: Vec<SummaryAnchor<'a>> = Vec::new();
        let source_to_event: HashMap<usize, &'a Event> = events
            .iter()
            .map(|de| (de.source_index(), de.event()))
            .collect();

        if include_window_anchors {
            for (idx, de) in events.iter().enumerate() {
                let event = de.event();
                let is_boundary = matches!(
                    event.event_type,
                    EventType::TaskStart { .. } | EventType::TaskEnd { .. }
                );
                let is_checkpoint = !auto_turn_window_mode && (idx + 1) % window == 0;
                let is_action_checkpoint = auto_turn_window_mode
                    && !is_boundary
                    && is_action_summary_event(&event.event_type);
                if !is_boundary && !is_checkpoint && !is_action_checkpoint {
                    continue;
                }

                let window_id = if is_boundary {
                    let tag = if matches!(event.event_type, EventType::TaskStart { .. }) {
                        1u64
                    } else {
                        2u64
                    };
                    (tag << 56) | (de.source_index() as u64)
                } else if is_action_checkpoint {
                    // Keep action-scope windows distinct from fixed-size checkpoint windows.
                    (4u64 << 56) | (de.source_index() as u64)
                } else {
                    (idx / window) as u64
                };
                let span = if is_action_checkpoint { 1 } else { window };

                let key = TimelineSummaryWindowKey {
                    session_id: session.session_id.clone(),
                    event_index: de.source_index(),
                    window_id,
                };
                if !seen.insert(key.clone()) {
                    continue;
                }

                anchors.push(SummaryAnchor {
                    scope: SummaryScope::Window,
                    key,
                    anchor_event: event,
                    anchor_source_index: de.source_index(),
                    display_index: idx,
                    start_display_index: idx.saturating_sub(span.saturating_sub(1)),
                    end_display_index: idx,
                    lane: de.lane(),
                    active_lanes: de.active_lanes().to_vec(),
                });
            }

            // Auto chronicle mode can end up with zero anchors on pure chat transcripts.
            // Fallback to assistant messages so summary generation still progresses.
            if auto_turn_window_mode && anchors.is_empty() {
                for (idx, de) in events.iter().enumerate() {
                    if !matches!(de.event().event_type, EventType::AgentMessage) {
                        continue;
                    }
                    let key = TimelineSummaryWindowKey {
                        session_id: session.session_id.clone(),
                        event_index: de.source_index(),
                        window_id: (5u64 << 56) | (de.source_index() as u64),
                    };
                    if !seen.insert(key.clone()) {
                        continue;
                    }
                    anchors.push(SummaryAnchor {
                        scope: SummaryScope::Window,
                        key,
                        anchor_event: de.event(),
                        anchor_source_index: de.source_index(),
                        display_index: idx,
                        start_display_index: idx,
                        end_display_index: idx,
                        lane: de.lane(),
                        active_lanes: de.active_lanes().to_vec(),
                    });
                }
            }
        }

        if include_turn_anchors {
            for turn in extract_visible_turns(events) {
                if turn.user_events.is_empty() && turn.agent_events.is_empty() {
                    continue;
                }
                let control_only_user = !turn.user_events.is_empty()
                    && turn
                        .user_events
                        .iter()
                        .all(|event| is_infra_warning_user_message(event));
                if turn.agent_events.is_empty() && control_only_user {
                    continue;
                }
                let Some(anchor_event) = source_to_event.get(&turn.anchor_source_index).copied()
                else {
                    continue;
                };
                let key = TimelineSummaryWindowKey {
                    session_id: session.session_id.clone(),
                    event_index: turn.anchor_source_index,
                    window_id: (3u64 << 56) | (turn.turn_index as u64),
                };
                if !seen.insert(key.clone()) {
                    continue;
                }

                let display_event = events
                    .get(turn.end_display_index)
                    .map(|de| de.event())
                    .unwrap_or(anchor_event);
                let display_source = events
                    .get(turn.end_display_index)
                    .map(|de| de.source_index())
                    .unwrap_or(turn.anchor_source_index);
                let lane = events
                    .get(turn.end_display_index)
                    .map(|de| de.lane())
                    .unwrap_or(0);
                let active_lanes = events
                    .get(turn.end_display_index)
                    .map(|de| de.active_lanes().to_vec())
                    .unwrap_or_else(|| vec![0]);

                anchors.push(SummaryAnchor {
                    scope: SummaryScope::Turn,
                    key,
                    anchor_event: display_event,
                    anchor_source_index: display_source,
                    display_index: turn.end_display_index,
                    start_display_index: turn.start_display_index,
                    end_display_index: turn.end_display_index,
                    lane,
                    active_lanes,
                });
            }
        }

        anchors
    }

    fn build_summary_context<'a>(
        &self,
        session: &Session,
        events: &[DisplayEvent<'a>],
        anchor: &SummaryAnchor<'a>,
    ) -> String {
        let start = anchor
            .start_display_index
            .min(events.len().saturating_sub(1));
        let end = anchor.end_display_index.min(events.len().saturating_sub(1));
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        let slice = &events[start..=end];
        let scope = match anchor.scope {
            SummaryScope::Turn => "turn",
            SummaryScope::Window => "window",
        };
        let turn_auto_mode = matches!(anchor.scope, SummaryScope::Turn)
            && self.daemon_config.daemon.summary_event_window == 0;

        let clip = |value: &str, max_chars: usize| -> String {
            let compact = value.trim().replace('\n', " ");
            if compact.chars().count() <= max_chars {
                compact
            } else {
                let mut out = String::new();
                for ch in compact.chars().take(max_chars.saturating_sub(3)) {
                    out.push(ch);
                }
                out.push_str("...");
                out
            }
        };

        let mut prompt_text_lines: Vec<String> = Vec::new();
        let mut prompt_constraints: Vec<String> = Vec::new();
        let mut agent_outcome: Vec<String> = Vec::new();
        let mut modified_files: HashMap<String, (String, u32)> = HashMap::new();
        let mut key_implementations: Vec<String> = Vec::new();
        let mut agent_quotes: Vec<String> = Vec::new();
        let mut agent_plan: Vec<String> = Vec::new();
        let mut tool_actions: Vec<String> = Vec::new();
        let mut errors: Vec<String> = Vec::new();
        let mut ignored_control_events: Vec<String> = Vec::new();
        let mut timeline_window: Vec<String> = Vec::new();
        let mut agent_msg_count = 0usize;
        let mut saw_task_end = false;

        for (offset, event) in slice.iter().enumerate() {
            let source_index = start + offset;
            let e = event.event();
            if is_control_event(e) {
                ignored_control_events
                    .push(format!("[{source_index}] {}", Self::compact_event_line(e)));
                continue;
            }

            let kind = Self::event_kind_label(&e.event_type);
            timeline_window.push(format!(
                "- [{source_index}] {kind} {}",
                Self::compact_event_line(e)
            ));

            match &e.event_type {
                EventType::UserMessage => {
                    for block in &e.content.blocks {
                        if let ContentBlock::Text { text } = block {
                            for line in text.lines().map(str::trim).filter(|line| !line.is_empty())
                            {
                                if prompt_text_lines.len() < 16 {
                                    prompt_text_lines.push(line.to_string());
                                }
                                let lower = line.to_ascii_lowercase();
                                if prompt_constraints.len() < 8
                                    && (lower.starts_with("must ")
                                        || lower.starts_with("should ")
                                        || lower.starts_with("please ")
                                        || lower.contains("do not ")
                                        || lower.contains("don't ")
                                        || lower.starts_with('-')
                                        || lower.starts_with('*'))
                                {
                                    prompt_constraints.push(clip(line, 180));
                                }
                            }
                        }
                    }
                }
                EventType::AgentMessage | EventType::SystemMessage | EventType::Thinking => {
                    if matches!(e.event_type, EventType::AgentMessage) {
                        agent_msg_count += 1;
                        for block in &e.content.blocks {
                            if let ContentBlock::Text { text } = block {
                                for line in
                                    text.lines().map(str::trim).filter(|line| !line.is_empty())
                                {
                                    if agent_quotes.len() < 3 {
                                        agent_quotes.push(clip(line, 220));
                                    }
                                    let lower = line.to_ascii_lowercase();
                                    if key_implementations.len() < 24
                                        && (lower.contains("implement")
                                            || lower.contains("updated")
                                            || lower.contains("fixed")
                                            || lower.contains("added")
                                            || lower.contains("refactor")
                                            || lower.contains("migrate"))
                                    {
                                        key_implementations.push(clip(line, 220));
                                    }
                                    if agent_plan.len() < 24
                                        && (lower.starts_with("plan")
                                            || lower.starts_with("next")
                                            || lower.starts_with("phase")
                                            || lower.starts_with("1.")
                                            || lower.starts_with("2.")
                                            || lower.starts_with("3.")
                                            || lower.starts_with("- "))
                                    {
                                        agent_plan.push(clip(line, 220));
                                    }
                                }
                                break;
                            }
                        }
                    }
                    let line = Self::compact_event_line(e);
                    if !line.is_empty() && agent_outcome.len() < 20 {
                        agent_outcome.push(line);
                    }
                }
                EventType::TaskStart { title } => {
                    if let Some(title) = title.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                        if agent_plan.len() < 24 {
                            agent_plan.push(format!("task started: {}", clip(title, 180)));
                        }
                    }
                }
                EventType::TaskEnd { summary } => {
                    saw_task_end = true;
                    if let Some(text) = summary.as_deref().map(str::trim).filter(|v| !v.is_empty())
                    {
                        if agent_outcome.len() < 20 {
                            agent_outcome.push(text.to_string());
                        }
                    }
                }
                EventType::ToolCall { name } => {
                    if tool_actions.len() < 32 {
                        tool_actions.push(format!("tool_call:{name}"));
                    }
                    if name.eq_ignore_ascii_case("update_plan") {
                        for block in &e.content.blocks {
                            if let ContentBlock::Json { data } = block {
                                if let Some(items) = data.get("plan").and_then(|v| v.as_array()) {
                                    for item in items {
                                        let step = item
                                            .get("step")
                                            .and_then(|v| v.as_str())
                                            .map(str::trim)
                                            .filter(|v| !v.is_empty());
                                        if let Some(step) = step {
                                            let status = item
                                                .get("status")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("unknown");
                                            if agent_plan.len() < 24 {
                                                agent_plan.push(format!(
                                                    "[{}] {}",
                                                    clip(status, 32),
                                                    clip(step, 180)
                                                ));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                EventType::ToolResult { name, is_error, .. } => {
                    let status = if *is_error { "error" } else { "ok" };
                    if tool_actions.len() < 32 {
                        tool_actions.push(format!("tool_result:{name}:{status}"));
                    }
                    if *is_error {
                        let detail = Self::first_text_block_line(&e.content.blocks, 180);
                        if errors.len() < 16 {
                            if detail.is_empty() {
                                errors.push(format!("tool {name} failed"));
                            } else {
                                errors.push(format!("tool {name} failed: {}", clip(&detail, 180)));
                            }
                        }
                    }
                }
                EventType::ShellCommand { command, exit_code } => {
                    let action = match exit_code {
                        Some(code) => format!("shell:{command} => {code}"),
                        None => format!("shell:{command}"),
                    };
                    if tool_actions.len() < 32 {
                        tool_actions.push(clip(&action, 200));
                    }
                    if let Some(code) = exit_code {
                        if *code != 0 && errors.len() < 16 {
                            errors.push(format!("shell exit {code}: {}", clip(command, 180)));
                        }
                    }
                }
                EventType::FileRead { path } => {
                    let entry = modified_files
                        .entry(path.clone())
                        .or_insert_with(|| ("read".to_string(), 0));
                    entry.0 = "read".to_string();
                    entry.1 += 1;
                }
                EventType::FileEdit { path, .. } => {
                    let entry = modified_files
                        .entry(path.clone())
                        .or_insert_with(|| ("edit".to_string(), 0));
                    entry.0 = "edit".to_string();
                    entry.1 += 1;
                    if key_implementations.len() < 24 {
                        key_implementations.push(format!("edited {path}"));
                    }
                }
                EventType::FileCreate { path } => {
                    let entry = modified_files
                        .entry(path.clone())
                        .or_insert_with(|| ("create".to_string(), 0));
                    entry.0 = "create".to_string();
                    entry.1 += 1;
                    if key_implementations.len() < 24 {
                        key_implementations.push(format!("created {path}"));
                    }
                }
                EventType::FileDelete { path } => {
                    let entry = modified_files
                        .entry(path.clone())
                        .or_insert_with(|| ("delete".to_string(), 0));
                    entry.0 = "delete".to_string();
                    entry.1 += 1;
                    if key_implementations.len() < 24 {
                        key_implementations.push(format!("deleted {path}"));
                    }
                }
                _ => {}
            }
        }

        let collapse_low_signal_actions = |actions: Vec<String>| -> Vec<String> {
            let mut merged = Vec::new();
            let mut grouped_read_open = 0usize;
            for action in actions {
                let lower = action.to_ascii_lowercase();
                let low_signal = lower.contains("tool_call:read")
                    || lower.contains("tool_result:read")
                    || lower.contains("tool_call:view")
                    || lower.contains("tool_result:view")
                    || lower.contains("tool_call:list_dir")
                    || lower.contains("tool_result:list_dir")
                    || lower.contains("tool_call:glob")
                    || lower.contains("tool_result:glob")
                    || lower.contains("tool_call:file_search")
                    || lower.contains("tool_result:file_search")
                    || lower.starts_with("read ")
                    || lower.starts_with("open ");
                if low_signal {
                    grouped_read_open += 1;
                } else {
                    merged.push(action);
                }
            }
            if grouped_read_open > 0 {
                merged.insert(
                    0,
                    format!("semantic-group: read/open/list actions x{grouped_read_open}"),
                );
            }
            merged
        };

        let dedupe_keep_order = |items: Vec<String>| -> Vec<String> {
            let mut out = Vec::new();
            for item in items {
                if !out.iter().any(|existing| existing == &item) {
                    out.push(item);
                }
            }
            out
        };

        prompt_constraints = dedupe_keep_order(prompt_constraints);
        key_implementations = dedupe_keep_order(key_implementations);
        agent_quotes = dedupe_keep_order(agent_quotes);
        agent_plan = dedupe_keep_order(agent_plan);
        tool_actions = dedupe_keep_order(tool_actions);
        errors = dedupe_keep_order(errors);
        if self.summary_content_mode_is_minimal() {
            tool_actions = collapse_low_signal_actions(tool_actions);
        }

        let mut modified_file_lines: Vec<String> = modified_files
            .into_iter()
            .map(|(path, (op, count))| format!("- path:{path} op:{op} count:{count}"))
            .collect();
        modified_file_lines.sort();
        modified_file_lines.truncate(24);

        let card_cap = (4usize + (agent_msg_count / 10)).clamp(6, 24);
        let turn_index_hint = if matches!(anchor.scope, SummaryScope::Turn) {
            (anchor.key.window_id & ((1u64 << 56) - 1)) as usize
        } else {
            anchor.display_index
        };
        let outcome_status = if !errors.is_empty() {
            "error"
        } else if saw_task_end {
            "completed"
        } else {
            "in_progress"
        };

        let prompt_text = if prompt_text_lines.is_empty() {
            "(none)".to_string()
        } else {
            prompt_text_lines.join(" | ")
        };
        let inner_event_count = timeline_window.len();
        let raw_event_count = slice.len();
        let prompt_intent = prompt_text_lines
            .first()
            .map(|line| clip(line, 180))
            .unwrap_or_else(|| "No explicit user prompt".to_string());
        let outcome_summary = agent_outcome
            .last()
            .map(|line| clip(line, 220))
            .unwrap_or_else(|| "No agent outcome recorded".to_string());
        let next_steps: Vec<String> = agent_plan
            .iter()
            .filter(|line| {
                let lower = line.to_ascii_lowercase();
                lower.contains("next")
                    || lower.contains("todo")
                    || lower.contains("pending")
                    || lower.contains("in_progress")
            })
            .take(5)
            .cloned()
            .collect();

        let mut lines: Vec<String> = Vec::with_capacity(slice.len() + 64);
        lines.push(format!("session_id: {}", session.session_id));
        lines.push(format!("tool: {}", session.agent.tool));
        lines.push(format!("model: {}", session.agent.model));
        lines.push(format!("scope: {scope}"));
        lines.push(format!("summary_mode: {}", self.summary_content_mode_key()));
        lines.push(format!("inner_event_count: {inner_event_count}"));
        lines.push(format!("raw_event_count: {raw_event_count}"));
        lines.push(format!("card_cap: {card_cap}"));
        lines.push(format!(
            "turn_meta: turn_index={} anchor_event_index={} event_span={}..{}",
            turn_index_hint, anchor.anchor_source_index, start, end
        ));
        lines.push("prompt:".to_string());
        lines.push(format!("- text: {}", clip(&prompt_text, 600)));
        lines.push(format!("- intent: {}", clip(&prompt_intent, 220)));
        if prompt_constraints.is_empty() {
            lines.push("- constraints: (none)".to_string());
        } else {
            lines.push("- constraints:".to_string());
            for c in prompt_constraints.iter().take(8) {
                lines.push(format!("  - {}", clip(c, 220)));
            }
        }
        lines.push("outcome:".to_string());
        lines.push(format!("- status: {outcome_status}"));
        lines.push(format!("- summary: {}", clip(&outcome_summary, 220)));
        lines.push("evidence.modified_files:".to_string());
        if modified_file_lines.is_empty() {
            lines.push("- (none)".to_string());
        } else {
            lines.extend(modified_file_lines);
        }
        lines.push("evidence.key_implementations:".to_string());
        if key_implementations.is_empty() {
            lines.push("- (none)".to_string());
        } else {
            for line in key_implementations.iter().take(24) {
                lines.push(format!("- {}", clip(line, 220)));
            }
        }
        lines.push("evidence.agent_quotes_candidates:".to_string());
        if agent_quotes.is_empty() {
            lines.push("- (none)".to_string());
        } else {
            for quote in agent_quotes.iter().take(3) {
                lines.push(format!("- {}", clip(quote, 220)));
            }
        }
        lines.push("evidence.agent_plan_candidates:".to_string());
        if agent_plan.is_empty() {
            lines.push("- (none)".to_string());
        } else {
            for plan in agent_plan.iter().take(24) {
                lines.push(format!("- {}", clip(plan, 220)));
            }
        }
        lines.push("evidence.tool_actions:".to_string());
        if tool_actions.is_empty() {
            lines.push("- (none)".to_string());
        } else {
            for action in tool_actions.iter().take(24) {
                lines.push(format!("- {}", clip(action, 220)));
            }
        }
        lines.push("evidence.errors:".to_string());
        if errors.is_empty() {
            lines.push("- (none)".to_string());
        } else {
            for error in errors.iter().take(16) {
                lines.push(format!("- {}", clip(error, 220)));
            }
        }
        lines.push("next_steps_hint:".to_string());
        if next_steps.is_empty() {
            lines.push("- (none)".to_string());
        } else {
            for step in next_steps {
                lines.push(format!("- {}", clip(&step, 220)));
            }
        }
        lines.push(format!(
            "ignored_control_events: {}",
            ignored_control_events.len()
        ));
        for entry in ignored_control_events.iter().take(6) {
            lines.push(format!("- {}", clip(entry, 220)));
        }
        lines.push("timeline_window:".to_string());
        if timeline_window.is_empty() {
            lines.push("- (none)".to_string());
        } else {
            lines.extend(timeline_window);
        }

        let auto_mode_hint = if turn_auto_mode {
            "auto_turn_mode: on (infer phase boundaries inside cards, keep scope as one turn)"
        } else {
            "auto_turn_mode: off"
        };
        let summary_mode_hint = if self.summary_content_mode_is_minimal() {
            "summary_mode_hint: minimal (merge semantically equivalent low-signal read/open/list actions; preserve key outcomes, files, plans, and errors)"
        } else {
            "summary_mode_hint: normal (keep action-level detail)"
        };
        let scope_mode_hint = if scope == "window" {
            "scope_mode_hint: window (focus on one primary action and its immediate outcome; avoid turn-wide boilerplate ordering)"
        } else {
            "scope_mode_hint: turn (use stable card ordering to summarize the full turn)"
        };

        format!(
            "Generate turn-summary JSON (v2) for this {scope}.\n\
             Return strict JSON only with keys:\n\
             kind, version, scope, turn_meta, prompt, outcome, evidence, cards, next_steps.\n\
             Required guarantees:\n\
             - evidence.modified_files, evidence.key_implementations, evidence.agent_quotes, evidence.agent_plan must not be dropped.\n\
             - cards must preserve evidence and use types: overview/files/implementation/plan/errors/more.\n\
             - Respect card_cap; if too many cards, emit a final `more` card.\n\
             - Do not copy instruction/control text into prompt.intent.\n\
             - Keep agent_quotes verbatim (1~3 lines max).\n\
             - Prefer factual execution outcomes over meta-instructions.\n\
             {auto_mode_hint}\n\
             {summary_mode_hint}\n\
             {scope_mode_hint}\n\n{}",
            lines.join("\n")
        )
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

    fn summary_allowed_for_session(&self, session: &Session) -> bool {
        if self.focus_detail_view || self.live_mode {
            return false;
        }
        if !self.daemon_config.daemon.summary_enabled {
            return false;
        }
        self.summary_backend_unavailable_reason(session).is_none()
    }

    fn summary_backend_unavailable_reason(&self, _session: &Session) -> Option<String> {
        let provider = self
            .daemon_config
            .daemon
            .summary_provider
            .as_deref()
            .unwrap_or("auto")
            .trim()
            .to_ascii_lowercase();

        match provider.as_str() {
            "" | "auto" => {
                if self.has_any_summary_api_key()
                    || self.has_openai_compatible_endpoint_config()
                    || std::env::var("OPS_TL_SUM_CLI_BIN")
                        .ok()
                        .is_some_and(|v| !v.trim().is_empty())
                {
                    None
                } else {
                    Some(
                        "no summary backend configured for auto mode; set API key/endpoint in Settings > Timeline Intelligence, set OPS_TL_SUM_CLI_BIN, or switch LLM Summary Mode".to_string(),
                    )
                }
            }
            "anthropic" => {
                if std::env::var("ANTHROPIC_API_KEY").is_ok() {
                    None
                } else {
                    Some("ANTHROPIC_API_KEY is missing".to_string())
                }
            }
            "openai" => {
                if std::env::var("OPENAI_API_KEY").is_ok() {
                    None
                } else {
                    Some("OPENAI_API_KEY is missing".to_string())
                }
            }
            "openai-compatible" => {
                if self.has_openai_compatible_endpoint_config()
                    || self.has_openai_compatible_api_key()
                {
                    None
                } else {
                    Some(
                        "OpenAI-compatible mode needs key or endpoint/base config (Settings > Timeline Intelligence or OPS_TL_SUM_*)"
                            .to_string(),
                    )
                }
            }
            "gemini" => {
                if std::env::var("GEMINI_API_KEY").is_ok()
                    || std::env::var("GOOGLE_API_KEY").is_ok()
                {
                    None
                } else {
                    Some("GEMINI_API_KEY (or GOOGLE_API_KEY) is missing".to_string())
                }
            }
            "cli" | "cli:auto" => {
                if Self::any_summary_cli_available() {
                    None
                } else {
                    Some("CLI mode selected but no summary CLI binary found".to_string())
                }
            }
            "cli:codex" => {
                if Self::command_exists("codex") {
                    None
                } else {
                    Some("codex CLI is not installed".to_string())
                }
            }
            "cli:claude" => {
                if Self::command_exists("claude") {
                    None
                } else {
                    Some("claude CLI is not installed".to_string())
                }
            }
            "cli:cursor" => {
                if Self::command_exists("cursor") || Self::command_exists("cursor-agent") {
                    None
                } else {
                    Some("cursor CLI is not installed".to_string())
                }
            }
            "cli:gemini" => {
                if Self::command_exists("gemini") {
                    None
                } else {
                    Some("gemini CLI is not installed".to_string())
                }
            }
            other => Some(format!("unsupported summary provider: {other}")),
        }
    }

    fn cfg_non_empty(value: Option<&str>) -> bool {
        value.is_some_and(|v| !v.trim().is_empty())
    }

    fn summary_content_mode_key(&self) -> &'static str {
        match self
            .daemon_config
            .daemon
            .summary_content_mode
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "minimal" | "min" => "minimal",
            _ => "normal",
        }
    }

    fn summary_content_mode_is_minimal(&self) -> bool {
        self.summary_content_mode_key() == "minimal"
    }

    fn summary_disk_cache_enabled(&self) -> bool {
        self.daemon_config.daemon.summary_disk_cache_enabled
    }

    fn summary_cache_path() -> Option<PathBuf> {
        config::config_dir()
            .ok()
            .map(|dir| dir.join("timeline_summary_cache.jsonl"))
    }

    fn stable_context_hash(input: &str) -> u64 {
        // FNV-1a 64-bit for stable cache key hashing across runs.
        const OFFSET: u64 = 0xcbf29ce484222325;
        const PRIME: u64 = 0x100000001b3;
        let mut hash = OFFSET;
        for byte in input.as_bytes() {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(PRIME);
        }
        hash
    }

    fn summary_cache_lookup_key(
        &self,
        key: &TimelineSummaryWindowKey,
        context: &str,
    ) -> Option<String> {
        if !self.summary_disk_cache_enabled() {
            return None;
        }
        let engine = describe_summary_engine(
            self.daemon_config.daemon.summary_provider.as_deref(),
            Some(&self.summary_runtime_config()),
        )
        .ok()?;
        let context_hash = Self::stable_context_hash(context);
        Some(format!(
            "v2|{}|{}|{}|{}|{}|{:016x}",
            key.session_id,
            key.event_index,
            key.window_id,
            engine,
            self.summary_content_mode_key(),
            context_hash
        ))
    }

    fn ensure_summary_disk_cache_loaded(&mut self) {
        if self.timeline_summary_disk_cache_loaded {
            return;
        }
        self.timeline_summary_disk_cache_loaded = true;
        if !self.summary_disk_cache_enabled() {
            return;
        }

        let Some(path) = Self::summary_cache_path() else {
            return;
        };
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(_) => return,
        };
        let reader = BufReader::new(file);
        for line in reader.lines().map_while(Result::ok) {
            let Ok(row) = serde_json::from_str::<PersistedTimelineSummaryRow>(&line) else {
                continue;
            };
            self.timeline_summary_disk_cache.insert(
                row.lookup_key,
                TimelineSummaryCacheEntry {
                    compact: row.compact,
                    payload: row.payload,
                    raw: row.raw,
                },
            );
        }
    }

    fn maybe_use_summary_disk_cache(
        &mut self,
        key: &TimelineSummaryWindowKey,
        lookup_key: &str,
    ) -> bool {
        self.ensure_summary_disk_cache_loaded();
        let Some(entry) = self.timeline_summary_disk_cache.get(lookup_key).cloned() else {
            return false;
        };
        self.timeline_summary_lookup_keys
            .insert(key.clone(), lookup_key.to_string());
        self.timeline_summary_cache.insert(key.clone(), entry);
        true
    }

    fn persist_summary_disk_cache(
        &mut self,
        lookup_key: String,
        entry: &TimelineSummaryCacheEntry,
    ) {
        if !self.summary_disk_cache_enabled() {
            return;
        }
        self.ensure_summary_disk_cache_loaded();
        self.timeline_summary_disk_cache
            .insert(lookup_key.clone(), entry.clone());

        let Some(path) = Self::summary_cache_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        else {
            return;
        };

        let saved_at_unix = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let row = PersistedTimelineSummaryRow {
            lookup_key,
            compact: entry.compact.clone(),
            payload: entry.payload.clone(),
            raw: entry.raw.clone(),
            saved_at_unix,
        };
        if let Ok(json) = serde_json::to_string(&row) {
            let _ = file.write_all(json.as_bytes());
            let _ = file.write_all(b"\n");
        }
    }

    fn has_any_summary_api_key(&self) -> bool {
        std::env::var("ANTHROPIC_API_KEY").is_ok()
            || std::env::var("OPENAI_API_KEY").is_ok()
            || std::env::var("GEMINI_API_KEY").is_ok()
            || std::env::var("GOOGLE_API_KEY").is_ok()
            || Self::cfg_non_empty(
                self.daemon_config
                    .daemon
                    .summary_openai_compat_key
                    .as_deref(),
            )
    }

    fn has_openai_compatible_endpoint_config(&self) -> bool {
        Self::cfg_non_empty(
            self.daemon_config
                .daemon
                .summary_openai_compat_endpoint
                .as_deref(),
        ) || Self::cfg_non_empty(
            self.daemon_config
                .daemon
                .summary_openai_compat_base
                .as_deref(),
        ) || std::env::var("OPS_TL_SUM_ENDPOINT")
            .ok()
            .is_some_and(|v| !v.trim().is_empty())
            || std::env::var("OPS_TL_SUM_BASE")
                .ok()
                .is_some_and(|v| !v.trim().is_empty())
            || std::env::var("OPENAI_BASE_URL")
                .ok()
                .is_some_and(|v| !v.trim().is_empty())
    }

    fn has_openai_compatible_api_key(&self) -> bool {
        Self::cfg_non_empty(
            self.daemon_config
                .daemon
                .summary_openai_compat_key
                .as_deref(),
        ) || std::env::var("OPS_TL_SUM_KEY")
            .ok()
            .is_some_and(|v| !v.trim().is_empty())
            || std::env::var("OPENAI_API_KEY")
                .ok()
                .is_some_and(|v| !v.trim().is_empty())
    }

    fn any_summary_cli_available() -> bool {
        Self::command_exists("codex")
            || Self::command_exists("claude")
            || Self::command_exists("cursor")
            || Self::command_exists("cursor-agent")
            || Self::command_exists("gemini")
    }

    fn ensure_summary_ready_for_session(&mut self, session: &Session) {
        if !self.daemon_config.daemon.summary_enabled {
            return;
        }

        if let Some(reason) = self.summary_backend_unavailable_reason(session) {
            self.daemon_config.daemon.summary_enabled = false;
            self.timeline_summary_cache.clear();
            self.timeline_summary_lookup_keys.clear();
            self.cancel_timeline_summary_jobs();
            self.flash_info(format!(
                "LLM summary auto-disabled: {} (Settings > Timeline Intelligence)",
                reason
            ));
        }
    }

    fn clear_timeline_summary_queue_state(&mut self) {
        for pending in &self.timeline_summary_pending {
            self.timeline_summary_lookup_keys.remove(&pending.key);
        }
        for key in &self.timeline_summary_inflight {
            self.timeline_summary_lookup_keys.remove(key);
        }
        self.timeline_summary_pending.clear();
        self.timeline_summary_inflight.clear();
        self.timeline_summary_inflight_started.clear();
        self.last_summary_request_at = None;
    }

    fn cancel_timeline_summary_jobs(&mut self) {
        self.clear_timeline_summary_queue_state();
        self.timeline_summary_epoch = self.timeline_summary_epoch.wrapping_add(1);
    }

    fn leave_detail_view(&mut self) {
        self.cancel_timeline_summary_jobs();
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

    fn summary_inflight_timeout() -> Duration {
        let timeout_ms = std::env::var("OPS_TL_SUM_INFLIGHT_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .unwrap_or(90_000)
            .max(5_000);
        Duration::from_millis(timeout_ms)
    }

    fn prune_stale_summary_inflight(&mut self) {
        if self.timeline_summary_inflight.is_empty() {
            return;
        }

        let timeout = Self::summary_inflight_timeout();
        let stale: Vec<TimelineSummaryWindowKey> = self
            .timeline_summary_inflight
            .iter()
            .filter_map(
                |key| match self.timeline_summary_inflight_started.get(key) {
                    Some(started) if started.elapsed() >= timeout => Some(key.clone()),
                    Some(_) => None,
                    None => Some(key.clone()),
                },
            )
            .collect();

        if stale.is_empty() {
            return;
        }

        let fallback = format!(
            "summary unavailable (summary job timed out after {}s)",
            timeout.as_secs()
        );
        for key in stale {
            self.timeline_summary_inflight.remove(&key);
            self.timeline_summary_inflight_started.remove(&key);
            self.timeline_summary_lookup_keys.remove(&key);
            self.timeline_summary_cache
                .entry(key)
                .or_insert_with(|| parse_timeline_summary_output(&fallback));
        }
    }

    fn pending_summary_contains(&self, key: &TimelineSummaryWindowKey) -> bool {
        self.timeline_summary_pending.iter().any(|r| &r.key == key)
    }

    fn maybe_prompt_summary_cli_setup(&mut self, key: &TimelineSummaryWindowKey, err: &str) {
        if self.summary_cli_prompted || self.modal.is_some() {
            return;
        }
        let missing_setup = Self::is_summary_setup_missing(err);
        let runtime_failure = Self::is_summary_cli_runtime_failure(err);
        if !missing_setup && !runtime_failure {
            return;
        }

        self.summary_cli_prompted = true;
        let available = self.available_summary_cli_providers(&key.session_id);
        if !available.is_empty() {
            let message = if missing_setup {
                format!(
                    "Summary is not configured. Run hello probe on installed CLIs ({})?",
                    available.join(", ")
                )
            } else {
                format!(
                    "Summary CLI failed. Run hello probe to pick a responsive CLI ({})?",
                    available.join(", ")
                )
            };
            self.modal = Some(Modal::Confirm {
                title: "Configure LLM Summary".to_string(),
                message,
                action: ConfirmAction::ProbeSummaryCli {
                    session_id: key.session_id.clone(),
                },
            });
        } else if missing_setup {
            self.flash_info(
                "Summary is not configured. Install one CLI (codex/claude/cursor/gemini) or add an API key.",
            );
        } else {
            self.flash_info(
                "Summary CLI failed. Ensure the selected CLI is authenticated, or switch provider in Settings.",
            );
        }
    }

    fn is_summary_setup_missing(err: &str) -> bool {
        let lower = err.to_ascii_lowercase();
        (lower.contains("no summary api key found")
            && lower.contains("no cli summary binary configured"))
            || lower.contains("could not resolve cli summary binary")
    }

    fn is_summary_cli_runtime_failure(err: &str) -> bool {
        let lower = err.to_ascii_lowercase();
        lower.contains("summary cli failed")
            || lower.contains("failed to execute summary cli")
            || lower.contains("summary cli probe timed out")
    }

    fn recommended_summary_cli_provider(&self, session_id: &str) -> Option<String> {
        self.available_summary_cli_providers(session_id)
            .into_iter()
            .next()
            .map(|provider| provider.to_string())
    }

    fn available_summary_cli_providers(&self, session_id: &str) -> Vec<&'static str> {
        let preferred = self
            .session_tool_for_summary(session_id)
            .and_then(Self::tool_to_summary_cli_provider);

        let mut order = Vec::new();
        if let Some(provider) = preferred {
            order.push(provider);
        }
        for provider in ["cli:codex", "cli:claude", "cli:cursor", "cli:gemini"] {
            if !order.contains(&provider) {
                order.push(provider);
            }
        }

        order
            .into_iter()
            .filter(|provider| Self::summary_cli_provider_available(provider))
            .collect()
    }

    fn session_tool_for_summary<'a>(&'a self, session_id: &str) -> Option<&'a str> {
        self.sessions
            .iter()
            .find(|session| session.session_id == session_id)
            .map(|session| session.agent.tool.as_str())
            .or_else(|| {
                self.selected_session()
                    .map(|session| session.agent.tool.as_str())
            })
    }

    fn tool_to_summary_cli_provider(tool: &str) -> Option<&'static str> {
        let lower = tool.to_ascii_lowercase();
        if lower.contains("codex") {
            Some("cli:codex")
        } else if lower.contains("claude") {
            Some("cli:claude")
        } else if lower.contains("cursor") {
            Some("cli:cursor")
        } else if lower.contains("gemini") {
            Some("cli:gemini")
        } else {
            None
        }
    }

    fn summary_cli_provider_available(provider: &str) -> bool {
        match provider {
            "cli:codex" => Self::command_exists("codex"),
            "cli:claude" => Self::command_exists("claude"),
            "cli:cursor" => Self::command_exists("cursor") || Self::command_exists("cursor-agent"),
            "cli:gemini" => Self::command_exists("gemini"),
            _ => false,
        }
    }

    fn detail_summary_warmup_duration() -> Duration {
        let ms = std::env::var("OPS_TL_SUM_WARMUP_MS")
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .unwrap_or(450)
            .clamp(0, 5_000);
        Duration::from_millis(ms)
    }

    fn command_exists(binary: &str) -> bool {
        Command::new("sh")
            .arg("-lc")
            .arg(format!("command -v {binary} >/dev/null 2>&1"))
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    pub fn schedule_detail_summary_jobs(&mut self) -> Option<AsyncCommand> {
        if self.view != View::SessionDetail {
            return None;
        }
        if self.focus_detail_view {
            return None;
        }
        self.refresh_live_mode();
        if self.live_mode {
            return None;
        }
        if self.detail_entered_at.elapsed() < Self::detail_summary_warmup_duration() {
            return None;
        }
        self.prune_stale_summary_inflight();
        let max_inflight = self.daemon_config.daemon.summary_max_inflight.max(1) as usize;
        if self.timeline_summary_inflight.len() >= max_inflight {
            return None;
        }

        let session = self.selected_session().cloned()?;
        self.ensure_summary_ready_for_session(&session);
        if !self.summary_allowed_for_session(&session) {
            return None;
        }

        let base = self.get_base_visible_events(&session);
        if base.is_empty() {
            return None;
        }

        let viewport = self.detail_viewport_height.max(6) as usize;
        let visible_start = self.detail_event_index.saturating_sub(viewport / 2);
        let visible_end = self
            .detail_event_index
            .saturating_add(viewport.saturating_mul(2));

        let mut visible_candidates: Vec<(usize, SummaryAnchor<'_>)> = Vec::new();
        let mut background_candidates: Vec<(usize, SummaryAnchor<'_>)> = Vec::new();
        self.ensure_summary_disk_cache_loaded();

        for anchor in self.build_summary_anchors(&session, &base) {
            if self.timeline_summary_cache.contains_key(&anchor.key)
                || self.timeline_summary_inflight.contains(&anchor.key)
                || self.pending_summary_contains(&anchor.key)
            {
                continue;
            }

            let distance = anchor.display_index.abs_diff(self.detail_event_index);
            let turn_focus_priority = self.detail_view_mode == DetailViewMode::Turn
                && matches!(anchor.scope, SummaryScope::Turn)
                && distance <= viewport;
            if turn_focus_priority
                || (anchor.display_index >= visible_start && anchor.display_index <= visible_end)
            {
                visible_candidates.push((distance, anchor));
            } else {
                background_candidates.push((distance, anchor));
            }
        }

        if !visible_candidates.is_empty() {
            const VISIBLE_ENQUEUE_BATCH: usize = 48;
            visible_candidates.sort_by_key(|(distance, _)| *distance);
            visible_candidates.truncate(VISIBLE_ENQUEUE_BATCH);
            for (_, anchor) in visible_candidates.into_iter().rev() {
                let context = self.build_summary_context(&session, &base, &anchor);
                let cache_lookup_key = self.summary_cache_lookup_key(&anchor.key, &context);
                if let Some(lookup_key) = cache_lookup_key.as_deref() {
                    if self.maybe_use_summary_disk_cache(&anchor.key, lookup_key) {
                        continue;
                    }
                }
                let req = TimelineSummaryWindowRequest {
                    context,
                    key: anchor.key,
                    visible_priority: true,
                    cache_lookup_key,
                };
                self.timeline_summary_pending.push_front(req);
            }
        }

        // Keep background fill lazy: only enqueue a small near-range batch per tick.
        background_candidates.sort_by_key(|(distance, _)| *distance);
        const BACKGROUND_ENQUEUE_BATCH: usize = 24;
        for (_, anchor) in background_candidates
            .into_iter()
            .take(BACKGROUND_ENQUEUE_BATCH)
        {
            let context = self.build_summary_context(&session, &base, &anchor);
            let cache_lookup_key = self.summary_cache_lookup_key(&anchor.key, &context);
            if let Some(lookup_key) = cache_lookup_key.as_deref() {
                if self.maybe_use_summary_disk_cache(&anchor.key, lookup_key) {
                    continue;
                }
            }
            let req = TimelineSummaryWindowRequest {
                context,
                key: anchor.key,
                visible_priority: false,
                cache_lookup_key,
            };
            self.timeline_summary_pending.push_back(req);
        }

        let req = self.timeline_summary_pending.pop_front()?;
        let debounce_ms = self.daemon_config.daemon.summary_debounce_ms.max(100);
        if !req.visible_priority
            && self
                .last_summary_request_at
                .is_some_and(|t| t.elapsed() < Duration::from_millis(debounce_ms))
        {
            self.timeline_summary_pending.push_front(req);
            return None;
        }

        self.timeline_summary_inflight.insert(req.key.clone());
        if let Some(lookup_key) = req.cache_lookup_key.clone() {
            self.timeline_summary_lookup_keys
                .insert(req.key.clone(), lookup_key);
        }
        let now = Instant::now();
        self.timeline_summary_inflight_started
            .insert(req.key.clone(), now);
        self.last_summary_request_at = Some(now);
        Some(AsyncCommand::GenerateTimelineSummary {
            key: req.key,
            epoch: self.timeline_summary_epoch,
            provider: self.daemon_config.daemon.summary_provider.clone(),
            context: req.context,
            agent_tool: session.agent.tool.clone(),
        })
    }

    pub fn turn_summary_key(
        session_id: &str,
        turn_index: usize,
        anchor_source_index: usize,
    ) -> TimelineSummaryWindowKey {
        TimelineSummaryWindowKey {
            session_id: session_id.to_string(),
            event_index: anchor_source_index,
            window_id: (3u64 << 56) | (turn_index as u64),
        }
    }

    pub fn turn_summary_entry(
        &self,
        session_id: &str,
        turn_index: usize,
        anchor_source_index: usize,
    ) -> Option<&TimelineSummaryCacheEntry> {
        let key = Self::turn_summary_key(session_id, turn_index, anchor_source_index);
        self.timeline_summary_cache.get(&key)
    }

    pub fn turn_summary_payload(
        &self,
        session_id: &str,
        turn_index: usize,
        anchor_source_index: usize,
    ) -> Option<&TimelineSummaryPayload> {
        self.turn_summary_entry(session_id, turn_index, anchor_source_index)
            .map(|entry| &entry.payload)
    }

    fn live_recent_cutoff() -> ChronoDuration {
        ChronoDuration::minutes(5)
    }

    fn selected_session_last_event_at(&self) -> Option<DateTime<Utc>> {
        self.selected_session()
            .and_then(|session| session.events.last().map(|event| event.timestamp))
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
        Utc::now().signed_duration_since(modified_at) <= Self::live_recent_cutoff()
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
        let previous = self.live_mode;
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
        if self.live_mode && !previous {
            self.cancel_timeline_summary_jobs();
        }
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

    fn resolve_selected_source_path(&self) -> Option<PathBuf> {
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

    fn summary_runtime_config(&self) -> SummaryRuntimeConfig {
        SummaryRuntimeConfig {
            model: self.daemon_config.daemon.summary_model.clone(),
            content_mode: Some(self.daemon_config.daemon.summary_content_mode.clone()),
            openai_compat_endpoint: self
                .daemon_config
                .daemon
                .summary_openai_compat_endpoint
                .clone(),
            openai_compat_base: self.daemon_config.daemon.summary_openai_compat_base.clone(),
            openai_compat_path: self.daemon_config.daemon.summary_openai_compat_path.clone(),
            openai_compat_style: self
                .daemon_config
                .daemon
                .summary_openai_compat_style
                .clone(),
            openai_compat_api_key: self.daemon_config.daemon.summary_openai_compat_key.clone(),
            openai_compat_api_key_header: self
                .daemon_config
                .daemon
                .summary_openai_compat_key_header
                .clone(),
        }
    }

    pub fn llm_summary_status_label(&self) -> String {
        if self.selected_session().is_none() {
            return "off".to_string();
        }
        if self.focus_detail_view || self.live_mode {
            return "off".to_string();
        }
        if !self.daemon_config.daemon.summary_enabled {
            return "off".to_string();
        }
        if describe_summary_engine(
            self.daemon_config.daemon.summary_provider.as_deref(),
            Some(&self.summary_runtime_config()),
        )
        .is_err()
        {
            return "off(no-backend)".to_string();
        }
        "on".to_string()
    }

    pub fn llm_summary_engine_label(&self) -> String {
        describe_summary_engine(
            self.daemon_config.daemon.summary_provider.as_deref(),
            Some(&self.summary_runtime_config()),
        )
        .unwrap_or_else(|_| "backend:none".to_string())
    }

    pub fn llm_summary_runtime_badge(&self) -> String {
        let status = self.llm_summary_status_label();
        match status.as_str() {
            "on" => format!("llm-summary:on {}", self.llm_summary_engine_label()),
            "off(no-backend)" => format!(
                "llm-summary:off(no-backend) {}",
                self.llm_summary_engine_label()
            ),
            _ => "llm-summary:off".to_string(),
        }
    }

    pub fn take_detail_hydrate_path(&mut self) -> Option<PathBuf> {
        if self.view != View::SessionDetail || !self.detail_hydrate_pending {
            return None;
        }
        self.detail_hydrate_pending = false;

        let session = self.selected_session()?;
        if !session.events.is_empty() || session.stats.event_count == 0 {
            return None;
        }

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
        self.timeline_summary_cache
            .retain(|key, _| key.session_id != sid);
        self.timeline_summary_pending
            .retain(|request| request.key.session_id != sid);
        self.timeline_summary_inflight
            .retain(|key| key.session_id != sid);
        self.timeline_summary_inflight_started
            .retain(|key, _| key.session_id != sid);
        self.timeline_summary_lookup_keys
            .retain(|key, _| key.session_id != sid);
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
            ViewMode::Team(_) | ViewMode::Repo(_) => {
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
