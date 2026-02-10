use crate::app::App;
use opensession_core::trace::{ContentBlock, EventType};
use ratatui::prelude::*;
use ratatui::widgets::{
    Block, Padding, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap,
};

pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let session = match app.selected_session() {
        Some(s) => s.clone(),
        None => {
            let p = Paragraph::new("No session selected")
                .block(Block::bordered())
                .style(Style::new().fg(Color::DarkGray));
            frame.render_widget(p, area);
            return;
        }
    };

    let [header_area, timeline_area] =
        Layout::vertical([Constraint::Length(6), Constraint::Fill(1)]).areas(area);

    // ── Session header ──────────────────────────────────────────────────
    let title = session
        .context
        .title
        .as_deref()
        .unwrap_or(&session.session_id);

    let tags_str = if session.context.tags.is_empty() {
        String::new()
    } else {
        format!(
            "  {}",
            session
                .context
                .tags
                .iter()
                .map(|t| format!("#{}", t))
                .collect::<Vec<_>>()
                .join(" ")
        )
    };

    let header_text = vec![
        Line::from(vec![Span::styled(
            title,
            Style::new().fg(Color::White).bold(),
        )]),
        Line::from(vec![
            Span::styled(
                &session.agent.tool,
                Style::new().fg(Color::Rgb(217, 119, 80)),
            ),
            Span::styled(" · ", Style::new().fg(Color::DarkGray)),
            Span::styled(
                &session.agent.model,
                Style::new().fg(Color::Rgb(100, 140, 220)),
            ),
            Span::styled(" · ", Style::new().fg(Color::DarkGray)),
            Span::styled(
                format!("{} msgs", session.stats.message_count),
                Style::new().fg(Color::Green),
            ),
            Span::styled(" · ", Style::new().fg(Color::DarkGray)),
            Span::styled(
                format!("{} events", session.stats.event_count),
                Style::new().fg(Color::Yellow),
            ),
            Span::styled(" · ", Style::new().fg(Color::DarkGray)),
            Span::styled(
                format!("{} tools", session.stats.tool_call_count),
                Style::new().fg(Color::Magenta),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                format!("{}", session.context.created_at.format("%Y-%m-%d %H:%M:%S")),
                Style::new().fg(Color::DarkGray),
            ),
            Span::styled(tags_str, Style::new().fg(Color::Rgb(100, 120, 160))),
        ]),
    ];

    let header = Paragraph::new(header_text).block(
        Block::bordered()
            .border_style(Style::new().fg(Color::Rgb(60, 65, 80)))
            .padding(Padding::new(1, 1, 0, 0)),
    );
    frame.render_widget(header, header_area);

    // ── Timeline ────────────────────────────────────────────────────────
    let visible_events = app.get_visible_events(&session);
    let total_visible = visible_events.len();

    if total_visible == 0 {
        let p = Paragraph::new("No events match the current filter.")
            .block(
                Block::bordered()
                    .title(" Timeline ")
                    .border_style(Style::new().fg(Color::DarkGray)),
            )
            .style(Style::new().fg(Color::DarkGray));
        frame.render_widget(p, timeline_area);
        return;
    }

    let inner_width = timeline_area.width.saturating_sub(2) as usize;
    let current_idx = app.detail_event_index;

    // Build lines with clear turn boundaries, tracking event->line mapping
    let mut lines: Vec<Line> = Vec::new();
    let mut event_line_positions: Vec<usize> = Vec::new(); // line index for each event
    let mut prev_role: Option<&str> = None;

    for (i, event) in visible_events.iter().enumerate() {
        let is_selected = i == current_idx;
        let role = event_role(&event.event_type);

        // Insert turn separator when role changes (User <-> Agent boundary)
        let role_changed = prev_role != Some(role);
        if role_changed && i > 0 {
            let sep = "─".repeat(inner_width.saturating_sub(2).min(120));
            lines.push(Line::from(Span::styled(
                sep,
                Style::new().fg(Color::Rgb(45, 50, 65)),
            )));
            lines.push(Line::raw(""));
        }

        // Turn role header (only on role change)
        if role_changed {
            let (role_label, role_color) = role_display(role);
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {} ", role_label),
                    Style::new().fg(Color::Black).bg(role_color).bold(),
                ),
                Span::raw("  "),
                Span::styled(
                    event.timestamp.format("%H:%M:%S").to_string(),
                    Style::new().fg(Color::DarkGray),
                ),
            ]));
            lines.push(Line::raw(""));
        }

        prev_role = Some(role);

        let (type_label, type_color) = event_type_display(&event.event_type);
        let summary = event_summary(&event.event_type);

        let highlight_bg = if is_selected {
            Style::new().bg(Color::Rgb(30, 35, 50))
        } else {
            Style::new()
        };

        let pointer = if is_selected { "▸" } else { " " };
        let pointer_color = if is_selected {
            Color::Cyan
        } else {
            Color::DarkGray
        };

        // Record line position for this event
        event_line_positions.push(lines.len());

        let event_line = Line::from(vec![
            Span::styled(format!(" {} ", pointer), Style::new().fg(pointer_color)),
            Span::styled("│ ", Style::new().fg(Color::Rgb(55, 60, 75))),
            Span::styled(
                format!("{:<6}", type_label),
                Style::new().fg(type_color).bold(),
            ),
            Span::raw(" "),
            Span::styled(summary, Style::new().fg(Color::White)),
        ])
        .style(highlight_bg);

        lines.push(event_line);

        // Show content preview for selected event
        if is_selected {
            render_content_preview(&event.content.blocks, &mut lines, inner_width);
        }
    }

    // Calculate scroll using tracked positions
    let visible_height = timeline_area.height.saturating_sub(2) as usize;
    let target_line = event_line_positions.get(current_idx).copied().unwrap_or(0);
    let scroll = if target_line >= visible_height {
        target_line.saturating_sub(visible_height / 3)
    } else {
        0
    };

    let timeline = Paragraph::new(lines.clone())
        .block(
            Block::bordered()
                .title(format!(
                    " Timeline ({}/{}) ",
                    current_idx + 1,
                    total_visible
                ))
                .border_style(Style::new().fg(Color::Rgb(60, 65, 80))),
        )
        .wrap(Wrap { trim: false })
        .scroll((scroll as u16, 0));

    frame.render_widget(timeline, timeline_area);

    // Scrollbar
    let total_lines = lines.len();
    if total_lines > visible_height {
        let mut scrollbar_state = ScrollbarState::new(total_lines).position(scroll);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .thumb_style(Style::new().fg(Color::Rgb(80, 85, 100)));
        frame.render_stateful_widget(
            scrollbar,
            timeline_area.inner(Margin {
                vertical: 1,
                horizontal: 0,
            }),
            &mut scrollbar_state,
        );
    }
}

fn render_content_preview(blocks: &[ContentBlock], lines: &mut Vec<Line>, _width: usize) {
    let gutter = "   │   ";
    let bg = Style::new().bg(Color::Rgb(30, 35, 50));

    for block in blocks {
        match block {
            ContentBlock::Text { text } => {
                for (li, line) in text.lines().take(10).enumerate() {
                    let truncated = if line.chars().count() > 100 {
                        let t: String = line.chars().take(97).collect();
                        format!("{}…", t)
                    } else {
                        line.to_string()
                    };
                    lines.push(
                        Line::from(vec![
                            Span::styled(gutter, Style::new().fg(Color::Rgb(55, 60, 75))),
                            Span::styled(truncated, Style::new().fg(Color::Rgb(170, 175, 190))),
                        ])
                        .style(bg),
                    );
                    if li == 9 {
                        lines.push(
                            Line::from(vec![
                                Span::styled(gutter, Style::new().fg(Color::Rgb(55, 60, 75))),
                                Span::styled("…more", Style::new().fg(Color::DarkGray)),
                            ])
                            .style(bg),
                        );
                    }
                }
            }
            ContentBlock::Code { code, language, .. } => {
                let lang = language.as_deref().unwrap_or("");
                lines.push(
                    Line::from(vec![
                        Span::styled(gutter, Style::new().fg(Color::Rgb(55, 60, 75))),
                        Span::styled(format!("```{}", lang), Style::new().fg(Color::DarkGray)),
                    ])
                    .style(bg),
                );
                for line in code.lines().take(8) {
                    let truncated = if line.chars().count() > 100 {
                        let t: String = line.chars().take(97).collect();
                        format!("{}…", t)
                    } else {
                        line.to_string()
                    };
                    lines.push(
                        Line::from(vec![
                            Span::styled(gutter, Style::new().fg(Color::Rgb(55, 60, 75))),
                            Span::styled(truncated, Style::new().fg(Color::Rgb(130, 200, 130))),
                        ])
                        .style(bg),
                    );
                }
                lines.push(
                    Line::from(vec![
                        Span::styled(gutter, Style::new().fg(Color::Rgb(55, 60, 75))),
                        Span::styled("```", Style::new().fg(Color::DarkGray)),
                    ])
                    .style(bg),
                );
            }
            ContentBlock::Json { data } => {
                let formatted =
                    serde_json::to_string_pretty(data).unwrap_or_else(|_| format!("{:?}", data));
                for line in formatted.lines().take(5) {
                    lines.push(
                        Line::from(vec![
                            Span::styled(gutter, Style::new().fg(Color::Rgb(55, 60, 75))),
                            Span::styled(
                                line.to_string(),
                                Style::new().fg(Color::Rgb(220, 200, 120)),
                            ),
                        ])
                        .style(bg),
                    );
                }
            }
            _ => {}
        }
    }
}

/// Map event type to a high-level "role" for turn boundary detection.
fn event_role(event_type: &EventType) -> &'static str {
    match event_type {
        EventType::UserMessage => "user",
        EventType::AgentMessage | EventType::Thinking => "agent",
        EventType::ToolCall { .. }
        | EventType::ToolResult { .. }
        | EventType::FileRead { .. }
        | EventType::CodeSearch { .. }
        | EventType::FileSearch { .. }
        | EventType::FileEdit { .. }
        | EventType::FileCreate { .. }
        | EventType::FileDelete { .. }
        | EventType::ShellCommand { .. }
        | EventType::WebSearch { .. }
        | EventType::WebFetch { .. }
        | EventType::ImageGenerate { .. }
        | EventType::VideoGenerate { .. }
        | EventType::AudioGenerate { .. } => "agent",
        EventType::SystemMessage => "system",
        EventType::TaskStart { .. } | EventType::TaskEnd { .. } => "task",
        EventType::Custom { .. } => "other",
    }
}

fn role_display(role: &str) -> (&str, Color) {
    match role {
        "user" => ("USER", Color::Rgb(80, 180, 100)),
        "agent" => ("AGENT", Color::Rgb(100, 140, 220)),
        "system" => ("SYSTEM", Color::Rgb(140, 140, 140)),
        "task" => ("TASK", Color::Rgb(180, 140, 80)),
        _ => ("OTHER", Color::Gray),
    }
}

fn event_type_display(event_type: &EventType) -> (&'static str, Color) {
    match event_type {
        EventType::UserMessage => ("User", Color::Rgb(80, 180, 100)),
        EventType::AgentMessage => ("Agent", Color::Rgb(100, 160, 240)),
        EventType::SystemMessage => ("Sys", Color::Gray),
        EventType::Thinking => ("Think", Color::Rgb(180, 120, 220)),
        EventType::ToolCall { .. } => ("Tool", Color::Rgb(220, 180, 60)),
        EventType::ToolResult { is_error: true, .. } => ("Error", Color::Rgb(220, 80, 80)),
        EventType::ToolResult { .. } => ("Result", Color::DarkGray),
        EventType::FileRead { .. } => ("Read", Color::Rgb(100, 160, 240)),
        EventType::CodeSearch { .. } => ("Search", Color::Rgb(80, 200, 200)),
        EventType::FileSearch { .. } => ("Find", Color::Rgb(80, 180, 160)),
        EventType::FileEdit { .. } => ("Edit", Color::Rgb(80, 200, 200)),
        EventType::FileCreate { .. } => ("Create", Color::Rgb(80, 200, 120)),
        EventType::FileDelete { .. } => ("Delete", Color::Rgb(220, 100, 100)),
        EventType::ShellCommand { .. } => ("Shell", Color::Rgb(220, 200, 80)),
        EventType::WebSearch { .. } => ("Search", Color::Rgb(180, 140, 220)),
        EventType::WebFetch { .. } => ("Fetch", Color::Rgb(180, 140, 220)),
        EventType::ImageGenerate { .. } => ("Image", Color::Cyan),
        EventType::VideoGenerate { .. } => ("Video", Color::Cyan),
        EventType::AudioGenerate { .. } => ("Audio", Color::Cyan),
        EventType::TaskStart { .. } => ("Start", Color::Rgb(120, 180, 80)),
        EventType::TaskEnd { .. } => ("End", Color::Rgb(120, 180, 80)),
        EventType::Custom { .. } => ("Custom", Color::Gray),
    }
}

fn event_summary(event_type: &EventType) -> String {
    match event_type {
        EventType::UserMessage => String::new(),
        EventType::AgentMessage => String::new(),
        EventType::SystemMessage => String::new(),
        EventType::Thinking => "…".to_string(),
        EventType::ToolCall { name } => format!("{}()", name),
        EventType::ToolResult { name, is_error, .. } => {
            if *is_error {
                format!("{} failed", name)
            } else {
                format!("{} ok", name)
            }
        }
        EventType::FileRead { path } => short_path(path).to_string(),
        EventType::CodeSearch { query } => truncate(query, 40),
        EventType::FileSearch { pattern } => pattern.clone(),
        EventType::FileEdit { path, .. } => short_path(path).to_string(),
        EventType::FileCreate { path } => short_path(path).to_string(),
        EventType::FileDelete { path } => short_path(path).to_string(),
        EventType::ShellCommand { command, exit_code } => {
            let cmd = if command.chars().count() > 50 {
                let t: String = command.chars().take(47).collect();
                format!("{}…", t)
            } else {
                command.clone()
            };
            match exit_code {
                Some(code) => format!("$ {} → {}", cmd, code),
                None => format!("$ {}", cmd),
            }
        }
        EventType::WebSearch { query } => query.clone(),
        EventType::WebFetch { url } => url.clone(),
        EventType::ImageGenerate { prompt } => truncate(prompt, 40),
        EventType::VideoGenerate { prompt } => truncate(prompt, 40),
        EventType::AudioGenerate { prompt } => truncate(prompt, 40),
        EventType::TaskStart { title } => title.as_deref().unwrap_or("unnamed").to_string(),
        EventType::TaskEnd { summary } => summary.as_deref().unwrap_or("").to_string(),
        EventType::Custom { kind } => kind.clone(),
    }
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
        let t: String = s.chars().take(max_len.saturating_sub(1)).collect();
        format!("{}…", t)
    }
}
