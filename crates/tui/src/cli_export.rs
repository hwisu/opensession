use anyhow::Result;
use opensession_core::trace::{ContentBlock, Event, EventType, Session};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::time::{Duration, Instant};
use tokio::runtime::{Handle, Runtime};
use tokio::task::block_in_place;

use crate::app::{extract_turns, App, DetailViewMode, DisplayEvent, View};
use crate::async_ops::{self, CommandResult};
use crate::session_timeline::LaneMarker;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliTimelineView {
    Linear,
    Turn,
}

#[derive(Debug, Clone)]
pub struct CliTimelineExportOptions {
    pub view: CliTimelineView,
    pub collapse_consecutive: bool,
    pub include_summaries: bool,
    pub generate_summaries: bool,
    pub summary_provider_override: Option<String>,
    pub summary_content_mode_override: Option<String>,
    pub summary_disk_cache_override: Option<bool>,
    pub max_rows: Option<usize>,
    pub summary_budget: Option<usize>,
    pub summary_timeout_ms: Option<u64>,
}

impl Default for CliTimelineExportOptions {
    fn default() -> Self {
        Self {
            view: CliTimelineView::Linear,
            collapse_consecutive: false,
            include_summaries: true,
            generate_summaries: false,
            summary_provider_override: None,
            summary_content_mode_override: None,
            summary_disk_cache_override: None,
            max_rows: None,
            summary_budget: None,
            summary_timeout_ms: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CliTimelineExport {
    pub session_id: String,
    pub tool: String,
    pub model: String,
    pub total_events: usize,
    pub rendered_rows: usize,
    pub max_active_agents: usize,
    pub max_lane_index: usize,
    pub generated_summaries: usize,
    pub rows: Vec<CliTimelineRow>,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CliTimelineRow {
    pub index: usize,
    pub view: String,
    pub row_type: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clock: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_timestamp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_timestamp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_clock: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_clock: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lane: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active_lanes: Vec<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_message_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_event_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_result_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_ops_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell_ops_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_lane_index_in_turn: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_agent_count_in_turn: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_preview: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llm_summary: Option<String>,
}

impl CliTimelineRow {
    fn new(index: usize, view: &str, row_type: &str, text: String) -> Self {
        Self {
            index,
            view: view.to_string(),
            row_type: row_type.to_string(),
            text,
            turn_index: None,
            source_index: None,
            timestamp: None,
            clock: None,
            start_timestamp: None,
            end_timestamp: None,
            start_clock: None,
            end_clock: None,
            duration_ms: None,
            lane: None,
            active_lanes: Vec::new(),
            marker: None,
            event_kind: None,
            summary: None,
            task_id: None,
            task_count: None,
            user_message_count: None,
            agent_event_count: None,
            tool_call_count: None,
            tool_result_count: None,
            file_ops_count: None,
            shell_ops_count: None,
            error_count: None,
            max_lane_index_in_turn: None,
            active_agent_count_in_turn: None,
            user_preview: None,
            llm_summary: None,
        }
    }
}

pub fn export_session_timeline(
    session: Session,
    options: CliTimelineExportOptions,
) -> Result<CliTimelineExport> {
    let mut app = App::new(vec![session]);
    app.view = View::SessionDetail;
    if app.filtered_sessions.is_empty() && !app.sessions.is_empty() {
        app.filtered_sessions = vec![0];
    }
    app.list_state.select(Some(0));
    app.daemon_config = crate::config::load_daemon_config();
    app.collapse_consecutive = options.collapse_consecutive;
    app.detail_view_mode = match options.view {
        CliTimelineView::Linear => DetailViewMode::Linear,
        CliTimelineView::Turn => DetailViewMode::Turn,
    };
    app.detail_viewport_height = u16::MAX;
    app.detail_event_index = 0;
    app.realtime_preview_enabled = app.daemon_config.daemon.detail_realtime_preview_enabled;
    // CLI export is non-interactive; skip detail warmup gating so summary jobs can run immediately.
    app.detail_entered_at = Instant::now() - Duration::from_secs(1);

    if !options.include_summaries {
        app.daemon_config.daemon.summary_enabled = false;
    }
    if let Some(provider) = options.summary_provider_override {
        app.daemon_config.daemon.summary_provider = Some(provider);
    }
    if let Some(mode) = options.summary_content_mode_override {
        app.daemon_config.daemon.summary_content_mode = mode;
    }
    if let Some(enabled) = options.summary_disk_cache_override {
        app.daemon_config.daemon.summary_disk_cache_enabled = enabled;
    }

    let mut generated_summaries = 0usize;
    if options.include_summaries
        && options.generate_summaries
        && app.daemon_config.daemon.summary_enabled
    {
        // Keep CLI export responsive on large sessions by default.
        let summary_budget = options.summary_budget.unwrap_or(96).max(1);
        let summary_timeout = match options.summary_timeout_ms {
            Some(0) => None,
            Some(ms) => Some(Duration::from_millis(ms.max(200))),
            None => Some(Duration::from_millis(12_000)),
        };
        let loop_started = Instant::now();
        let mut owned_runtime = if Handle::try_current().is_err() {
            Some(Runtime::new()?)
        } else {
            None
        };
        // Drive the same scheduler used by TUI until queue drains (or guard trips).
        let mut idle_ticks = 0u32;
        for _ in 0..4096 {
            if generated_summaries >= summary_budget {
                break;
            }
            if let Some(timeout) = summary_timeout {
                if loop_started.elapsed() >= timeout {
                    break;
                }
            }
            if let Some(cmd) = app.schedule_detail_summary_jobs() {
                let result = if let Some(rt) = owned_runtime.as_mut() {
                    rt.block_on(async_ops::execute(cmd, &app.daemon_config))
                } else {
                    let handle = Handle::current();
                    block_in_place(|| handle.block_on(async_ops::execute(cmd, &app.daemon_config)))
                };
                if matches!(result, CommandResult::SummaryDone { .. }) {
                    generated_summaries += 1;
                }
                app.apply_command_result(result);
                idle_ticks = 0;
                continue;
            }

            if app.timeline_summary_pending.is_empty() && app.timeline_summary_inflight.is_empty() {
                break;
            }

            // Scheduler can defer background anchors; give it short ticks.
            std::thread::sleep(Duration::from_millis(50));
            idle_ticks += 1;
            if idle_ticks > 80 {
                break;
            }
        }
    }

    let selected = app
        .selected_session()
        .cloned()
        .expect("single-session app must have selected session");

    let base = app.get_base_visible_events(&selected);
    let max_lane_index = base
        .iter()
        .flat_map(|de| {
            de.active_lanes()
                .iter()
                .copied()
                .chain(std::iter::once(de.lane()))
        })
        .max()
        .unwrap_or(0);
    let max_active_agents = base
        .iter()
        .map(|de| de.active_lanes().iter().filter(|lane| **lane > 0).count())
        .max()
        .unwrap_or(0);

    let (mut rows, mut lines) = match options.view {
        CliTimelineView::Linear => {
            let visible = if options.include_summaries {
                app.get_visible_events(&selected)
            } else {
                base.clone()
            };
            let rows = build_linear_rows(&visible, &base);
            let lines: Vec<String> = rows.iter().map(|row| row.text.clone()).collect();
            (rows, lines)
        }
        CliTimelineView::Turn => {
            let rows = build_turn_rows(&app, &selected.session_id, &base);
            let lines: Vec<String> = rows.iter().map(|row| row.text.clone()).collect();
            (rows, lines)
        }
    };

    if let Some(max_rows) = options.max_rows {
        rows.truncate(max_rows);
        lines.truncate(max_rows);
    }

    Ok(CliTimelineExport {
        session_id: selected.session_id.clone(),
        tool: selected.agent.tool.clone(),
        model: selected.agent.model.clone(),
        total_events: selected.events.len(),
        rendered_rows: lines.len(),
        max_active_agents,
        max_lane_index,
        generated_summaries,
        rows,
        lines,
    })
}

fn build_linear_rows(
    events: &[DisplayEvent<'_>],
    base_events: &[DisplayEvent<'_>],
) -> Vec<CliTimelineRow> {
    let max_lane = events
        .iter()
        .flat_map(|de| {
            de.active_lanes()
                .iter()
                .copied()
                .chain(std::iter::once(de.lane()))
        })
        .max()
        .unwrap_or(0);
    let lane_count = max_lane + 1;
    let source_turn_map = source_index_turn_map(base_events);

    let mut rows = Vec::with_capacity(events.len());
    for (idx, display_event) in events.iter().enumerate() {
        let event = display_event.event();
        let ts = event.timestamp.format("%H:%M:%S").to_string();
        let lane_text = lane_cells(display_event, lane_count);
        let mut row = match display_event {
            DisplayEvent::SummaryRow {
                summary, window_id, ..
            } => {
                let mut row = CliTimelineRow::new(
                    idx,
                    "linear",
                    "llm_summary",
                    format!("{idx:>4} {ts}  {lane_text} [llm #{window_id}] {summary}"),
                );
                row.event_kind = Some("llm_summary".to_string());
                row.summary = Some(summary.clone());
                row
            }
            DisplayEvent::Collapsed { count, kind, .. } => {
                let mut row = CliTimelineRow::new(
                    idx,
                    "linear",
                    "collapsed",
                    format!("{idx:>4} {ts}  {lane_text} {kind} x{count}"),
                );
                row.event_kind = Some(kind.to_ascii_lowercase());
                row.summary = Some(format!("{kind} x{count}"));
                row
            }
            DisplayEvent::Single {
                event,
                lane,
                marker,
                ..
            } => {
                let (kind, summary) = event_display(event);
                let mut body = format!("{kind:>10} {summary}");
                if let Some(badge) = lane_assignment_badge(event, *lane, *marker) {
                    body.push(' ');
                    body.push_str(&badge);
                }
                let mut row = CliTimelineRow::new(
                    idx,
                    "linear",
                    "event",
                    format!("{idx:>4} {ts}  {lane_text} {body}"),
                );
                row.event_kind = Some(kind.to_string());
                row.summary = Some(summary);
                row
            }
        };

        row.turn_index = source_turn_map
            .get(&display_event.source_index())
            .copied()
            .map(|value| value + 1);
        row.source_index = Some(display_event.source_index());
        row.timestamp = Some(event.timestamp.to_rfc3339());
        row.clock = Some(ts);
        row.lane = Some(display_event.lane());
        row.active_lanes = display_event.active_lanes().to_vec();
        row.marker = lane_marker_name(display_event.marker()).map(ToString::to_string);
        row.task_id = event_task_id(event);
        rows.push(row);
    }
    rows
}

fn build_turn_rows(
    app: &App,
    session_id: &str,
    events: &[DisplayEvent<'_>],
) -> Vec<CliTimelineRow> {
    let turns = extract_turns(events);
    let mut rows = Vec::with_capacity(turns.len());
    for turn in turns {
        let turn_key = App::turn_summary_key(session_id, turn.turn_index, turn.anchor_source_index);
        let llm_summary = app
            .timeline_summary_cache
            .get(&turn_key)
            .map(|entry| entry.compact.clone())
            .unwrap_or_else(|| {
                if !app.daemon_config.daemon.summary_enabled {
                    "(LLM summary off)".to_string()
                } else if app.should_skip_realtime_for_selected() {
                    "(LLM summary waiting for live refresh)".to_string()
                } else {
                    "(LLM summary pending)".to_string()
                }
            });

        let user_preview = turn
            .user_events
            .first()
            .map(|event| event_summary(&event.event_type, &event.content.blocks))
            .filter(|line| !line.is_empty())
            .unwrap_or_else(|| "(no user message)".to_string());

        let mut task_ids: HashSet<String> = HashSet::new();
        let mut tool_call_count = 0usize;
        let mut tool_result_count = 0usize;
        let mut file_ops_count = 0usize;
        let mut shell_ops_count = 0usize;
        let mut error_count = 0usize;
        for event in &turn.agent_events {
            if let Some(task_id) = event_task_id(event) {
                task_ids.insert(task_id);
            }
            match &event.event_type {
                EventType::ToolCall { .. } => tool_call_count += 1,
                EventType::ToolResult { is_error, .. } => {
                    tool_result_count += 1;
                    if *is_error {
                        error_count += 1;
                    }
                }
                EventType::FileRead { .. }
                | EventType::FileEdit { .. }
                | EventType::FileCreate { .. }
                | EventType::FileDelete { .. } => file_ops_count += 1,
                EventType::ShellCommand { .. } => shell_ops_count += 1,
                EventType::Custom { kind } => {
                    let lower = kind.to_ascii_lowercase();
                    if lower.contains("error") || lower.contains("fail") {
                        error_count += 1;
                    }
                }
                _ => {}
            }
        }

        let mut min_ts: Option<chrono::DateTime<chrono::Utc>> = None;
        let mut max_ts: Option<chrono::DateTime<chrono::Utc>> = None;
        for event in turn.user_events.iter().chain(turn.agent_events.iter()) {
            min_ts = Some(min_ts.map_or(event.timestamp, |current| current.min(event.timestamp)));
            max_ts = Some(max_ts.map_or(event.timestamp, |current| current.max(event.timestamp)));
        }
        let duration_ms = min_ts
            .zip(max_ts)
            .map(|(start, end)| end.signed_duration_since(start).num_milliseconds());

        let mut lanes: BTreeSet<usize> = BTreeSet::new();
        let mut max_active_agents = 0usize;
        for display_idx in turn.start_display_index..=turn.end_display_index {
            if let Some(display_event) = events.get(display_idx) {
                lanes.insert(display_event.lane());
                for lane in display_event.active_lanes() {
                    lanes.insert(*lane);
                }
                let active_agents = display_event
                    .active_lanes()
                    .iter()
                    .filter(|lane| **lane > 0)
                    .count()
                    .max(usize::from(display_event.lane() > 0));
                max_active_agents = max_active_agents.max(active_agents);
            }
        }

        let start_clock = min_ts.map(|value| value.format("%H:%M:%S").to_string());
        let end_clock = max_ts.map(|value| value.format("%H:%M:%S").to_string());
        let active_agent_count = lanes
            .iter()
            .filter(|lane| **lane > 0)
            .count()
            .max(max_active_agents);
        let max_lane_index = lanes.iter().copied().max().unwrap_or(0);

        let mut row = CliTimelineRow::new(
            turn.turn_index,
            "turn",
            "turn",
            format!(
                "Turn {:>3} | {} agent events | user: {} | llm: {}",
                turn.turn_index + 1,
                turn.agent_events.len(),
                truncate(&user_preview, 80),
                truncate(&llm_summary, 120),
            ),
        );
        row.turn_index = Some(turn.turn_index + 1);
        row.source_index = Some(turn.anchor_source_index);
        row.start_timestamp = min_ts.map(|value| value.to_rfc3339());
        row.end_timestamp = max_ts.map(|value| value.to_rfc3339());
        row.start_clock = start_clock;
        row.end_clock = end_clock;
        row.duration_ms = duration_ms;
        row.task_count = Some(task_ids.len());
        row.user_message_count = Some(turn.user_events.len());
        row.agent_event_count = Some(turn.agent_events.len());
        row.tool_call_count = Some(tool_call_count);
        row.tool_result_count = Some(tool_result_count);
        row.file_ops_count = Some(file_ops_count);
        row.shell_ops_count = Some(shell_ops_count);
        row.error_count = Some(error_count);
        row.max_lane_index_in_turn = Some(max_lane_index);
        row.active_agent_count_in_turn = Some(active_agent_count);
        row.user_preview = Some(truncate(&user_preview, 180));
        row.llm_summary = Some(truncate(&llm_summary, 220));
        row.summary = Some(truncate(&llm_summary, 220));
        rows.push(row);
    }
    rows
}

fn source_index_turn_map(events: &[DisplayEvent<'_>]) -> HashMap<usize, usize> {
    let turns = extract_turns(events);
    let mut map = HashMap::new();
    for turn in turns {
        for display_idx in turn.start_display_index..=turn.end_display_index {
            if let Some(display_event) = events.get(display_idx) {
                map.entry(display_event.source_index())
                    .or_insert(turn.turn_index);
            }
        }
    }
    map
}

fn lane_marker_name(marker: LaneMarker) -> Option<&'static str> {
    match marker {
        LaneMarker::Fork => Some("fork"),
        LaneMarker::Merge => Some("merge"),
        LaneMarker::None => None,
    }
}

fn event_task_id(event: &Event) -> Option<String> {
    event
        .task_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            event
                .attributes
                .get("subagent_id")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        })
}

fn lane_cells(event: &DisplayEvent<'_>, lane_count: usize) -> String {
    let mut out = String::with_capacity(lane_count * 2);
    for lane in 0..lane_count {
        let active = event.active_lanes().contains(&lane);
        let ch = if lane == event.lane() {
            match event {
                DisplayEvent::SummaryRow { .. } => 'S',
                _ => match event.marker() {
                    LaneMarker::Fork => '+',
                    LaneMarker::Merge => '-',
                    LaneMarker::None => '*',
                },
            }
        } else if active {
            '|'
        } else {
            ' '
        };
        out.push(ch);
        if lane + 1 < lane_count {
            out.push(' ');
        }
    }
    out
}

fn event_display(event: &Event) -> (&'static str, String) {
    let kind = match event.event_type {
        EventType::UserMessage => "user",
        EventType::AgentMessage => "agent",
        EventType::SystemMessage => "system",
        EventType::Thinking => "think",
        EventType::ToolCall { .. } => "tool",
        EventType::ToolResult { is_error: true, .. } => "error",
        EventType::ToolResult { .. } => "result",
        EventType::FileRead { .. } => "read",
        EventType::CodeSearch { .. } => "search",
        EventType::FileSearch { .. } => "find",
        EventType::FileEdit { .. } => "edit",
        EventType::FileCreate { .. } => "create",
        EventType::FileDelete { .. } => "delete",
        EventType::ShellCommand { .. } => "shell",
        EventType::WebSearch { .. } => "web",
        EventType::WebFetch { .. } => "fetch",
        EventType::ImageGenerate { .. } => "image",
        EventType::VideoGenerate { .. } => "video",
        EventType::AudioGenerate { .. } => "audio",
        EventType::TaskStart { .. } => "start",
        EventType::TaskEnd { .. } => "end",
        EventType::Custom { .. } => "custom",
        _ => "other",
    };
    (
        kind,
        event_summary(&event.event_type, &event.content.blocks),
    )
}

fn event_summary(event_type: &EventType, blocks: &[ContentBlock]) -> String {
    match event_type {
        EventType::UserMessage | EventType::AgentMessage => first_text_line(blocks, 96),
        EventType::SystemMessage => String::new(),
        EventType::Thinking => "thinking".to_string(),
        EventType::ToolCall { name } => format!("{name}()"),
        EventType::ToolResult { name, is_error, .. } => {
            if *is_error {
                format!("{name} failed")
            } else {
                format!("{name} ok")
            }
        }
        EventType::FileRead { path } => short_path(path).to_string(),
        EventType::CodeSearch { query } => truncate(query, 80),
        EventType::FileSearch { pattern } => truncate(pattern, 80),
        EventType::FileEdit { path, diff } => {
            if let Some(d) = diff {
                let (add, del) = count_diff_lines(d);
                format!("{} +{} -{}", short_path(path), add, del)
            } else {
                short_path(path).to_string()
            }
        }
        EventType::FileCreate { path } => short_path(path).to_string(),
        EventType::FileDelete { path } => short_path(path).to_string(),
        EventType::ShellCommand { command, exit_code } => match exit_code {
            Some(code) => format!("{} => {}", truncate(command, 80), code),
            None => truncate(command, 80),
        },
        EventType::WebSearch { query } => truncate(query, 80),
        EventType::WebFetch { url } => truncate(url, 80),
        EventType::ImageGenerate { prompt }
        | EventType::VideoGenerate { prompt }
        | EventType::AudioGenerate { prompt } => truncate(prompt, 80),
        EventType::TaskStart { title } => {
            let title = title.as_deref().unwrap_or_default().trim();
            if title.is_empty() {
                "start".to_string()
            } else {
                format!("start {}", truncate(title, 140))
            }
        }
        EventType::TaskEnd { summary } => {
            let summary = summary.as_deref().unwrap_or_default().trim();
            if summary.is_empty() {
                "end".to_string()
            } else {
                format!("end {}", truncate(summary, 180))
            }
        }
        EventType::Custom { kind } => kind.clone(),
        _ => String::new(),
    }
}

fn lane_assignment_badge(event: &Event, lane: usize, marker: LaneMarker) -> Option<String> {
    if lane == 0 || marker != LaneMarker::Fork {
        return None;
    }
    if !matches!(event.event_type, EventType::TaskStart { .. }) {
        return None;
    }

    let task = event
        .task_id
        .as_deref()
        .map(compact_task_id)
        .unwrap_or_default();
    if task.is_empty() {
        Some(format!("[L{lane}]"))
    } else {
        Some(format!("[L{lane} {task}]"))
    }
}

fn compact_task_id(task_id: &str) -> String {
    let trimmed = task_id.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.chars().count() <= 18 {
        return trimmed.to_string();
    }
    let head: String = trimmed.chars().take(12).collect();
    let tail: String = trimmed
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{head}...{tail}")
}

fn first_text_line(blocks: &[ContentBlock], max_chars: usize) -> String {
    for block in blocks {
        if let ContentBlock::Text { text } = block {
            if let Some(line) = text.lines().next() {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    return truncate(trimmed, max_chars);
                }
            }
        }
    }
    String::new()
}

fn short_path(path: &str) -> &str {
    let parts: Vec<&str> = path.rsplitn(3, '/').collect();
    if parts.len() >= 2 {
        let start = path.len() - parts[0].len() - parts[1].len() - 1;
        &path[start..]
    } else {
        path
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let mut out = String::new();
        for ch in s.chars().take(max_len.saturating_sub(1)) {
            out.push(ch);
        }
        out.push('…');
        out
    }
}

fn count_diff_lines(diff: &str) -> (usize, usize) {
    let mut added = 0;
    let mut removed = 0;
    for line in diff.lines() {
        if line.starts_with('+') && !line.starts_with("+++") {
            added += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            removed += 1;
        }
    }
    (added, removed)
}
