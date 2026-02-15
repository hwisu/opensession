#![allow(dead_code)]

use crate::app::{extract_visible_turns, App, DisplayEvent, EventFilter};
use crate::session_timeline::LaneMarker;
use crate::theme::{self, Theme};
use crate::timeline_summary::TimelineSummaryPayload;
use chrono::{DateTime, Utc};
use opensession_core::trace::{ContentBlock, Event, EventType};
use ratatui::prelude::*;
use ratatui::widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap};
use std::collections::{BTreeSet, HashMap, HashSet};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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
            lines.push(Line::from(Span::styled(
                "Extra timeline analysis is disabled for this session.",
                Style::new().fg(Theme::TEXT_SECONDARY),
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
    let summary_off = app.live_mode || app.llm_summary_status_label() != "on";
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

    let mut lines: Vec<Line> = Vec::new();
    let mut event_line_positions: Vec<usize> = Vec::with_capacity(total_visible);

    for (i, display_event) in events.iter().enumerate() {
        let event = display_event.event();
        let selected = i == current_idx;
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
        spans.push(Span::styled(
            lane_cells(display_event, lane_count),
            Style::new().fg(Theme::TREE),
        ));
        spans.push(Span::raw(" "));

        match display_event {
            DisplayEvent::SummaryRow {
                summary, window_id, ..
            } => {
                let (summary_kind, summary_color, summary_id) = summary_row_badge(*window_id);
                spans.push(Span::styled(
                    format!("[llm {summary_kind} #{summary_id}] "),
                    Style::new().fg(summary_color).bold(),
                ));
                spans.push(Span::styled(summary, Style::new().fg(Theme::TEXT_PRIMARY)));
            }
            DisplayEvent::Collapsed { count, kind, .. } => {
                spans.push(Span::styled(
                    format!("{kind} x{count}"),
                    Style::new().fg(Theme::ROLE_AGENT).bold(),
                ));
            }
            DisplayEvent::Single {
                event,
                lane,
                marker,
                ..
            } => {
                let (kind, kind_color) = event_type_display(&event.event_type, summary_off);
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
                if let Some(badge) = lane_assignment_badge(event, *lane, *marker) {
                    spans.push(Span::styled(
                        format!(" {badge}"),
                        Style::new().fg(Theme::ACCENT_CYAN),
                    ));
                }
                if let Some(task_badge) = event_task_badge(event) {
                    spans.push(Span::styled(
                        format!(" {task_badge}"),
                        Style::new().fg(Theme::ACCENT_TEAL).bold(),
                    ));
                }
            }
        }

        let line_style = if selected {
            Style::new().bg(Theme::BG_SURFACE)
        } else {
            Style::new()
        };
        lines.push(Line::from(spans).style(line_style));

        let expanded = app.expanded_events.contains(&i);
        if expanded {
            if let DisplayEvent::Single { event, .. } = display_event {
                append_content_preview(&mut lines, event, 3);
            }
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

fn summary_row_badge(window_id: u64) -> (&'static str, Color, u64) {
    let tag = window_id >> 56;
    let local_id = window_id & ((1u64 << 56) - 1);
    match tag {
        1 => ("phase-start", Theme::ACCENT_YELLOW, local_id),
        2 => ("phase-end", Theme::ACCENT_ORANGE, local_id),
        3 => ("turn", Theme::ACCENT_BLUE, local_id),
        4 => ("action", Theme::ACCENT_TEAL, local_id),
        _ => ("window", Theme::ACCENT_CYAN, window_id),
    }
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
        if let ContentBlock::Text { text } = block {
            for line in text.lines().take(max_lines) {
                lines.push(Line::from(vec![
                    Span::styled("    | ", Style::new().fg(Theme::GUTTER)),
                    Span::styled(truncate(line, 120), Style::new().fg(Theme::TEXT_SECONDARY)),
                ]));
            }
            return;
        }
    }
}

fn event_type_display(event_type: &EventType, summary_off: bool) -> (&'static str, Color) {
    match event_type {
        EventType::UserMessage => ("user", Theme::ROLE_USER),
        EventType::AgentMessage => ("agent", Theme::ROLE_AGENT_BRIGHT),
        EventType::SystemMessage => ("system", Theme::ROLE_SYSTEM),
        EventType::Thinking => ("think", Theme::ACCENT_PURPLE),
        EventType::ToolCall { .. } => ("tool", Theme::ACCENT_YELLOW),
        EventType::ToolResult { is_error: true, .. } => ("error", Theme::ACCENT_RED),
        EventType::ToolResult { .. } => (
            "result",
            if summary_off {
                Theme::ACCENT_GREEN
            } else {
                Theme::TEXT_MUTED
            },
        ),
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
        EventType::Custom { .. } => (
            "custom",
            if summary_off {
                Theme::ACCENT_CYAN
            } else {
                Theme::TEXT_MUTED
            },
        ),
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
        EventType::Thinking => "reasoning".to_string(),
        EventType::ToolCall { name } => format!("{name}()"),
        EventType::ToolResult { name, is_error, .. } => {
            let hint = first_meaningful_text_line_opt(blocks, 48)
                .or_else(|| first_json_block_hint(blocks, 48))
                .or_else(|| first_code_line(blocks, 48));
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
        EventType::FileRead { path } => short_path(path).to_string(),
        EventType::CodeSearch { query } => truncate(query, 52),
        EventType::FileSearch { pattern } => truncate(pattern, 52),
        EventType::FileEdit { path, diff } => {
            if let Some(d) = diff {
                let (add, del) = count_diff_lines(d);
                format!("{} +{} -{}", short_path(path), add, del)
            } else {
                short_path(path).to_string()
            }
        }
        EventType::FileCreate { path } => format!("+ {}", short_path(path)),
        EventType::FileDelete { path } => format!("- {}", short_path(path)),
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
        _ => event_summary(event_type, blocks),
    }
}

fn event_summary(event_type: &EventType, blocks: &[ContentBlock]) -> String {
    match event_type {
        EventType::UserMessage | EventType::AgentMessage => first_text_line(blocks, 80),
        EventType::SystemMessage => String::new(),
        EventType::Thinking => "thinking".to_string(),
        EventType::ToolCall { name } => {
            let hint = first_json_block_hint(blocks, 72)
                .or_else(|| first_code_line(blocks, 72))
                .or_else(|| first_meaningful_text_line_opt(blocks, 72))
                .or_else(|| first_text_line_opt(blocks, 72));
            if let Some(hint) = hint {
                format!("{name} {hint}")
            } else {
                format!("{name}()")
            }
        }
        EventType::ToolResult { name, is_error, .. } => {
            let hint = first_meaningful_text_line_opt(blocks, 72)
                .or_else(|| first_code_line(blocks, 72))
                .or_else(|| first_json_block_hint(blocks, 72));
            if *is_error {
                if let Some(hint) = hint {
                    format!("{name} error: {hint}")
                } else {
                    format!("{name} failed")
                }
            } else if let Some(hint) = hint {
                format!("{name}: {hint}")
            } else {
                format!("{name} ok")
            }
        }
        EventType::FileRead { path } => short_path(path).to_string(),
        EventType::CodeSearch { query } => truncate(query, 70),
        EventType::FileSearch { pattern } => truncate(pattern, 70),
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
            Some(code) => format!("{} => {}", compact_shell_command(command, 70), code),
            None => compact_shell_command(command, 70),
        },
        EventType::WebSearch { query } => truncate(query, 70),
        EventType::WebFetch { url } => truncate(url, 70),
        EventType::ImageGenerate { prompt }
        | EventType::VideoGenerate { prompt }
        | EventType::AudioGenerate { prompt } => truncate(prompt, 70),
        EventType::TaskStart { title } => {
            if let Some(title) = title.as_deref() {
                let snippet = compact_text_snippet(title, 60);
                if snippet.is_empty() {
                    "start".to_string()
                } else {
                    format!("start {snippet}")
                }
            } else {
                "start".to_string()
            }
        }
        EventType::TaskEnd { summary } => {
            if let Some(summary) = summary.as_deref() {
                let snippet = compact_text_snippet(summary, 72);
                if snippet.is_empty() {
                    "end".to_string()
                } else {
                    format!("end {snippet}")
                }
            } else {
                "end".to_string()
            }
        }
        EventType::Custom { kind } => {
            let hint = first_meaningful_text_line_opt(blocks, 70)
                .or_else(|| first_json_block_hint(blocks, 70))
                .or_else(|| first_code_line(blocks, 70));
            if let Some(hint) = hint {
                if hint.eq_ignore_ascii_case(kind) {
                    compact_text_snippet(kind, 70)
                } else {
                    format!("{kind}: {hint}")
                }
            } else {
                compact_text_snippet(kind, 70)
            }
        }
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

fn event_task_badge(event: &Event) -> Option<String> {
    let task_id = event
        .task_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())?;
    Some(format!("[task:{}]", compact_task_id(task_id)))
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

fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};
    use opensession_core::trace::{Agent, Content, Event, EventType, Session};

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
    fn collect_turn_user_lines_keeps_full_multiline_prompt() {
        let long_line = "A".repeat(200);
        let text = format!("line1\nline2\nline3\nline4\nline5\nline6\n{long_line}");
        let user_event = make_event(EventType::UserMessage, &text);
        let turn = crate::app::Turn {
            turn_index: 0,
            start_display_index: 0,
            end_display_index: 0,
            anchor_source_index: 0,
            user_events: vec![&user_event],
            agent_events: vec![],
        };

        let lines = collect_turn_user_lines(&turn);
        assert_eq!(lines.len(), 7);
        assert_eq!(lines[6], long_line);
    }

    #[test]
    fn collect_turn_user_lines_drops_internal_summary_events() {
        let user_event = make_event(EventType::UserMessage, "real user prompt");
        let internal_summary_event = make_event(
            EventType::UserMessage,
            "You are generating a turn-summary payload.\n\
             Return JSON only (no markdown, no prose) with keys:\n\
             {\"kind\":\"turn-summary\",\"evidence\":{\"agent_quotes\":[\"...\"]}}",
        );
        let turn = crate::app::Turn {
            turn_index: 0,
            start_display_index: 0,
            end_display_index: 0,
            anchor_source_index: 0,
            user_events: vec![&user_event, &internal_summary_event],
            agent_events: vec![],
        };

        let lines = collect_turn_user_lines(&turn);
        assert_eq!(lines, vec!["real user prompt".to_string()]);
    }

    #[test]
    fn turn_prompt_card_shows_expand_hint_when_collapsed() {
        let prompt = (1..=20)
            .map(|idx| format!("line {idx}"))
            .collect::<Vec<_>>()
            .join("\n");
        let user_event = make_event(EventType::UserMessage, &prompt);
        let turn = crate::app::Turn {
            turn_index: 0,
            start_display_index: 0,
            end_display_index: 0,
            anchor_source_index: 0,
            user_events: vec![&user_event],
            agent_events: vec![],
        };

        let lines = render_turn_prompt_card(0, &turn, true, false, 90);
        let rendered = lines
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("more prompt lines (p: expand)"));
    }

    #[test]
    fn summary_payload_prefers_cards_over_raw_thread() {
        let session = Session::new(
            "test-session".to_string(),
            Agent {
                provider: "anthropic".to_string(),
                model: "claude".to_string(),
                tool: "claude-code".to_string(),
                tool_version: None,
            },
        );
        let app = App::new(vec![session]);
        let user_event = make_event(EventType::UserMessage, "prompt");
        let agent_event = make_event(EventType::AgentMessage, "response");
        let turn = crate::app::Turn {
            turn_index: 0,
            start_display_index: 0,
            end_display_index: 1,
            anchor_source_index: 0,
            user_events: vec![&user_event],
            agent_events: vec![&agent_event],
        };
        let payload = TimelineSummaryPayload {
            kind: "turn-summary".to_string(),
            version: "2.0".to_string(),
            scope: "turn".to_string(),
            turn_meta: crate::timeline_summary::TurnSummaryTurnMeta::default(),
            prompt: crate::timeline_summary::TurnSummaryPrompt {
                text: "prompt".to_string(),
                intent: "Implement fix".to_string(),
                constraints: vec![],
            },
            outcome: crate::timeline_summary::TurnSummaryOutcome {
                status: "completed".to_string(),
                summary: "done".to_string(),
            },
            evidence: crate::timeline_summary::TurnSummaryEvidence::default(),
            cards: vec![crate::timeline_summary::BehaviorCard {
                card_type: "overview".to_string(),
                title: "Overview".to_string(),
                lines: vec!["updated renderer".to_string()],
                severity: "info".to_string(),
            }],
            next_steps: vec!["verify".to_string()],
        };

        let lines = render_turn_response_panel(&app, &turn, Some(&payload), false, true, 80, "on");
        let rendered = lines
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("Agent Output"));
        assert!(rendered.contains("response"));
        assert!(rendered.contains("Turn Summary"));
        assert!(!rendered.contains("Agent Thread"));
    }

    #[test]
    fn summary_off_uses_task_board_fallback() {
        let session = Session::new(
            "test-session-off".to_string(),
            Agent {
                provider: "anthropic".to_string(),
                model: "claude".to_string(),
                tool: "claude-code".to_string(),
                tool_version: None,
            },
        );
        let app = App::new(vec![session]);
        let user_event = make_event(EventType::UserMessage, "prompt");
        let agent_event = make_event(EventType::AgentMessage, "implemented fallback");
        let turn = crate::app::Turn {
            turn_index: 0,
            start_display_index: 0,
            end_display_index: 1,
            anchor_source_index: 0,
            user_events: vec![&user_event],
            agent_events: vec![&agent_event],
        };

        let lines = render_turn_response_panel(&app, &turn, None, false, true, 80, "off");
        let rendered = lines
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("Task Board"));
        assert!(rendered.contains("Summary is off"));
        assert!(rendered.contains("[main]"));
        assert!(rendered.contains("output: implemented fallback"));
    }

    #[test]
    fn summary_off_groups_agent_events_by_task() {
        let session = Session::new(
            "test-session-multi".to_string(),
            Agent {
                provider: "anthropic".to_string(),
                model: "claude".to_string(),
                tool: "claude-code".to_string(),
                tool_version: None,
            },
        );
        let app = App::new(vec![session]);
        let user_event = make_event(EventType::UserMessage, "prompt");
        let agent_main = make_event(EventType::AgentMessage, "main response");
        let agent_sub = make_event_with_task(
            EventType::AgentMessage,
            "subagent response",
            "task-1234567890abcdef",
        );
        let turn = crate::app::Turn {
            turn_index: 0,
            start_display_index: 0,
            end_display_index: 2,
            anchor_source_index: 0,
            user_events: vec![&user_event],
            agent_events: vec![&agent_main, &agent_sub],
        };

        let lines = render_turn_response_panel(&app, &turn, None, false, true, 80, "off");
        let rendered = lines
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("[main]"));
        assert!(rendered.contains("task task-"));
        assert!(rendered.contains("output: subagent response"));
    }

    #[test]
    fn summary_off_shows_event_chunks_for_long_lane() {
        let session = Session::new(
            "test-session-chunks".to_string(),
            Agent {
                provider: "anthropic".to_string(),
                model: "claude".to_string(),
                tool: "claude-code".to_string(),
                tool_version: None,
            },
        );
        let app = App::new(vec![session]);
        let user_event = make_event(EventType::UserMessage, "prompt");
        let mut lane_events: Vec<Event> = Vec::new();
        for idx in 0..21 {
            lane_events.push(make_event_with_task(
                EventType::AgentMessage,
                &format!("lane event {idx}"),
                "task-chunk-1234567890abcdef",
            ));
        }
        let agent_refs: Vec<&Event> = lane_events.iter().collect();
        let turn = crate::app::Turn {
            turn_index: 0,
            start_display_index: 0,
            end_display_index: 21,
            anchor_source_index: 0,
            user_events: vec![&user_event],
            agent_events: agent_refs,
        };

        let lines = render_turn_response_panel(&app, &turn, None, false, true, 120, "off");
        let rendered = lines
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("events chunk 1/2"));
        assert!(rendered.contains("events chunk 2/2"));
        assert!(rendered.contains("lane event 0"));
    }

    #[test]
    fn summary_off_collapses_extra_lanes_into_overflow_panel() {
        let session = Session::new(
            "test-session-overflow".to_string(),
            Agent {
                provider: "anthropic".to_string(),
                model: "claude".to_string(),
                tool: "claude-code".to_string(),
                tool_version: None,
            },
        );
        let app = App::new(vec![session]);
        let user_event = make_event(EventType::UserMessage, "prompt");

        let mut events = vec![make_event(EventType::AgentMessage, "main output")];
        for idx in 0..5 {
            events.push(make_event_with_task(
                EventType::AgentMessage,
                &format!("subagent {idx}"),
                &format!("task-overflow-{idx}"),
            ));
        }
        let agent_refs: Vec<&Event> = events.iter().collect();
        let turn = crate::app::Turn {
            turn_index: 0,
            start_display_index: 0,
            end_display_index: 6,
            anchor_source_index: 0,
            user_events: vec![&user_event],
            agent_events: agent_refs,
        };

        let lines = render_turn_response_panel(&app, &turn, None, false, true, 160, "off");
        let rendered = lines
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("[main]"));
        assert!(rendered.contains("[+2 lanes]"));
    }

    #[test]
    fn lane_inference_assigns_codex_untagged_events_to_active_task() {
        let mut start = make_event_with_task(
            EventType::TaskStart { title: None },
            "start task",
            "turn-active-123456",
        );
        start.event_id = "codex-1".to_string();
        let mut think = make_event(EventType::Thinking, "plan next steps");
        think.event_id = "codex-2".to_string();
        let mut shell = make_event(
            EventType::ShellCommand {
                command: "rg -n lane".to_string(),
                exit_code: None,
            },
            "rg -n lane",
        );
        shell.event_id = "codex-3".to_string();
        let turn = crate::app::Turn {
            turn_index: 0,
            start_display_index: 0,
            end_display_index: 2,
            anchor_source_index: 0,
            user_events: vec![],
            agent_events: vec![&start, &think, &shell],
        };

        let entries = build_turn_lane_entries(&turn, &HashSet::new());
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].lane, 1);
        assert_eq!(entries[1].lane, 1);
        assert_eq!(entries[2].lane, 1);
    }

    #[test]
    fn lane_inference_routes_tool_result_to_tool_call_lane() {
        let mut call = make_event_with_task(
            EventType::ToolCall {
                name: "exec_command".to_string(),
            },
            "exec",
            "task-lane-xyz",
        );
        call.event_id = "call-event-1".to_string();
        let mut result = make_event(
            EventType::ToolResult {
                name: "exec_command".to_string(),
                is_error: false,
                call_id: Some("call-event-1".to_string()),
            },
            "ok",
        );
        result.event_id = "result-event-2".to_string();
        let turn = crate::app::Turn {
            turn_index: 0,
            start_display_index: 0,
            end_display_index: 1,
            anchor_source_index: 0,
            user_events: vec![],
            agent_events: vec![&call, &result],
        };

        let entries = build_turn_lane_entries(&turn, &HashSet::new());
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].lane, 1);
        assert_eq!(entries[1].lane, 1);
    }

    #[test]
    fn summary_off_formats_structured_json_event_rows() {
        let session = Session::new(
            "test-session-structured-json".to_string(),
            Agent {
                provider: "anthropic".to_string(),
                model: "claude".to_string(),
                tool: "claude-code".to_string(),
                tool_version: None,
            },
        );
        let app = App::new(vec![session]);
        let user_event = make_event(EventType::UserMessage, "prompt");
        let structured = make_event(
            EventType::Custom {
                kind: "agent_reasoning".to_string(),
            },
            r#"{"lane":"main","summary":"thinking","task":"main","ts":"13:25:18","type":"think"}"#,
        );
        let turn = crate::app::Turn {
            turn_index: 0,
            start_display_index: 0,
            end_display_index: 1,
            anchor_source_index: 0,
            user_events: vec![&user_event],
            agent_events: vec![&structured],
        };

        let lines = render_turn_response_panel(&app, &turn, None, false, true, 120, "off");
        let rendered = lines
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("13:25:18"));
        assert!(rendered.contains("thinking"));
        assert!(!rendered.contains("{\"lane\":\"main\""));
    }

    #[test]
    fn summary_off_collapses_single_synthetic_end_stub_tasks() {
        let session = Session::new(
            "test-session-synthetic".to_string(),
            Agent {
                provider: "anthropic".to_string(),
                model: "claude".to_string(),
                tool: "claude-code".to_string(),
                tool_version: None,
            },
        );
        let app = App::new(vec![session]);
        let main = make_event(EventType::AgentMessage, "main response");
        let stub_a = make_event_with_task(
            EventType::TaskEnd {
                summary: Some("synthetic end (missing task_complete)".to_string()),
            },
            "",
            "task-aaaaaaaaaaaaaaaa",
        );
        let stub_b = make_event_with_task(
            EventType::TaskEnd {
                summary: Some("synthetic end (missing task_complete)".to_string()),
            },
            "",
            "task-bbbbbbbbbbbbbbbb",
        );
        let turn = crate::app::Turn {
            turn_index: 0,
            start_display_index: 0,
            end_display_index: 2,
            anchor_source_index: 0,
            user_events: vec![],
            agent_events: vec![&main, &stub_a, &stub_b],
        };

        let lines = render_turn_response_panel(&app, &turn, None, false, true, 100, "off");
        let rendered = lines
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("synthetic-end stubs: 2 collapsed"));
        assert!(!rendered.contains("task task-aaaa"));
        assert!(!rendered.contains("task task-bbbb"));
    }

    #[test]
    fn agent_output_preview_prefers_agent_message_over_task_end_summary() {
        let task_end = make_event_with_task(
            EventType::TaskEnd {
                summary: Some("task summary".to_string()),
            },
            "",
            "task-1",
        );
        let agent = make_event(EventType::AgentMessage, "final assistant output");
        let turn = crate::app::Turn {
            turn_index: 0,
            start_display_index: 0,
            end_display_index: 1,
            anchor_source_index: 0,
            user_events: vec![],
            agent_events: vec![&task_end, &agent],
        };

        let preview = turn_agent_output_preview(&turn);
        assert_eq!(preview.as_deref(), Some("final assistant output"));
    }

    #[test]
    fn task_chronicle_bucket_marks_running_and_counts_ops() {
        let start = make_event_with_task(EventType::TaskStart { title: None }, "", "task-1");
        let tool = make_event_with_task(
            EventType::ToolCall {
                name: "search".to_string(),
            },
            "",
            "task-1",
        );
        let shell = make_event_with_task(
            EventType::ShellCommand {
                command: "ls".to_string(),
                exit_code: Some(0),
            },
            "",
            "task-1",
        );
        let turn = crate::app::Turn {
            turn_index: 0,
            start_display_index: 0,
            end_display_index: 2,
            anchor_source_index: 0,
            user_events: vec![],
            agent_events: vec![&start, &tool, &shell],
        };
        let buckets = build_task_chronicle_buckets(&turn);
        let task = buckets
            .iter()
            .find(|bucket| bucket.task_key == "task-1")
            .expect("task bucket should exist");

        assert_eq!(task.status, TaskBucketStatus::Running);
        assert_eq!(task.tool_ops, 1);
        assert_eq!(task.shell_ops, 1);
    }

    #[test]
    fn task_activity_rows_do_not_duplicate_end_prefix() {
        let session = Session::new(
            "test-session-end-prefix".to_string(),
            Agent {
                provider: "anthropic".to_string(),
                model: "claude".to_string(),
                tool: "claude-code".to_string(),
                tool_version: None,
            },
        );
        let app = App::new(vec![session]);
        let start = make_event_with_task(
            EventType::TaskStart {
                title: Some("validate".to_string()),
            },
            "",
            "task-end-prefix",
        );
        let end = make_event_with_task(
            EventType::TaskEnd {
                summary: Some("completed check".to_string()),
            },
            "",
            "task-end-prefix",
        );
        let agent = make_event_with_task(EventType::AgentMessage, "done output", "task-end-prefix");
        let turn = crate::app::Turn {
            turn_index: 0,
            start_display_index: 0,
            end_display_index: 2,
            anchor_source_index: 0,
            user_events: vec![],
            agent_events: vec![&start, &end, &agent],
        };

        let lines = render_turn_response_panel(&app, &turn, None, false, true, 100, "off");
        let rendered = lines
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("end completed check"));
        assert!(!rendered.contains("end end completed check"));
    }

    #[test]
    fn task_activity_rows_do_not_duplicate_start_prefix() {
        let session = Session::new(
            "test-session-start-prefix".to_string(),
            Agent {
                provider: "anthropic".to_string(),
                model: "claude".to_string(),
                tool: "claude-code".to_string(),
                tool_version: None,
            },
        );
        let app = App::new(vec![session]);
        let start = make_event_with_task(EventType::TaskStart { title: None }, "", "task-start");
        let turn = crate::app::Turn {
            turn_index: 0,
            start_display_index: 0,
            end_display_index: 0,
            anchor_source_index: 0,
            user_events: vec![],
            agent_events: vec![&start],
        };

        let lines = render_turn_response_panel(&app, &turn, None, false, true, 100, "off");
        let rendered = lines
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!rendered.contains("start start"));
    }

    #[test]
    fn task_board_summary_uses_json_hint_for_tool_call() {
        let event = Event {
            event_id: "tool-call".to_string(),
            timestamp: Utc::now(),
            event_type: EventType::ToolCall {
                name: "request_user_input".to_string(),
            },
            task_id: Some("task-a".to_string()),
            content: Content {
                blocks: vec![ContentBlock::Json {
                    data: serde_json::json!({
                        "questions": [{"id": "plan"}]
                    }),
                }],
            },
            duration_ms: None,
            attributes: std::collections::HashMap::new(),
        };

        let summary = task_board_event_summary(&event, 120);
        assert!(summary.contains("request_user_input"));
        assert!(summary.contains("questions=1"));
    }

    #[test]
    fn task_board_summary_uses_tool_result_text() {
        let event = Event {
            event_id: "tool-result".to_string(),
            timestamp: Utc::now(),
            event_type: EventType::ToolResult {
                name: "exec_command".to_string(),
                is_error: false,
                call_id: None,
            },
            task_id: Some("task-a".to_string()),
            content: Content::text("updated 4 files\nnext line"),
            duration_ms: None,
            attributes: std::collections::HashMap::new(),
        };

        let summary = task_board_event_summary(&event, 120);
        assert!(summary.contains("exec_command"));
        assert!(summary.contains("updated 4 files"));
    }

    #[test]
    fn event_summary_tool_call_uses_json_hint() {
        let rendered = event_summary(
            &EventType::ToolCall {
                name: "exec_command".to_string(),
            },
            &[ContentBlock::Json {
                data: serde_json::json!({
                    "cmd": "cargo test -p opensession-tui"
                }),
            }],
        );
        assert!(rendered.contains("exec_command"));
        assert!(rendered.contains("cmd=cargo test -p opensession-tui"));
    }

    #[test]
    fn event_summary_tool_result_skips_chunk_id_noise() {
        let rendered = event_summary(
            &EventType::ToolResult {
                name: "exec_command".to_string(),
                is_error: false,
                call_id: None,
            },
            &[ContentBlock::Text {
                text: "Chunk ID: 0827f7\nupdated 2 files".to_string(),
            }],
        );
        assert!(rendered.contains("updated 2 files"));
        assert!(!rendered.to_ascii_lowercase().contains("chunk id"));
    }

    #[test]
    fn task_board_action_hints_prioritize_meaningful_actions() {
        let mut file_edit = make_event(
            EventType::FileEdit {
                path: "/tmp/project/src/main.rs".to_string(),
                diff: None,
            },
            "",
        );
        file_edit.timestamp = Utc::now() - Duration::seconds(3);

        let mut shell = make_event(
            EventType::ShellCommand {
                command: "cargo test -p opensession-tui".to_string(),
                exit_code: Some(0),
            },
            "",
        );
        shell.timestamp = Utc::now() - Duration::seconds(2);

        let mut low_signal_result = make_event(
            EventType::ToolResult {
                name: "exec_command".to_string(),
                is_error: false,
                call_id: None,
            },
            "Chunk ID: 123abc\nWall time: 0.1s",
        );
        low_signal_result.timestamp = Utc::now() - Duration::seconds(1);

        let turn = crate::app::Turn {
            turn_index: 0,
            start_display_index: 0,
            end_display_index: 2,
            anchor_source_index: 0,
            user_events: vec![],
            agent_events: vec![&file_edit, &shell, &low_signal_result],
        };
        let buckets = build_task_chronicle_buckets(&turn);
        let bucket = buckets
            .iter()
            .find(|bucket| bucket.task_key == "main")
            .expect("main bucket");

        let hints = task_bucket_action_hints(bucket, 3);
        let rendered = hints.join(" | ").to_ascii_lowercase();
        assert!(rendered.contains("edit"));
        assert!(rendered.contains("shell"));
        assert!(!rendered.contains("chunk id"));
    }

    #[test]
    fn task_activity_lines_skip_low_signal_chunk_rows() {
        let mut shell = make_event(
            EventType::ShellCommand {
                command: "pnpm test".to_string(),
                exit_code: Some(0),
            },
            "",
        );
        shell.timestamp = Utc::now() - Duration::seconds(2);

        let mut low_signal_result = make_event(
            EventType::ToolResult {
                name: "exec_command".to_string(),
                is_error: false,
                call_id: None,
            },
            "Chunk ID: abc123",
        );
        low_signal_result.timestamp = Utc::now() - Duration::seconds(1);

        let turn = crate::app::Turn {
            turn_index: 0,
            start_display_index: 0,
            end_display_index: 1,
            anchor_source_index: 0,
            user_events: vec![],
            agent_events: vec![&shell, &low_signal_result],
        };
        let buckets = build_task_chronicle_buckets(&turn);
        let bucket = buckets
            .iter()
            .find(|bucket| bucket.task_key == "main")
            .expect("main bucket");

        let lines = task_bucket_activity_lines(bucket, 2);
        let rendered = lines.join(" | ").to_ascii_lowercase();
        assert!(rendered.contains("shell"));
        assert!(!rendered.contains("chunk id"));
    }

    #[test]
    fn live_fallback_panel_includes_recent_activity_stream() {
        let session = Session::new(
            "test-session-live-feed".to_string(),
            Agent {
                provider: "anthropic".to_string(),
                model: "claude".to_string(),
                tool: "claude-code".to_string(),
                tool_version: None,
            },
        );
        let mut app = App::new(vec![session]);
        app.live_mode = true;

        let shell = make_event(
            EventType::ShellCommand {
                command: "cargo test -p opensession-tui".to_string(),
                exit_code: Some(0),
            },
            "",
        );
        let edit = make_event(
            EventType::FileEdit {
                path: "/tmp/project/crates/tui/src/views/session_detail.rs".to_string(),
                diff: None,
            },
            "",
        );
        let noisy = make_event(
            EventType::ToolResult {
                name: "exec_command".to_string(),
                is_error: false,
                call_id: None,
            },
            "Chunk ID: 0827f7",
        );
        let turn = crate::app::Turn {
            turn_index: 0,
            start_display_index: 0,
            end_display_index: 2,
            anchor_source_index: 0,
            user_events: vec![],
            agent_events: vec![&shell, &edit, &noisy],
        };

        let lines = render_turn_response_panel(&app, &turn, None, false, true, 110, "off");
        let rendered = lines
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("Live activity"));
        assert!(rendered.contains("shell"));
        assert!(rendered.contains("edit"));
        assert!(!rendered.to_ascii_lowercase().contains("chunk id"));
    }

    #[test]
    fn task_board_prioritizes_running_buckets_over_main_done() {
        let mut main = make_event(EventType::AgentMessage, "main output");
        main.timestamp = Utc::now() - Duration::minutes(3);
        let mut start = make_event_with_task(
            EventType::TaskStart {
                title: Some("spawn".to_string()),
            },
            "",
            "task-latest",
        );
        start.timestamp = Utc::now();
        let turn = crate::app::Turn {
            turn_index: 0,
            start_display_index: 0,
            end_display_index: 1,
            anchor_source_index: 0,
            user_events: vec![],
            agent_events: vec![&main, &start],
        };

        let buckets = build_task_chronicle_buckets(&turn);
        assert!(!buckets.is_empty());
        assert_eq!(buckets[0].task_key, "task-latest");
        assert_eq!(buckets[0].status, TaskBucketStatus::Running);
    }

    #[test]
    fn event_task_badge_includes_compact_task_id() {
        let event = make_event_with_task(EventType::AgentMessage, "done", "task-1234567890abcdef");
        let badge = event_task_badge(&event).expect("badge should exist");
        assert!(badge.starts_with("[task:"));
    }

    #[test]
    fn summary_off_promotes_result_and_custom_colors() {
        let (_, on_result) = event_type_display(
            &EventType::ToolResult {
                name: "tool".to_string(),
                is_error: false,
                call_id: None,
            },
            false,
        );
        let (_, off_result) = event_type_display(
            &EventType::ToolResult {
                name: "tool".to_string(),
                is_error: false,
                call_id: None,
            },
            true,
        );
        assert_ne!(on_result, off_result);

        let (_, on_custom) = event_type_display(
            &EventType::Custom {
                kind: "note".to_string(),
            },
            false,
        );
        let (_, off_custom) = event_type_display(
            &EventType::Custom {
                kind: "note".to_string(),
            },
            true,
        );
        assert_ne!(on_custom, off_custom);
    }

    #[test]
    fn task_end_summary_is_compacted_to_single_line() {
        let summary = "line1\n\nline2 with extra details that should be compacted";
        let rendered = event_summary(
            &EventType::TaskEnd {
                summary: Some(summary.to_string()),
            },
            &[],
        );
        assert!(!rendered.contains('\n'));
        assert!(rendered.starts_with("end "));
    }

    #[test]
    fn terminal_mouse_dump_is_replaced_with_safe_label() {
        let dump = "[<35;152;36M35;152;37M35;151;37M35;150;37M35;149;38M35;148;38M";
        let compact = compact_text_snippet(dump, 120);
        assert_eq!(compact, "(terminal mouse input omitted)");
    }

    #[test]
    fn turn_render_range_centers_near_focus() {
        assert_eq!(turn_render_range(20, 10, 7), 7..14);
        assert_eq!(turn_render_range(20, 1, 7), 0..7);
        assert_eq!(turn_render_range(20, 19, 7), 13..20);
    }

    #[test]
    fn turn_summary_cards_show_all_cards_without_ellipsis() {
        let payload = TimelineSummaryPayload {
            kind: "turn-summary".to_string(),
            version: "2.0".to_string(),
            scope: "turn".to_string(),
            turn_meta: crate::timeline_summary::TurnSummaryTurnMeta::default(),
            prompt: crate::timeline_summary::TurnSummaryPrompt {
                text: "prompt".to_string(),
                intent: "Implement fix".to_string(),
                constraints: vec![],
            },
            outcome: crate::timeline_summary::TurnSummaryOutcome {
                status: "completed".to_string(),
                summary: "done".to_string(),
            },
            evidence: crate::timeline_summary::TurnSummaryEvidence::default(),
            cards: vec![
                crate::timeline_summary::BehaviorCard {
                    card_type: "overview".to_string(),
                    title: "Overview".to_string(),
                    lines: vec!["line-1".to_string()],
                    severity: "info".to_string(),
                },
                crate::timeline_summary::BehaviorCard {
                    card_type: "files".to_string(),
                    title: "Files".to_string(),
                    lines: vec!["line-2".to_string()],
                    severity: "info".to_string(),
                },
                crate::timeline_summary::BehaviorCard {
                    card_type: "plan".to_string(),
                    title: "Plan".to_string(),
                    lines: vec!["line-3".to_string()],
                    severity: "info".to_string(),
                },
            ],
            next_steps: vec!["verify".to_string()],
        };

        let lines =
            render_turn_summary_cards(&payload, false, 80, &HashSet::from([EventFilter::All]));
        let rendered = lines
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("Overview"));
        assert!(rendered.contains("Files"));
        assert!(rendered.contains("Plan"));
        assert!(!rendered.contains("more cards"));
    }

    #[test]
    fn turn_summary_cards_follow_file_filter() {
        let payload = TimelineSummaryPayload {
            kind: "turn-summary".to_string(),
            version: "2.0".to_string(),
            scope: "turn".to_string(),
            turn_meta: crate::timeline_summary::TurnSummaryTurnMeta::default(),
            prompt: crate::timeline_summary::TurnSummaryPrompt {
                text: "prompt".to_string(),
                intent: "Implement fix".to_string(),
                constraints: vec![],
            },
            outcome: crate::timeline_summary::TurnSummaryOutcome {
                status: "completed".to_string(),
                summary: "done".to_string(),
            },
            evidence: crate::timeline_summary::TurnSummaryEvidence::default(),
            cards: vec![
                crate::timeline_summary::BehaviorCard {
                    card_type: "overview".to_string(),
                    title: "Overview".to_string(),
                    lines: vec!["high-level".to_string()],
                    severity: "info".to_string(),
                },
                crate::timeline_summary::BehaviorCard {
                    card_type: "files".to_string(),
                    title: "Files".to_string(),
                    lines: vec!["path:src/main.rs".to_string()],
                    severity: "info".to_string(),
                },
            ],
            next_steps: vec![],
        };

        let lines =
            render_turn_summary_cards(&payload, true, 100, &HashSet::from([EventFilter::FileOps]));
        let rendered = lines
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("Files"));
        assert!(!rendered.contains("Overview"));
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

fn render_turn_view(
    frame: &mut Frame,
    app: &mut App,
    session_id: &str,
    events: &[DisplayEvent],
    area: Rect,
) {
    let turns = extract_visible_turns(events);
    app.observe_turn_tail_proximity(turns.len());
    if turns.is_empty() {
        let mut lines = vec![Line::from(Span::styled(
            "No turns found.",
            Style::new().fg(Color::DarkGray),
        ))];
        if let Some(issue) = app.detail_issue_for_session(session_id) {
            lines.push(Line::raw(""));
            lines.push(Line::from(Span::styled(
                "Detected session parse issue:",
                Style::new().fg(Theme::ACCENT_RED).bold(),
            )));
            lines.push(Line::from(Span::styled(
                issue.to_string(),
                Style::new().fg(Theme::ACCENT_RED),
            )));
            lines.push(Line::from(Span::styled(
                "Extra timeline analysis is disabled for this session.",
                Style::new().fg(Theme::TEXT_SECONDARY),
            )));
        }
        let p = Paragraph::new(lines)
            .block(Theme::block_dim().title(" Split View "))
            .wrap(Wrap { trim: false });
        frame.render_widget(p, area);
        return;
    }

    app.turn_index = app.turn_index.min(turns.len().saturating_sub(1));

    let [left_area, right_area] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(area);
    let summary_status = app.llm_summary_status_label();
    let turn_window_budget = ((area.height as usize).saturating_sub(4) / 6).clamp(3, 10);
    let turn_window = turn_render_range(turns.len(), app.turn_index, turn_window_budget);

    let mut left_lines: Vec<Line> = Vec::new();
    let mut right_lines: Vec<Line> = Vec::new();
    let mut line_offsets: Vec<u16> = vec![0; turns.len()];

    if turn_window.start > 0 {
        left_lines.push(Line::from(vec![Span::styled(
            format!(
                " … {} earlier turns hidden (K/N: prev turn)",
                turn_window.start
            ),
            Style::new().fg(Theme::TEXT_MUTED),
        )]));
        right_lines.push(Line::raw(""));
        left_lines.push(Line::raw(""));
        right_lines.push(Line::raw(""));
    }

    for (turn_idx, turn) in turns
        .iter()
        .enumerate()
        .skip(turn_window.start)
        .take(turn_window.len())
    {
        line_offsets[turn_idx] = left_lines.len() as u16;

        let focused = turn_idx == app.turn_index;
        let raw_override = app.turn_raw_overrides.contains(&turn_idx);
        let summary_payload =
            app.turn_summary_payload(session_id, turn.turn_index, turn.anchor_source_index);
        let left_width = left_area.width.saturating_sub(4).max(1);
        let right_width = right_area.width.saturating_sub(4).max(1);
        let prompt_expanded = app.turn_prompt_expanded.contains(&turn_idx);

        let prompt_rows =
            render_turn_prompt_card(turn_idx, turn, focused, prompt_expanded, left_width);
        for line in prompt_rows {
            left_lines.push(line);
            right_lines.push(Line::raw(""));
        }

        let right_rows = render_turn_response_panel(
            app,
            turn,
            summary_payload,
            raw_override,
            focused,
            right_width,
            summary_status.as_str(),
        );
        for line in right_rows {
            left_lines.push(Line::raw(""));
            right_lines.push(line);
        }

        left_lines.push(Line::raw(""));
        right_lines.push(Line::raw(""));
    }

    if turn_window.end < turns.len() {
        let hidden = turns.len().saturating_sub(turn_window.end);
        left_lines.push(Line::from(vec![Span::styled(
            format!(" … {hidden} later turns hidden (J/n: next turn)"),
            Style::new().fg(Theme::TEXT_MUTED),
        )]));
        right_lines.push(Line::raw(""));
        left_lines.push(Line::raw(""));
        right_lines.push(Line::raw(""));
    }

    app.turn_line_offsets = line_offsets;
    let visible_h = left_area.height.saturating_sub(2);
    let total = left_lines.len() as u16;
    let max_scroll = total.saturating_sub(visible_h);
    if app.live_mode && app.detail_follow_state().is_following {
        app.turn_agent_scroll = max_scroll;
    }
    app.turn_agent_scroll = app.turn_agent_scroll.min(max_scroll);
    let scroll = (app.turn_agent_scroll, app.turn_h_scroll);

    let left_para = Paragraph::new(left_lines.clone())
        .block(Theme::block().title(" User Prompts "))
        .scroll(scroll);
    let right_para = Paragraph::new(right_lines.clone())
        .block(Theme::block().title(turn_right_panel_title(summary_status.as_str())))
        .scroll(scroll);

    frame.render_widget(left_para, left_area);
    frame.render_widget(right_para, right_area);

    if right_lines.len() > visible_h as usize {
        let mut scrollbar_state =
            ScrollbarState::new(right_lines.len()).position(app.turn_agent_scroll as usize);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .thumb_style(Style::new().fg(Theme::TEXT_MUTED));
        frame.render_stateful_widget(
            scrollbar,
            right_area.inner(Margin {
                vertical: 1,
                horizontal: 0,
            }),
            &mut scrollbar_state,
        );
    }
}

fn turn_render_range(
    turn_count: usize,
    focused_turn: usize,
    budget: usize,
) -> std::ops::Range<usize> {
    if turn_count == 0 {
        return 0..0;
    }
    let budget = budget.max(1).min(turn_count);
    let half = budget / 2;
    let mut start = focused_turn.saturating_sub(half);
    if start + budget > turn_count {
        start = turn_count.saturating_sub(budget);
    }
    let end = (start + budget).min(turn_count);
    start..end
}

fn render_turn_prompt_card(
    turn_idx: usize,
    turn: &crate::app::Turn<'_>,
    focused: bool,
    prompt_expanded: bool,
    content_width: u16,
) -> Vec<Line<'static>> {
    let title_style = if focused {
        Style::new().fg(Theme::ACCENT_BLUE).bold()
    } else {
        Style::new().fg(Theme::TEXT_SECONDARY).bold()
    };
    let border_style = if focused {
        Style::new().fg(Theme::ACCENT_BLUE)
    } else {
        Style::new().fg(Theme::GUTTER)
    };

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(if focused { ">" } else { " " }, title_style),
        Span::styled(format!(" #{} User's prompt", turn_idx + 1), title_style),
    ]));
    lines.push(Line::from(vec![Span::styled("  ┌", border_style)]));

    let prompt_lines = collect_turn_user_lines(turn);
    let prompt_limit = if prompt_expanded {
        usize::MAX
    } else if focused {
        12
    } else {
        4
    };
    let total_prompt_lines = prompt_lines.len();
    for text in prompt_lines.into_iter().take(prompt_limit) {
        lines.extend(wrap_text_lines(
            "  │ ",
            &truncate(&text, 320),
            border_style,
            Style::new().fg(Theme::TEXT_PRIMARY),
            content_width,
        ));
    }
    if total_prompt_lines > prompt_limit {
        let more_line = if focused {
            format!(
                "… {} more prompt lines (p: expand)",
                total_prompt_lines - prompt_limit
            )
        } else {
            format!("… {} more prompt lines", total_prompt_lines - prompt_limit)
        };
        lines.extend(wrap_text_lines(
            "  │ ",
            &more_line,
            border_style,
            Style::new().fg(Theme::TEXT_MUTED),
            content_width,
        ));
    } else if focused && prompt_expanded && total_prompt_lines > 12 {
        lines.extend(wrap_text_lines(
            "  │ ",
            "(p: collapse)",
            border_style,
            Style::new().fg(Theme::TEXT_MUTED),
            content_width,
        ));
    }

    lines.push(Line::from(vec![Span::styled("  └", border_style)]));
    lines
}

fn collect_turn_user_lines(turn: &crate::app::Turn<'_>) -> Vec<String> {
    let mut lines = Vec::new();
    for event in &turn.user_events {
        if App::is_internal_summary_user_event(event) {
            continue;
        }
        let mut pushed_any = false;
        for block in &event.content.blocks {
            for fragment in App::block_text_fragments(block) {
                for line in fragment.lines() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || App::is_internal_summary_title(trimmed) {
                        continue;
                    }
                    lines.push(trimmed.to_string());
                    pushed_any = true;
                }
            }
        }
        if !pushed_any {
            let summary = event_summary(&event.event_type, &event.content.blocks);
            if !summary.trim().is_empty() && !App::is_internal_summary_title(&summary) {
                lines.push(summary);
            }
        }
    }

    if lines.is_empty() {
        lines.push("(no user message)".to_string());
    }
    lines
}

fn render_turn_response_panel(
    app: &App,
    turn: &crate::app::Turn<'_>,
    summary_payload: Option<&TimelineSummaryPayload>,
    raw_override: bool,
    focused: bool,
    content_width: u16,
    summary_status: &str,
) -> Vec<Line<'static>> {
    if let Some(payload) = summary_payload {
        if !raw_override {
            let mut lines = render_agent_output_preview(turn, focused, content_width);
            if !lines.is_empty() {
                lines.push(Line::raw(""));
            }
            lines.extend(render_turn_summary_cards(
                payload,
                focused,
                content_width,
                &app.event_filters,
            ));
            return lines;
        }
    }

    if raw_override {
        return render_turn_raw_thread(
            app,
            turn,
            summary_payload.is_some(),
            raw_override,
            focused,
            content_width,
        );
    }

    if summary_status != "on" {
        return render_turn_fallback_panel(
            turn,
            summary_status,
            focused,
            content_width,
            app.live_mode,
        );
    }

    render_turn_pending_row(app, turn, focused, content_width)
}

fn turn_right_panel_title(summary_status: &str) -> &'static str {
    let _ = summary_status;
    " Agent Chronicle "
}

fn render_agent_output_preview(
    turn: &crate::app::Turn<'_>,
    focused: bool,
    content_width: u16,
) -> Vec<Line<'static>> {
    let Some(preview) = turn_agent_output_preview(turn) else {
        return Vec::new();
    };
    let border_style = if focused {
        Style::new().fg(Theme::ACCENT_CYAN)
    } else {
        Style::new().fg(Theme::GUTTER)
    };
    let mut lines = Vec::new();
    lines.push(Line::from(vec![Span::styled(
        " Agent Output",
        Style::new().fg(Theme::ACCENT_CYAN).bold(),
    )]));
    lines.push(Line::from(vec![Span::styled("  ┌", border_style)]));
    lines.extend(wrap_text_lines(
        "  │ ",
        &preview,
        border_style,
        Style::new().fg(Theme::TEXT_PRIMARY),
        content_width,
    ));
    lines.push(Line::from(vec![Span::styled("  └", border_style)]));
    lines
}

fn turn_agent_output_preview(turn: &crate::app::Turn<'_>) -> Option<String> {
    for event in turn.agent_events.iter().rev() {
        if matches!(event.event_type, EventType::AgentMessage) {
            let text = first_text_line(&event.content.blocks, 220);
            if !text.trim().is_empty() {
                return Some(text);
            }
        }
    }

    for event in turn.agent_events.iter().rev() {
        if let EventType::TaskEnd {
            summary: Some(summary),
        } = &event.event_type
        {
            let summary = summary.trim();
            if !summary.is_empty() {
                return Some(truncate(summary, 220));
            }
        }
    }
    None
}

fn render_turn_fallback_panel(
    turn: &crate::app::Turn<'_>,
    summary_status: &str,
    focused: bool,
    content_width: u16,
    live_mode: bool,
) -> Vec<Line<'static>> {
    let _ = summary_status;
    let (status_text, status_color) = if live_mode {
        (
            "Live mode: rendering task-level execution board.",
            Theme::ACCENT_YELLOW,
        )
    } else {
        (
            "Rendering task-level execution board.",
            Theme::TEXT_SECONDARY,
        )
    };
    let title_style = if focused {
        Style::new().fg(status_color).bold()
    } else {
        Style::new().fg(Theme::TEXT_SECONDARY).bold()
    };

    let mut lines = Vec::new();
    lines.push(Line::from(vec![Span::styled(" Task Board", title_style)]));
    lines.push(Line::from(vec![Span::styled(
        format!(" {status_text}"),
        Style::new().fg(status_color),
    )]));
    if live_mode {
        let live_rows = turn_live_activity_rows(turn, if focused { 6 } else { 3 });
        if !live_rows.is_empty() {
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![Span::styled(
                " Live activity",
                Style::new().fg(Theme::ACCENT_TEAL).bold(),
            )]));
            for row in live_rows {
                lines.extend(wrap_text_lines(
                    "  - ",
                    &row,
                    Style::new().fg(Theme::TEXT_MUTED),
                    Style::new().fg(Theme::TEXT_PRIMARY),
                    content_width,
                ));
            }
        }
    }

    let buckets = build_task_chronicle_buckets(turn);
    if buckets.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "  (no agent events captured)",
            Style::new().fg(Theme::TEXT_MUTED),
        )]));
        return lines;
    }

    let mut synthetic_stub_count = 0usize;
    let mut synthetic_latest: Option<DateTime<Utc>> = None;
    let mut synthetic_stub_task_ids: HashSet<String> = HashSet::new();
    for bucket in &buckets {
        if task_bucket_is_synthetic_end_stub(bucket) {
            synthetic_stub_count += 1;
            synthetic_stub_task_ids.insert(bucket.task_key.clone());
            if let Some(ts) = bucket.last_timestamp {
                if synthetic_latest.map(|current| ts > current).unwrap_or(true) {
                    synthetic_latest = Some(ts);
                }
            }
        }
    }

    let lane_panels = build_turn_lane_panels(turn, &synthetic_stub_task_ids);
    if lane_panels.is_empty() {
        if synthetic_stub_count > 0 {
            lines.push(Line::from(vec![Span::styled(
                "  (only synthetic-end stub tasks in this turn)",
                Style::new().fg(Theme::TEXT_MUTED),
            )]));
        } else {
            lines.push(Line::from(vec![Span::styled(
                "  (no lane events captured)",
                Style::new().fg(Theme::TEXT_MUTED),
            )]));
        }
        return lines;
    }

    let running = lane_panels
        .iter()
        .filter(|panel| panel.status == TaskBucketStatus::Running)
        .count();
    let errors = lane_panels
        .iter()
        .filter(|panel| panel.status == TaskBucketStatus::Error)
        .count();
    let done = lane_panels
        .iter()
        .filter(|panel| panel.status == TaskBucketStatus::Done)
        .count();
    let total_tool_ops = lane_panels
        .iter()
        .map(|panel| panel.tool_ops)
        .sum::<usize>();
    let total_file_ops = lane_panels
        .iter()
        .map(|panel| panel.file_ops)
        .sum::<usize>();
    let total_shell_ops = lane_panels
        .iter()
        .map(|panel| panel.shell_ops)
        .sum::<usize>();
    let total_error_ops = lane_panels
        .iter()
        .map(|panel| panel.error_count)
        .sum::<usize>();
    lines.push(Line::from(vec![Span::styled(
        format!(
            " running:{running}  error:{errors}  done:{done}  buckets:{}",
            lane_panels.len()
        ),
        Style::new().fg(Theme::TEXT_SECONDARY),
    )]));
    lines.push(Line::from(vec![Span::styled(
        format!(
            " ops  tool:{total_tool_ops}  file:{total_file_ops}  shell:{total_shell_ops}  err:{total_error_ops}"
        ),
        Style::new().fg(Theme::TEXT_MUTED),
    )]));
    if synthetic_stub_count > 0 {
        let suffix = synthetic_latest
            .map(format_time_ago)
            .map(|age| format!(" · last {age}"))
            .unwrap_or_default();
        lines.push(Line::from(vec![Span::styled(
            format!(" synthetic-end stubs: {synthetic_stub_count} collapsed{suffix}"),
            Style::new().fg(Theme::TEXT_MUTED),
        )]));
    }
    lines.push(Line::raw(""));

    let lane_panels = select_turn_lane_panels_for_render(lane_panels);
    lines.extend(render_turn_lane_panel_grid(
        &lane_panels,
        focused,
        content_width,
    ));

    if synthetic_stub_count > 0 {
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![Span::styled(
            "  (only synthetic-end stub tasks in this turn)",
            Style::new().fg(Theme::TEXT_MUTED),
        )]));
    }

    lines
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskBucketStatus {
    Running,
    Done,
    Error,
}

#[derive(Debug, Clone)]
struct TaskChronicleBucket<'a> {
    task_key: String,
    events: Vec<&'a Event>,
    last_timestamp: Option<DateTime<Utc>>,
    status: TaskBucketStatus,
    tool_ops: usize,
    file_ops: usize,
    shell_ops: usize,
    error_count: usize,
    last_output: Option<String>,
}

const TURN_RAW_CHUNK_SIZE: usize = 20;
const TURN_MAX_INDIVIDUAL_PANELS: usize = 4;

#[derive(Debug, Clone)]
struct TurnLaneEntry<'a> {
    event: &'a Event,
    lane: usize,
    marker: LaneMarker,
}

#[derive(Debug, Clone)]
struct LaneChroniclePanel<'a> {
    label: String,
    lane: usize,
    entries: Vec<TurnLaneEntry<'a>>,
    status: TaskBucketStatus,
    tool_ops: usize,
    file_ops: usize,
    shell_ops: usize,
    error_count: usize,
    last_output: Option<String>,
    last_timestamp: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
struct TurnLaneRow {
    timestamp: String,
    lane_label: String,
    event_kind: String,
    task_label: String,
    marker: Option<&'static str>,
    summary: String,
}

#[derive(Default)]
struct StructuredTurnLogSummary {
    timestamp: Option<String>,
    lane: Option<String>,
    event_kind: Option<String>,
    task: Option<String>,
    summary: Option<String>,
}

fn build_turn_lane_panels<'a>(
    turn: &crate::app::Turn<'a>,
    synthetic_stub_task_ids: &HashSet<String>,
) -> Vec<LaneChroniclePanel<'a>> {
    let entries = build_turn_lane_entries(turn, synthetic_stub_task_ids);
    if entries.is_empty() {
        return Vec::new();
    }

    let mut groups: Vec<(usize, Vec<TurnLaneEntry<'a>>)> = Vec::new();
    for entry in entries {
        if let Some((_, lane_entries)) = groups.iter_mut().find(|(lane, _)| *lane == entry.lane) {
            lane_entries.push(entry);
        } else {
            groups.push((entry.lane, vec![entry]));
        }
    }

    groups.sort_by_key(|(lane, _)| *lane);
    groups
        .into_iter()
        .map(|(lane, lane_entries)| build_lane_chronicle_panel(lane, lane_entries, None))
        .collect()
}

fn build_turn_lane_entries<'a>(
    turn: &crate::app::Turn<'a>,
    synthetic_stub_task_ids: &HashSet<String>,
) -> Vec<TurnLaneEntry<'a>> {
    let mut entries = Vec::new();
    let mut task_lane: HashMap<String, usize> = HashMap::new();
    let mut active_lanes: BTreeSet<usize> = BTreeSet::new();
    let mut reusable_lanes: BTreeSet<usize> = BTreeSet::new();
    let mut tool_call_lane_by_event_id: HashMap<String, usize> = HashMap::new();
    let mut next_lane = 1usize;

    for &event in &turn.agent_events {
        let task_id = event_task_key(event);
        if task_id.is_some_and(|task| {
            synthetic_stub_task_ids.contains(task) && is_synthetic_task_end_event(event)
        }) {
            continue;
        }

        let mut lane = 0usize;
        let mut marker = LaneMarker::None;
        let call_result_lane = match &event.event_type {
            EventType::ToolResult {
                call_id: Some(call_event_id),
                ..
            } => tool_call_lane_by_event_id.get(call_event_id).copied(),
            _ => None,
        };

        match &event.event_type {
            EventType::TaskStart { .. } => {
                if let Some(task_id) = task_id {
                    let existed = task_lane.contains_key(task_id);
                    lane = task_lane.get(task_id).copied().unwrap_or_else(|| {
                        allocate_turn_lane(
                            &mut task_lane,
                            &mut reusable_lanes,
                            &mut next_lane,
                            task_id,
                        )
                    });
                    if lane > 0 {
                        let was_active = active_lanes.contains(&lane);
                        active_lanes.insert(lane);
                        if !existed && !was_active {
                            marker = LaneMarker::Fork;
                        }
                    }
                }
            }
            EventType::TaskEnd { .. } => {
                if let Some(task_id) = task_id {
                    lane = task_lane.get(task_id).copied().unwrap_or_else(|| {
                        allocate_turn_lane(
                            &mut task_lane,
                            &mut reusable_lanes,
                            &mut next_lane,
                            task_id,
                        )
                    });
                    if lane > 0 {
                        marker = LaneMarker::Merge;
                    }
                } else if let Some(mapped_lane) = call_result_lane {
                    lane = mapped_lane;
                } else if let Some(inferred_lane) = infer_codex_untagged_lane(event, &active_lanes)
                {
                    lane = inferred_lane;
                }
            }
            _ => {
                if let Some(task_id) = task_id {
                    if let Some(existing_lane) = task_lane.get(task_id).copied() {
                        lane = existing_lane;
                        if lane > 0 {
                            active_lanes.insert(lane);
                        }
                    } else {
                        lane = allocate_turn_lane(
                            &mut task_lane,
                            &mut reusable_lanes,
                            &mut next_lane,
                            task_id,
                        );
                        if lane > 0 {
                            active_lanes.insert(lane);
                            marker = LaneMarker::Fork;
                        }
                    }
                } else if let Some(mapped_lane) = call_result_lane {
                    lane = mapped_lane;
                } else if let Some(inferred_lane) = infer_codex_untagged_lane(event, &active_lanes)
                {
                    lane = inferred_lane;
                }
            }
        }

        if matches!(event.event_type, EventType::ToolCall { .. }) && lane > 0 {
            tool_call_lane_by_event_id.insert(event.event_id.clone(), lane);
        }

        entries.push(TurnLaneEntry {
            event,
            lane,
            marker,
        });

        if matches!(event.event_type, EventType::TaskEnd { .. }) {
            if let Some(task_id) = task_id {
                if let Some(ended_lane) = task_lane.remove(task_id) {
                    if ended_lane > 0 {
                        active_lanes.remove(&ended_lane);
                        reusable_lanes.insert(ended_lane);
                    }
                }
            }
        }
    }

    entries
}

fn event_task_key(event: &Event) -> Option<&str> {
    event
        .task_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            event
                .attributes
                .get("subagent_id")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
}

fn infer_codex_untagged_lane(event: &Event, active_lanes: &BTreeSet<usize>) -> Option<usize> {
    if !event.event_id.starts_with("codex-") || active_lanes.len() != 1 {
        return None;
    }

    if matches!(
        event.event_type,
        EventType::TaskStart { .. } | EventType::TaskEnd { .. }
    ) {
        return None;
    }

    active_lanes.iter().next().copied()
}

fn allocate_turn_lane(
    task_lane: &mut HashMap<String, usize>,
    reusable_lanes: &mut BTreeSet<usize>,
    next_lane: &mut usize,
    task_id: &str,
) -> usize {
    let allocated = if let Some(reused) = reusable_lanes.iter().next().copied() {
        reusable_lanes.remove(&reused);
        reused
    } else {
        let lane = *next_lane;
        *next_lane += 1;
        lane
    };
    task_lane.insert(task_id.to_string(), allocated);
    allocated
}

fn build_lane_chronicle_panel<'a>(
    lane: usize,
    entries: Vec<TurnLaneEntry<'a>>,
    custom_label: Option<String>,
) -> LaneChroniclePanel<'a> {
    let mut open_tasks: HashSet<String> = HashSet::new();
    let mut open_untagged = 0usize;
    let mut saw_end = false;
    let mut tool_ops = 0usize;
    let mut file_ops = 0usize;
    let mut shell_ops = 0usize;
    let mut error_count = 0usize;
    let mut seen_tasks: HashSet<String> = HashSet::new();
    let mut task_ids: Vec<String> = Vec::new();

    for entry in &entries {
        if let Some(task_id) = entry
            .event
            .task_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            if seen_tasks.insert(task_id.to_string()) {
                task_ids.push(task_id.to_string());
            }
        }

        match &entry.event.event_type {
            EventType::TaskStart { .. } => {
                if let Some(task_id) = entry
                    .event
                    .task_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    open_tasks.insert(task_id.to_string());
                } else {
                    open_untagged = open_untagged.saturating_add(1);
                }
            }
            EventType::TaskEnd { .. } => {
                saw_end = true;
                if let Some(task_id) = entry
                    .event
                    .task_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    open_tasks.remove(task_id);
                } else {
                    open_untagged = open_untagged.saturating_sub(1);
                }
            }
            EventType::ToolCall { .. } => tool_ops += 1,
            EventType::ToolResult { is_error, .. } => {
                if *is_error {
                    error_count += 1;
                }
            }
            EventType::FileRead { .. }
            | EventType::FileEdit { .. }
            | EventType::FileCreate { .. }
            | EventType::FileDelete { .. } => file_ops += 1,
            EventType::ShellCommand { .. } => shell_ops += 1,
            EventType::Custom { kind } => {
                let lower = kind.to_ascii_lowercase();
                if lower.contains("error") || lower.contains("fail") {
                    error_count += 1;
                }
            }
            _ => {}
        }
    }

    let status = if error_count > 0 {
        TaskBucketStatus::Error
    } else if !open_tasks.is_empty() || open_untagged > 0 {
        TaskBucketStatus::Running
    } else if saw_end || !entries.is_empty() {
        TaskBucketStatus::Done
    } else {
        TaskBucketStatus::Running
    };

    let last_output = entries.iter().rev().find_map(|entry| {
        if !matches!(
            entry.event.event_type,
            EventType::AgentMessage
                | EventType::TaskEnd { .. }
                | EventType::ToolResult { .. }
                | EventType::Custom { .. }
        ) {
            return None;
        }
        let text = task_board_event_summary(entry.event, 220);
        if text.trim().is_empty() {
            None
        } else {
            Some(text)
        }
    });

    let label = custom_label.unwrap_or_else(|| lane_panel_label(lane, &entries, &task_ids));

    LaneChroniclePanel {
        label,
        lane,
        last_timestamp: entries.last().map(|entry| entry.event.timestamp),
        entries,
        status,
        tool_ops,
        file_ops,
        shell_ops,
        error_count,
        last_output,
    }
}

fn lane_panel_label(lane: usize, entries: &[TurnLaneEntry<'_>], task_ids: &[String]) -> String {
    if lane == 0 {
        return "main".to_string();
    }

    if let Some(title) = entries
        .iter()
        .find_map(|entry| match &entry.event.event_type {
            EventType::TaskStart { title } => title
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| compact_text_snippet(value, 28)),
            _ => None,
        })
    {
        if !title.is_empty() {
            return format!("lane {lane} {title}");
        }
    }

    if let Some(task_id) = task_ids.first() {
        return format!("lane {lane} task {}", compact_task_id(task_id));
    }

    format!("lane {lane}")
}

fn select_turn_lane_panels_for_render<'a>(
    mut panels: Vec<LaneChroniclePanel<'a>>,
) -> Vec<LaneChroniclePanel<'a>> {
    panels.sort_by_key(|panel| panel.lane);
    if panels.len() <= TURN_MAX_INDIVIDUAL_PANELS {
        return panels;
    }

    let overflow = panels.split_off(TURN_MAX_INDIVIDUAL_PANELS);
    panels.push(merge_overflow_lane_panels(overflow));
    panels
}

fn merge_overflow_lane_panels<'a>(panels: Vec<LaneChroniclePanel<'a>>) -> LaneChroniclePanel<'a> {
    let lane_count = panels.len();
    let mut entries: Vec<TurnLaneEntry<'a>> = panels
        .into_iter()
        .flat_map(|panel| panel.entries.into_iter())
        .collect();
    entries.sort_by(|lhs, rhs| {
        lhs.event
            .timestamp
            .cmp(&rhs.event.timestamp)
            .then_with(|| lhs.event.event_id.cmp(&rhs.event.event_id))
    });
    build_lane_chronicle_panel(usize::MAX, entries, Some(format!("+{lane_count} lanes")))
}

fn render_turn_lane_panel_grid(
    panels: &[LaneChroniclePanel<'_>],
    focused: bool,
    content_width: u16,
) -> Vec<Line<'static>> {
    if panels.is_empty() {
        return Vec::new();
    }

    let panel_count = panels.len().max(1);
    let gap_width = 2usize;
    let usable_width = content_width as usize;
    let panel_width =
        usable_width.saturating_sub(gap_width * panel_count.saturating_sub(1)) / panel_count;
    let panel_width = panel_width.max(1);

    let rendered_panels: Vec<Vec<String>> = panels
        .iter()
        .map(|panel| render_single_turn_lane_panel(panel, focused, panel_width))
        .collect();
    let max_rows = rendered_panels
        .iter()
        .map(std::vec::Vec::len)
        .max()
        .unwrap_or(0);

    let mut lines = Vec::new();
    for row_idx in 0..max_rows {
        let mut spans = Vec::new();
        for (panel_idx, panel) in panels.iter().enumerate() {
            let cell_raw = rendered_panels[panel_idx]
                .get(row_idx)
                .map(String::as_str)
                .unwrap_or("");
            let cell = pad_panel_cell(cell_raw, panel_width);
            let style = if row_idx == 0 {
                Style::new()
                    .fg(task_bucket_status_badge(&panel.status).1)
                    .bold()
            } else if cell_raw.starts_with("events chunk")
                || cell_raw.starts_with("... ")
                || cell_raw.starts_with("events:")
            {
                Style::new().fg(Theme::TEXT_MUTED)
            } else if cell_raw.starts_with("output:") {
                Style::new().fg(Theme::ACCENT_CYAN)
            } else {
                Style::new().fg(Theme::TEXT_PRIMARY)
            };
            spans.push(Span::styled(cell, style));
            if panel_idx + 1 < panel_count {
                spans.push(Span::styled(
                    " ".repeat(gap_width),
                    Style::new().fg(Theme::GUTTER),
                ));
            }
        }
        lines.push(Line::from(spans));
    }

    lines
}

fn render_single_turn_lane_panel(
    panel: &LaneChroniclePanel<'_>,
    focused: bool,
    width: usize,
) -> Vec<String> {
    let age = panel
        .last_timestamp
        .map(format_time_ago)
        .unwrap_or_else(|| "-".to_string());
    let (status_label, _) = task_bucket_status_badge(&panel.status);

    let mut lines = vec![
        truncate_to_width(
            format!(
                "[{}] {status_label} · {} events · last {age}",
                panel.label,
                panel.entries.len()
            ),
            width,
        ),
        truncate_to_width(
            format!(
                "ops  tool:{}  file:{}  shell:{}  err:{}",
                panel.tool_ops, panel.file_ops, panel.shell_ops, panel.error_count
            ),
            width,
        ),
    ];

    let bucket = lane_panel_to_bucket(panel);
    let action_hints = task_bucket_action_hints(&bucket, if focused { 3 } else { 2 });
    if !action_hints.is_empty() {
        lines.push(truncate_to_width(
            format!("actions: {}", action_hints.join("  ·  ")),
            width,
        ));
    }

    if let Some(last_output) = panel.last_output.as_deref() {
        lines.push(truncate_to_width(
            format!("output: {}", truncate(last_output, 180)),
            width,
        ));
    } else {
        lines.push(truncate_to_width("output: (none)".to_string(), width));
    }

    let activity_lines = task_bucket_activity_lines(&bucket, if focused { 2 } else { 1 });
    if !activity_lines.is_empty() {
        lines.push(truncate_to_width(
            format!("recent: {}", activity_lines.join("  |  ")),
            width,
        ));
    }

    let rows = lane_panel_rows(panel);
    if rows.is_empty() {
        lines.push(truncate_to_width("events: (none)".to_string(), width));
        return lines;
    }

    let chunks: Vec<&[TurnLaneRow]> = rows.chunks(TURN_RAW_CHUNK_SIZE).collect();
    let visible_chunk_count = if focused {
        chunks.len()
    } else {
        chunks.len().min(1)
    };
    for (chunk_idx, chunk) in chunks.iter().take(visible_chunk_count).enumerate() {
        lines.push(truncate_to_width(
            format!("events chunk {}/{}", chunk_idx + 1, chunks.len()),
            width,
        ));
        for row in *chunk {
            lines.push(truncate_to_width(format_lane_panel_row(row), width));
        }
    }

    if visible_chunk_count < chunks.len() {
        lines.push(truncate_to_width(
            format!("... {} more chunks", chunks.len() - visible_chunk_count),
            width,
        ));
    }

    lines
}

fn lane_panel_to_bucket<'a>(panel: &'a LaneChroniclePanel<'a>) -> TaskChronicleBucket<'a> {
    TaskChronicleBucket {
        task_key: panel.label.clone(),
        events: panel.entries.iter().map(|entry| entry.event).collect(),
        last_timestamp: panel.last_timestamp,
        status: panel.status,
        tool_ops: panel.tool_ops,
        file_ops: panel.file_ops,
        shell_ops: panel.shell_ops,
        error_count: panel.error_count,
        last_output: panel.last_output.clone(),
    }
}

fn lane_panel_rows(panel: &LaneChroniclePanel<'_>) -> Vec<TurnLaneRow> {
    let mut rows = Vec::new();
    let mut last_key = String::new();

    for entry in &panel.entries {
        let row = build_turn_lane_row(entry);
        if should_hide_lane_row(entry.event, &row.summary) {
            continue;
        }

        let dedupe_key = normalize_activity_key(&format!("{} {}", row.event_kind, row.summary));
        let is_boundary = matches!(
            entry.event.event_type,
            EventType::TaskStart { .. }
                | EventType::TaskEnd { .. }
                | EventType::ToolResult { is_error: true, .. }
        );
        if !is_boundary && !last_key.is_empty() && dedupe_key == last_key {
            continue;
        }
        last_key = dedupe_key;
        rows.push(row);
    }

    if rows.is_empty() {
        if let Some(entry) = panel.entries.last() {
            rows.push(build_turn_lane_row(entry));
        }
    }

    rows
}

fn build_turn_lane_row(entry: &TurnLaneEntry<'_>) -> TurnLaneRow {
    let (kind, _) = event_type_display(&entry.event.event_type, true);
    let mut summary = task_board_event_summary(entry.event, 120);
    summary = strip_kind_prefix(&summary, kind);
    if summary.is_empty() {
        summary = kind.to_string();
    }
    let task_label = event_task_key(entry.event)
        .map(compact_task_id)
        .unwrap_or_else(|| "main".to_string());
    let mut row = TurnLaneRow {
        timestamp: entry.event.timestamp.format("%H:%M:%S").to_string(),
        lane_label: if entry.lane == 0 {
            "main".to_string()
        } else {
            format!("L{}", entry.lane)
        },
        event_kind: kind.to_string(),
        task_label,
        marker: lane_marker_text(entry.marker),
        summary,
    };

    if let Some(structured) = parse_structured_turn_log_summary(entry.event) {
        if let Some(ts) = structured.timestamp {
            row.timestamp = ts;
        }
        if let Some(kind) = structured.event_kind {
            row.event_kind = kind;
        }
        if row.task_label == "main" {
            if let Some(task) = structured.task {
                row.task_label = task;
            }
        }
        if entry.lane == 0 {
            if let Some(lane) = structured.lane {
                row.lane_label = lane;
            }
        }
        if let Some(summary) = structured.summary {
            row.summary = summary;
        }
    }

    row
}

fn should_hide_lane_row(event: &Event, summary: &str) -> bool {
    match &event.event_type {
        EventType::Thinking => true,
        EventType::ToolResult { is_error, .. } => {
            !*is_error
                && (is_low_signal_activity_summary(summary)
                    || summary.eq_ignore_ascii_case("ok")
                    || normalize_activity_key(summary).ends_with(" ok"))
        }
        EventType::AgentMessage | EventType::Custom { .. } => {
            is_low_signal_activity_summary(summary)
        }
        _ => false,
    }
}

fn format_lane_panel_row(row: &TurnLaneRow) -> String {
    let marker = row
        .marker
        .map(|value| format!(" [{value}]"))
        .unwrap_or_default();
    let task_hint =
        if row.task_label != "main" && matches!(row.event_kind.as_str(), "start" | "end") {
            format!(" [task {}]", row.task_label)
        } else {
            String::new()
        };
    format!(
        "{} {:<6} {}{}{}",
        row.timestamp, row.event_kind, row.summary, task_hint, marker
    )
}

fn lane_marker_text(marker: LaneMarker) -> Option<&'static str> {
    match marker {
        LaneMarker::Fork => Some("fork"),
        LaneMarker::Merge => Some("merge"),
        LaneMarker::None => None,
    }
}

fn pad_panel_cell(text: &str, width: usize) -> String {
    let mut out = truncate_to_width(text.to_string(), width);
    let current_width = UnicodeWidthStr::width(out.as_str());
    if current_width < width {
        out.push_str(&" ".repeat(width - current_width));
    }
    out
}

fn task_bucket_status_badge(status: &TaskBucketStatus) -> (&'static str, Color) {
    match status {
        TaskBucketStatus::Running => ("running", Theme::ACCENT_YELLOW),
        TaskBucketStatus::Done => ("done", Theme::ACCENT_GREEN),
        TaskBucketStatus::Error => ("error", Theme::ACCENT_RED),
    }
}

fn task_bucket_status_sort_value(status: &TaskBucketStatus) -> usize {
    match status {
        TaskBucketStatus::Running => 0,
        TaskBucketStatus::Error => 1,
        TaskBucketStatus::Done => 2,
    }
}

fn sort_task_buckets<'a>(
    mut buckets: Vec<TaskChronicleBucket<'a>>,
) -> Vec<TaskChronicleBucket<'a>> {
    buckets.sort_by(|lhs, rhs| {
        task_bucket_status_sort_value(&lhs.status)
            .cmp(&task_bucket_status_sort_value(&rhs.status))
            .then_with(|| rhs.last_timestamp.cmp(&lhs.last_timestamp))
            .then_with(|| rhs.events.len().cmp(&lhs.events.len()))
            .then_with(|| lhs.task_key.cmp(&rhs.task_key))
    });
    buckets
}

fn build_task_chronicle_buckets<'a>(turn: &crate::app::Turn<'a>) -> Vec<TaskChronicleBucket<'a>> {
    let buckets = build_task_chronicle_buckets_unsorted(turn);
    sort_task_buckets(buckets)
}

fn build_task_chronicle_buckets_unsorted<'a>(
    turn: &crate::app::Turn<'a>,
) -> Vec<TaskChronicleBucket<'a>> {
    let mut groups: Vec<(String, Vec<&'a Event>)> = Vec::new();
    for &event in &turn.agent_events {
        let key = event
            .task_id
            .as_deref()
            .map(str::trim)
            .filter(|task| !task.is_empty())
            .unwrap_or("main")
            .to_string();
        if let Some((_, events)) = groups.iter_mut().find(|(group_key, _)| *group_key == key) {
            events.push(event);
        } else {
            groups.push((key, vec![event]));
        }
    }

    groups
        .into_iter()
        .map(|(task_key, events)| {
            let mut open_tasks = 0usize;
            let mut saw_end = false;
            let mut tool_ops = 0usize;
            let mut file_ops = 0usize;
            let mut shell_ops = 0usize;
            let mut error_count = 0usize;

            for event in &events {
                match &event.event_type {
                    EventType::TaskStart { .. } => {
                        open_tasks = open_tasks.saturating_add(1);
                    }
                    EventType::TaskEnd { .. } => {
                        open_tasks = open_tasks.saturating_sub(1);
                        saw_end = true;
                    }
                    EventType::ToolCall { .. } => tool_ops += 1,
                    EventType::ToolResult { is_error, .. } => {
                        if *is_error {
                            error_count += 1;
                        }
                    }
                    EventType::FileRead { .. }
                    | EventType::FileEdit { .. }
                    | EventType::FileCreate { .. }
                    | EventType::FileDelete { .. } => file_ops += 1,
                    EventType::ShellCommand { .. } => shell_ops += 1,
                    EventType::Custom { kind } => {
                        let lower = kind.to_ascii_lowercase();
                        if lower.contains("error") || lower.contains("fail") {
                            error_count += 1;
                        }
                    }
                    _ => {}
                }
            }

            let status = if error_count > 0 {
                TaskBucketStatus::Error
            } else if open_tasks > 0 {
                TaskBucketStatus::Running
            } else if saw_end || !events.is_empty() {
                TaskBucketStatus::Done
            } else {
                TaskBucketStatus::Running
            };

            let last_output = events.iter().rev().find_map(|event| {
                if !matches!(
                    event.event_type,
                    EventType::AgentMessage
                        | EventType::TaskEnd { .. }
                        | EventType::ToolResult { .. }
                        | EventType::Custom { .. }
                ) {
                    return None;
                }
                let text = task_board_event_summary(event, 220);
                if text.trim().is_empty() {
                    None
                } else {
                    Some(text)
                }
            });

            TaskChronicleBucket {
                task_key,
                last_timestamp: events.last().map(|event| event.timestamp),
                events,
                status,
                tool_ops,
                file_ops,
                shell_ops,
                error_count,
                last_output,
            }
        })
        .collect()
}

fn normalize_activity_key(text: &str) -> String {
    text.split_whitespace()
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn json_value_hint(value: &serde_json::Value, max_len: usize) -> Option<String> {
    match value {
        serde_json::Value::String(text) => {
            let text = compact_text_snippet(text, max_len);
            if text.is_empty() {
                None
            } else {
                Some(text)
            }
        }
        serde_json::Value::Number(number) => Some(number.to_string()),
        serde_json::Value::Bool(flag) => Some(flag.to_string()),
        serde_json::Value::Array(items) => {
            if items.is_empty() {
                None
            } else {
                Some(format!("items={}", items.len()))
            }
        }
        serde_json::Value::Object(map) => {
            let preferred_keys = [
                "path",
                "file_path",
                "file",
                "query",
                "pattern",
                "url",
                "command",
                "cmd",
                "subject",
                "recipient",
                "title",
                "task_id",
                "turn_id",
                "status",
                "reason",
            ];

            for key in preferred_keys {
                if let Some(value) = map.get(key) {
                    if key == "command" {
                        if let Some(arr) = value.as_array() {
                            let parts = arr
                                .iter()
                                .filter_map(|v| v.as_str())
                                .collect::<Vec<_>>()
                                .join(" ");
                            let parts = compact_text_snippet(&parts, max_len.saturating_sub(9));
                            if !parts.is_empty() {
                                return Some(format!("command={parts}"));
                            }
                        }
                    }
                    if let Some(hint) =
                        json_value_hint(value, max_len.saturating_sub(key.len() + 1))
                    {
                        return Some(format!("{key}={hint}"));
                    }
                }
            }

            if let Some(questions) = map.get("questions").and_then(|value| value.as_array()) {
                return Some(format!("questions={}", questions.len()));
            }
            if let Some(tool_uses) = map.get("tool_uses").and_then(|value| value.as_array()) {
                return Some(format!("tool_uses={}", tool_uses.len()));
            }

            let keys = map
                .keys()
                .take(3)
                .map(|key| key.to_string())
                .collect::<Vec<_>>();
            if keys.is_empty() {
                None
            } else {
                Some(format!("keys={}", keys.join(",")))
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

fn first_text_line_opt(blocks: &[ContentBlock], max_len: usize) -> Option<String> {
    let line = first_text_line(blocks, max_len);
    if line.trim().is_empty() {
        None
    } else {
        Some(line)
    }
}

fn is_low_signal_text_line(line: &str) -> bool {
    let lower = line.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return true;
    }
    if lower.starts_with("chunk id:") || lower.contains("chunk id:") {
        return true;
    }
    if lower.starts_with("wall time:")
        || lower.starts_with("process exited with code")
        || lower.starts_with("original token count:")
        || lower.starts_with("token count:")
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

fn attributes_hint(
    attributes: &std::collections::HashMap<String, serde_json::Value>,
    max_len: usize,
) -> Option<String> {
    let preferred_keys = [
        "reason", "message", "error", "status", "path", "query", "pattern", "url", "command", "cmd",
    ];
    for key in preferred_keys {
        if let Some(value) = attributes.get(key) {
            if let Some(rendered) = json_value_hint(value, max_len.saturating_sub(key.len() + 1)) {
                return Some(format!("{key}={rendered}"));
            }
        }
    }
    None
}

fn strip_kind_prefix(summary: &str, kind: &str) -> String {
    let trimmed = summary.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let lower_kind = kind.to_ascii_lowercase();
    let lower_summary = trimmed.to_ascii_lowercase();
    if lower_summary == lower_kind {
        return String::new();
    }
    let prefix = format!("{lower_kind} ");
    if lower_summary.starts_with(&prefix) {
        let stripped = trimmed
            .chars()
            .skip(kind.chars().count())
            .collect::<String>();
        return stripped.trim_start().to_string();
    }
    trimmed.to_string()
}

fn task_board_event_summary(event: &Event, max_len: usize) -> String {
    if let Some(parsed) = parse_structured_turn_log_summary(event) {
        if let Some(summary) = parsed.summary.filter(|value| !value.is_empty()) {
            return compact_text_snippet(&summary, max_len);
        }
    }

    let summary = match &event.event_type {
        EventType::TaskStart { title } => {
            if let Some(title) = title
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                title.to_string()
            } else if let Some(hint) = first_text_line_opt(&event.content.blocks, max_len) {
                if hint.eq_ignore_ascii_case("task started") {
                    String::new()
                } else {
                    hint
                }
            } else {
                String::new()
            }
        }
        EventType::TaskEnd { summary } => {
            if let Some(summary) = summary
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                summary.to_string()
            } else if let Some(hint) = first_text_line_opt(&event.content.blocks, max_len) {
                hint
            } else {
                "completed".to_string()
            }
        }
        EventType::ToolCall { name } => {
            let hint = first_json_block_hint(&event.content.blocks, max_len.saturating_sub(8))
                .or_else(|| first_code_line(&event.content.blocks, max_len.saturating_sub(8)))
                .or_else(|| first_text_line_opt(&event.content.blocks, max_len.saturating_sub(8)))
                .or_else(|| attributes_hint(&event.attributes, max_len.saturating_sub(8)));
            if let Some(hint) = hint {
                format!("{name} {hint}")
            } else {
                format!("{name}()")
            }
        }
        EventType::ToolResult { name, is_error, .. } => {
            let hint =
                first_meaningful_text_line_opt(&event.content.blocks, max_len.saturating_sub(16))
                    .or_else(|| first_code_line(&event.content.blocks, max_len.saturating_sub(16)))
                    .or_else(|| {
                        first_json_block_hint(&event.content.blocks, max_len.saturating_sub(16))
                    })
                    .or_else(|| attributes_hint(&event.attributes, max_len.saturating_sub(16)));
            match (is_error, hint) {
                (true, Some(hint)) => format!("{name} error: {hint}"),
                (false, Some(hint)) => format!("{name}: {hint}"),
                (true, None) => format!("{name} failed"),
                (false, None) => format!("{name} ok"),
            }
        }
        EventType::Custom { kind } => {
            let hint = attributes_hint(&event.attributes, max_len.saturating_sub(10))
                .or_else(|| {
                    first_meaningful_text_line_opt(
                        &event.content.blocks,
                        max_len.saturating_sub(10),
                    )
                })
                .or_else(|| {
                    first_json_block_hint(&event.content.blocks, max_len.saturating_sub(10))
                })
                .or_else(|| first_code_line(&event.content.blocks, max_len.saturating_sub(10)));
            if let Some(hint) = hint {
                format!("{kind} {hint}")
            } else {
                kind.to_string()
            }
        }
        _ => event_summary(&event.event_type, &event.content.blocks),
    };

    compact_text_snippet(&summary, max_len)
}

fn parse_structured_turn_log_summary(event: &Event) -> Option<StructuredTurnLogSummary> {
    for block in &event.content.blocks {
        match block {
            ContentBlock::Json { data } => {
                if let Some(summary) = parse_structured_turn_log_summary_json(data) {
                    return Some(summary);
                }
            }
            ContentBlock::Text { text } => {
                for line in text.lines() {
                    if let Some(summary) = parse_structured_turn_log_summary_line(line) {
                        return Some(summary);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_structured_turn_log_summary_line(line: &str) -> Option<StructuredTurnLogSummary> {
    let trimmed = line.trim();
    if !(trimmed.starts_with('{') && trimmed.ends_with('}')) {
        return None;
    }
    let parsed = serde_json::from_str::<serde_json::Value>(trimmed).ok()?;
    parse_structured_turn_log_summary_json(&parsed)
}

fn parse_structured_turn_log_summary_json(
    value: &serde_json::Value,
) -> Option<StructuredTurnLogSummary> {
    let obj = value.as_object()?;
    let summary = StructuredTurnLogSummary {
        timestamp: extract_structured_text(obj, &["ts", "timestamp", "time"])
            .and_then(|value| normalize_structured_timestamp(&value)),
        lane: extract_structured_text(obj, &["lane"])
            .and_then(|value| normalize_structured_lane(&value)),
        event_kind: extract_structured_text(obj, &["type", "event_type", "event", "kind"])
            .and_then(|value| normalize_structured_event_kind(&value)),
        task: extract_structured_text(obj, &["task", "task_id", "subagent_id"])
            .and_then(|value| normalize_structured_task(&value)),
        summary: extract_structured_value(obj, &["summary", "message", "text"])
            .and_then(|value| json_value_hint(value, 140))
            .map(|value| compact_text_snippet(&value, 140))
            .filter(|value| !value.is_empty()),
    };

    let has_frame = summary.timestamp.is_some() || summary.lane.is_some() || summary.task.is_some();
    let has_payload = summary.summary.is_some() || summary.event_kind.is_some();
    if has_frame && has_payload {
        Some(summary)
    } else {
        None
    }
}

fn extract_structured_text(
    map: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Option<String> {
    extract_structured_value(map, keys).and_then(|value| json_value_hint(value, 80))
}

fn extract_structured_value<'a>(
    map: &'a serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Option<&'a serde_json::Value> {
    for key in keys {
        if let Some(value) = map.get(*key) {
            return Some(value);
        }
    }
    None
}

fn normalize_structured_timestamp(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if is_hms_timestamp(trimmed) {
        return Some(trimmed.to_string());
    }
    if let Ok(parsed) = DateTime::parse_from_rfc3339(trimmed) {
        return Some(parsed.with_timezone(&Utc).format("%H:%M:%S").to_string());
    }
    None
}

fn is_hms_timestamp(value: &str) -> bool {
    value.len() == 8
        && value.chars().enumerate().all(|(idx, ch)| {
            if idx == 2 || idx == 5 {
                ch == ':'
            } else {
                ch.is_ascii_digit()
            }
        })
}

fn normalize_structured_lane(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.eq_ignore_ascii_case("main") {
        return Some("main".to_string());
    }
    let lower = trimmed.to_ascii_lowercase();
    if let Some(num) = lower
        .strip_prefix('l')
        .and_then(|value| value.parse::<usize>().ok())
    {
        return Some(format!("L{num}"));
    }
    if let Some(num) = lower
        .strip_prefix("lane ")
        .and_then(|value| value.parse::<usize>().ok())
    {
        return Some(format!("L{num}"));
    }
    if let Ok(num) = lower.parse::<usize>() {
        return Some(format!("L{num}"));
    }
    None
}

fn normalize_structured_event_kind(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalized = trimmed.to_ascii_lowercase().replace(['_', ' '], "-");
    Some(compact_text_snippet(&normalized, 12))
}

fn normalize_structured_task(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.eq_ignore_ascii_case("main") {
        return Some("main".to_string());
    }
    Some(compact_task_id(trimmed))
}

fn is_low_signal_tool_name(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "read" | "view" | "open" | "list_dir" | "glob" | "file_search" | "search" | "grep" | "ls"
    )
}

fn is_low_signal_activity_summary(summary: &str) -> bool {
    let lower = summary.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return true;
    }
    if lower == "thinking" || lower == "start" || lower == "completed" {
        return true;
    }
    if lower.contains("chunk id:")
        || lower.contains("wall time:")
        || lower.contains("process exited with code")
    {
        return true;
    }
    false
}

fn task_event_activity_priority(event: &Event, summary: &str) -> u8 {
    match &event.event_type {
        EventType::FileEdit { .. }
        | EventType::FileCreate { .. }
        | EventType::FileDelete { .. } => 4,
        EventType::ShellCommand { .. } => 4,
        EventType::ToolCall { name } => {
            if is_low_signal_tool_name(name) {
                2
            } else {
                4
            }
        }
        EventType::ToolResult { is_error, .. } => {
            if *is_error {
                4
            } else if is_low_signal_activity_summary(summary) {
                0
            } else {
                2
            }
        }
        EventType::TaskEnd { .. } => {
            if is_low_signal_activity_summary(summary) {
                1
            } else {
                3
            }
        }
        EventType::FileRead { .. }
        | EventType::CodeSearch { .. }
        | EventType::FileSearch { .. }
        | EventType::WebSearch { .. }
        | EventType::WebFetch { .. } => 2,
        EventType::Custom { .. } => {
            if is_low_signal_activity_summary(summary) {
                1
            } else {
                3
            }
        }
        EventType::AgentMessage => {
            if is_low_signal_activity_summary(summary) {
                0
            } else {
                1
            }
        }
        EventType::TaskStart { .. } | EventType::Thinking => 0,
        _ => 1,
    }
}

fn task_bucket_action_hints(bucket: &TaskChronicleBucket<'_>, limit: usize) -> Vec<String> {
    if limit == 0 {
        return Vec::new();
    }

    let mut hints = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for event in bucket.events.iter().rev() {
        let (kind, _) = event_type_display(&event.event_type, true);
        let mut summary = task_board_event_summary(event, 110);
        summary = strip_kind_prefix(&summary, kind);
        if summary.is_empty() {
            continue;
        }
        if task_event_activity_priority(event, &summary) < 3 {
            continue;
        }
        let summary = if kind.eq_ignore_ascii_case("shell") {
            compact_shell_command(&summary, 80)
        } else {
            compact_text_snippet(&summary, 80)
        };
        let action = compact_text_snippet(&format!("{kind} {summary}"), 110);
        if action.is_empty() {
            continue;
        }
        let key = normalize_activity_key(&action);
        if !seen.insert(key) {
            continue;
        }
        hints.push(action);
        if hints.len() >= limit {
            break;
        }
    }

    hints.reverse();
    hints
}

fn turn_live_activity_rows(turn: &crate::app::Turn<'_>, limit: usize) -> Vec<String> {
    if limit == 0 {
        return Vec::new();
    }

    let mut rows = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for event in turn.agent_events.iter().rev() {
        let (kind, _) = event_type_display(&event.event_type, true);
        let mut summary = task_board_event_summary(event, 120);
        summary = strip_kind_prefix(&summary, kind);
        if summary.is_empty()
            && !matches!(
                event.event_type,
                EventType::TaskStart { .. } | EventType::TaskEnd { .. }
            )
        {
            continue;
        }
        let priority = task_event_activity_priority(event, &summary);
        if priority < 2 {
            continue;
        }

        let task_prefix = event
            .task_id
            .as_deref()
            .map(str::trim)
            .filter(|task_id| !task_id.is_empty())
            .map(|task_id| format!("[task {}] ", compact_task_id(task_id)))
            .unwrap_or_default();
        let body = if summary.is_empty() {
            kind.to_string()
        } else {
            format!("{kind} {summary}")
        };
        let row = compact_text_snippet(
            &format!(
                "{} {}{}",
                event.timestamp.format("%H:%M:%S"),
                task_prefix,
                body
            ),
            180,
        );
        if row.is_empty() {
            continue;
        }
        let row_key = normalize_activity_key(&row);
        if !seen.insert(row_key) {
            continue;
        }
        rows.push(row);
        if rows.len() >= limit {
            break;
        }
    }

    rows.reverse();
    rows
}

fn task_bucket_activity_lines(bucket: &TaskChronicleBucket<'_>, limit: usize) -> Vec<String> {
    if limit == 0 {
        return Vec::new();
    }

    let mut strong_lines = Vec::new();
    let mut weak_lines = Vec::new();
    let mut fallback_line: Option<String> = None;
    let mut seen: HashSet<String> = HashSet::new();
    let output_key = bucket
        .last_output
        .as_deref()
        .map(normalize_activity_key)
        .unwrap_or_default();

    for event in bucket.events.iter().rev() {
        let (kind, _) = event_type_display(&event.event_type, true);
        let mut summary = task_board_event_summary(event, 120);
        summary = strip_kind_prefix(&summary, kind);
        if summary.is_empty()
            && !matches!(
                event.event_type,
                EventType::TaskStart { .. } | EventType::TaskEnd { .. }
            )
        {
            continue;
        }
        let priority = task_event_activity_priority(event, &summary);
        let summary_key = normalize_activity_key(&summary);
        if !output_key.is_empty()
            && summary_key == output_key
            && matches!(
                event.event_type,
                EventType::AgentMessage
                    | EventType::TaskEnd { .. }
                    | EventType::ToolResult { .. }
                    | EventType::Custom { .. }
            )
        {
            continue;
        }
        let row = if summary.is_empty() {
            format!("{} {:>8}", event.timestamp.format("%H:%M:%S"), kind)
        } else {
            format!(
                "{} {:>8} {}",
                event.timestamp.format("%H:%M:%S"),
                kind,
                summary
            )
        };
        let row_key = normalize_activity_key(&row);
        if !seen.insert(row_key) {
            continue;
        }
        match priority {
            3..=u8::MAX => strong_lines.push(row),
            1..=2 => weak_lines.push(row),
            _ => {
                if fallback_line.is_none() {
                    fallback_line = Some(row);
                }
            }
        }
    }

    let mut lines = Vec::new();
    for row in strong_lines {
        lines.push(row);
        if lines.len() >= limit {
            break;
        }
    }
    if lines.len() < limit {
        for row in weak_lines {
            lines.push(row);
            if lines.len() >= limit {
                break;
            }
        }
    }
    if lines.is_empty() {
        if let Some(row) = fallback_line {
            lines.push(row);
        }
    }

    lines.reverse();
    lines
}

fn is_synthetic_task_end_event(event: &Event) -> bool {
    let EventType::TaskEnd {
        summary: Some(summary),
    } = &event.event_type
    else {
        return false;
    };
    let lower = summary.to_ascii_lowercase();
    lower.contains("synthetic end") || lower.contains("missing task_complete")
}

fn task_bucket_is_synthetic_end_stub(bucket: &TaskChronicleBucket<'_>) -> bool {
    if bucket.task_key == "main" || bucket.events.len() != 1 {
        return false;
    }
    bucket
        .events
        .first()
        .is_some_and(|event| is_synthetic_task_end_event(event))
}

fn format_time_ago(ts: DateTime<Utc>) -> String {
    let delta = (Utc::now() - ts).num_seconds().max(0);
    if delta < 60 {
        format!("{delta}s ago")
    } else if delta < 3600 {
        format!("{}m ago", delta / 60)
    } else if delta < 86_400 {
        format!("{}h ago", delta / 3600)
    } else {
        format!("{}d ago", delta / 86_400)
    }
}

fn render_turn_pending_row(
    app: &App,
    turn: &crate::app::Turn<'_>,
    focused: bool,
    content_width: u16,
) -> Vec<Line<'static>> {
    let border_style = if focused {
        Style::new().fg(Theme::ACCENT_YELLOW)
    } else {
        Style::new().fg(Theme::GUTTER)
    };
    let pending = if app.should_skip_realtime_for_selected() {
        "Waiting while live updates are active"
    } else {
        "Task board pending"
    };
    let mut lines = Vec::new();
    lines.push(Line::from(vec![Span::styled(
        " Task Status",
        Style::new().fg(Theme::TEXT_SECONDARY).bold(),
    )]));
    lines.push(Line::from(vec![Span::styled("  ┌", border_style)]));
    lines.extend(wrap_text_lines(
        "  │ ",
        pending,
        border_style,
        Style::new().fg(Theme::TEXT_MUTED),
        content_width,
    ));
    lines.extend(wrap_text_lines(
        "  │ ",
        &format!("{} agent events captured", turn.agent_events.len()),
        border_style,
        Style::new().fg(Theme::TEXT_MUTED),
        content_width,
    ));
    lines.push(Line::from(vec![Span::styled("  └", border_style)]));
    lines
}

fn render_turn_raw_thread(
    _app: &App,
    turn: &crate::app::Turn<'_>,
    has_summary: bool,
    raw_override: bool,
    focused: bool,
    content_width: u16,
) -> Vec<Line<'static>> {
    let title_style = if focused {
        Style::new().fg(Theme::ACCENT_ORANGE).bold()
    } else {
        Style::new().fg(Theme::TEXT_SECONDARY).bold()
    };
    let border_style = if focused {
        Style::new().fg(Theme::ACCENT_ORANGE)
    } else {
        Style::new().fg(Theme::GUTTER)
    };

    let mut lines = Vec::new();
    lines.push(Line::from(vec![Span::styled(
        " Agent Thread (raw override)",
        title_style,
    )]));
    if has_summary && raw_override {
        lines.push(Line::from(vec![Span::styled(
            " [Enter/a: return to summary cards]",
            Style::new().fg(Theme::TEXT_MUTED),
        )]));
    }
    lines.push(Line::from(vec![Span::styled("  ┌", border_style)]));
    if turn.agent_events.is_empty() {
        lines.extend(wrap_text_lines(
            "  │ ",
            "(no agent events)",
            border_style,
            Style::new().fg(Theme::TEXT_MUTED),
            content_width,
        ));
    } else {
        let event_limit = if focused { 14 } else { 6 };
        for event in turn.agent_events.iter().take(event_limit) {
            let (kind, kind_color) = event_type_display(&event.event_type, true);
            lines.extend(wrap_text_lines(
                "  │ ",
                &format!("{kind:>8}"),
                border_style,
                Style::new().fg(kind_color).bold(),
                content_width,
            ));

            let event_summary_line = event_summary(&event.event_type, &event.content.blocks);
            lines.extend(wrap_text_lines(
                "  │ ",
                &truncate(&event_summary_line, 320),
                border_style,
                Style::new().fg(Theme::TEXT_PRIMARY),
                content_width,
            ));

            for block in &event.content.blocks {
                if let ContentBlock::Text { text } = block {
                    for line in text.lines().take(2) {
                        lines.extend(wrap_text_lines(
                            "  │   ",
                            &truncate(line.trim(), 220),
                            border_style,
                            Style::new().fg(Theme::TEXT_SECONDARY),
                            content_width,
                        ));
                    }
                    break;
                }
            }
        }
        if turn.agent_events.len() > event_limit {
            lines.extend(wrap_text_lines(
                "  │ ",
                &format!(
                    "… {} more agent events",
                    turn.agent_events.len() - event_limit
                ),
                border_style,
                Style::new().fg(Theme::TEXT_MUTED),
                content_width,
            ));
        }
    }
    lines.push(Line::from(vec![Span::styled("  └", border_style)]));
    lines
}

fn render_turn_summary_cards(
    payload: &TimelineSummaryPayload,
    focused: bool,
    content_width: u16,
    active_filters: &HashSet<EventFilter>,
) -> Vec<Line<'static>> {
    let scope = payload.scope.trim().to_ascii_lowercase();
    let accent = if scope == "window" {
        Theme::ACCENT_TEAL
    } else {
        Theme::ACCENT_BLUE
    };
    let title_style = if focused {
        Style::new().fg(accent).bold()
    } else {
        Style::new().fg(Theme::TEXT_SECONDARY).bold()
    };
    let title = if scope == "window" {
        "Chronicle Summary"
    } else {
        "Turn Summary"
    };
    let mut lines = Vec::new();
    lines.push(Line::from(vec![Span::styled(
        format!(" {title} ({})", payload.version),
        title_style,
    )]));
    lines.push(Line::from(vec![Span::styled(
        " [Enter/a: show raw behavior]",
        Style::new().fg(Theme::TEXT_MUTED),
    )]));

    let visible_cards: Vec<&crate::timeline_summary::BehaviorCard> = payload
        .cards
        .iter()
        .filter(|card| summary_card_matches_filters(card, active_filters))
        .collect();
    if visible_cards.is_empty() {
        let label = if active_filters.contains(&EventFilter::All) {
            " (no summary cards)"
        } else {
            " (no summary cards for current filter)"
        };
        lines.push(Line::from(vec![Span::styled(
            label,
            Style::new().fg(Theme::TEXT_MUTED),
        )]));
        return lines;
    }

    for (idx, card) in visible_cards.iter().enumerate() {
        if idx > 0 {
            lines.push(Line::raw(""));
        }
        lines.extend(render_behavior_card(card, content_width));
    }
    lines
}

fn summary_card_matches_filters(
    card: &crate::timeline_summary::BehaviorCard,
    active_filters: &HashSet<EventFilter>,
) -> bool {
    if active_filters.contains(&EventFilter::All) || active_filters.is_empty() {
        return true;
    }

    let card_type = card.card_type.to_ascii_lowercase();
    let title = card.title.to_ascii_lowercase();
    let severity = card.severity.to_ascii_lowercase();
    let lines_joined = card
        .lines
        .iter()
        .map(|line| line.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ");

    for filter in active_filters {
        let matches = match filter {
            EventFilter::All => true,
            EventFilter::Messages => {
                card_type == "overview"
                    || card_type == "plan"
                    || title.contains("overview")
                    || title.contains("message")
                    || title.contains("prompt")
            }
            EventFilter::ToolCalls => {
                card_type == "implementation"
                    || card_type == "errors"
                    || title.contains("tool")
                    || lines_joined.contains("tool_")
                    || lines_joined.contains("tool ")
                    || lines_joined.contains("exec_command")
                    || (severity == "error" && !card_type.eq("files"))
            }
            EventFilter::Thinking => {
                card_type == "plan" || title.contains("plan") || title.contains("reason")
            }
            EventFilter::FileOps => {
                card_type == "files"
                    || title.contains("file")
                    || lines_joined.contains(" path:")
                    || lines_joined.contains(".rs")
                    || lines_joined.contains(".ts")
                    || lines_joined.contains(".js")
                    || lines_joined.contains(".md")
            }
            EventFilter::Shell => {
                title.contains("shell")
                    || lines_joined.contains("shell:")
                    || lines_joined.contains("cargo ")
                    || lines_joined.contains("npm ")
                    || lines_joined.contains("pnpm ")
                    || lines_joined.contains("bash ")
            }
        };
        if matches {
            return true;
        }
    }

    false
}

fn render_behavior_card(
    card: &crate::timeline_summary::BehaviorCard,
    content_width: u16,
) -> Vec<Line<'static>> {
    let border_color = summary_card_border_color(card);
    let border_style = Style::new().fg(border_color);
    let header_style = Style::new().fg(border_color).bold();
    let body_style = Style::new().fg(summary_card_body_color(card));
    let mut lines = Vec::new();
    lines.push(Line::from(vec![Span::styled("  ┌", border_style)]));
    let header_text = format!("[{}] {} ({})", card.card_type, card.title, card.severity);
    lines.extend(wrap_text_lines(
        "  │ ",
        &header_text,
        border_style,
        header_style,
        content_width,
    ));
    let entries: Vec<String> = card
        .lines
        .iter()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .collect();
    for entry in entries {
        lines.extend(wrap_text_lines(
            "  │ ",
            &format!("- {}", truncate(&entry, 220)),
            border_style,
            body_style,
            content_width,
        ));
    }
    lines.push(Line::from(vec![Span::styled("  └", border_style)]));
    lines
}

fn summary_card_border_color(card: &crate::timeline_summary::BehaviorCard) -> Color {
    if card.severity.eq_ignore_ascii_case("error") {
        return Theme::ACCENT_RED;
    }
    if card.severity.eq_ignore_ascii_case("warn") {
        return Theme::ACCENT_YELLOW;
    }
    match card.card_type.as_str() {
        "overview" => Theme::ACCENT_BLUE,
        "files" => Theme::ACCENT_CYAN,
        "implementation" => Theme::ACCENT_GREEN,
        "plan" => Theme::ACCENT_YELLOW,
        "errors" => Theme::ACCENT_RED,
        "more" => Theme::TEXT_SECONDARY,
        _ => Theme::TEXT_PRIMARY,
    }
}

fn summary_card_body_color(card: &crate::timeline_summary::BehaviorCard) -> Color {
    if card.severity.eq_ignore_ascii_case("error") {
        return Theme::ACCENT_RED;
    }
    if card.severity.eq_ignore_ascii_case("warn") {
        return Theme::ACCENT_YELLOW;
    }
    match card.card_type.as_str() {
        "files" => Theme::ACCENT_CYAN,
        "implementation" => Theme::ACCENT_GREEN,
        "plan" => Theme::ACCENT_YELLOW,
        "errors" => Theme::ACCENT_RED,
        "more" => Theme::TEXT_SECONDARY,
        _ => Theme::TEXT_PRIMARY,
    }
}

fn wrap_text_lines(
    prefix: &str,
    text: &str,
    prefix_style: Style,
    text_style: Style,
    content_width: u16,
) -> Vec<Line<'static>> {
    let prefix_width = UnicodeWidthStr::width(prefix);
    let available = content_width.saturating_sub(prefix_width as u16).max(1) as usize;
    let mut lines = Vec::new();
    for text_line in text.split('\n') {
        let chunks = split_by_width(text_line, available);
        if chunks.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(prefix.to_string(), prefix_style),
                Span::styled(String::new(), text_style),
            ]));
            continue;
        }
        for chunk in chunks {
            let chunk = truncate_to_width(chunk, available);
            lines.push(Line::from(vec![
                Span::styled(prefix.to_string(), prefix_style),
                Span::styled(chunk, text_style),
            ]));
        }
    }
    lines
}

fn truncate_to_width(text: String, max_chars: usize) -> String {
    if text.is_empty() || max_chars == 0 {
        return String::new();
    }

    if UnicodeWidthStr::width(text.as_str()) <= max_chars {
        return text;
    }

    let mut output = String::new();
    let mut width = 0usize;
    for ch in text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(1);
        if output.is_empty() && ch_width > max_chars {
            output.push(ch);
            break;
        }
        if width + ch_width > max_chars {
            break;
        }
        output.push(ch);
        width += ch_width;
    }
    output
}

fn split_by_width(text: &str, max_chars: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    if max_chars == 0 {
        return vec![text.to_string()];
    }

    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;

    for ch in text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(1);

        if current.is_empty() && ch_width > max_chars {
            lines.push(ch.to_string());
            continue;
        }

        if current_width + ch_width > max_chars && !current.is_empty() {
            lines.push(std::mem::take(&mut current));
            current_width = 0;
        }

        current.push(ch);
        current_width += ch_width;
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}
