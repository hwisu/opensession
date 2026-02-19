use crate::app::{
    App, ConnectionContext, EventFilter, FlashLevel, ServerStatus, SettingsSection, UploadPhase,
    View, ViewMode,
};
use crate::theme::Theme;
use crate::views::{handoff, help, modal, session_detail, session_list, settings, setup, tab_bar};
use opensession_core::trace::{ContentBlock, EventType, Session};
use ratatui::prelude::*;
use ratatui::widgets::{Clear, Paragraph};

pub fn render(frame: &mut Frame, app: &mut App) {
    // Setup is always full-screen
    if matches!(app.view, View::Setup) {
        setup::render(frame, app, frame.area());
        // Modal overlay
        if let Some(ref m) = app.modal {
            modal::render(frame, m, &app.edit_buffer);
        }
        return;
    }

    // `opensession view` focus mode: hide global tab chrome,
    // but keep compact session summary.
    if app.focus_detail_view {
        let show_footer = should_show_footer(app);
        let (summary_area, body_area, footer_area) = if show_footer {
            let [summary, body, footer] = Layout::vertical([
                Constraint::Length(2),
                Constraint::Fill(1),
                Constraint::Length(1),
            ])
            .areas(frame.area());
            (summary, body, Some(footer))
        } else {
            let [summary, body] =
                Layout::vertical([Constraint::Length(2), Constraint::Fill(1)]).areas(frame.area());
            (summary, body, None)
        };
        render_focus_session_summary(frame, app, summary_area);
        if matches!(app.view, View::Help) {
            help::render(frame, body_area);
        } else if app.selected_session().is_some() {
            app.view = View::SessionDetail;
            session_detail::render(frame, app, body_area);
        } else {
            let waiting = Paragraph::new("Waiting for session data...")
                .block(Theme::block_dim().title(" View "))
                .style(Style::new().fg(Theme::TEXT_MUTED));
            frame.render_widget(waiting, body_area);
        }
        if let Some(footer_area) = footer_area {
            render_footer(frame, app, footer_area);
        }
        if let Some(ref m) = app.modal {
            modal::render(frame, m, &app.edit_buffer);
        }
        return;
    }

    let show_footer = should_show_footer(app);
    let (tab_area, header_area, body_area, footer_area) = if show_footer {
        let [tab, header, body, footer] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .areas(frame.area());
        (tab, header, body, Some(footer))
    } else {
        let [tab, header, body] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Fill(1),
        ])
        .areas(frame.area());
        (tab, header, body, None)
    };

    // Tab bar
    tab_bar::render(
        frame,
        &app.active_tab,
        &app.view,
        tab_area,
        app.is_local_mode(),
    );

    // Header
    render_header(frame, app, header_area);

    // Body
    match app.view {
        View::SessionList => session_list::render(frame, app, body_area),
        View::SessionDetail => session_detail::render(frame, app, body_area),
        View::Settings => settings::render(frame, app, body_area),
        View::Handoff => handoff::render(frame, app, body_area),
        View::Help => {}  // rendered as overlay below
        View::Setup => {} // handled above
    }

    if let Some(footer_area) = footer_area {
        render_footer(frame, app, footer_area);
    }

    // Upload popup overlay
    if app.upload_popup.is_some() {
        render_upload_popup(frame, app);
    }

    if app.repo_picker_open {
        render_repo_picker(frame, app);
    }

    // Help overlay
    if matches!(app.view, View::Help) {
        help::render(frame, frame.area());
    }

    // Modal overlay
    if let Some(ref m) = app.modal {
        modal::render(frame, m, &app.edit_buffer);
    }
}

fn should_show_footer(app: &App) -> bool {
    app.flash_message.is_some() || matches!(app.view, View::Settings | View::Handoff)
}

fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    match app.view {
        View::SessionList => {
            let block = Theme::block();
            let inner = block.inner(area);
            frame.render_widget(block, area);

            // Left side: title + connection badge + view mode + session count
            let count = app.session_count();
            let mode_label = match &app.view_mode {
                ViewMode::Local => "Local".to_string(),
                ViewMode::Repo(r) => format!("Repo: {r}"),
            };

            // Connection context badge
            let (badge_text, badge_fg, badge_bg) = match &app.connection_ctx {
                ConnectionContext::Local => ("LOCAL".to_string(), Color::Black, Theme::BADGE_LOCAL),
                ConnectionContext::Server { .. } => {
                    ("SERVER".to_string(), Color::Black, Theme::BADGE_SERVER)
                }
                ConnectionContext::CloudPersonal => {
                    ("PERSONAL".to_string(), Color::Black, Theme::BADGE_PERSONAL)
                }
            };

            let session_count_span = if app.loading_sessions {
                Span::styled("Loading...", Style::new().fg(Theme::ACCENT_YELLOW).italic())
            } else {
                Span::styled(
                    format!("{} sessions", count),
                    Style::new().fg(Theme::TEXT_SECONDARY),
                )
            };

            let mut left_spans = vec![
                Span::styled(
                    " opensession ",
                    Style::new().fg(Theme::ACCENT_ORANGE).bold(),
                ),
                Span::styled(" ", Style::new()),
                Span::styled(
                    format!(" {} ", badge_text),
                    Style::new().fg(badge_fg).bg(badge_bg).bold(),
                ),
                Span::styled("  ", Style::new()),
                Span::styled(mode_label, Style::new().fg(Theme::ACCENT_BLUE)),
                Span::styled("  ", Style::new()),
                session_count_span,
            ];

            if !app.search_query.is_empty() {
                left_spans.push(Span::styled(
                    format!("  (filtered from {})", app.sessions.len()),
                    Style::new().fg(Color::DarkGray),
                ));
            }

            // Tool filter indicator
            if let Some(tool) = app.active_tool_filter() {
                left_spans.push(Span::styled("  ", Style::new()));
                left_spans.push(Span::styled(
                    format!(" tool:{tool} "),
                    Style::new().fg(Color::Black).bg(Color::Magenta).bold(),
                ));
            }
            if !app.is_default_time_range() {
                left_spans.push(Span::styled("  ", Style::new()));
                left_spans.push(Span::styled(
                    format!(" range:{} ", app.session_time_range_label()),
                    Style::new().fg(Color::Black).bg(Color::Cyan).bold(),
                ));
            }
            // Page indicator
            if app.total_pages() > 1 {
                left_spans.push(Span::styled(
                    format!("  Page {}/{}", app.page + 1, app.total_pages()),
                    Style::new().fg(Theme::TEXT_SECONDARY),
                ));
            }

            // Startup status indicators
            let status = &app.startup_status;
            if status.repos_detected > 0 {
                left_spans.push(Span::styled("  ", Style::new().fg(Color::DarkGray)));
                left_spans.push(Span::styled(
                    format!("{} repos", status.repos_detected),
                    Style::new().fg(Theme::TEXT_MUTED),
                ));
            }

            let left_line = Line::from(left_spans);
            let p = Paragraph::new(left_line).alignment(Alignment::Left);
            frame.render_widget(p, inner);

            // Right side: daemon status + server status
            let mut right_spans = Vec::new();

            // Daemon status
            if let Some(pid) = status.daemon_pid {
                right_spans.push(Span::styled(
                    format!("daemon:{pid} "),
                    Style::new().fg(Theme::ACCENT_GREEN),
                ));
            } else if status.config_exists {
                right_spans.push(Span::styled(
                    "daemon:off ",
                    Style::new().fg(Theme::TEXT_SECONDARY),
                ));
            }

            // Server status
            if let Some(ref info) = app.server_info {
                right_spans.extend(build_server_status_spans(info));
            }

            if !right_spans.is_empty() {
                let right_line = Line::from(right_spans);
                let p_right = Paragraph::new(right_line).alignment(Alignment::Right);
                frame.render_widget(p_right, inner);
            }
        }
        View::SessionDetail => {
            let filters = [
                ("1:All", EventFilter::All),
                ("2:Msgs", EventFilter::Messages),
                ("3:Tools", EventFilter::ToolCalls),
                ("4:Think", EventFilter::Thinking),
                ("5:Files", EventFilter::FileOps),
                ("6:Shell", EventFilter::Shell),
            ];

            let mut spans = vec![Span::styled(" ", Style::new())];
            for (label, filter) in &filters {
                let active = app.event_filters.contains(filter);
                let style = if active {
                    Style::new().fg(Color::Black).bg(Theme::ACCENT_BLUE).bold()
                } else {
                    Style::new().fg(Theme::TEXT_MUTED)
                };
                spans.push(Span::styled(format!(" {} ", label), style));
                spans.push(Span::styled(" ", Style::new()));
            }
            spans.push(Span::styled(
                "   d ",
                Style::new().fg(Theme::TEXT_KEY).bold(),
            ));
            spans.push(Span::styled("diff", Style::new().fg(Theme::TEXT_KEY_DESC)));
            spans.push(Span::styled(
                "  always expanded",
                Style::new().fg(Theme::TEXT_MUTED),
            ));

            let line = Line::from(spans);
            let p = Paragraph::new(line).block(Theme::block());
            frame.render_widget(p, area);
        }
        View::Settings => {
            let block = Theme::block();
            let inner = block.inner(area);
            frame.render_widget(block, area);

            // Section tabs
            let mut spans = vec![Span::styled(
                " Settings  ",
                Style::new().fg(Theme::TEXT_PRIMARY).bold(),
            )];
            for section in SettingsSection::ORDER {
                let is_active = section == app.settings_section;
                let style = if is_active {
                    Style::new().fg(Color::Black).bg(Theme::ACCENT_BLUE).bold()
                } else {
                    Style::new().fg(Theme::TAB_INACTIVE)
                };
                spans.push(Span::styled(format!(" {} ", section.label()), style));
                spans.push(Span::styled(" ", Style::new()));
            }
            let dirty_mark = if app.config_dirty { " *" } else { "" };
            spans.push(Span::styled(
                dirty_mark,
                Style::new().fg(Theme::ACCENT_YELLOW),
            ));
            spans.push(Span::styled("   ", Style::new()));
            spans.push(Span::styled(
                " [/] ",
                Style::new()
                    .fg(Color::Black)
                    .bg(Theme::ACCENT_YELLOW)
                    .bold(),
            ));
            spans.push(Span::styled(
                " switch section",
                Style::new().fg(Theme::TEXT_MUTED),
            ));

            let p = Paragraph::new(Line::from(spans));
            frame.render_widget(p, inner);
        }
        View::Handoff => {
            let block = Theme::block();
            let inner = block.inner(area);
            frame.render_widget(block, area);

            let line = Line::from(vec![
                Span::styled(" Handoff ", Style::new().fg(Theme::TEXT_PRIMARY).bold()),
                Span::styled("  ", Style::new()),
                Span::styled(
                    "execution-contract quick menu",
                    Style::new().fg(Theme::TEXT_MUTED),
                ),
            ]);
            frame.render_widget(Paragraph::new(line), inner);
        }
        _ => {}
    }
}

fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    if area.height == 0 {
        return;
    }
    if let Some((ref msg, level)) = app.flash_message {
        let color = match level {
            FlashLevel::Success => Theme::ACCENT_GREEN,
            FlashLevel::Error => Theme::ACCENT_RED,
            FlashLevel::Info => Theme::ACCENT_BLUE,
        };
        let line = Line::from(vec![Span::styled(msg.as_str(), Style::new().fg(color))]);
        frame.render_widget(Paragraph::new(line), area);
        return;
    }

    if matches!(app.view, View::Settings) {
        frame.render_widget(Paragraph::new(settings_footer_line(app)), area);
        return;
    }

    if matches!(app.view, View::Handoff) {
        frame.render_widget(Paragraph::new(handoff_footer_line()), area);
    }
}

fn settings_footer_line(app: &App) -> Line<'static> {
    let key_style = Style::new().fg(Theme::TEXT_KEY).bold();
    let desc_style = Style::new().fg(Theme::TEXT_KEY_DESC);

    if app.editing_field || app.password_form.editing {
        Line::from(vec![
            Span::styled(" Enter ", key_style),
            Span::styled("apply", desc_style),
            Span::styled("  Esc ", key_style),
            Span::styled("cancel", desc_style),
            Span::styled("  ? ", key_style),
            Span::styled("help", desc_style),
        ])
    } else {
        Line::from(vec![
            Span::styled(" j/k ", key_style),
            Span::styled("move", desc_style),
            Span::styled("  Enter ", key_style),
            Span::styled("edit/cycle", desc_style),
            Span::styled(
                "  [/] ",
                Style::new()
                    .fg(Color::Black)
                    .bg(Theme::ACCENT_YELLOW)
                    .bold(),
            ),
            Span::styled("section prev/next", desc_style),
            Span::styled("  s ", key_style),
            Span::styled("save", desc_style),
            Span::styled("  q/Esc ", key_style),
            Span::styled("back", desc_style),
            Span::styled("  ? ", key_style),
            Span::styled("help", desc_style),
        ])
    }
}

fn handoff_footer_line() -> Line<'static> {
    let key_style = Style::new().fg(Theme::TEXT_KEY).bold();
    let desc_style = Style::new().fg(Theme::TEXT_KEY_DESC);
    Line::from(vec![
        Span::styled(" 1/2/3 ", key_style),
        Span::styled("tabs", desc_style),
        Span::styled("  Enter ", key_style),
        Span::styled("open selected session", desc_style),
        Span::styled("  Esc ", key_style),
        Span::styled("back", desc_style),
        Span::styled("  ? ", key_style),
        Span::styled("help", desc_style),
    ])
}

fn render_focus_session_summary(frame: &mut Frame, app: &App, area: Rect) {
    let block = Theme::block();
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(session) = app.selected_session() else {
        frame.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(
                " View mode: waiting for selected session",
                Style::new().fg(Theme::TEXT_MUTED),
            )])),
            inner,
        );
        return;
    };

    let title = session
        .context
        .title
        .as_deref()
        .unwrap_or(session.session_id.as_str());
    let agent_count = app
        .session_max_active_agents
        .get(&session.session_id)
        .copied()
        .unwrap_or(1)
        .max(1);
    let event_count = if session.events.is_empty() {
        session.stats.event_count
    } else {
        session.events.len() as u64
    };
    let message_count = session.stats.message_count;
    let actor = app
        .selected_session_actor_label()
        .unwrap_or_else(|| "anonymous".to_string());
    let live_style = if app.live_mode {
        Style::new().fg(Color::Black).bg(Theme::ACCENT_RED).bold()
    } else {
        Style::new().fg(Theme::TEXT_MUTED)
    };

    let line1 = Line::from(vec![
        Span::styled(
            format!(" {} ", if app.live_mode { "LIVE" } else { "VIEW" }),
            live_style,
        ),
        Span::styled(" ", Style::new()),
        Span::styled(
            compact_line(title, 44),
            Style::new().fg(Theme::TEXT_PRIMARY).bold(),
        ),
        Span::styled("  ", Style::new()),
        Span::styled(actor, Style::new().fg(Theme::ACCENT_CYAN)),
        Span::styled("  ", Style::new()),
        Span::styled(
            format!(
                "{} · {} · ev:{} msgs:{} agents:{}",
                session.agent.tool, session.agent.model, event_count, message_count, agent_count
            ),
            Style::new().fg(Theme::TEXT_SECONDARY),
        ),
    ]);

    let prompt = latest_prompt_preview(session).unwrap_or_else(|| "(no prompt)".to_string());
    let output = latest_output_preview(session).unwrap_or_else(|| "(no output)".to_string());
    let line2 = Line::from(vec![
        Span::styled(" prompt ", Style::new().fg(Theme::TEXT_KEY).bold()),
        Span::styled(
            compact_line(&prompt, 68),
            Style::new().fg(Theme::TEXT_PRIMARY),
        ),
        Span::styled("  |  ", Style::new().fg(Theme::GUTTER)),
        Span::styled(" output ", Style::new().fg(Theme::TEXT_KEY).bold()),
        Span::styled(
            compact_line(&output, 68),
            Style::new().fg(Theme::TEXT_PRIMARY),
        ),
    ]);

    frame.render_widget(Paragraph::new(vec![line1, line2]), inner);
}

fn latest_prompt_preview(session: &Session) -> Option<String> {
    session
        .events
        .iter()
        .rev()
        .find(|event| {
            matches!(event.event_type, EventType::UserMessage)
                && !App::is_internal_summary_user_event(event)
        })
        .and_then(|event| event_first_text(event.content.blocks.as_slice()))
}

fn latest_output_preview(session: &Session) -> Option<String> {
    for event in session.events.iter().rev() {
        if matches!(event.event_type, EventType::AgentMessage) {
            if let Some(text) = event_first_text(event.content.blocks.as_slice()) {
                if !text.is_empty() {
                    return Some(text);
                }
            }
        }
    }
    for event in session.events.iter().rev() {
        if let EventType::TaskEnd {
            summary: Some(summary),
        } = &event.event_type
        {
            let compact = compact_line(summary, 180);
            if !compact.is_empty() {
                return Some(compact);
            }
        }
    }
    None
}

fn event_first_text(blocks: &[ContentBlock]) -> Option<String> {
    for block in blocks {
        for fragment in App::block_text_fragments(block) {
            for line in fragment.lines() {
                let compact = compact_line(line, 220);
                if !compact.is_empty() {
                    return Some(compact);
                }
            }
        }
    }
    None
}

fn compact_line(text: &str, max_chars: usize) -> String {
    let one_line = text
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if one_line.is_empty() {
        return String::new();
    }
    if one_line.chars().count() <= max_chars {
        return one_line;
    }
    let mut out = String::new();
    for ch in one_line.chars().take(max_chars.saturating_sub(1)) {
        out.push(ch);
    }
    out.push('…');
    out
}

use crate::app::ServerInfo;

fn render_upload_popup(frame: &mut Frame, app: &App) {
    let popup = match &app.upload_popup {
        Some(p) => p,
        None => return,
    };

    // Center the popup — dynamic height based on content
    let area = frame.area();
    let popup_width = 60u16.min(area.width.saturating_sub(4));
    let content_lines = match &popup.phase {
        UploadPhase::FetchingTeams | UploadPhase::Uploading => 3,
        UploadPhase::SelectTeam => {
            let status_line = if popup.status.is_some() { 1 } else { 0 };
            popup.teams.len() as u16 + 3 + status_line
        }
        UploadPhase::Done => popup.results.len() as u16 + 3,
    };
    let popup_height = (content_lines + 2).min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    frame.render_widget(Clear, popup_area);

    let title = match &popup.phase {
        UploadPhase::FetchingTeams => " Fetching Upload Targets... ",
        UploadPhase::SelectTeam => " Publish Session ",
        UploadPhase::Uploading => " Uploading... ",
        UploadPhase::Done => " Upload Results ",
    };

    let block = Theme::block_accent().title(title);
    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let key_style = Style::new().fg(Theme::TEXT_KEY);
    let desc_style = Style::new().fg(Theme::TEXT_KEY_DESC);

    let mut lines = Vec::new();

    match &popup.phase {
        UploadPhase::FetchingTeams => {
            lines.push(Line::from(Span::styled(
                "  Loading...",
                Style::new().fg(Theme::ACCENT_BLUE),
            )));
        }
        UploadPhase::SelectTeam => {
            for (i, target) in popup.teams.iter().enumerate() {
                let is_cursor = i == popup.selected;
                let is_checked = popup.checked.get(i).copied().unwrap_or(false);
                let check = if is_checked { "[x]" } else { "[ ]" };
                let pointer = if is_cursor { ">" } else { " " };
                let badge = if target.is_personal {
                    "public"
                } else {
                    "scope"
                };
                // Pad target name to align badges
                let name_width = 30usize.saturating_sub(target.name.len());
                let padding = " ".repeat(name_width);
                let style = if is_cursor {
                    Style::new().fg(Theme::TEXT_PRIMARY).bold()
                } else if is_checked {
                    Style::new().fg(Theme::ACCENT_BLUE)
                } else {
                    Style::new().fg(Theme::TEXT_SECONDARY)
                };
                let badge_style = Style::new().fg(Theme::TEXT_MUTED);
                lines.push(Line::from(vec![
                    Span::styled(
                        format!(" {} {} {}{}", pointer, check, target.name, padding),
                        style,
                    ),
                    Span::styled(badge, badge_style),
                ]));
            }
            lines.push(Line::raw(""));
            let checked_count = popup.checked.iter().filter(|&&c| c).count();
            let enter_label = if checked_count > 0 {
                format!("upload ({checked_count})  ")
            } else {
                "upload  ".to_string()
            };
            lines.push(Line::from(vec![
                Span::styled(" Space ", key_style),
                Span::styled("toggle  ", desc_style),
                Span::styled("a ", key_style),
                Span::styled("all  ", desc_style),
                Span::styled("Enter ", key_style),
                Span::styled(enter_label, desc_style),
                Span::styled("Esc ", key_style),
                Span::styled("cancel", desc_style),
            ]));
            if let Some(ref status) = popup.status {
                lines.push(Line::from(Span::styled(
                    format!("  {status}"),
                    Style::new().fg(Theme::ACCENT_YELLOW),
                )));
            }
        }
        UploadPhase::Uploading => {
            if let Some(ref status) = popup.status {
                lines.push(Line::from(Span::styled(
                    format!("  {status}"),
                    Style::new().fg(Theme::ACCENT_BLUE),
                )));
            } else {
                lines.push(Line::from(Span::styled(
                    "  Uploading session...",
                    Style::new().fg(Theme::ACCENT_BLUE),
                )));
            }
        }
        UploadPhase::Done => {
            for (target_name, result) in &popup.results {
                match result {
                    Ok(url) => {
                        lines.push(Line::from(vec![
                            Span::styled("  + ", Style::new().fg(Theme::ACCENT_GREEN)),
                            Span::styled(
                                format!("{target_name}: "),
                                Style::new().fg(Theme::TEXT_PRIMARY),
                            ),
                            Span::styled(url.as_str(), Style::new().fg(Theme::TEXT_SECONDARY)),
                        ]));
                    }
                    Err(e) => {
                        lines.push(Line::from(vec![
                            Span::styled("  x ", Style::new().fg(Theme::ACCENT_RED)),
                            Span::styled(
                                format!("{target_name}: "),
                                Style::new().fg(Theme::TEXT_PRIMARY),
                            ),
                            Span::styled(e.as_str(), Style::new().fg(Theme::ACCENT_RED)),
                        ]));
                    }
                }
            }
            lines.push(Line::raw(""));
            lines.push(Line::from(Span::styled(
                "  Press any key to close",
                Style::new().fg(Color::DarkGray),
            )));
        }
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

fn render_repo_picker(frame: &mut Frame, app: &App) {
    let entries = app.repo_picker_entries();
    let selected = app.repo_picker_selected_index();

    let area = frame.area();
    let popup_width = 76u16.min(area.width.saturating_sub(4));
    let popup_height = 16u16.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    frame.render_widget(Clear, popup_area);

    let block = Theme::block_accent().title(" Repo Picker ");
    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let key_style = Style::new().fg(Theme::TEXT_KEY);
    let desc_style = Style::new().fg(Theme::TEXT_KEY_DESC);

    let mut lines = vec![
        Line::from(vec![
            Span::styled(" Search: ", Style::new().fg(Theme::TEXT_SECONDARY)),
            Span::styled(
                format!("{}|", app.repo_picker_query),
                Style::new().fg(Theme::ACCENT_YELLOW),
            ),
        ]),
        Line::raw(""),
    ];

    if entries.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No repository matches search.",
            Style::new().fg(Theme::TEXT_MUTED),
        )));
    } else {
        let visible_rows = inner.height.saturating_sub(5) as usize;
        let visible_rows = visible_rows.max(1);
        let start = selected.saturating_sub(visible_rows / 2);
        let end = (start + visible_rows).min(entries.len());
        for (idx, repo) in entries.iter().enumerate().skip(start).take(end - start) {
            let is_selected = idx == selected;
            lines.push(Line::from(vec![
                Span::styled(
                    if is_selected { " > " } else { "   " },
                    if is_selected {
                        Style::new().fg(Theme::ACCENT_BLUE).bold()
                    } else {
                        Style::new().fg(Theme::TEXT_MUTED)
                    },
                ),
                Span::styled(
                    repo.as_str(),
                    if is_selected {
                        Style::new().fg(Theme::TEXT_PRIMARY).bold()
                    } else {
                        Style::new().fg(Theme::TEXT_SECONDARY)
                    },
                ),
            ]));
        }
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled(" j/k ", key_style),
        Span::styled("move  ", desc_style),
        Span::styled("Enter ", key_style),
        Span::styled("open repo  ", desc_style),
        Span::styled("Esc ", key_style),
        Span::styled("close", desc_style),
    ]));

    frame.render_widget(Paragraph::new(lines), inner);
}

fn build_server_status_spans(info: &ServerInfo) -> Vec<Span<'_>> {
    let mut spans = Vec::new();

    // Shorten URL for display
    let display_url = info
        .url
        .trim_start_matches("http://")
        .trim_start_matches("https://");

    match &info.status {
        ServerStatus::Online(version) => {
            spans.push(Span::styled(
                format!("{} ", display_url),
                Style::new().fg(Theme::TEXT_SECONDARY),
            ));
            spans.push(Span::styled(
                format!("online v{} ", version),
                Style::new().fg(Theme::ACCENT_GREEN),
            ));
        }
        ServerStatus::Offline => {
            spans.push(Span::styled(
                format!("{} ", display_url),
                Style::new().fg(Theme::TEXT_SECONDARY),
            ));
            spans.push(Span::styled("offline ", Style::new().fg(Theme::ACCENT_RED)));
        }
        ServerStatus::Unknown => {
            // Web target: show last upload time if available
            if let Some(ref time) = info.last_upload {
                // Show only date portion for brevity
                let display_time = if time.len() > 10 { &time[..10] } else { time };
                spans.push(Span::styled(
                    format!("last upload: {} ", display_time),
                    Style::new().fg(Color::DarkGray),
                ));
            }
        }
    }

    spans
}

#[cfg(test)]
mod tests {
    use super::{
        build_server_status_spans, compact_line, event_first_text, handoff_footer_line,
        latest_output_preview, latest_prompt_preview, settings_footer_line, should_show_footer,
    };
    use crate::app::{App, ServerInfo, ServerStatus, View};
    use chrono::Utc;
    use opensession_core::trace::{Agent, Content, ContentBlock, Event, EventType, Session};
    use std::collections::HashMap;

    fn make_event(event_type: EventType, text: &str) -> Event {
        Event {
            event_id: format!("e-{text}"),
            timestamp: Utc::now(),
            event_type,
            task_id: None,
            content: Content::text(text),
            duration_ms: None,
            attributes: HashMap::new(),
        }
    }

    fn make_session(events: Vec<Event>) -> Session {
        let mut session = Session::new(
            "s-ui-test".to_string(),
            Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );
        session.events = events;
        session.recompute_stats();
        session
    }

    fn spans_to_text(spans: &[ratatui::text::Span<'_>]) -> String {
        spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>()
            .join("")
    }

    #[test]
    fn compact_line_collapses_whitespace() {
        assert_eq!(
            compact_line("  hello   world\t\tfrom\nui  ", 100),
            "hello world from ui"
        );
    }

    #[test]
    fn compact_line_truncates_with_ellipsis() {
        let out = compact_line("abcdefghijklmnopqrstuvwxyz", 8);
        assert_eq!(out, "abcdefg…");
    }

    #[test]
    fn event_first_text_returns_first_non_empty_line() {
        let blocks = vec![ContentBlock::Text {
            text: "\n   \nfirst line\nsecond".to_string(),
        }];
        assert_eq!(event_first_text(&blocks), Some("first line".to_string()));
    }

    #[test]
    fn event_first_text_returns_none_for_empty_blocks() {
        let blocks = vec![ContentBlock::Text {
            text: "   \n\t\n".to_string(),
        }];
        assert_eq!(event_first_text(&blocks), None);
    }

    #[test]
    fn latest_prompt_preview_ignores_internal_summary_user_events() {
        let session = make_session(vec![
            make_event(
                EventType::UserMessage,
                "You are generating a turn-summary payload. Return JSON only.",
            ),
            make_event(EventType::UserMessage, "real prompt"),
        ]);
        assert_eq!(
            latest_prompt_preview(&session),
            Some("real prompt".to_string())
        );
    }

    #[test]
    fn latest_output_preview_prefers_agent_message() {
        let session = make_session(vec![
            make_event(
                EventType::TaskEnd {
                    summary: Some("fallback summary".to_string()),
                },
                "",
            ),
            make_event(EventType::AgentMessage, "agent output"),
        ]);
        assert_eq!(
            latest_output_preview(&session),
            Some("agent output".to_string())
        );
    }

    #[test]
    fn latest_output_preview_falls_back_to_task_end_summary() {
        let session = make_session(vec![make_event(
            EventType::TaskEnd {
                summary: Some("task finished cleanly".to_string()),
            },
            "",
        )]);
        assert_eq!(
            latest_output_preview(&session),
            Some("task finished cleanly".to_string())
        );
    }

    #[test]
    fn latest_output_preview_returns_none_without_output() {
        let session = make_session(vec![make_event(EventType::UserMessage, "only prompt")]);
        assert_eq!(latest_output_preview(&session), None);
    }

    #[test]
    fn build_server_status_spans_formats_online_status() {
        let info = ServerInfo {
            url: "https://example.com".to_string(),
            status: ServerStatus::Online("1.2.3".to_string()),
            last_upload: None,
        };
        let text = spans_to_text(&build_server_status_spans(&info));
        assert!(text.contains("example.com"));
        assert!(text.contains("online v1.2.3"));
    }

    #[test]
    fn build_server_status_spans_formats_offline_status() {
        let info = ServerInfo {
            url: "http://localhost:3000".to_string(),
            status: ServerStatus::Offline,
            last_upload: None,
        };
        let text = spans_to_text(&build_server_status_spans(&info));
        assert!(text.contains("localhost:3000"));
        assert!(text.contains("offline"));
    }

    #[test]
    fn build_server_status_spans_formats_unknown_status_with_last_upload_date() {
        let info = ServerInfo {
            url: "https://opensession.io".to_string(),
            status: ServerStatus::Unknown,
            last_upload: Some("2026-02-15T12:34:56Z".to_string()),
        };
        let text = spans_to_text(&build_server_status_spans(&info));
        assert!(text.contains("last upload: 2026-02-15"));
        assert!(!text.contains("12:34:56"));
    }

    #[test]
    fn latest_prompt_preview_works_with_normal_user_message() {
        let session = make_session(vec![
            make_event(EventType::UserMessage, "first prompt"),
            make_event(EventType::UserMessage, "latest prompt"),
        ]);
        assert_eq!(
            latest_prompt_preview(&session),
            Some("latest prompt".to_string())
        );
    }

    #[test]
    fn settings_view_shows_footer_without_flash_message() {
        let mut app = App::new(Vec::new());
        assert!(!should_show_footer(&app));

        app.view = View::Settings;
        assert!(should_show_footer(&app));

        app.view = View::Handoff;
        assert!(should_show_footer(&app));
    }

    #[test]
    fn settings_footer_line_shows_navigation_shortcuts() {
        let app = App::new(Vec::new());
        let text = spans_to_text(&settings_footer_line(&app).spans);

        assert!(text.contains("j/k"));
        assert!(text.contains("Enter"));
        assert!(text.contains("[/]"));
        assert!(text.contains("prev/next"));
        assert!(text.contains("q/Esc"));
        assert!(text.contains("help"));
    }

    #[test]
    fn settings_footer_line_shows_edit_shortcuts_while_editing() {
        let mut app = App::new(Vec::new());
        app.editing_field = true;

        let text = spans_to_text(&settings_footer_line(&app).spans);

        assert!(text.contains("Enter"));
        assert!(text.contains("apply"));
        assert!(text.contains("Esc"));
        assert!(text.contains("cancel"));
        assert!(!text.contains("[/]"));
    }

    #[test]
    fn handoff_footer_line_shows_handoff_shortcuts() {
        let text = spans_to_text(&handoff_footer_line().spans);
        assert!(text.contains("1/2/3"));
        assert!(text.contains("Enter"));
        assert!(text.contains("Esc"));
        assert!(text.contains("help"));
    }
}
