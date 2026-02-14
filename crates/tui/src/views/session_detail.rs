use crate::app::{extract_visible_turns, App, DetailViewMode, DisplayEvent};
use crate::session_timeline::LaneMarker;
use crate::theme::{self, Theme};
use crate::timeline_summary::TimelineSummaryPayload;
use opensession_core::trace::{ContentBlock, Event, EventType};
use ratatui::prelude::*;
use ratatui::widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap};

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

    if app.focus_detail_view && app.detail_view_mode == DetailViewMode::Turn {
        let mut base_events = app.get_base_visible_events(&session);
        if base_events.is_empty() {
            base_events = app.get_visible_events(&session);
        }
        render_turn_view(frame, app, &session.session_id, &base_events, area);
        return;
    }

    let [header_area, bar_area, timeline_area] = Layout::vertical([
        Constraint::Length(7),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(area);

    render_session_header(frame, app, &session, header_area);
    app.detail_viewport_height = timeline_area.height.saturating_sub(2);

    let mut visible_events = app.get_visible_events(&session);
    let mut base_events = app.get_base_visible_events(&session);
    if visible_events.is_empty() {
        let p = Paragraph::new("No events match the current filter.")
            .block(Theme::block_dim().title(" Timeline "))
            .style(Style::new().fg(Color::DarkGray));
        frame.render_widget(p, timeline_area);
        return;
    }

    if app.detail_event_index >= visible_events.len() {
        app.detail_event_index = visible_events.len() - 1;
    }

    render_timeline_bar(frame, bar_area, &visible_events, app.detail_event_index);

    if app.detail_view_mode == DetailViewMode::Turn {
        if base_events.is_empty() {
            base_events = visible_events.clone();
        }
        render_turn_view(frame, app, &session.session_id, &base_events, timeline_area);
        return;
    }

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
    if let Some(nick) = app.selected_session_nickname() {
        line1.push(Span::styled("  @", Style::new().fg(Theme::TEXT_MUTED)));
        line1.push(Span::styled(
            nick,
            Style::new().fg(theme::user_color(nick)).bold(),
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
                spans.push(Span::styled(
                    format!("[llm #{window_id}] "),
                    Style::new().fg(Theme::ACCENT_BLUE).bold(),
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
                let (kind, kind_color) = event_type_display(&event.event_type);
                spans.push(Span::styled(
                    format!("{kind:>10} "),
                    Style::new().fg(kind_color).bold(),
                ));
                spans.push(Span::styled(
                    event_summary(&event.event_type, &event.content.blocks),
                    Style::new().fg(Theme::TEXT_PRIMARY),
                ));
                if let Some(badge) = lane_assignment_badge(event, *lane, *marker) {
                    spans.push(Span::styled(
                        format!(" {badge}"),
                        Style::new().fg(Theme::ACCENT_CYAN),
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

        let expanded = app.expanded_events.contains(&i) || selected;
        if expanded {
            if let DisplayEvent::Single { event, .. } = display_event {
                append_content_preview(&mut lines, event, 3);
            }
        }
    }

    let visible_height = area.height.saturating_sub(2) as usize;
    let target_line = event_line_positions.get(current_idx).copied().unwrap_or(0);
    let scroll = if target_line >= visible_height {
        target_line.saturating_sub(visible_height / 3)
    } else {
        0
    };
    app.detail_scroll = scroll as u16;

    let timeline = Paragraph::new(lines.clone())
        .block(Theme::block().title(format!(
            " Timeline ({}/{}) ",
            current_idx + 1,
            total_visible
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

fn event_type_display(event_type: &EventType) -> (&'static str, Color) {
    match event_type {
        EventType::UserMessage => ("user", Theme::ROLE_USER),
        EventType::AgentMessage => ("agent", Theme::ROLE_AGENT_BRIGHT),
        EventType::SystemMessage => ("system", Theme::ROLE_SYSTEM),
        EventType::Thinking => ("think", Theme::ACCENT_PURPLE),
        EventType::ToolCall { .. } => ("tool", Theme::ACCENT_YELLOW),
        EventType::ToolResult { is_error: true, .. } => ("error", Theme::ACCENT_RED),
        EventType::ToolResult { .. } => ("result", Theme::TEXT_MUTED),
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
        EventType::Custom { .. } => ("custom", Theme::TEXT_MUTED),
        _ => ("other", Theme::TEXT_MUTED),
    }
}

fn event_summary(event_type: &EventType, blocks: &[ContentBlock]) -> String {
    match event_type {
        EventType::UserMessage | EventType::AgentMessage => first_text_line(blocks, 80),
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
            Some(code) => format!("{} => {}", truncate(command, 70), code),
            None => truncate(command, 70),
        },
        EventType::WebSearch { query } => truncate(query, 70),
        EventType::WebFetch { url } => truncate(url, 70),
        EventType::ImageGenerate { prompt }
        | EventType::VideoGenerate { prompt }
        | EventType::AudioGenerate { prompt } => truncate(prompt, 70),
        EventType::TaskStart { title } => format!("start {}", title.clone().unwrap_or_default()),
        EventType::TaskEnd { summary } => format!("end {}", summary.clone().unwrap_or_default()),
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
    format!("{head}…{tail}")
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
    use chrono::Utc;
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

        let lines = render_turn_response_panel(&app, &turn, Some(&payload), false, true);
        let rendered = lines
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("Turn Summary"));
        assert!(!rendered.contains("Agent Thread"));
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
    if turns.is_empty() {
        let p = Paragraph::new("No turns found.")
            .block(Theme::block_dim().title(" Split View "))
            .style(Style::new().fg(Color::DarkGray));
        frame.render_widget(p, area);
        return;
    }

    app.turn_index = app.turn_index.min(turns.len().saturating_sub(1));

    let [left_area, right_area] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(area);

    let mut left_lines: Vec<Line> = Vec::new();
    let mut right_lines: Vec<Line> = Vec::new();
    let mut line_offsets: Vec<u16> = Vec::new();

    for (turn_idx, turn) in turns.iter().enumerate() {
        line_offsets.push(left_lines.len() as u16);

        let focused = turn_idx == app.turn_index;
        let raw_override = app.turn_raw_overrides.contains(&turn_idx);
        let summary_payload =
            app.turn_summary_payload(session_id, turn.turn_index, turn.anchor_source_index);

        let prompt_rows = render_turn_prompt_card(turn_idx, turn, focused);
        for line in prompt_rows {
            left_lines.push(line);
            right_lines.push(Line::raw(""));
        }

        let right_rows =
            render_turn_response_panel(app, turn, summary_payload, raw_override, focused);
        for line in right_rows {
            left_lines.push(Line::raw(""));
            right_lines.push(line);
        }

        left_lines.push(Line::raw(""));
        right_lines.push(Line::raw(""));
    }

    app.turn_line_offsets = line_offsets;
    let visible_h = left_area.height.saturating_sub(2);
    let total = left_lines.len() as u16;
    let max_scroll = total.saturating_sub(visible_h);
    app.turn_agent_scroll = app.turn_agent_scroll.min(max_scroll);
    let scroll = (app.turn_agent_scroll, app.turn_h_scroll);

    let left_para = Paragraph::new(left_lines.clone())
        .block(Theme::block().title(" User Prompts "))
        .scroll(scroll);
    let right_para = Paragraph::new(right_lines.clone())
        .block(Theme::block().title(" Turn Summaries "))
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

fn render_turn_prompt_card(
    turn_idx: usize,
    turn: &crate::app::Turn<'_>,
    focused: bool,
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
    for text in prompt_lines {
        lines.push(Line::from(vec![
            Span::styled("  │ ", border_style),
            Span::styled(text, Style::new().fg(Theme::TEXT_PRIMARY)),
        ]));
    }

    lines.push(Line::from(vec![Span::styled("  └", border_style)]));
    lines
}

fn collect_turn_user_lines(turn: &crate::app::Turn<'_>) -> Vec<String> {
    let mut lines = Vec::new();
    for event in &turn.user_events {
        let mut pushed_any = false;
        for block in &event.content.blocks {
            if let ContentBlock::Text { text } = block {
                for line in text.lines() {
                    lines.push(line.to_string());
                    pushed_any = true;
                }
            }
        }
        if !pushed_any {
            lines.push(event_summary(&event.event_type, &event.content.blocks));
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
) -> Vec<Line<'static>> {
    if let Some(payload) = summary_payload {
        if !raw_override {
            return render_turn_summary_cards(payload, focused);
        }
    }

    if raw_override {
        return render_turn_raw_thread(app, turn, summary_payload.is_some(), raw_override, focused);
    }

    render_turn_pending_row(app, turn, focused)
}

fn render_turn_pending_row(
    app: &App,
    turn: &crate::app::Turn<'_>,
    focused: bool,
) -> Vec<Line<'static>> {
    let border_style = if focused {
        Style::new().fg(Theme::ACCENT_YELLOW)
    } else {
        Style::new().fg(Theme::GUTTER)
    };
    let pending = if !app.daemon_config.daemon.summary_enabled {
        "LLM summary is off"
    } else if app.should_skip_realtime_for_selected() {
        "LLM summary skipped by Neglect Live Session rule"
    } else {
        "LLM summary pending"
    };
    vec![
        Line::from(vec![Span::styled(
            " Summary Status",
            Style::new().fg(Theme::TEXT_SECONDARY).bold(),
        )]),
        Line::from(vec![Span::styled("  ┌", border_style)]),
        Line::from(vec![
            Span::styled("  │ ", border_style),
            Span::styled(pending, Style::new().fg(Theme::TEXT_MUTED)),
        ]),
        Line::from(vec![
            Span::styled("  │ ", border_style),
            Span::styled(
                format!("{} agent events captured", turn.agent_events.len()),
                Style::new().fg(Theme::TEXT_MUTED),
            ),
        ]),
        Line::from(vec![Span::styled("  └", border_style)]),
    ]
}

fn render_turn_raw_thread(
    _app: &App,
    turn: &crate::app::Turn<'_>,
    has_summary: bool,
    raw_override: bool,
    focused: bool,
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
            " [Enter: return to summary cards]",
            Style::new().fg(Theme::TEXT_MUTED),
        )]));
    }
    lines.push(Line::from(vec![Span::styled("  ┌", border_style)]));
    if turn.agent_events.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("  │ ", border_style),
            Span::styled("(no agent events)", Style::new().fg(Theme::TEXT_MUTED)),
        ]));
    } else {
        for event in &turn.agent_events {
            let (kind, kind_color) = event_type_display(&event.event_type);
            lines.push(Line::from(vec![
                Span::styled("  │ ", border_style),
                Span::styled(format!("{kind:>8} "), Style::new().fg(kind_color).bold()),
                Span::styled(
                    event_summary(&event.event_type, &event.content.blocks),
                    Style::new().fg(Theme::TEXT_PRIMARY),
                ),
            ]));

            for block in &event.content.blocks {
                if let ContentBlock::Text { text } = block {
                    for line in text.lines().take(3) {
                        lines.push(Line::from(vec![
                            Span::styled("  │   ", border_style),
                            Span::styled(line.to_string(), Style::new().fg(Theme::TEXT_SECONDARY)),
                        ]));
                    }
                    break;
                }
            }
        }
    }
    lines.push(Line::from(vec![Span::styled("  └", border_style)]));
    lines
}

fn render_turn_summary_cards(
    payload: &TimelineSummaryPayload,
    focused: bool,
) -> Vec<Line<'static>> {
    let title_style = if focused {
        Style::new().fg(Theme::ACCENT_BLUE).bold()
    } else {
        Style::new().fg(Theme::TEXT_SECONDARY).bold()
    };
    let mut lines = Vec::new();
    lines.push(Line::from(vec![Span::styled(
        format!(" Turn Summary ({})", payload.version),
        title_style,
    )]));
    for (idx, card) in payload.cards.iter().enumerate() {
        if idx > 0 {
            lines.push(Line::raw(""));
        }
        lines.extend(render_behavior_card(card, focused));
    }
    lines
}

fn render_behavior_card(
    card: &crate::timeline_summary::BehaviorCard,
    focused: bool,
) -> Vec<Line<'static>> {
    let border_style = if focused {
        Style::new().fg(Theme::ACCENT_BLUE)
    } else {
        Style::new().fg(Theme::GUTTER)
    };
    let kind_style = match card.card_type.as_str() {
        "errors" => Style::new().fg(Theme::ACCENT_RED).bold(),
        "plan" => Style::new().fg(Theme::ACCENT_CYAN).bold(),
        "files" => Style::new().fg(Theme::ACCENT_GREEN).bold(),
        "implementation" => Style::new().fg(Theme::ACCENT_ORANGE).bold(),
        _ => Style::new().fg(Theme::ACCENT_BLUE).bold(),
    };
    let severity_style = match card.severity.as_str() {
        "error" => Style::new().fg(Theme::ACCENT_RED),
        "warn" => Style::new().fg(Theme::ACCENT_YELLOW),
        _ => Style::new().fg(Theme::TEXT_MUTED),
    };

    let mut lines = Vec::new();
    lines.push(Line::from(vec![Span::styled("  ┌", border_style)]));
    lines.push(Line::from(vec![
        Span::styled("  │ ", border_style),
        Span::styled(format!("[{}] ", card.card_type), kind_style),
        Span::styled(
            card.title.clone(),
            Style::new().fg(Theme::TEXT_PRIMARY).bold(),
        ),
        Span::styled(format!(" ({})", card.severity), severity_style),
    ]));
    for entry in card.lines.iter().filter_map(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }) {
        lines.push(Line::from(vec![
            Span::styled("  │ ", border_style),
            Span::styled(format!("- {entry}"), Style::new().fg(Theme::TEXT_PRIMARY)),
        ]));
    }
    lines.push(Line::from(vec![Span::styled("  └", border_style)]));
    lines
}
