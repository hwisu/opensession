use crate::app::{extract_turns, App, DetailViewMode, DisplayEvent, TaskViewMode};
use crate::theme::{self, Theme};
use opensession_core::trace::{ContentBlock, EventType};
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

    let [header_area, bar_area, timeline_area] = Layout::vertical([
        Constraint::Length(7),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(area);

    // ── Session header (ampcode style) ────────────────────────────────
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

    // Line 1: title + optional user badge
    let mut line1 = vec![Span::styled(
        title,
        Style::new().fg(Theme::TEXT_PRIMARY).bold(),
    )];
    if let Some(nick) = app.selected_session_nickname() {
        let color = theme::user_color(nick);
        line1.push(Span::styled(
            format!("  @{nick}"),
            Style::new().fg(color).bold(),
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

    // Line 2: tool · model · duration
    let duration = format_duration(session.stats.duration_seconds);
    let line2 = vec![
        Span::styled(&session.agent.tool, Style::new().fg(Theme::ACCENT_ORANGE)),
        Span::styled(" · ", Style::new().fg(Theme::GUTTER)),
        Span::styled(&session.agent.model, Style::new().fg(Theme::ROLE_AGENT)),
        Span::styled(" · ", Style::new().fg(Theme::GUTTER)),
        Span::styled(duration, Style::new().fg(Theme::TEXT_SECONDARY)),
    ];

    // Line 3: prompts · files · +N −N lines
    let prompts = session.stats.user_message_count;
    let files = session.stats.files_changed;
    let added = session.stats.lines_added;
    let removed = session.stats.lines_removed;
    let mut line3 = vec![
        Span::styled(
            format!("{} prompts", prompts),
            Style::new().fg(Theme::TEXT_SECONDARY),
        ),
        Span::styled(" · ", Style::new().fg(Theme::GUTTER)),
        Span::styled(
            format!("{} files", files),
            Style::new().fg(Theme::ACCENT_PURPLE),
        ),
    ];
    if added > 0 || removed > 0 {
        line3.push(Span::styled(" · ", Style::new().fg(Theme::GUTTER)));
        line3.push(Span::styled(
            format!("+{}", added),
            Style::new().fg(Theme::ACCENT_GREEN),
        ));
        line3.push(Span::styled(" ", Style::new()));
        line3.push(Span::styled(
            format!("−{}", removed),
            Style::new().fg(Theme::ACCENT_RED),
        ));
    }

    // Token usage
    let input_tokens = session.stats.total_input_tokens;
    let output_tokens = session.stats.total_output_tokens;
    if input_tokens > 0 || output_tokens > 0 {
        line3.push(Span::styled(" · ", Style::new().fg(Theme::GUTTER)));
        line3.push(Span::styled(
            format!("{}in", format_token_count(input_tokens)),
            Style::new().fg(Theme::TOKEN_IN),
        ));
        line3.push(Span::styled(" ", Style::new()));
        line3.push(Span::styled(
            format!("{}out", format_token_count(output_tokens)),
            Style::new().fg(Theme::TOKEN_OUT),
        ));
    }

    // Line 4: timestamp · tags
    let mut line4 = vec![Span::styled(
        format!("{}", session.context.created_at.format("%Y-%m-%d %H:%M")),
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

    let header_text = vec![
        Line::from(line1),
        Line::from(line2),
        Line::from(line3),
        Line::from(line4),
    ];

    let header = Paragraph::new(header_text)
        .block(Theme::block().padding(ratatui::widgets::Padding::new(1, 1, 0, 0)));
    frame.render_widget(header, header_area);

    // ── Timeline ────────────────────────────────────────────────────────
    let visible_events = app.get_visible_events(&session);
    let total_visible = visible_events.len();

    // Timeline density bar
    render_timeline_bar(frame, bar_area, &visible_events, app.detail_event_index);

    // Turn view mode
    if app.detail_view_mode == DetailViewMode::Turn {
        render_turn_view(frame, app, &visible_events, timeline_area);
        return;
    }

    if total_visible == 0 {
        let p = Paragraph::new("No events match the current filter.")
            .block(Theme::block_dim().title(" Timeline "))
            .style(Style::new().fg(Color::DarkGray));
        frame.render_widget(p, timeline_area);
        return;
    }

    let inner_width = timeline_area.width.saturating_sub(2) as usize;
    let current_idx = app.detail_event_index;

    // Build lines with clear turn boundaries, tracking event->line mapping
    let mut lines: Vec<Line> = Vec::new();
    let mut event_line_positions: Vec<usize> = Vec::new();
    let mut prev_role: Option<&str> = None;
    let mut task_stack: Vec<String> = Vec::new();
    let is_chrono = app.task_view_mode == TaskViewMode::Detail;
    let mut prev_category: Option<&str> = None;

    for (i, display_event) in visible_events.iter().enumerate() {
        let event = display_event.event();
        let is_selected = i == current_idx;
        let role = event_role(&event.event_type);

        // Track task tree depth (only in chronological mode)
        // Calculate depth before push/pop so TaskEnd shows at correct nesting level
        let depth = if is_chrono { task_stack.len() } else { 0 };
        if is_chrono {
            match &event.event_type {
                EventType::TaskStart { .. } => {
                    task_stack.push(event.task_id.clone().unwrap_or_else(|| format!("t{}", i)));
                }
                EventType::TaskEnd { .. } => {
                    task_stack.pop();
                }
                _ => {}
            }
        }

        // Insert turn separator when role changes
        let role_changed = prev_role != Some(role);
        if role_changed && i > 0 {
            let sep = "─".repeat(inner_width.saturating_sub(2).min(120));
            lines.push(Line::from(Span::styled(
                sep,
                Style::new().fg(Theme::SEPARATOR),
            )));
            lines.push(Line::raw(""));
        }

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

        // Sub-divider for category change within agent turn (Think→Agent→Tool)
        let category = event_type_category(&event.event_type);
        if !role_changed && prev_role == Some("agent") {
            if let Some(pc) = prev_category {
                if pc != category {
                    let sub_sep = "· ".repeat(inner_width.saturating_sub(6).min(40) / 2);
                    lines.push(Line::from(Span::styled(
                        format!("    {}", sub_sep.trim_end()),
                        Style::new().fg(Theme::SEPARATOR),
                    )));
                }
            }
        }
        prev_category = Some(category);
        prev_role = Some(role);

        // User message gets subtle green background
        let base_bg = if matches!(event.event_type, EventType::UserMessage) {
            Style::new().bg(Theme::BG_USER_MSG)
        } else {
            Style::new()
        };
        let highlight_bg = if is_selected {
            Style::new().bg(Theme::BG_SURFACE)
        } else {
            base_bg
        };
        let pointer = if is_selected { "▸" } else { " " };
        let pointer_color = if is_selected {
            Color::Cyan
        } else {
            Color::DarkGray
        };

        // Build tree prefix for nested tasks
        let tree_color = Style::new().fg(Theme::TREE);
        let tree_prefix = if depth == 0 {
            String::new()
        } else {
            let mut prefix = String::new();
            for _ in 0..depth.saturating_sub(1) {
                prefix.push_str("│ ");
            }
            match &event.event_type {
                EventType::TaskStart { .. } => prefix.push_str("┬─"),
                EventType::TaskEnd { .. } => prefix.push_str("└─"),
                _ => prefix.push_str("│ "),
            }
            prefix
        };

        event_line_positions.push(lines.len());

        // Render based on DisplayEvent variant
        match display_event {
            DisplayEvent::Collapsed { count, kind, .. } => {
                let (_, collapsed_type_color) = event_type_display(&event.event_type);
                let mut spans = vec![
                    Span::styled(format!(" {} ", pointer), Style::new().fg(pointer_color)),
                    Span::styled("│ ", Style::new().fg(collapsed_type_color)),
                    Span::styled(
                        format!("{:<6}", kind),
                        Style::new().fg(Theme::ROLE_AGENT).bold(),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        format!("×{} collapsed", count),
                        Style::new().fg(Theme::TEXT_SECONDARY),
                    ),
                ];
                // Show first item path as hint
                if let EventType::FileRead { path } = &event.event_type {
                    spans.push(Span::styled(
                        format!("  {}, …", short_path(path)),
                        Style::new().fg(Color::DarkGray),
                    ));
                }
                lines.push(Line::from(spans).style(highlight_bg));
            }
            DisplayEvent::TaskSummary {
                summary,
                inner_count,
                ..
            } => {
                let mut spans = vec![
                    Span::styled(format!(" {} ", pointer), Style::new().fg(pointer_color)),
                    Span::styled("│ ", Style::new().fg(Theme::ROLE_TASK)),
                    Span::styled("Task  ", Style::new().fg(Theme::ROLE_TASK).bold()),
                    Span::styled(summary.clone(), Style::new().fg(Theme::TEXT_PRIMARY)),
                ];
                if *inner_count > 0 {
                    spans.push(Span::styled(
                        format!("  [{} events]", inner_count),
                        Style::new().fg(Color::DarkGray),
                    ));
                }
                lines.push(Line::from(spans).style(highlight_bg));
            }
            DisplayEvent::Single(_) => {
                let (type_label, type_color) = event_type_display(&event.event_type);
                let summary = event_summary(&event.event_type, &event.content.blocks);
                let summary_style = if summary.contains("interrupted") {
                    Style::new().fg(Theme::ACCENT_YELLOW)
                } else if matches!(&event.event_type, EventType::ToolResult { is_error, .. } if !is_error)
                    && event.content.blocks.is_empty()
                {
                    Style::new().fg(Theme::TEXT_MUTED)
                } else {
                    Style::new().fg(Theme::TEXT_PRIMARY)
                };
                let mut spans = vec![Span::styled(
                    format!(" {} ", pointer),
                    Style::new().fg(pointer_color),
                )];
                if !tree_prefix.is_empty() {
                    spans.push(Span::styled(tree_prefix, tree_color));
                } else {
                    spans.push(Span::styled("│ ", Style::new().fg(type_color)));
                }
                // Skip type label for UserMessage/AgentMessage (role separator already shows it)
                if matches!(
                    event.event_type,
                    EventType::UserMessage | EventType::AgentMessage
                ) {
                    spans.push(Span::raw("       "));
                } else {
                    spans.push(Span::styled(
                        format!("{:<6}", type_label),
                        Style::new().fg(type_color).bold(),
                    ));
                    spans.push(Span::raw(" "));
                }
                spans.push(Span::styled(summary, summary_style));
                lines.push(Line::from(spans).style(highlight_bg));

                // Content preview for expanded events
                let explicitly_expanded = app.expanded_events.contains(&i);
                let should_expand = is_selected
                    || explicitly_expanded
                    || matches!(
                        event.event_type,
                        EventType::AgentMessage | EventType::Thinking
                    );
                if should_expand {
                    let max_lines = if explicitly_expanded {
                        100
                    } else if is_selected {
                        10
                    } else {
                        5
                    };
                    let max_diff = if explicitly_expanded { 100 } else { 20 };
                    if let EventType::FileEdit {
                        diff: Some(ref d), ..
                    } = &event.event_type
                    {
                        render_diff_preview(d, &mut lines, max_diff);
                    } else if matches!(&event.event_type, EventType::FileEdit { diff: None, .. }) {
                        lines.push(Line::from(vec![
                            Span::styled("   │ ", Style::new().fg(Theme::GUTTER)),
                            Span::styled(
                                "(no diff available)",
                                Style::new().fg(Color::DarkGray).italic(),
                            ),
                        ]));
                        render_content_preview(
                            &event.content.blocks,
                            &mut lines,
                            inner_width,
                            max_lines,
                        );
                    } else {
                        render_content_preview(
                            &event.content.blocks,
                            &mut lines,
                            inner_width,
                            max_lines,
                        );
                    }
                    // Breathing room after expanded content
                    lines.push(Line::raw(""));
                }
            }
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
        .block(Theme::block().title(format!(
            " Timeline ({}/{}) ",
            current_idx + 1,
            total_visible
        )))
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
            .thumb_style(Style::new().fg(Theme::TEXT_MUTED));
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

fn render_diff_preview(diff: &str, lines: &mut Vec<Line>, max_diff_lines: usize) {
    let gutter = "   │ ";
    let bg = Style::new().bg(Theme::BG_SURFACE);

    for line in diff.lines().take(max_diff_lines) {
        let style = if line.starts_with("+++") || line.starts_with("---") {
            Style::new().fg(Theme::DIFF_HEADER).bold()
        } else if line.starts_with('+') {
            Style::new().fg(Theme::ACCENT_GREEN).bg(Theme::BG_DIFF_ADD)
        } else if line.starts_with('-') {
            Style::new().fg(Theme::ACCENT_RED).bg(Theme::BG_DIFF_DEL)
        } else if line.starts_with("@@") {
            Style::new().fg(Theme::DIFF_HUNK)
        } else {
            Style::new().fg(Theme::TEXT_SECONDARY)
        };
        let truncated = if line.chars().count() > 100 {
            let t: String = line.chars().take(97).collect();
            format!("{t}…")
        } else {
            line.to_string()
        };
        lines.push(
            Line::from(vec![
                Span::styled(gutter, Style::new().fg(Theme::GUTTER)),
                Span::styled(truncated, style),
            ])
            .style(bg),
        );
    }
    let total = diff.lines().count();
    if total > max_diff_lines {
        lines.push(
            Line::from(vec![
                Span::styled(gutter, Style::new().fg(Theme::GUTTER)),
                Span::styled(
                    format!("…{} more lines", total - max_diff_lines),
                    Style::new().fg(Color::DarkGray),
                ),
            ])
            .style(bg),
        );
    }
}

fn render_content_preview(
    blocks: &[ContentBlock],
    lines: &mut Vec<Line<'static>>,
    _width: usize,
    max_lines: usize,
) {
    let gutter = "   │   ";
    let bg = Style::new().bg(Theme::BG_SURFACE);

    for block in blocks {
        match block {
            ContentBlock::Text { text } => {
                let limit = max_lines;
                let total_lines = text.lines().count();
                for (li, line) in text.lines().take(limit).enumerate() {
                    let truncated = if line.chars().count() > 100 {
                        let t: String = line.chars().take(97).collect();
                        format!("{}…", t)
                    } else {
                        line.to_string()
                    };
                    let mut spans = vec![Span::styled(gutter, Style::new().fg(Theme::GUTTER))];
                    spans.extend(parse_text_line_to_spans(&truncated));
                    lines.push(Line::from(spans).style(bg));
                    if li == limit - 1 && total_lines > limit {
                        lines.push(
                            Line::from(vec![
                                Span::styled(gutter, Style::new().fg(Theme::GUTTER)),
                                Span::styled("…more", Style::new().fg(Color::DarkGray)),
                            ])
                            .style(bg),
                        );
                    }
                }
            }
            ContentBlock::Code { code, language, .. } => {
                let lang = language.as_deref().unwrap_or("");
                let code_limit = max_lines.saturating_sub(2);
                lines.push(
                    Line::from(vec![
                        Span::styled(gutter, Style::new().fg(Theme::GUTTER)),
                        Span::styled(format!("```{}", lang), Style::new().fg(Color::DarkGray)),
                    ])
                    .style(bg),
                );
                for line in code.lines().take(code_limit) {
                    let truncated = if line.chars().count() > 100 {
                        let t: String = line.chars().take(97).collect();
                        format!("{}…", t)
                    } else {
                        line.to_string()
                    };
                    lines.push(
                        Line::from(vec![
                            Span::styled(gutter, Style::new().fg(Theme::GUTTER)),
                            Span::styled(truncated, Style::new().fg(Theme::CODE_TEXT)),
                        ])
                        .style(bg),
                    );
                }
                lines.push(
                    Line::from(vec![
                        Span::styled(gutter, Style::new().fg(Theme::GUTTER)),
                        Span::styled("```", Style::new().fg(Color::DarkGray)),
                    ])
                    .style(bg),
                );
            }
            ContentBlock::Json { data } => {
                render_json_kv(data, lines, gutter, max_lines);
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
        _ => "other",
    }
}

fn role_display(role: &str) -> (&str, Color) {
    match role {
        "user" => ("USER", Theme::ROLE_USER),
        "agent" => ("AGENT", Theme::ROLE_AGENT),
        "system" => ("SYSTEM", Theme::ROLE_SYSTEM),
        "task" => ("TASK", Theme::ROLE_TASK),
        _ => ("OTHER", Color::Gray),
    }
}

fn event_type_display(event_type: &EventType) -> (&'static str, Color) {
    match event_type {
        EventType::UserMessage => ("User", Theme::ROLE_USER),
        EventType::AgentMessage => ("Agent", Theme::ROLE_AGENT_BRIGHT),
        EventType::SystemMessage => ("Sys", Theme::ROLE_SYSTEM),
        EventType::Thinking => ("Think", Color::Rgb(180, 120, 220)),
        EventType::ToolCall { .. } => ("Tool", Theme::ACCENT_YELLOW),
        EventType::ToolResult { is_error: true, .. } => ("Error", Theme::ACCENT_RED),
        EventType::ToolResult { .. } => ("Result", Color::DarkGray),
        EventType::FileRead { .. } => ("Read", Theme::ROLE_AGENT_BRIGHT),
        EventType::CodeSearch { .. } => ("Search", Theme::ACCENT_CYAN),
        EventType::FileSearch { .. } => ("Find", Theme::ACCENT_TEAL),
        EventType::FileEdit { .. } => ("Edit", Theme::ACCENT_CYAN),
        EventType::FileCreate { .. } => ("Create", Theme::ACCENT_GREEN),
        EventType::FileDelete { .. } => ("Delete", Color::Rgb(220, 100, 100)),
        EventType::ShellCommand { .. } => ("Shell", Color::Rgb(220, 200, 80)),
        EventType::WebSearch { .. } => ("Search", Theme::ACCENT_PURPLE),
        EventType::WebFetch { .. } => ("Fetch", Theme::ACCENT_PURPLE),
        EventType::ImageGenerate { .. } => ("Image", Color::Cyan),
        EventType::VideoGenerate { .. } => ("Video", Color::Cyan),
        EventType::AudioGenerate { .. } => ("Audio", Color::Cyan),
        EventType::TaskStart { .. } => ("Start", Color::Rgb(120, 180, 80)),
        EventType::TaskEnd { .. } => ("End", Color::Rgb(120, 180, 80)),
        EventType::Custom { .. } => ("Custom", Color::Gray),
        _ => ("?", Color::Gray),
    }
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

fn event_type_category(et: &EventType) -> &'static str {
    match et {
        EventType::Thinking => "thinking",
        EventType::AgentMessage => "message",
        EventType::UserMessage | EventType::SystemMessage => "message",
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
        | EventType::AudioGenerate { .. } => "tool",
        EventType::TaskStart { .. } | EventType::TaskEnd { .. } => "task",
        EventType::Custom { .. } => "other",
        _ => "other",
    }
}

fn event_summary(event_type: &EventType, blocks: &[ContentBlock]) -> String {
    match event_type {
        EventType::UserMessage => first_text_line(blocks, 60),
        EventType::AgentMessage => first_text_line(blocks, 60),
        EventType::SystemMessage => String::new(),
        EventType::Thinking => "…".to_string(),
        EventType::ToolCall { name } => format!("{}()", name),
        EventType::ToolResult { name, is_error, .. } => {
            if *is_error {
                let err_preview = first_text_line(blocks, 40);
                if err_preview.is_empty() {
                    format!("{} failed", name)
                } else {
                    format!("{} failed: {}", name, err_preview)
                }
            } else {
                format!("{} ok", name)
            }
        }
        EventType::FileRead { path } => short_path(path).to_string(),
        EventType::CodeSearch { query } => truncate(query, 40),
        EventType::FileSearch { pattern } => pattern.clone(),
        EventType::FileEdit { path, diff } => {
            let path_str = short_path(path).to_string();
            if let Some(d) = diff {
                let (a, r) = count_diff_lines(d);
                format!("{} +{} −{}", path_str, a, r)
            } else {
                path_str
            }
        }
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
        EventType::TaskStart { title } => {
            format!("▶ {}", title.as_deref().unwrap_or("unnamed"))
        }
        EventType::TaskEnd { summary } => {
            format!("■ {}", summary.as_deref().unwrap_or(""))
        }
        EventType::Custom { kind } => kind.clone(),
        _ => String::new(),
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

fn format_token_count(n: u64) -> String {
    if n == 0 {
        "0".to_string()
    } else if n < 1_000 {
        format!("{}", n)
    } else if n < 1_000_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    }
}

fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

// ── Timeline density bar ──────────────────────────────────────────────

fn render_timeline_bar(frame: &mut Frame, area: Rect, events: &[DisplayEvent], current_idx: usize) {
    if events.is_empty() || area.width < 10 {
        return;
    }

    let counter = format!(" ({}/{}) ", current_idx + 1, events.len());
    let bar_width = (area.width as usize).saturating_sub(counter.len() + 2);
    if bar_width == 0 {
        return;
    }

    // Get time range for density calculation
    let first_ts = events.first().unwrap().event().timestamp;
    let last_ts = events.last().unwrap().event().timestamp;
    let total_secs = (last_ts - first_ts).num_seconds().max(1) as f64;

    // Count events per bucket (by timestamp)
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
    let density_chars = [' ', '░', '▒', '▓', '█'];

    let mut spans = vec![Span::styled(" ", Style::new())];

    for (b, &count) in buckets.iter().enumerate() {
        let level = if count == 0 {
            0
        } else {
            ((count as f64 / max_count as f64) * 4.0).ceil() as usize
        };
        let ch = density_chars[level.min(4)];

        let style = if b == current_bucket_idx {
            Style::new().fg(Color::Black).bg(Theme::ACCENT_BLUE)
        } else {
            Style::new().fg(Theme::BAR_DIM)
        };

        spans.push(Span::styled(ch.to_string(), style));
    }

    spans.push(Span::styled(counter, Style::new().fg(Color::DarkGray)));

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

// ── Turn View ─────────────────────────────────────────────────────────

fn render_turn_view(frame: &mut Frame, app: &mut App, events: &[DisplayEvent], area: Rect) {
    let turns = extract_turns(events);

    if turns.is_empty() {
        let p = Paragraph::new("No turns found.")
            .block(Theme::block_dim().title(" Split View "))
            .style(Style::new().fg(Color::DarkGray));
        frame.render_widget(p, area);
        return;
    }

    app.turn_index = app.turn_index.min(turns.len() - 1);

    // Split area horizontally 50/50
    let [left_area, right_area] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(area);

    let left_width = left_area.width.saturating_sub(2) as usize;
    let right_width = right_area.width.saturating_sub(2) as usize;

    let mut left_lines: Vec<Line> = Vec::new();
    let mut right_lines: Vec<Line> = Vec::new();
    let mut line_offsets: Vec<u16> = Vec::new();

    for (ti, turn) in turns.iter().enumerate() {
        // Record turn start line offset
        line_offsets.push(left_lines.len() as u16);

        // Turn separator
        if ti > 0 {
            let l_sep = "─".repeat(left_width.min(60));
            let r_sep = "─".repeat(right_width.min(60));
            left_lines.push(Line::from(Span::styled(
                l_sep,
                Style::new().fg(Theme::SEPARATOR),
            )));
            right_lines.push(Line::from(Span::styled(
                r_sep,
                Style::new().fg(Theme::SEPARATOR),
            )));
        }

        // Turn header
        let is_focused = ti == app.turn_index;
        let turn_header_style = if is_focused {
            Style::new().fg(Theme::ACCENT_BLUE).bold()
        } else {
            Style::new().fg(Theme::TEXT_SECONDARY)
        };
        let focus_marker = if is_focused { "▸" } else { " " };
        left_lines.push(Line::from(vec![
            Span::styled(focus_marker, turn_header_style),
            Span::styled(format!(" Turn {}", ti + 1), turn_header_style),
        ]));
        right_lines.push(Line::from(vec![
            Span::styled(" ", Style::new()),
            Span::styled(
                format!("{} events", turn.agent_events.len()),
                Style::new().fg(Theme::TEXT_SECONDARY),
            ),
        ]));

        let expanded = app.expanded_turns.contains(&ti);
        let user_lines = render_turn_user_content(turn);
        let agent_lines = render_turn_agent_content(turn, expanded);

        left_lines.extend(user_lines);
        right_lines.extend(agent_lines);

        // Pad shorter side
        while left_lines.len() < right_lines.len() {
            left_lines.push(Line::raw(""));
        }
        while right_lines.len() < left_lines.len() {
            right_lines.push(Line::raw(""));
        }
    }

    app.turn_line_offsets = line_offsets;

    let visible_h = left_area.height.saturating_sub(2);
    let total = left_lines.len() as u16;
    let max_scroll = total.saturating_sub(visible_h);
    app.turn_agent_scroll = app.turn_agent_scroll.min(max_scroll);
    let scroll = (app.turn_agent_scroll, 0);

    let left_block = Theme::block().title(" User ");
    let right_block = Theme::block().title(" Agent ");

    let left_para = Paragraph::new(left_lines.clone())
        .block(left_block)
        .wrap(Wrap { trim: false })
        .scroll(scroll);
    let right_para = Paragraph::new(right_lines.clone())
        .block(right_block)
        .wrap(Wrap { trim: false })
        .scroll(scroll);

    frame.render_widget(left_para, left_area);
    frame.render_widget(right_para, right_area);

    // Scrollbar on right panel
    let total_lines = right_lines.len();
    if total_lines > visible_h as usize {
        let mut scrollbar_state =
            ScrollbarState::new(total_lines).position(app.turn_agent_scroll as usize);
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

fn render_turn_user_content(turn: &crate::app::Turn<'_>) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for event in &turn.user_events {
        for block in &event.content.blocks {
            if let ContentBlock::Text { text } = block {
                for line in text.lines() {
                    lines.push(Line::from(parse_text_line_to_spans(line)));
                }
            }
        }
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no user message)",
            Style::new().fg(Color::DarkGray),
        )));
    }
    lines
}

fn render_turn_agent_content(turn: &crate::app::Turn<'_>, expanded: bool) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let max_content_lines = if expanded { 100 } else { 8 };
    let max_diff_lines = if expanded { 100 } else { 10 };

    for event in &turn.agent_events {
        // Skip trivial success ToolResults (non-error, empty content)
        if matches!(&event.event_type, EventType::ToolResult { is_error, .. } if !is_error)
            && event.content.blocks.is_empty()
        {
            continue;
        }

        let (type_label, type_color) = event_type_display(&event.event_type);
        let summary = event_summary(&event.event_type, &event.content.blocks);

        lines.push(Line::from(vec![
            Span::styled("│ ", Style::new().fg(type_color)),
            Span::styled(
                format!("{:<6}", type_label),
                Style::new().fg(type_color).bold(),
            ),
            Span::raw(" "),
            Span::styled(summary, Style::new().fg(Theme::TEXT_PRIMARY)),
        ]));

        // Content preview — all event types with content blocks
        let should_show_content = matches!(
            event.event_type,
            EventType::AgentMessage
                | EventType::Thinking
                | EventType::ToolResult { .. }
                | EventType::ShellCommand { .. }
        );

        if should_show_content {
            render_split_content_blocks(&event.content.blocks, &mut lines, max_content_lines);
        }

        // Diff preview for file edits
        match &event.event_type {
            EventType::FileEdit {
                diff: Some(ref d), ..
            } => {
                for line in d.lines().take(max_diff_lines) {
                    let style = if line.starts_with("+++") || line.starts_with("---") {
                        Style::new().fg(Theme::DIFF_HEADER).bold()
                    } else if line.starts_with('+') {
                        Style::new().fg(Theme::ACCENT_GREEN).bg(Theme::BG_DIFF_ADD)
                    } else if line.starts_with('-') {
                        Style::new().fg(Theme::ACCENT_RED).bg(Theme::BG_DIFF_DEL)
                    } else if line.starts_with("@@") {
                        Style::new().fg(Theme::DIFF_HUNK)
                    } else {
                        Style::new().fg(Theme::TEXT_SECONDARY)
                    };
                    let truncated = truncate(line, 80);
                    lines.push(Line::from(vec![
                        Span::styled("  │   ", Style::new().fg(Theme::GUTTER)),
                        Span::styled(truncated, style),
                    ]));
                }
                let total = d.lines().count();
                if total > max_diff_lines {
                    lines.push(Line::from(vec![
                        Span::styled("  │   ", Style::new().fg(Theme::GUTTER)),
                        Span::styled(
                            format!("…{} more lines", total - max_diff_lines),
                            Style::new().fg(Color::DarkGray),
                        ),
                    ]));
                }
            }
            EventType::FileEdit { diff: None, .. } => {
                lines.push(Line::from(vec![
                    Span::styled("  │   ", Style::new().fg(Theme::GUTTER)),
                    Span::styled(
                        "(no diff available)",
                        Style::new().fg(Color::DarkGray).italic(),
                    ),
                ]));
                render_split_content_blocks(&event.content.blocks, &mut lines, max_content_lines);
            }
            _ => {}
        }
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no agent events)",
            Style::new().fg(Color::DarkGray),
        )));
    }
    lines
}

/// Render content blocks for the split view with a gutter prefix.
fn render_split_content_blocks(
    blocks: &[ContentBlock],
    lines: &mut Vec<Line<'static>>,
    max_lines: usize,
) {
    let gutter = "  │   ";
    for block in blocks {
        match block {
            ContentBlock::Text { text } => {
                let total = text.lines().count();
                for (li, line) in text.lines().take(max_lines).enumerate() {
                    let truncated = truncate(line.trim(), 80);
                    lines.push(Line::from(vec![
                        Span::styled(gutter, Style::new().fg(Theme::GUTTER)),
                        Span::styled(truncated, Style::new().fg(Theme::TEXT_CONTENT)),
                    ]));
                    if li == max_lines - 1 && total > max_lines {
                        lines.push(Line::from(vec![
                            Span::styled(gutter, Style::new().fg(Theme::GUTTER)),
                            Span::styled(
                                format!("…{} more lines", total - max_lines),
                                Style::new().fg(Color::DarkGray),
                            ),
                        ]));
                    }
                }
            }
            ContentBlock::Code { code, language, .. } => {
                let lang = language.as_deref().unwrap_or("");
                lines.push(Line::from(vec![
                    Span::styled(gutter, Style::new().fg(Theme::GUTTER)),
                    Span::styled(format!("```{}", lang), Style::new().fg(Color::DarkGray)),
                ]));
                let code_limit = max_lines.saturating_sub(2);
                for line in code.lines().take(code_limit) {
                    let truncated = truncate(line, 80);
                    lines.push(Line::from(vec![
                        Span::styled(gutter, Style::new().fg(Theme::GUTTER)),
                        Span::styled(truncated, Style::new().fg(Theme::CODE_TEXT)),
                    ]));
                }
                lines.push(Line::from(vec![
                    Span::styled(gutter, Style::new().fg(Theme::GUTTER)),
                    Span::styled("```", Style::new().fg(Color::DarkGray)),
                ]));
            }
            ContentBlock::Json { data } => {
                render_json_kv(data, lines, gutter, max_lines);
            }
            _ => {}
        }
    }
}

/// Render JSON as key:value table for flat objects, or pretty-print otherwise.
fn render_json_kv(
    data: &serde_json::Value,
    lines: &mut Vec<Line<'static>>,
    gutter: &str,
    max_lines: usize,
) {
    let bg = Style::new().bg(Theme::BG_SURFACE);
    let gutter_s = gutter.to_string();

    if let serde_json::Value::Object(map) = data {
        // Check if flat (all values are scalars)
        let is_flat = map.values().all(|v| {
            matches!(
                v,
                serde_json::Value::Null
                    | serde_json::Value::Bool(_)
                    | serde_json::Value::Number(_)
                    | serde_json::Value::String(_)
            )
        });
        if is_flat && !map.is_empty() {
            let max_key_len = map.keys().map(|k| k.len()).max().unwrap_or(0).min(20);
            for (i, (key, val)) in map.iter().enumerate() {
                if i >= max_lines {
                    lines.push(
                        Line::from(vec![
                            Span::styled(gutter_s.clone(), Style::new().fg(Theme::GUTTER)),
                            Span::styled(
                                format!("…{} more", map.len() - i),
                                Style::new().fg(Color::DarkGray),
                            ),
                        ])
                        .style(bg),
                    );
                    break;
                }
                let val_str = match val {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                let val_truncated = truncate(&val_str, 60);
                lines.push(
                    Line::from(vec![
                        Span::styled(gutter_s.clone(), Style::new().fg(Theme::GUTTER)),
                        Span::styled(
                            format!("{:>width$}", key, width = max_key_len),
                            Style::new().fg(Theme::ACCENT_CYAN),
                        ),
                        Span::styled(": ", Style::new().fg(Theme::TEXT_MUTED)),
                        Span::styled(val_truncated, Style::new().fg(Theme::JSON_TEXT)),
                    ])
                    .style(bg),
                );
            }
            return;
        }
    }

    // Fallback: pretty-print
    let formatted = serde_json::to_string_pretty(data).unwrap_or_else(|_| format!("{:?}", data));
    for (i, line) in formatted.lines().enumerate() {
        if i >= max_lines {
            lines.push(
                Line::from(vec![
                    Span::styled(gutter_s.clone(), Style::new().fg(Theme::GUTTER)),
                    Span::styled("…more", Style::new().fg(Color::DarkGray)),
                ])
                .style(bg),
            );
            break;
        }
        lines.push(
            Line::from(vec![
                Span::styled(gutter_s.clone(), Style::new().fg(Theme::GUTTER)),
                Span::styled(line.to_string(), Style::new().fg(Theme::JSON_TEXT)),
            ])
            .style(bg),
        );
    }
}

/// Parse a text line into styled spans with basic markdown highlighting.
fn parse_text_line_to_spans(line: &str) -> Vec<Span<'static>> {
    let trimmed = line.trim();

    // Heading: # ...
    if let Some(rest) = trimmed.strip_prefix("# ") {
        return vec![Span::styled(
            format!("# {}", rest),
            Style::new().fg(Theme::ACCENT_BLUE).bold(),
        )];
    }
    if let Some(rest) = trimmed.strip_prefix("## ") {
        return vec![Span::styled(
            format!("## {}", rest),
            Style::new().fg(Theme::ACCENT_BLUE).bold(),
        )];
    }
    if let Some(rest) = trimmed.strip_prefix("### ") {
        return vec![Span::styled(
            format!("### {}", rest),
            Style::new().fg(Theme::ACCENT_BLUE).bold(),
        )];
    }

    // Inline parsing for `code` and **bold**
    let mut spans = Vec::new();
    let mut chars = line.char_indices().peekable();
    let mut buf = String::new();
    let owned = line.to_string();

    while let Some(&(pos, ch)) = chars.peek() {
        if ch == '`' {
            // Flush buffer
            if !buf.is_empty() {
                spans.push(Span::styled(
                    std::mem::take(&mut buf),
                    Style::new().fg(Theme::TEXT_PRIMARY),
                ));
            }
            chars.next(); // consume opening `
            let start = pos + 1;
            let mut found_end = false;
            while let Some(&(end_pos, c)) = chars.peek() {
                if c == '`' {
                    let code_str = owned[start..end_pos].to_string();
                    spans.push(Span::styled(code_str, Style::new().fg(Theme::CODE_TEXT)));
                    chars.next(); // consume closing `
                    found_end = true;
                    break;
                }
                chars.next();
            }
            if !found_end {
                buf.push('`');
                buf.push_str(&owned[start..]);
                break;
            }
        } else if ch == '*' {
            // Check for **bold**
            let next_is_star = {
                let mut p = chars.clone();
                p.next();
                p.peek().map(|&(_, c)| c) == Some('*')
            };
            if next_is_star {
                if !buf.is_empty() {
                    spans.push(Span::styled(
                        std::mem::take(&mut buf),
                        Style::new().fg(Theme::TEXT_PRIMARY),
                    ));
                }
                chars.next(); // first *
                chars.next(); // second *
                let bold_start = pos + 2;
                let mut found_end = false;
                while let Some(&(end_pos, c)) = chars.peek() {
                    if c == '*' {
                        let next_star = {
                            let mut p = chars.clone();
                            p.next();
                            p.peek().map(|&(_, c2)| c2) == Some('*')
                        };
                        if next_star {
                            let bold_str = owned[bold_start..end_pos].to_string();
                            spans.push(Span::styled(
                                bold_str,
                                Style::new().fg(Theme::TEXT_PRIMARY).bold(),
                            ));
                            chars.next(); // first *
                            chars.next(); // second *
                            found_end = true;
                            break;
                        }
                    }
                    chars.next();
                }
                if !found_end {
                    buf.push_str("**");
                    buf.push_str(&owned[bold_start..]);
                    break;
                }
            } else {
                buf.push(ch);
                chars.next();
            }
        } else {
            buf.push(ch);
            chars.next();
        }
    }

    if !buf.is_empty() {
        spans.push(Span::styled(buf, Style::new().fg(Theme::TEXT_PRIMARY)));
    }

    if spans.is_empty() {
        spans.push(Span::styled(
            line.to_string(),
            Style::new().fg(Theme::TEXT_PRIMARY),
        ));
    }
    spans
}
