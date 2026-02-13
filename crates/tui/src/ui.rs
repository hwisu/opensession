use crate::app::{
    App, ConnectionContext, DetailViewMode, EventFilter, FlashLevel, ListLayout, ServerStatus,
    SettingsSection, TaskViewMode, UploadPhase, View, ViewMode,
};
use crate::theme::Theme;
use crate::views::{
    help, invitations, modal, session_detail, session_list, settings, setup, tab_bar, team_detail,
    teams,
};
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

    let [tab_area, header_area, body_area, footer_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());

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
        View::Teams => teams::render(frame, app, body_area),
        View::TeamDetail => team_detail::render(frame, app, body_area),
        View::Invitations => invitations::render(frame, app, body_area),
        View::Help => {}  // rendered as overlay below
        View::Setup => {} // handled above
    }

    // Footer
    render_footer(frame, app, footer_area);

    // Upload popup overlay (legacy)
    if app.upload_popup.is_some() {
        render_upload_popup(frame, app);
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
                ViewMode::Team(t) => format!("Team: {t}"),
                ViewMode::Repo(r) => format!("Repo: {r}"),
            };

            // Connection context badge
            let (badge_text, badge_fg, badge_bg) = match &app.connection_ctx {
                ConnectionContext::Local => ("LOCAL".to_string(), Color::Black, Theme::BADGE_LOCAL),
                ConnectionContext::Docker { .. } => {
                    ("DOCKER".to_string(), Color::Black, Theme::BADGE_DOCKER)
                }
                ConnectionContext::CloudPersonal => {
                    ("PERSONAL".to_string(), Color::Black, Theme::BADGE_PERSONAL)
                }
                ConnectionContext::CloudTeam { team_name } => (
                    format!("\u{2191} {team_name}"),
                    Color::Black,
                    Theme::BADGE_TEAM,
                ),
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
            if let Some(ref tool) = app.tool_filter {
                left_spans.push(Span::styled("  ", Style::new()));
                left_spans.push(Span::styled(
                    format!(" tool:{tool} "),
                    Style::new().fg(Color::Black).bg(Color::Magenta).bold(),
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

            // Task view mode indicator (only when session has sub-agents)
            if app.session_has_sub_agents() {
                spans.push(Span::styled(" │ ", Style::new().fg(Theme::GUTTER)));
                let (task_label, task_style) = match app.task_view_mode {
                    TaskViewMode::Summary => (
                        "Tasks:Summary",
                        Style::new().fg(Color::Black).bg(Theme::ROLE_TASK).bold(),
                    ),
                    TaskViewMode::Detail => ("Tasks:Detail", Style::new().fg(Theme::ROLE_TASK)),
                };
                spans.push(Span::styled(format!(" {} ", task_label), task_style));
            }

            // Collapse indicator
            if app.collapse_consecutive {
                spans.push(Span::styled(" ", Style::new()));
                spans.push(Span::styled(
                    " c:on ",
                    Style::new().fg(Color::Black).bg(Theme::ACCENT_GREEN).bold(),
                ));
            }

            let line = Line::from(spans);
            let p = Paragraph::new(line).block(Theme::block());
            frame.render_widget(p, area);
        }
        View::Teams | View::TeamDetail => {
            let block = Theme::block();
            let inner = block.inner(area);
            frame.render_widget(block, area);

            let title = if matches!(app.view, View::TeamDetail) {
                app.team_detail
                    .as_ref()
                    .map(|d| format!(" Team: {} ", d.team.name))
                    .unwrap_or_else(|| " Team Detail ".to_string())
            } else {
                " Teams ".to_string()
            };

            let spans = vec![Span::styled(
                title,
                Style::new().fg(Theme::ACCENT_ORANGE).bold(),
            )];
            let p = Paragraph::new(Line::from(spans));
            frame.render_widget(p, inner);
        }
        View::Invitations => {
            let block = Theme::block();
            let inner = block.inner(area);
            frame.render_widget(block, area);

            let count = app.invitations.len();
            let spans = vec![
                Span::styled(" Inbox ", Style::new().fg(Theme::ACCENT_ORANGE).bold()),
                Span::styled(
                    format!(" {} pending", count),
                    Style::new().fg(Theme::TEXT_SECONDARY),
                ),
            ];
            let p = Paragraph::new(Line::from(spans));
            frame.render_widget(p, inner);
        }
        View::Settings => {
            let block = Theme::block();
            let inner = block.inner(area);
            frame.render_widget(block, area);

            // Section tabs
            let sections = [
                (SettingsSection::Profile, "Profile"),
                (SettingsSection::Account, "Account"),
                (SettingsSection::DaemonConfig, "Config"),
            ];
            let mut spans = vec![Span::styled(
                " Settings  ",
                Style::new().fg(Theme::TEXT_PRIMARY).bold(),
            )];
            for (section, label) in &sections {
                let is_active = *section == app.settings_section;
                let style = if is_active {
                    Style::new().fg(Color::Black).bg(Theme::ACCENT_BLUE).bold()
                } else {
                    Style::new().fg(Theme::TAB_INACTIVE)
                };
                spans.push(Span::styled(format!(" {} ", label), style));
                spans.push(Span::styled(" ", Style::new()));
            }
            let dirty_mark = if app.config_dirty { " *" } else { "" };
            spans.push(Span::styled(
                dirty_mark,
                Style::new().fg(Theme::ACCENT_YELLOW),
            ));

            let p = Paragraph::new(Line::from(spans));
            frame.render_widget(p, inner);
        }
        _ => {}
    }
}

fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    let key_style = Style::new().fg(Theme::TEXT_KEY);
    let desc_style = Style::new().fg(Theme::TEXT_KEY_DESC);

    let help = match app.view {
        View::SessionList => {
            if app.searching {
                Line::from(vec![
                    Span::styled(
                        " / ",
                        Style::new()
                            .fg(Color::Black)
                            .bg(Theme::ACCENT_YELLOW)
                            .bold(),
                    ),
                    Span::styled(
                        format!(" {}", app.search_query),
                        Style::new().fg(Theme::TEXT_PRIMARY),
                    ),
                    Span::styled("_", Style::new().fg(Theme::ACCENT_YELLOW)),
                    Span::styled("  ESC cancel  Enter confirm", desc_style),
                ])
            } else if app.list_layout == ListLayout::ByUser && app.is_db_view() {
                Line::from(vec![
                    Span::styled(" h/l ", key_style),
                    Span::styled("columns  ", desc_style),
                    Span::styled("j/k ", key_style),
                    Span::styled("navigate  ", desc_style),
                    Span::styled("Enter ", key_style),
                    Span::styled("open  ", desc_style),
                    Span::styled("m ", key_style),
                    Span::styled("single  ", desc_style),
                    Span::styled("Tab ", key_style),
                    Span::styled("view  ", desc_style),
                    Span::styled("q ", key_style),
                    Span::styled("quit", desc_style),
                ])
            } else {
                let mut spans = vec![
                    Span::styled(" j/k ", key_style),
                    Span::styled("navigate  ", desc_style),
                    Span::styled("Enter ", key_style),
                    Span::styled("open  ", desc_style),
                    Span::styled("/ ", key_style),
                    Span::styled("search  ", desc_style),
                    Span::styled("Tab ", key_style),
                    Span::styled("view  ", desc_style),
                ];
                if app.is_db_view() {
                    spans.push(Span::styled("m ", key_style));
                    spans.push(Span::styled("by-user  ", desc_style));
                    spans.push(Span::styled("f ", key_style));
                    spans.push(Span::styled("tool  ", desc_style));
                    spans.push(Span::styled("d ", key_style));
                    spans.push(Span::styled("delete  ", desc_style));
                }
                if app.total_pages() > 1 {
                    spans.push(Span::styled("[/] ", key_style));
                    spans.push(Span::styled("page  ", desc_style));
                }
                if !matches!(app.connection_ctx, ConnectionContext::Local) {
                    spans.push(Span::styled("p ", key_style));
                    spans.push(Span::styled("publish  ", desc_style));
                }
                spans.push(Span::styled("q ", key_style));
                spans.push(Span::styled("quit", desc_style));
                Line::from(spans)
            }
        }
        View::SessionDetail => {
            if app.detail_view_mode == DetailViewMode::Turn {
                Line::from(vec![
                    Span::styled(" j/k ", key_style),
                    Span::styled("scroll  ", desc_style),
                    Span::styled("n/N ", key_style),
                    Span::styled("turn  ", desc_style),
                    Span::styled("Enter ", key_style),
                    Span::styled("expand  ", desc_style),
                    Span::styled("g/G ", key_style),
                    Span::styled("first/last  ", desc_style),
                    Span::styled("v ", key_style),
                    Span::styled("linear  ", desc_style),
                    Span::styled("Esc ", key_style),
                    Span::styled("back", desc_style),
                ])
            } else {
                Line::from(vec![
                    Span::styled(" j/k ", key_style),
                    Span::styled("scroll  ", desc_style),
                    Span::styled("u/U ", key_style),
                    Span::styled("user  ", desc_style),
                    Span::styled("n/N ", key_style),
                    Span::styled("type  ", desc_style),
                    Span::styled("Enter ", key_style),
                    Span::styled("expand  ", desc_style),
                    Span::styled("v ", key_style),
                    Span::styled("split  ", desc_style),
                    Span::styled("1-6 ", key_style),
                    Span::styled("filter  ", desc_style),
                    Span::styled("Esc ", key_style),
                    Span::styled("back", desc_style),
                ])
            }
        }
        View::Teams => Line::from(vec![
            Span::styled(" j/k ", key_style),
            Span::styled("navigate  ", desc_style),
            Span::styled("Enter ", key_style),
            Span::styled("detail  ", desc_style),
            Span::styled("n ", key_style),
            Span::styled("new team  ", desc_style),
            Span::styled("r ", key_style),
            Span::styled("refresh  ", desc_style),
            Span::styled("q ", key_style),
            Span::styled("quit", desc_style),
        ]),
        View::TeamDetail => Line::from(vec![
            Span::styled(" Tab ", key_style),
            Span::styled("section  ", desc_style),
            Span::styled("j/k ", key_style),
            Span::styled("navigate  ", desc_style),
            Span::styled("d ", key_style),
            Span::styled("remove  ", desc_style),
            Span::styled("r ", key_style),
            Span::styled("refresh  ", desc_style),
            Span::styled("Esc ", key_style),
            Span::styled("back", desc_style),
        ]),
        View::Invitations => Line::from(vec![
            Span::styled(" j/k ", key_style),
            Span::styled("navigate  ", desc_style),
            Span::styled("a ", key_style),
            Span::styled("accept  ", desc_style),
            Span::styled("d ", key_style),
            Span::styled("decline  ", desc_style),
            Span::styled("r ", key_style),
            Span::styled("refresh  ", desc_style),
            Span::styled("q ", key_style),
            Span::styled("quit", desc_style),
        ]),
        View::Settings => {
            let mut spans = vec![
                Span::styled(" [/] ", key_style),
                Span::styled("section  ", desc_style),
            ];
            match app.settings_section {
                SettingsSection::Profile => {
                    spans.push(Span::styled("r ", key_style));
                    spans.push(Span::styled("refresh  ", desc_style));
                }
                SettingsSection::Account => {
                    spans.push(Span::styled("j/k ", key_style));
                    spans.push(Span::styled("navigate  ", desc_style));
                    spans.push(Span::styled("Enter ", key_style));
                    spans.push(Span::styled("edit  ", desc_style));
                    spans.push(Span::styled("r ", key_style));
                    spans.push(Span::styled("regen key  ", desc_style));
                }
                SettingsSection::DaemonConfig => {
                    spans.push(Span::styled("j/k ", key_style));
                    spans.push(Span::styled("navigate  ", desc_style));
                    spans.push(Span::styled("Enter ", key_style));
                    spans.push(Span::styled("edit  ", desc_style));
                    spans.push(Span::styled("s ", key_style));
                    spans.push(Span::styled("save  ", desc_style));
                    spans.push(Span::styled("d ", key_style));
                    spans.push(Span::styled("daemon  ", desc_style));
                }
            }
            spans.push(Span::styled("Esc ", key_style));
            spans.push(Span::styled("back", desc_style));
            Line::from(spans)
        }
        _ => Line::raw(""),
    };

    let mut spans = help.spans;
    // Persistent first-run hint after setup skip
    if matches!(app.view, View::SessionList) && !app.startup_status.config_exists {
        spans.push(Span::styled("  |  ", Style::new().fg(Theme::GUTTER)));
        spans.push(Span::styled(
            "setup later: 4:Settings > Config",
            Style::new().fg(Theme::TEXT_HINT),
        ));
    }

    // Append flash message to any view's footer
    if let Some((ref msg, level)) = app.flash_message {
        let color = match level {
            FlashLevel::Success => Theme::ACCENT_GREEN,
            FlashLevel::Error => Theme::ACCENT_RED,
            FlashLevel::Info => Theme::ACCENT_BLUE,
        };
        spans.push(Span::styled("  ", Style::new()));
        spans.push(Span::styled(msg.as_str(), Style::new().fg(color)));
    }
    let help = Line::from(spans);

    let paragraph = Paragraph::new(help);
    frame.render_widget(paragraph, area);
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
        UploadPhase::FetchingTeams => " Fetching Teams... ",
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
            for (i, team) in popup.teams.iter().enumerate() {
                let is_cursor = i == popup.selected;
                let is_checked = popup.checked.get(i).copied().unwrap_or(false);
                let disabled = team.is_personal && !popup.git_storage_ready;
                let check = if disabled {
                    "[-]"
                } else if is_checked {
                    "[x]"
                } else {
                    "[ ]"
                };
                let pointer = if is_cursor { ">" } else { " " };
                let badge = if team.is_personal {
                    if disabled {
                        "git required"
                    } else {
                        "git"
                    }
                } else {
                    "server"
                };
                // Pad team name to align badges
                let name_width = 30usize.saturating_sub(team.name.len());
                let padding = " ".repeat(name_width);
                let style = if disabled {
                    Style::new().fg(Theme::TEXT_MUTED)
                } else if is_cursor {
                    Style::new().fg(Theme::TEXT_PRIMARY).bold()
                } else if is_checked {
                    Style::new().fg(Theme::ACCENT_BLUE)
                } else {
                    Style::new().fg(Theme::TEXT_SECONDARY)
                };
                let badge_style = if disabled {
                    Style::new().fg(Theme::ACCENT_RED)
                } else {
                    Style::new().fg(Theme::TEXT_MUTED)
                };
                lines.push(Line::from(vec![
                    Span::styled(
                        format!(" {} {} {}{}", pointer, check, team.name, padding),
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
            for (team_name, result) in &popup.results {
                match result {
                    Ok(url) => {
                        lines.push(Line::from(vec![
                            Span::styled("  + ", Style::new().fg(Theme::ACCENT_GREEN)),
                            Span::styled(
                                format!("{team_name}: "),
                                Style::new().fg(Theme::TEXT_PRIMARY),
                            ),
                            Span::styled(url.as_str(), Style::new().fg(Theme::TEXT_SECONDARY)),
                        ]));
                    }
                    Err(e) => {
                        lines.push(Line::from(vec![
                            Span::styled("  x ", Style::new().fg(Theme::ACCENT_RED)),
                            Span::styled(
                                format!("{team_name}: "),
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
