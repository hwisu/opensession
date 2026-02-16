#![allow(dead_code)]

use crate::app::{extract_visible_turns, App, DisplayEvent};
use crate::session_timeline::LaneMarker;
use crate::theme::{self, Theme};
use chrono::{DateTime, Utc};
use opensession_core::trace::{ContentBlock, Event, EventType};
use ratatui::prelude::*;
use ratatui::widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap};
use std::collections::BTreeSet;

pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let session = match app.selected_session() {
        Some(s) => s.clone(),
        None => {
            let p = Paragraph::new("No session selected")
                .block(Theme::block_dim())
                .style(Style::new().fg(Color::DarkGray));
            frame.render_widget(p, area);
            return;
        }
    };

    let [header_area, bar_area, timeline_area] = Layout::vertical([
        Constraint::Length(7),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(area);

    render_session_header(frame, app, &session, header_area);
    app.detail_viewport_height = timeline_area.height.saturating_sub(2);

    let mut visible_events = app.get_visible_events(&session);
    if visible_events.is_empty() {
        let mut lines = vec![Line::from(Span::styled(
            "No events match the current filter.",
            Style::new().fg(Color::DarkGray),
        ))];
        if let Some(issue) = app.detail_issue_for_session(&session.session_id) {
            lines.push(Line::raw(""));
            lines.push(Line::from(Span::styled(
                "Detected session parse issue:",
                Style::new().fg(Theme::ACCENT_RED).bold(),
            )));
            lines.push(Line::from(Span::styled(
                issue.to_string(),
                Style::new().fg(Theme::ACCENT_RED),
            )));
        }
        let p = Paragraph::new(lines)
            .block(Theme::block_dim().title(" Timeline "))
            .wrap(Wrap { trim: false });
        frame.render_widget(p, timeline_area);
        return;
    }

    if app.detail_event_index >= visible_events.len() {
        app.detail_event_index = visible_events.len() - 1;
    }

    app.observe_linear_tail_proximity(visible_events.len());
    render_timeline_bar(frame, bar_area, &visible_events, app.detail_event_index);
    render_lane_timeline(frame, app, &mut visible_events, timeline_area);
}

fn render_session_header(
    frame: &mut Frame,
    app: &App,
    session: &opensession_core::trace::Session,
    area: Rect,
) {
    let title = session
        .context
        .title
        .as_deref()
        .unwrap_or(&session.session_id);
    let attrs = &session.context.attributes;
    let repo = attrs
        .get("git_repo_name")
        .or_else(|| attrs.get("repo"))
        .and_then(|v| v.as_str());
    let branch = attrs
        .get("git_branch")
        .or_else(|| attrs.get("branch"))
        .and_then(|v| v.as_str());

    let mut line1 = vec![Span::styled(
        title,
        Style::new().fg(Theme::TEXT_PRIMARY).bold(),
    )];
    if let Some(actor) = app.selected_session_actor_label() {
        let actor_color_key = actor.trim_start_matches('@').to_string();
        line1.push(Span::styled("  ", Style::new().fg(Theme::TEXT_MUTED)));
        line1.push(Span::styled(
            actor,
            Style::new().fg(theme::user_color(&actor_color_key)).bold(),
        ));
    }
    if let Some(r) = repo {
        line1.push(Span::styled("  ", Style::new()));
        line1.push(Span::styled(r, Style::new().fg(Theme::ACCENT_BLUE)));
        if let Some(b) = branch {
            line1.push(Span::styled(
                format!("/{b}"),
                Style::new().fg(Theme::ACCENT_GREEN),
            ));
        }
    }

    let line2 = vec![
        Span::styled(&session.agent.tool, Style::new().fg(Theme::ACCENT_ORANGE)),
        Span::styled(" | ", Style::new().fg(Theme::GUTTER)),
        Span::styled(&session.agent.model, Style::new().fg(Theme::ROLE_AGENT)),
        Span::styled(" | ", Style::new().fg(Theme::GUTTER)),
        Span::styled(
            format_duration(session.stats.duration_seconds),
            Style::new().fg(Theme::TEXT_SECONDARY),
        ),
    ];

    let line3 = vec![
        Span::styled(
            format!("{} prompts", session.stats.user_message_count),
            Style::new().fg(Theme::TEXT_SECONDARY),
        ),
        Span::styled(" | ", Style::new().fg(Theme::GUTTER)),
        Span::styled(
            format!("{} events", session.stats.event_count),
            Style::new().fg(Theme::TEXT_SECONDARY),
        ),
        Span::styled(" | ", Style::new().fg(Theme::GUTTER)),
        Span::styled(
            format!("{} files", session.stats.files_changed),
            Style::new().fg(Theme::ACCENT_PURPLE),
        ),
        Span::styled(" | ", Style::new().fg(Theme::GUTTER)),
        Span::styled(
            format!(
                "+{} -{}",
                session.stats.lines_added, session.stats.lines_removed
            ),
            Style::new().fg(Theme::TEXT_MUTED),
        ),
    ];

    let mut line4 = vec![Span::styled(
        session
            .context
            .created_at
            .format("%Y-%m-%d %H:%M")
            .to_string(),
        Style::new().fg(Color::DarkGray),
    )];
    if !session.context.tags.is_empty() {
        let tags = session
            .context
            .tags
            .iter()
            .map(|t| format!("#{t}"))
            .collect::<Vec<_>>()
            .join(" ");
        line4.push(Span::styled("  ", Style::new()));
        line4.push(Span::styled(tags, Style::new().fg(Theme::TAG_COLOR)));
    }

    let header = Paragraph::new(vec![
        Line::from(line1),
        Line::from(line2),
        Line::from(line3),
        Line::from(line4),
    ])
    .block(Theme::block().padding(ratatui::widgets::Padding::new(1, 1, 0, 0)));

    frame.render_widget(header, area);
}

fn render_lane_timeline(
    frame: &mut Frame,
    app: &mut App,
    events: &mut [DisplayEvent<'_>],
    area: Rect,
) {
    let total_visible = events.len();
    let current_idx = app.detail_event_index.min(total_visible.saturating_sub(1));
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
    let (turn_lookup, turn_groups) = build_linear_turn_group_lookup(events);
    let mut active_turn: Option<usize> = None;
    let mut active_task: Option<String> = None;

    let mut lines: Vec<Line> = Vec::new();
    lines.push(timeline_lane_legend_line());
    lines.push(Line::raw(""));
    let mut event_line_positions: Vec<usize> = Vec::with_capacity(total_visible);

    for (i, display_event) in events.iter().enumerate() {
        let event = display_event.event();
        let selected = i == current_idx;
        let turn_idx = turn_lookup.get(i).copied().flatten();

        if turn_idx != active_turn {
            if i > 0 {
                lines.push(Line::raw(""));
            }
            if let Some(turn_idx) = turn_idx {
                if let Some(group) = turn_groups.get(turn_idx) {
                    lines.push(timeline_turn_group_line(group));
                }
            }
            active_turn = turn_idx;
            active_task = None;
        }

        if i > 0 {
            let previous_timestamp = events[i - 1].event().timestamp;
            if let Some(separator_line) =
                timeline_separator_line(previous_timestamp, event.timestamp)
            {
                lines.push(separator_line);
            }
        }

        let task_key = display_event_task_key(display_event);
        if task_key != active_task {
            if let Some(task_key) = task_key.as_deref() {
                lines.push(timeline_task_group_line(task_key, display_event.lane()));
            }
            active_task = task_key;
        }

        event_line_positions.push(lines.len());

        let mut spans = Vec::new();
        spans.push(Span::styled(
            if selected { ">" } else { " " },
            Style::new().fg(if selected {
                Theme::ACCENT_BLUE
            } else {
                Theme::TEXT_MUTED
            }),
        ));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            event.timestamp.format("%H:%M:%S").to_string(),
            Style::new().fg(Theme::TEXT_MUTED),
        ));
        spans.push(Span::raw("  "));
        spans.push(Span::styled("│", Style::new().fg(Theme::TREE).bold()));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            format!("{:<11}", display_event_lane_label(display_event)),
            Style::new().fg(Theme::ACCENT_TEAL),
        ));
        let active_agents = display_event_agent_count(display_event);
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format_active_agents(active_agents),
            if active_agents > 1 {
                Style::new().fg(Theme::ACCENT_CYAN).bold()
            } else {
                Style::new().fg(Theme::TEXT_MUTED)
            },
        ));
        spans.push(Span::raw(" "));

        match display_event {
            DisplayEvent::Collapsed { count, kind, .. } => {
                spans.push(Span::styled(
                    format!("{kind} x{count}"),
                    Style::new().fg(Theme::ROLE_AGENT).bold(),
                ));
            }
            DisplayEvent::Single {
                event,
                lane: _,
                marker: _,
                ..
            } => {
                let (kind, kind_color) = event_type_display(&event.event_type);
                if matches!(
                    event.event_type,
                    EventType::TaskStart { .. } | EventType::TaskEnd { .. }
                ) {
                    spans.push(Span::styled(
                        format!(" {kind:^8} "),
                        Style::new().fg(Color::Black).bg(kind_color).bold(),
                    ));
                } else {
                    spans.push(Span::styled(
                        format!("{kind:>10} "),
                        Style::new().fg(kind_color).bold(),
                    ));
                }
                spans.push(Span::styled(
                    event_compact_summary(&event.event_type, &event.content.blocks),
                    Style::new().fg(Theme::TEXT_PRIMARY),
                ));
            }
        }

        let line_style = if selected {
            Style::new().bg(Theme::BG_SURFACE)
        } else {
            Style::new()
        };
        lines.push(Line::from(spans).style(line_style));

        let expanded = !app.expanded_events.contains(&i);
        if expanded {
            append_event_detail_rows(&mut lines, display_event, 3);
        }
    }

    let visible_height = area.height.saturating_sub(2) as usize;
    let target_line = event_line_positions.get(current_idx).copied().unwrap_or(0);
    let max_scroll = lines.len().saturating_sub(visible_height);
    let mut scroll = if target_line >= visible_height {
        target_line.saturating_sub(visible_height / 3)
    } else {
        0
    };
    if app.live_mode && app.detail_follow_state().is_following {
        scroll = max_scroll;
    }
    app.detail_scroll = scroll as u16;

    let timeline = Paragraph::new(lines.clone())
        .block(Theme::block().title(format!(
            " Timeline ({}/{}) lanes:{} ",
            current_idx + 1,
            total_visible,
            lane_count
        )))
        .wrap(Wrap { trim: false })
        .scroll((app.detail_scroll, app.detail_h_scroll));
    frame.render_widget(timeline, area);

    if lines.len() > visible_height {
        let mut scrollbar_state = ScrollbarState::new(lines.len()).position(scroll);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .thumb_style(Style::new().fg(Theme::TEXT_MUTED));
        frame.render_stateful_widget(
            scrollbar,
            area.inner(Margin {
                vertical: 1,
                horizontal: 0,
            }),
            &mut scrollbar_state,
        );
    }
}

#[derive(Debug, Clone)]
struct LinearTurnGroupMeta {
    turn_number: usize,
    event_count: usize,
    task_count: usize,
    prompt: String,
}

fn build_linear_turn_group_lookup(
    events: &[DisplayEvent<'_>],
) -> (Vec<Option<usize>>, Vec<LinearTurnGroupMeta>) {
    let mut lookup = vec![None; events.len()];
    let turns = extract_visible_turns(events);
    let mut groups = Vec::with_capacity(turns.len());
    if events.is_empty() {
        return (lookup, groups);
    }
    if turns.is_empty() {
        lookup.fill(Some(0));
        let mut task_ids: BTreeSet<String> = BTreeSet::new();
        for event in events {
            if let Some(task_id) = event
                .event()
                .task_id
                .as_deref()
                .map(str::trim)
                .filter(|id| !id.is_empty())
            {
                task_ids.insert(compact_task_id(task_id));
            }
        }
        let prompt = events
            .iter()
            .filter_map(|event| {
                first_meaningful_text_line_opt(&event.event().content.blocks, 72)
                    .or_else(|| first_text_line_opt(&event.event().content.blocks, 72))
            })
            .find(|text| !text.is_empty())
            .or_else(|| {
                events.first().map(|event| {
                    event_compact_summary(&event.event().event_type, &event.event().content.blocks)
                })
            })
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| "(prompt omitted)".to_string());
        groups.push(LinearTurnGroupMeta {
            turn_number: 1,
            event_count: events.len(),
            task_count: task_ids.len(),
            prompt,
        });
        return (lookup, groups);
    }

    for (idx, turn) in turns.iter().enumerate() {
        let start = turn.start_display_index.min(events.len().saturating_sub(1));
        let end = turn.end_display_index.min(events.len().saturating_sub(1));
        if start > end {
            continue;
        }
        for entry in lookup.iter_mut().take(end + 1).skip(start) {
            *entry = Some(idx);
        }

        let prompt = turn
            .user_events
            .iter()
            .filter_map(|event| {
                first_meaningful_text_line_opt(&event.content.blocks, 72)
                    .or_else(|| first_text_line_opt(&event.content.blocks, 72))
            })
            .find(|text| !text.is_empty())
            .unwrap_or_else(|| "(prompt omitted)".to_string());
        let mut task_ids: BTreeSet<String> = BTreeSet::new();
        for event in turn.user_events.iter().chain(turn.agent_events.iter()) {
            if let Some(task_id) = event
                .task_id
                .as_deref()
                .map(str::trim)
                .filter(|id| !id.is_empty())
            {
                task_ids.insert(compact_task_id(task_id));
            }
        }
        groups.push(LinearTurnGroupMeta {
            turn_number: turn.turn_index + 1,
            event_count: turn.user_events.len() + turn.agent_events.len(),
            task_count: task_ids.len(),
            prompt,
        });
    }

    (lookup, groups)
}

fn timeline_turn_group_line(group: &LinearTurnGroupMeta) -> Line<'static> {
    let mut spans = vec![
        Span::styled("  ── ", Style::new().fg(Theme::GUTTER)),
        Span::styled(
            format!("Turn #{}", group.turn_number),
            Style::new().fg(Theme::ACCENT_BLUE).bold(),
        ),
        Span::styled(
            format!(" · {} events", group.event_count),
            Style::new().fg(Theme::TEXT_MUTED),
        ),
    ];
    if group.task_count > 0 {
        spans.push(Span::styled(
            format!(" · {} tasks", group.task_count),
            Style::new().fg(Theme::ACCENT_TEAL),
        ));
    }
    spans.push(Span::styled(" · ", Style::new().fg(Theme::GUTTER)));
    spans.push(Span::styled(
        group.prompt.clone(),
        Style::new().fg(Theme::TEXT_SECONDARY),
    ));
    Line::from(spans)
}

fn display_event_task_key(event: &DisplayEvent<'_>) -> Option<String> {
    event
        .event()
        .task_id
        .as_deref()
        .and_then(task_id_display_label)
}

fn timeline_task_group_line(task_key: &str, lane: usize) -> Line<'static> {
    let lane_label = if lane == 0 {
        "main".to_string()
    } else {
        format!("L{lane}")
    };
    Line::from(vec![
        Span::styled("    ↳ ", Style::new().fg(Theme::ACCENT_TEAL)),
        Span::styled(
            format!("task {task_key}"),
            Style::new().fg(Theme::ACCENT_TEAL).bold(),
        ),
        Span::styled(
            format!(" · lane {lane_label}"),
            Style::new().fg(Theme::TEXT_MUTED),
        ),
    ])
}

fn display_event_lane_label(event: &DisplayEvent<'_>) -> String {
    let lane_label = if event.lane() == 0 {
        "main".to_string()
    } else {
        format!("L{}", event.lane())
    };
    match event.marker() {
        LaneMarker::Fork => format!("{lane_label} fork"),
        LaneMarker::Merge => format!("{lane_label} merge"),
        LaneMarker::None => lane_label,
    }
}

fn display_event_agent_count(event: &DisplayEvent<'_>) -> usize {
    let mut lanes: BTreeSet<usize> = event.active_lanes().iter().copied().collect();
    lanes.insert(event.lane());
    let active_agents = lanes.into_iter().filter(|lane| *lane > 0).count();
    active_agents.max(1)
}

fn format_active_agents(active_agents: usize) -> String {
    if active_agents == 1 {
        "1 agent".to_string()
    } else {
        format!("{active_agents} agents")
    }
}

fn timeline_lane_legend_line() -> Line<'static> {
    Line::from(vec![
        Span::styled("  Legend ", Style::new().fg(Theme::TEXT_KEY).bold()),
        Span::styled("lane(main/Ln)", Style::new().fg(Theme::ACCENT_TEAL)),
        Span::styled(" │ ", Style::new().fg(Theme::GUTTER)),
        Span::styled("N agents", Style::new().fg(Theme::ACCENT_CYAN)),
        Span::styled(" │ ", Style::new().fg(Theme::GUTTER)),
        Span::styled(
            "Enter: fold/unfold event",
            Style::new().fg(Theme::TEXT_MUTED),
        ),
    ])
}

fn timeline_separator_line(
    previous: DateTime<Utc>,
    current: DateTime<Utc>,
) -> Option<Line<'static>> {
    if current <= previous {
        return None;
    }

    let previous_minute_bucket = previous.timestamp().div_euclid(60);
    let current_minute_bucket = current.timestamp().div_euclid(60);
    if previous_minute_bucket == current_minute_bucket {
        return None;
    }

    let delta_seconds = (current - previous).num_seconds().max(1);
    let elapsed = format_elapsed_short(delta_seconds);
    let previous_five_minute_bucket = previous.timestamp().div_euclid(300);
    let current_five_minute_bucket = current.timestamp().div_euclid(300);

    if previous_five_minute_bucket != current_five_minute_bucket {
        Some(Line::from(vec![
            Span::styled("  ◆ ", Style::new().fg(Theme::ACCENT_YELLOW).bold()),
            Span::styled(
                format!("{} (+{})", current.format("%H:%M"), elapsed),
                Style::new().fg(Theme::ACCENT_YELLOW),
            ),
        ]))
    } else {
        Some(Line::from(vec![
            Span::styled("  · ", Style::new().fg(Theme::TEXT_MUTED)),
            Span::styled(
                format!("{} (+{})", current.format("%H:%M"), elapsed),
                Style::new().fg(Theme::TEXT_MUTED),
            ),
        ]))
    }
}

fn format_elapsed_short(delta_seconds: i64) -> String {
    if delta_seconds < 60 {
        format!("{delta_seconds}s")
    } else if delta_seconds < 3600 {
        let minutes = delta_seconds / 60;
        let seconds = delta_seconds % 60;
        if seconds == 0 {
            format!("{minutes}m")
        } else {
            format!("{minutes}m{seconds}s")
        }
    } else {
        let hours = delta_seconds / 3600;
        let minutes = (delta_seconds % 3600) / 60;
        format!("{hours}h{minutes}m")
    }
}

fn append_event_detail_rows<'a>(
    lines: &mut Vec<Line<'a>>,
    display_event: &DisplayEvent<'_>,
    max_preview_lines: usize,
) {
    match display_event {
        DisplayEvent::Single {
            event,
            source_index,
            lane,
            marker,
            ..
        } => {
            let mut meta_parts = vec![format!("#{source_index}")];
            let lane_label = if *lane == 0 {
                "main".to_string()
            } else {
                format!("L{lane}")
            };
            meta_parts.push(format!("lane {lane_label}"));
            if let Some(marker) = lane_marker_text(*marker) {
                meta_parts.push(marker.to_string());
            }
            if let Some(task) = display_event_task_key(display_event) {
                meta_parts.push(format!("task {task}"));
            }
            if let Some(duration_ms) = event.duration_ms {
                meta_parts.push(format!("{duration_ms}ms"));
            }
            lines.push(timeline_detail_kv_line(
                "meta",
                meta_parts.join(" · "),
                Style::new().fg(Theme::TEXT_MUTED),
            ));

            let action = event_compact_summary(&event.event_type, &event.content.blocks);
            if !action.is_empty() {
                lines.push(timeline_detail_kv_line(
                    "action",
                    action,
                    Style::new().fg(Theme::TEXT_SECONDARY),
                ));
            }

            if matches!(
                event.event_type,
                EventType::ToolResult { is_error: true, .. }
            ) {
                lines.push(timeline_detail_kv_line(
                    "status",
                    "error".to_string(),
                    Style::new().fg(Theme::ACCENT_RED).bold(),
                ));
            } else if matches!(event.event_type, EventType::ToolResult { .. }) {
                lines.push(timeline_detail_kv_line(
                    "status",
                    "ok".to_string(),
                    Style::new().fg(Theme::ACCENT_GREEN),
                ));
            }

            append_content_preview(lines, event, max_preview_lines);
        }
        DisplayEvent::Collapsed {
            first,
            source_index,
            count,
            kind,
            ..
        } => {
            lines.push(timeline_detail_kv_line(
                "group",
                format!("{kind} ×{count} (from #{source_index})"),
                Style::new().fg(Theme::TEXT_SECONDARY),
            ));
            append_content_preview(lines, first, max_preview_lines.min(2));
        }
    }
}

fn timeline_detail_kv_line<'a>(label: &str, value: String, value_style: Style) -> Line<'a> {
    Line::from(vec![
        Span::styled("    | ", Style::new().fg(Theme::GUTTER)),
        Span::styled(format!("{label:<7} "), Style::new().fg(Theme::TEXT_MUTED)),
        Span::styled(value, value_style),
    ])
}

fn append_content_preview<'a>(lines: &mut Vec<Line<'a>>, event: &Event, max_lines: usize) {
    if let EventType::FileEdit {
        diff: Some(diff), ..
    } = &event.event_type
    {
        for line in diff.lines().take(max_lines) {
            let style = if line.starts_with('+') {
                Style::new().fg(Theme::ACCENT_GREEN)
            } else if line.starts_with('-') {
                Style::new().fg(Theme::ACCENT_RED)
            } else {
                Style::new().fg(Theme::TEXT_MUTED)
            };
            lines.push(Line::from(vec![
                Span::styled("    | ", Style::new().fg(Theme::GUTTER)),
                Span::styled(truncate(line, 120), style),
            ]));
        }
        return;
    }

    for block in &event.content.blocks {
        match block {
            ContentBlock::Text { text } => {
                let mut shown = 0usize;
                for line in text.lines() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || is_low_signal_text_line(trimmed) {
                        continue;
                    }
                    lines.push(Line::from(vec![
                        Span::styled("    | ", Style::new().fg(Theme::GUTTER)),
                        Span::styled(
                            truncate(trimmed, 120),
                            Style::new().fg(Theme::TEXT_SECONDARY),
                        ),
                    ]));
                    shown += 1;
                    if shown >= max_lines {
                        break;
                    }
                }
                if shown > 0 {
                    return;
                }
            }
            ContentBlock::Code { code, .. } => {
                let mut shown = 0usize;
                for line in code.lines() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    lines.push(Line::from(vec![
                        Span::styled("    | ", Style::new().fg(Theme::GUTTER)),
                        Span::styled(
                            truncate(trimmed, 120),
                            Style::new().fg(Theme::TEXT_SECONDARY),
                        ),
                    ]));
                    shown += 1;
                    if shown >= max_lines {
                        break;
                    }
                }
                if shown > 0 {
                    return;
                }
            }
            ContentBlock::Json { data } => {
                if let Some(hint) = json_value_hint(data, 100) {
                    lines.push(Line::from(vec![
                        Span::styled("    | ", Style::new().fg(Theme::GUTTER)),
                        Span::styled(truncate(&hint, 120), Style::new().fg(Theme::TEXT_SECONDARY)),
                    ]));
                    return;
                }
            }
            _ => {}
        }
    }
}

fn event_type_display(event_type: &EventType) -> (&'static str, Color) {
    match event_type {
        EventType::UserMessage => ("user", Theme::ROLE_USER),
        EventType::AgentMessage => ("agent", Theme::ROLE_AGENT_BRIGHT),
        EventType::SystemMessage => ("system", Theme::ROLE_SYSTEM),
        EventType::Thinking => ("think", Theme::ACCENT_PURPLE),
        EventType::ToolCall { .. } => ("tool", Theme::ACCENT_YELLOW),
        EventType::ToolResult { is_error: true, .. } => ("error", Theme::ACCENT_RED),
        EventType::ToolResult { .. } => ("result", Theme::ACCENT_GREEN),
        EventType::FileRead { .. } => ("read", Theme::ROLE_AGENT_BRIGHT),
        EventType::CodeSearch { .. } => ("search", Theme::ACCENT_CYAN),
        EventType::FileSearch { .. } => ("find", Theme::ACCENT_TEAL),
        EventType::FileEdit { .. } => ("edit", Theme::ACCENT_CYAN),
        EventType::FileCreate { .. } => ("create", Theme::ACCENT_GREEN),
        EventType::FileDelete { .. } => ("delete", Theme::ACCENT_RED),
        EventType::ShellCommand { .. } => ("shell", Theme::ACCENT_YELLOW),
        EventType::WebSearch { .. } => ("web", Theme::ACCENT_PURPLE),
        EventType::WebFetch { .. } => ("fetch", Theme::ACCENT_PURPLE),
        EventType::ImageGenerate { .. } => ("image", Theme::ACCENT_BLUE),
        EventType::VideoGenerate { .. } => ("video", Theme::ACCENT_BLUE),
        EventType::AudioGenerate { .. } => ("audio", Theme::ACCENT_BLUE),
        EventType::TaskStart { .. } => ("start", Theme::ROLE_TASK),
        EventType::TaskEnd { .. } => ("end", Theme::ROLE_TASK),
        EventType::Custom { .. } => ("custom", Theme::ACCENT_CYAN),
        _ => ("other", Theme::TEXT_MUTED),
    }
}

fn event_compact_summary(event_type: &EventType, blocks: &[ContentBlock]) -> String {
    match event_type {
        EventType::UserMessage => {
            let text = first_text_line(blocks, 56);
            if text.is_empty() {
                "(user prompt)".to_string()
            } else {
                text
            }
        }
        EventType::AgentMessage => {
            let text = first_meaningful_text_line_opt(blocks, 56)
                .or_else(|| first_text_line_opt(blocks, 56))
                .unwrap_or_default();
            if text.is_empty() {
                "(agent reply)".to_string()
            } else {
                text
            }
        }
        EventType::SystemMessage => "(system)".to_string(),
        EventType::Thinking => first_meaningful_text_line_opt(blocks, 56)
            .or_else(|| first_text_line_opt(blocks, 56))
            .unwrap_or_else(|| "thinking".to_string()),
        EventType::ToolCall { name } => tool_call_compact_summary(name, blocks),
        EventType::ToolResult { name, is_error, .. } => {
            let hint = tool_result_hint(name, blocks, 52);
            if *is_error {
                if let Some(hint) = hint {
                    format!("{name} error: {hint}")
                } else {
                    format!("{name} error")
                }
            } else if let Some(hint) = hint {
                format!("{name}: {hint}")
            } else {
                format!("{name} ok")
            }
        }
        EventType::FileRead { path } => display_path_label(path),
        EventType::CodeSearch { query } => truncate(query, 52),
        EventType::FileSearch { pattern } => truncate(pattern, 52),
        EventType::FileEdit { path, diff } => {
            let label = display_path_label(path);
            if let Some(d) = diff {
                let (add, del) = count_diff_lines(d);
                format!("{label} +{add} -{del}")
            } else {
                label
            }
        }
        EventType::FileCreate { path } => format!("+ {}", display_path_label(path)),
        EventType::FileDelete { path } => format!("- {}", display_path_label(path)),
        EventType::ShellCommand { command, exit_code } => {
            let cmd = compact_shell_command(command, 52);
            match exit_code {
                Some(code) if *code != 0 => format!("{cmd} => {code}"),
                _ => cmd,
            }
        }
        EventType::WebSearch { query } => truncate(query, 52),
        EventType::WebFetch { url } => truncate(url, 52),
        EventType::ImageGenerate { prompt }
        | EventType::VideoGenerate { prompt }
        | EventType::AudioGenerate { prompt } => truncate(prompt, 52),
        EventType::TaskStart { title } => title
            .as_deref()
            .map(|text| compact_text_snippet(text, 48))
            .filter(|text| !text.is_empty())
            .map(|text| format!("start {text}"))
            .unwrap_or_else(|| "start".to_string()),
        EventType::TaskEnd { summary } => summary
            .as_deref()
            .map(|text| compact_text_snippet(text, 48))
            .filter(|text| !text.is_empty())
            .map(|text| format!("end {text}"))
            .unwrap_or_else(|| "end".to_string()),
        EventType::Custom { kind } => compact_text_snippet(kind, 52),
        _ => String::new(),
    }
}

fn tool_result_hint(name: &str, blocks: &[ContentBlock], max_len: usize) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "exec_command" | "shell" | "bash" | "execute_command" | "spawn_process"
    ) {
        return first_meaningful_text_line_opt(blocks, max_len)
            .or_else(|| first_code_line(blocks, max_len));
    }
    first_meaningful_text_line_opt(blocks, max_len)
        .or_else(|| first_json_block_hint(blocks, max_len))
        .or_else(|| first_code_line(blocks, max_len))
}

fn tool_call_compact_summary(name: &str, blocks: &[ContentBlock]) -> String {
    let lower = name.to_ascii_lowercase();
    let json = first_json_block(blocks);
    match lower.as_str() {
        "exec_command" | "shell" | "bash" | "execute_command" | "spawn_process" => {
            if let Some(cmd) = tool_command_hint(blocks, json, 56) {
                format!("run {cmd}")
            } else {
                "run command".to_string()
            }
        }
        "write_stdin" => {
            let chars = json
                .and_then(|value| json_find_string(value, &["chars", "text", "input"]))
                .map(|value| compact_text_snippet(&value, 28));
            let target = json.and_then(|value| json_find_string(value, &["session_id", "id"]));
            match (chars, target) {
                (Some(chars), Some(target)) => format!("stdin {chars} -> {target}"),
                (Some(chars), None) => format!("stdin {chars}"),
                (None, Some(target)) => format!("stdin -> {target}"),
                (None, None) => "stdin update".to_string(),
            }
        }
        "read_file" | "read" | "view" => tool_path_hint(blocks, json)
            .map(|path| format!("read {path}"))
            .unwrap_or_else(|| "read file".to_string()),
        "write_file" | "write_to_file" | "create_file" | "create" => tool_path_hint(blocks, json)
            .map(|path| format!("write {path}"))
            .unwrap_or_else(|| "write file".to_string()),
        "apply_patch" | "apply_diff" | "replace_in_file" | "search_and_replace"
        | "insert_content" => tool_path_hint(blocks, json)
            .map(|path| format!("edit {path}"))
            .unwrap_or_else(|| "edit file".to_string()),
        "search_query" | "websearch" | "web_search" => tool_query_hint(json)
            .map(|query| format!("search {query}"))
            .unwrap_or_else(|| "search web".to_string()),
        "image_query" => tool_query_hint(json)
            .map(|query| format!("image {query}"))
            .unwrap_or_else(|| "search images".to_string()),
        "grep" | "search_files" | "find_references" | "find" => {
            if let Some(pattern) = tool_pattern_hint(json) {
                format!("find {pattern}")
            } else {
                name.to_string()
            }
        }
        "list_files" | "glob" => json
            .and_then(|value| json_find_string(value, &["path", "pattern", "glob", "cwd"]))
            .map(|value| format!("list {}", compact_text_snippet(&value, 44)))
            .unwrap_or_else(|| "list files".to_string()),
        "open" => json
            .and_then(|value| json_find_string(value, &["ref_id", "url"]))
            .map(|value| format!("open {}", compact_text_snippet(&value, 44)))
            .unwrap_or_else(|| "open page".to_string()),
        "click" => {
            let target = json.and_then(|value| json_find_string(value, &["ref_id", "url"]));
            let id = json
                .and_then(|value| json_find_u64(value, &["id"]))
                .map(|value| value.to_string());
            match (target, id) {
                (Some(target), Some(id)) => format!("click {id} in {target}"),
                (None, Some(id)) => format!("click {id}"),
                (Some(target), None) => format!("click in {target}"),
                (None, None) => "click".to_string(),
            }
        }
        "weather" => json
            .and_then(|value| json_find_string(value, &["location"]))
            .map(|value| format!("weather {}", compact_text_snippet(&value, 44)))
            .unwrap_or_else(|| "weather".to_string()),
        "finance" => json
            .and_then(|value| json_find_string(value, &["ticker", "symbol"]))
            .map(|value| format!("quote {}", compact_text_snippet(&value, 44)))
            .unwrap_or_else(|| "quote".to_string()),
        "time" => json
            .and_then(|value| json_find_string(value, &["utc_offset"]))
            .map(|value| format!("time {value}"))
            .unwrap_or_else(|| "time".to_string()),
        "sports" => {
            let league = json.and_then(|value| json_find_string(value, &["league"]));
            let action = json.and_then(|value| json_find_string(value, &["fn"]));
            match (league, action) {
                (Some(league), Some(action)) => format!("sports {league} {action}"),
                (Some(league), None) => format!("sports {league}"),
                _ => "sports".to_string(),
            }
        }
        "update_plan" => {
            let steps = json
                .and_then(|value| json_find_array_len(value, &["plan"]))
                .unwrap_or(0);
            if steps > 0 {
                format!("update plan ({steps} steps)")
            } else {
                "update plan".to_string()
            }
        }
        "request_user_input" => {
            let questions = json
                .and_then(|value| json_find_array_len(value, &["questions"]))
                .unwrap_or(0);
            if questions > 0 {
                format!("request input ({questions})")
            } else {
                "request input".to_string()
            }
        }
        "parallel" => {
            let tools = json
                .and_then(|value| json_find_array_len(value, &["tool_uses"]))
                .unwrap_or(0);
            if tools > 0 {
                format!("run {tools} tools")
            } else {
                "run tools".to_string()
            }
        }
        _ => {
            if let Some(hint) = first_json_block_hint(blocks, 48) {
                format!("{name}: {hint}")
            } else if let Some(hint) = first_meaningful_text_line_opt(blocks, 48) {
                format!("{name}: {hint}")
            } else {
                name.to_string()
            }
        }
    }
}

fn tool_command_hint(
    blocks: &[ContentBlock],
    json: Option<&serde_json::Value>,
    max_len: usize,
) -> Option<String> {
    if let Some(code) = first_code_line(blocks, max_len) {
        return Some(compact_shell_command(&code, max_len));
    }
    let cmd = json
        .and_then(|value| json_find_string(value, &["cmd", "command"]))
        .map(|value| compact_shell_command(&value, max_len));
    if cmd.is_some() {
        return cmd;
    }
    json.and_then(|value| {
        json_find_command_array(value).map(|parts| compact_shell_command(&parts.join(" "), max_len))
    })
}

fn tool_query_hint(json: Option<&serde_json::Value>) -> Option<String> {
    json.and_then(|value| json_find_string(value, &["q", "query", "text"]))
        .map(|value| compact_text_snippet(&value, 44))
        .filter(|value| !value.is_empty())
}

fn tool_pattern_hint(json: Option<&serde_json::Value>) -> Option<String> {
    json.and_then(|value| json_find_string(value, &["pattern", "regex", "content_pattern"]))
        .map(|value| compact_text_snippet(&value, 44))
        .filter(|value| !value.is_empty())
}

fn tool_path_hint(blocks: &[ContentBlock], json: Option<&serde_json::Value>) -> Option<String> {
    let from_json = json.and_then(|value| {
        json_find_string(
            value,
            &[
                "path",
                "file_path",
                "filePath",
                "filepath",
                "target_path",
                "targetPath",
                "file",
                "filename",
                "ref_id",
                "uri",
                "url",
            ],
        )
    });
    let from_text = first_text_line_opt(blocks, 64);
    from_json
        .or(from_text)
        .map(|value| compact_text_snippet(&value, 64))
        .filter(|value| !is_low_signal_value(value))
        .map(|value| {
            if value.contains('/') {
                short_path(&value).to_string()
            } else {
                value
            }
        })
}

fn display_path_label(path: &str) -> String {
    if is_low_signal_value(path) {
        "file".to_string()
    } else {
        short_path(path).to_string()
    }
}

fn task_id_display_label(task_id: &str) -> Option<String> {
    let trimmed = task_id.trim();
    if trimmed.is_empty() || is_low_signal_task_id(trimmed) {
        None
    } else {
        Some(compact_task_id(trimmed))
    }
}

fn is_low_signal_task_id(task_id: &str) -> bool {
    let lower = task_id.trim().to_ascii_lowercase();
    lower.starts_with("call_") || lower.starts_with("call-") || lower.starts_with("task:call_")
}

fn is_low_signal_value(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    lower == "unknown" || lower == "(unknown)" || lower == "null" || is_low_signal_task_id(trimmed)
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
    format!("{head}…{tail}")
}

fn first_json_block(blocks: &[ContentBlock]) -> Option<&serde_json::Value> {
    blocks.iter().find_map(|block| {
        if let ContentBlock::Json { data } = block {
            Some(data)
        } else {
            None
        }
    })
}

fn json_find_string(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    match value {
        serde_json::Value::Object(map) => {
            for key in keys {
                if let Some(raw) = map.get(*key).and_then(|entry| entry.as_str()) {
                    let trimmed = raw.trim();
                    if !is_low_signal_value(trimmed) {
                        return Some(trimmed.to_string());
                    }
                }
            }
            for nested in map.values() {
                if let Some(found) = json_find_string(nested, keys) {
                    return Some(found);
                }
            }
            None
        }
        serde_json::Value::Array(values) => {
            for nested in values {
                if let Some(found) = json_find_string(nested, keys) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn json_find_u64(value: &serde_json::Value, keys: &[&str]) -> Option<u64> {
    match value {
        serde_json::Value::Object(map) => {
            for key in keys {
                if let Some(num) = map.get(*key).and_then(|entry| entry.as_u64()) {
                    return Some(num);
                }
            }
            for nested in map.values() {
                if let Some(found) = json_find_u64(nested, keys) {
                    return Some(found);
                }
            }
            None
        }
        serde_json::Value::Array(values) => {
            for nested in values {
                if let Some(found) = json_find_u64(nested, keys) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn json_find_array_len(value: &serde_json::Value, keys: &[&str]) -> Option<usize> {
    match value {
        serde_json::Value::Object(map) => {
            for key in keys {
                if let Some(values) = map.get(*key).and_then(|entry| entry.as_array()) {
                    return Some(values.len());
                }
            }
            for nested in map.values() {
                if let Some(found) = json_find_array_len(nested, keys) {
                    return Some(found);
                }
            }
            None
        }
        serde_json::Value::Array(values) => {
            for nested in values {
                if let Some(found) = json_find_array_len(nested, keys) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn json_find_command_array(value: &serde_json::Value) -> Option<Vec<String>> {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(parts) = map.get("command").and_then(|entry| entry.as_array()) {
                let command: Vec<String> = parts
                    .iter()
                    .filter_map(|entry| entry.as_str())
                    .map(str::trim)
                    .filter(|entry| !entry.is_empty())
                    .map(ToString::to_string)
                    .collect();
                if !command.is_empty() {
                    return Some(command);
                }
            }
            for nested in map.values() {
                if let Some(found) = json_find_command_array(nested) {
                    return Some(found);
                }
            }
            None
        }
        serde_json::Value::Array(values) => {
            for nested in values {
                if let Some(found) = json_find_command_array(nested) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn first_text_line(blocks: &[ContentBlock], max_chars: usize) -> String {
    for block in blocks {
        if let ContentBlock::Text { text } = block {
            if let Some(line) = text.lines().next() {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    return compact_text_snippet(trimmed, max_chars);
                }
            }
        }
    }
    String::new()
}

fn first_text_line_opt(blocks: &[ContentBlock], max_len: usize) -> Option<String> {
    let line = first_text_line(blocks, max_len);
    if line.trim().is_empty() {
        None
    } else {
        Some(line)
    }
}

fn is_low_signal_text_line(line: &str) -> bool {
    let trimmed = line.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower.is_empty() {
        return true;
    }
    if matches!(lower.as_str(), "think reasoning" | "edit unknown" | "()") {
        return true;
    }
    if lower.starts_with("chunk id:") || lower.contains("chunk id:") {
        return true;
    }
    if lower.starts_with("wall time:")
        || lower.starts_with("process exited with code")
        || lower.starts_with("original token count:")
        || lower.starts_with("token count:")
        || (lower.starts_with("result ") && lower.contains("output:"))
    {
        return true;
    }
    if lower.contains("[task:call_") || (lower.contains("[l") && lower.contains(" call_")) {
        return true;
    }
    if trimmed
        .chars()
        .all(|ch| ch == '*' || ch == '|' || ch == '+' || ch == '-' || ch == ' ')
    {
        return true;
    }
    false
}

fn first_meaningful_text_line_opt(blocks: &[ContentBlock], max_len: usize) -> Option<String> {
    for block in blocks {
        if let ContentBlock::Text { text } = block {
            for line in text.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || is_low_signal_text_line(trimmed) {
                    continue;
                }
                let snippet = compact_text_snippet(trimmed, max_len);
                if !snippet.is_empty() {
                    return Some(snippet);
                }
            }
        }
    }
    None
}

fn json_value_hint(value: &serde_json::Value, max_len: usize) -> Option<String> {
    match value {
        serde_json::Value::String(s) => {
            let hint = compact_text_snippet(s, max_len);
            if hint.is_empty() {
                None
            } else {
                Some(hint)
            }
        }
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        serde_json::Value::Array(values) => Some(format!("items={}", values.len())),
        serde_json::Value::Object(map) => {
            if let Some((key, value)) = map.iter().next() {
                let rendered = json_value_hint(value, max_len.saturating_sub(key.len() + 1))
                    .unwrap_or_else(|| "...".to_string());
                Some(format!("{key}={rendered}"))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn first_json_block_hint(blocks: &[ContentBlock], max_len: usize) -> Option<String> {
    for block in blocks {
        if let ContentBlock::Json { data } = block {
            if let Some(hint) = json_value_hint(data, max_len) {
                if !hint.trim().is_empty() {
                    return Some(hint);
                }
            }
        }
    }
    None
}

fn first_code_line(blocks: &[ContentBlock], max_len: usize) -> Option<String> {
    for block in blocks {
        if let ContentBlock::Code { code, .. } = block {
            let first_line = code.lines().next().unwrap_or("").trim();
            if !first_line.is_empty() {
                return Some(compact_text_snippet(first_line, max_len));
            }
        }
    }
    None
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

fn compact_shell_command(command: &str, max_len: usize) -> String {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    if tokens.is_empty() {
        return String::new();
    }

    let mut compact = Vec::new();
    for token in tokens.iter().take(8) {
        compact.push(compact_shell_token(token));
    }
    if tokens.len() > 8 {
        compact.push("…".to_string());
    }
    compact_text_snippet(&compact.join(" "), max_len)
}

fn compact_shell_token(token: &str) -> String {
    let mut start = 0usize;
    let mut end = token.len();
    for (idx, ch) in token.char_indices() {
        if ch == '/' || ch.is_ascii_alphanumeric() || ch == '~' {
            start = idx;
            break;
        }
    }
    for (idx, ch) in token.char_indices().rev() {
        if ch == '/' || ch.is_ascii_alphanumeric() || ch == '~' {
            end = idx + ch.len_utf8();
            break;
        }
    }
    if start >= end || start >= token.len() || end > token.len() {
        return token.to_string();
    }

    let core = &token[start..end];
    let compact_core = if core.starts_with('/') {
        short_path(core).to_string()
    } else {
        core.to_string()
    };
    format!("{}{}{}", &token[..start], compact_core, &token[end..])
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

fn looks_like_terminal_mouse_dump(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.len() < 40 {
        return false;
    }
    let semicolons = trimmed.matches(';').count();
    let mouse_marks = trimmed.matches('M').count() + trimmed.matches('m').count();
    let digits = trimmed.chars().filter(|ch| ch.is_ascii_digit()).count();
    let letters = trimmed
        .chars()
        .filter(|ch| ch.is_ascii_alphabetic())
        .count();
    (trimmed.starts_with("[<") || trimmed.contains("[<"))
        && semicolons >= 6
        && mouse_marks >= 4
        && digits > letters.saturating_mul(3)
}

fn compact_text_snippet(text: &str, max_len: usize) -> String {
    let mut cleaned = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_control() && ch != '\n' && ch != '\t' {
            continue;
        }
        cleaned.push(ch);
    }
    let collapsed = cleaned
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if collapsed.is_empty() {
        return String::new();
    }
    if looks_like_terminal_mouse_dump(&collapsed) {
        return "(terminal mouse input omitted)".to_string();
    }
    truncate(&collapsed, max_len)
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

fn lane_marker_text(marker: LaneMarker) -> Option<&'static str> {
    match marker {
        LaneMarker::Fork => Some("fork"),
        LaneMarker::Merge => Some("merge"),
        LaneMarker::None => None,
    }
}

fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

fn render_timeline_bar(frame: &mut Frame, area: Rect, events: &[DisplayEvent], current_idx: usize) {
    if events.is_empty() || area.width < 10 {
        return;
    }

    let counter = format!(" ({}/{}) ", current_idx + 1, events.len());
    let bar_width = (area.width as usize).saturating_sub(counter.len() + 2);
    if bar_width == 0 {
        return;
    }

    let first_ts = events
        .first()
        .map(|e| e.event().timestamp)
        .unwrap_or_else(chrono::Utc::now);
    let last_ts = events
        .last()
        .map(|e| e.event().timestamp)
        .unwrap_or(first_ts);
    let total_secs = (last_ts - first_ts).num_seconds().max(1) as f64;
    let mut buckets = vec![0u32; bar_width];
    let mut current_bucket_idx = 0;

    for (i, de) in events.iter().enumerate() {
        let t = (de.event().timestamp - first_ts).num_seconds() as f64;
        let bucket = ((t / total_secs) * (bar_width - 1) as f64) as usize;
        let bucket = bucket.min(bar_width - 1);
        buckets[bucket] += 1;
        if i == current_idx {
            current_bucket_idx = bucket;
        }
    }

    let max_count = *buckets.iter().max().unwrap_or(&1).max(&1);
    let density_chars = [' ', '.', ':', '=', '#'];
    let mut spans = vec![Span::styled(" ", Style::new())];
    for (idx, count) in buckets.iter().enumerate() {
        let level = if *count == 0 {
            0
        } else {
            ((*count as f64 / max_count as f64) * 4.0).ceil() as usize
        }
        .min(4);
        let style = if idx == current_bucket_idx {
            Style::new().fg(Color::Black).bg(Theme::ACCENT_BLUE)
        } else {
            Style::new().fg(Theme::BAR_DIM)
        };
        spans.push(Span::styled(density_chars[level].to_string(), style));
    }
    spans.push(Span::styled(counter, Style::new().fg(Color::DarkGray)));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};
    use opensession_core::trace::{Content, Event, EventType};

    fn make_event(event_type: EventType, text: &str) -> Event {
        Event {
            event_id: format!("event-{event_type:?}"),
            timestamp: Utc::now(),
            event_type,
            task_id: None,
            content: Content::text(text),
            duration_ms: None,
            attributes: std::collections::HashMap::new(),
        }
    }

    fn make_event_with_task(event_type: EventType, text: &str, task_id: &str) -> Event {
        let mut event = make_event(event_type, text);
        event.task_id = Some(task_id.to_string());
        event
    }

    #[test]
    fn format_elapsed_short_formats_seconds_minutes_and_hours() {
        assert_eq!(format_elapsed_short(45), "45s");
        assert_eq!(format_elapsed_short(120), "2m");
        assert_eq!(format_elapsed_short(125), "2m5s");
        assert_eq!(format_elapsed_short(3720), "1h2m");
    }

    #[test]
    fn timeline_separator_line_marks_minute_and_five_minute_boundaries() {
        let base = Utc
            .timestamp_opt(1_700_000_000, 0)
            .single()
            .expect("valid fixed timestamp");
        let minor = timeline_separator_line(base, base + Duration::seconds(61))
            .expect("minute separator should be present");
        let major = timeline_separator_line(base, base + Duration::seconds(301))
            .expect("five-minute separator should be present");

        assert!(minor.to_string().contains('·'));
        assert!(major.to_string().contains('◆'));
    }

    #[test]
    fn display_event_agent_count_uses_active_lanes() {
        let event = make_event(EventType::AgentMessage, "agent output");
        let display = DisplayEvent::Single {
            event: &event,
            source_index: 0,
            lane: 2,
            marker: LaneMarker::None,
            active_lanes: vec![0, 1, 2],
        };

        assert_eq!(display_event_agent_count(&display), 2);
    }

    #[test]
    fn build_linear_turn_group_lookup_tracks_turn_boundaries() {
        let events = vec![
            make_event(EventType::UserMessage, "first prompt"),
            make_event(EventType::AgentMessage, "first response"),
            make_event(EventType::UserMessage, "second prompt"),
            make_event(EventType::AgentMessage, "second response"),
        ];
        let display: Vec<DisplayEvent<'_>> = events
            .iter()
            .enumerate()
            .map(|(idx, event)| DisplayEvent::Single {
                event,
                source_index: idx,
                lane: 0,
                marker: LaneMarker::None,
                active_lanes: vec![0],
            })
            .collect();

        let (lookup, groups) = build_linear_turn_group_lookup(&display);
        assert_eq!(lookup, vec![Some(0), Some(0), Some(1), Some(1)]);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].event_count, 2);
        assert_eq!(groups[1].event_count, 2);
        assert!(groups[0].prompt.contains("first prompt"));
        assert!(groups[1].prompt.contains("second prompt"));
    }

    #[test]
    fn append_event_detail_rows_emits_meta_action_and_preview() {
        let event = make_event_with_task(
            EventType::FileEdit {
                path: "/tmp/repo/src/main.rs".to_string(),
                diff: Some("- old\n+ new".to_string()),
            },
            "updated file",
            "task-42",
        );
        let display = DisplayEvent::Single {
            event: &event,
            source_index: 7,
            lane: 2,
            marker: LaneMarker::Fork,
            active_lanes: vec![0, 2],
        };

        let mut lines: Vec<Line<'_>> = Vec::new();
        append_event_detail_rows(&mut lines, &display, 2);
        let rendered = lines
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("meta"));
        assert!(rendered.contains("#7"));
        assert!(rendered.contains("lane L2"));
        assert!(rendered.contains("task task-42"));
        assert!(rendered.contains("action"));
        assert!(rendered.contains("src/main.rs"));
        assert!(rendered.contains("+ new"));
    }

    #[test]
    fn task_id_display_label_hides_call_ids() {
        assert!(task_id_display_label("call_Xvu4vvSffgP").is_none());
        let label = task_id_display_label("task-1234567890abcdef")
            .expect("normal task labels stay visible");
        assert!(label.starts_with("task-"));
    }

    #[test]
    fn terminal_mouse_dump_is_replaced_with_safe_label() {
        let noisy = "[<0;58;14M[<0;58;14M[<0;58;14M[<0;58;14M";
        assert_eq!(
            compact_text_snippet(noisy, 80),
            "(terminal mouse input omitted)"
        );
    }

    #[test]
    fn event_summary_tool_result_skips_chunk_id_noise() {
        let event = make_event(
            EventType::ToolResult {
                name: "exec_command".to_string(),
                is_error: false,
                call_id: None,
            },
            "Chunk ID: abc\nWall time: 0.1 seconds\nreal output",
        );
        assert!(
            event_compact_summary(&event.event_type, &event.content.blocks).contains("real output")
        );
    }

    #[test]
    fn event_summary_tool_call_uses_tool_specific_fields() {
        let event = Event {
            event_id: "tool-call-json".to_string(),
            timestamp: Utc::now(),
            event_type: EventType::ToolCall {
                name: "exec_command".to_string(),
            },
            task_id: None,
            content: Content {
                blocks: vec![ContentBlock::Json {
                    data: serde_json::json!({"cmd":"cargo test -p opensession-tui"}),
                }],
            },
            duration_ms: None,
            attributes: std::collections::HashMap::new(),
        };

        assert!(
            event_compact_summary(&event.event_type, &event.content.blocks)
                .contains("run cargo test")
        );
    }

    #[test]
    fn event_summary_file_edit_unknown_path_uses_generic_label() {
        let event = make_event(
            EventType::FileEdit {
                path: "unknown".to_string(),
                diff: Some("+ hello\n- bye".to_string()),
            },
            "",
        );

        let summary = event_compact_summary(&event.event_type, &event.content.blocks);
        assert!(summary.starts_with("file +1 -1"));
        assert!(!summary.contains("unknown"));
    }

    #[test]
    fn lane_inference_assigns_codex_untagged_events_to_active_task() {
        let task_start = make_event_with_task(
            EventType::TaskStart {
                title: Some("active task".to_string()),
            },
            "",
            "task-active-123",
        );
        let untagged = make_event(EventType::AgentMessage, "work item");
        let task_end = make_event_with_task(
            EventType::TaskEnd {
                summary: Some("done".to_string()),
            },
            "",
            "task-active-123",
        );

        let display = vec![
            DisplayEvent::Single {
                event: &task_start,
                source_index: 0,
                lane: 1,
                marker: LaneMarker::Fork,
                active_lanes: vec![0],
            },
            DisplayEvent::Single {
                event: &untagged,
                source_index: 1,
                lane: 1,
                marker: LaneMarker::None,
                active_lanes: vec![0, 1],
            },
            DisplayEvent::Single {
                event: &task_end,
                source_index: 2,
                lane: 1,
                marker: LaneMarker::Merge,
                active_lanes: vec![0, 1],
            },
        ];

        let (lookup, groups) = build_linear_turn_group_lookup(&display);
        assert_eq!(lookup, vec![Some(0), Some(0), Some(0)]);
        assert_eq!(groups.len(), 1);
    }

    #[test]
    fn lane_inference_routes_tool_result_to_tool_call_lane() {
        let call = make_event_with_task(
            EventType::ToolCall {
                name: "exec_command".to_string(),
            },
            "",
            "task-tool-abc",
        );
        let result = make_event_with_task(
            EventType::ToolResult {
                name: "exec_command".to_string(),
                is_error: false,
                call_id: None,
            },
            "ok",
            "task-tool-abc",
        );

        let display = vec![
            DisplayEvent::Single {
                event: &call,
                source_index: 0,
                lane: 2,
                marker: LaneMarker::None,
                active_lanes: vec![0, 2],
            },
            DisplayEvent::Single {
                event: &result,
                source_index: 1,
                lane: 2,
                marker: LaneMarker::None,
                active_lanes: vec![0, 2],
            },
        ];

        assert_eq!(display_event_agent_count(&display[0]), 1);
        assert_eq!(display_event_agent_count(&display[1]), 1);
    }
}
