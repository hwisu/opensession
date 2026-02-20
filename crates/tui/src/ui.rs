use crate::app::{
    extract_visible_turns, App, ConnectionContext, DetailViewMode, EventFilter, FlashLevel,
    ServerStatus, SettingsSection, UploadPhase, View, ViewMode,
};
use crate::theme::Theme;
use crate::views::{handoff, help, modal, session_detail, session_list, settings, setup, tab_bar};
use chrono::Duration as ChronoDuration;
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
        if app.selected_session().is_some() {
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
        if app.help_overlay_open {
            help::render(frame, frame.area(), app);
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
        View::Help => session_list::render(frame, app, body_area), // compatibility fallback
        View::Setup => {}                                          // handled above
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
    if app.help_overlay_open {
        help::render(frame, frame.area(), app);
    }

    // Modal overlay
    if let Some(ref m) = app.modal {
        modal::render(frame, m, &app.edit_buffer);
    }
}

fn should_show_footer(app: &App) -> bool {
    app.flash_message.is_some()
        || matches!(
            app.view,
            View::SessionList | View::SessionDetail | View::Settings | View::Handoff
        )
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

            // Connection context badge: local mode is the default, so only show
            // remote-capable contexts to reduce persistent visual noise.
            let badge = match &app.connection_ctx {
                ConnectionContext::Local => None,
                ConnectionContext::Server { .. } => {
                    Some(("SERVER".to_string(), Color::Black, Theme::BADGE_SERVER))
                }
                ConnectionContext::CloudPersonal => {
                    Some(("PERSONAL".to_string(), Color::Black, Theme::BADGE_PERSONAL))
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
                Span::styled("  ", Style::new()),
                Span::styled(mode_label, Style::new().fg(Theme::ACCENT_BLUE)),
                Span::styled("  ", Style::new()),
                session_count_span,
            ];

            if let Some((badge_text, badge_fg, badge_bg)) = badge {
                left_spans.insert(1, Span::styled(" ", Style::new()));
                left_spans.insert(
                    2,
                    Span::styled(
                        format!(" {} ", badge_text),
                        Style::new().fg(badge_fg).bg(badge_bg).bold(),
                    ),
                );
                left_spans.insert(3, Span::styled(" ", Style::new()));
            }

            if !app.search_query.is_empty() {
                left_spans.push(Span::styled(
                    format!("  (filtered from {})", app.sessions.len()),
                    Style::new().fg(Color::DarkGray),
                ));
            }

            // Agent filter indicator
            if let Some(agent) = app.active_agent_filter() {
                left_spans.push(Span::styled("  ", Style::new()));
                left_spans.push(Span::styled(
                    format!(" agent:{agent} "),
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
            if !app.is_default_sort_order() {
                left_spans.push(Span::styled("  ", Style::new()));
                left_spans.push(Span::styled(
                    format!(" order:{} ", app.session_sort_order_label()),
                    Style::new()
                        .fg(Color::Black)
                        .bg(Theme::ACCENT_YELLOW)
                        .bold(),
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
            let mut spans = vec![
                Span::styled(
                    " Session Detail ",
                    Style::new().fg(Theme::TEXT_PRIMARY).bold(),
                ),
                Span::styled("  ", Style::new()),
            ];
            spans.extend(event_filter_hotkey_spans(app));
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

            let candidates = app.handoff_candidates();
            let candidate_count = candidates.len();
            let picked_count = app.handoff_selected_candidates().len();
            let preview_count = app.handoff_effective_candidates().len();
            let scope = match &app.view_mode {
                ViewMode::Local => "Local".to_string(),
                ViewMode::Repo(repo) => format!("Repo: {repo}"),
            };

            let line = Line::from(vec![
                Span::styled(" Handoff ", Style::new().fg(Theme::TEXT_PRIMARY).bold()),
                Span::styled("  ", Style::new()),
                Span::styled(scope, Style::new().fg(Theme::ACCENT_BLUE)),
                Span::styled("  ", Style::new()),
                Span::styled(
                    format!("{candidate_count} candidates"),
                    Style::new().fg(Theme::TEXT_SECONDARY),
                ),
                Span::styled("  ", Style::new()),
                Span::styled(
                    format!(" picked {picked_count} "),
                    if picked_count > 0 {
                        Style::new().fg(Color::Black).bg(Theme::ACCENT_GREEN).bold()
                    } else {
                        Style::new().fg(Theme::TEXT_MUTED)
                    },
                ),
                Span::styled("  ", Style::new()),
                Span::styled(
                    format!("preview {preview_count}"),
                    Style::new().fg(Theme::ACCENT_CYAN),
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

    if matches!(app.view, View::SessionList) {
        frame.render_widget(
            Paragraph::new(session_list_footer_line(app, area.width)),
            area,
        );
        return;
    }

    if matches!(app.view, View::SessionDetail) {
        frame.render_widget(
            Paragraph::new(session_detail_footer_line(app, area.width)),
            area,
        );
        return;
    }

    if matches!(app.view, View::Handoff) {
        frame.render_widget(Paragraph::new(handoff_footer_line(area.width)), area);
    }
}

fn event_filter_hotkey_spans(app: &App) -> Vec<Span<'static>> {
    let filters = [
        ("1", "All", EventFilter::All),
        ("2", "User", EventFilter::User),
        ("3", "Agent", EventFilter::Agent),
        ("4", "Think", EventFilter::Think),
        ("5", "Tools", EventFilter::Tools),
        ("6", "Files", EventFilter::Files),
        ("7", "Shell", EventFilter::Shell),
        ("8", "Task", EventFilter::Task),
        ("9", "Web", EventFilter::Web),
        ("0", "Other", EventFilter::Other),
    ];

    let mut spans = Vec::with_capacity(filters.len() * 2);
    for (idx, (key, label, filter)) in filters.iter().enumerate() {
        let token = format!("[{key}]{label}");
        let is_active = app.event_filters.contains(filter);
        let style = if is_active {
            Style::new().fg(Color::Black).bg(Theme::ACCENT_BLUE).bold()
        } else {
            Style::new().fg(Theme::TEXT_SECONDARY)
        };
        spans.push(Span::styled(token, style));
        if idx + 1 != filters.len() {
            spans.push(Span::styled(" ", Style::new().fg(Theme::TEXT_MUTED)));
        }
    }
    spans
}

#[derive(Clone)]
enum FooterSegment {
    Plain(String),
    Shortcut { key: String, desc: String },
}

impl FooterSegment {
    fn display_width(&self) -> usize {
        match self {
            FooterSegment::Plain(value) => value.chars().count(),
            FooterSegment::Shortcut { key, desc } => key.chars().count() + 1 + desc.chars().count(),
        }
    }
}

fn fit_footer_segments(mut segments: Vec<FooterSegment>, width: u16) -> Vec<FooterSegment> {
    if segments.is_empty() {
        return segments;
    }
    let max_width = width.max(1) as usize;
    loop {
        let separators_width = segments.len().saturating_sub(1) * 5;
        let content_width = segments
            .iter()
            .map(FooterSegment::display_width)
            .sum::<usize>();
        if separators_width + content_width <= max_width || segments.len() <= 1 {
            return segments;
        }
        segments.pop();
    }
}

fn render_footer_segments(segments: Vec<FooterSegment>, width: u16) -> Line<'static> {
    let key_style = Style::new().fg(Theme::TEXT_KEY).bold();
    let desc_style = Style::new().fg(Theme::TEXT_KEY_DESC);
    let fitted = fit_footer_segments(segments, width);
    let mut spans = vec![Span::styled(" ", desc_style)];

    for (idx, segment) in fitted.into_iter().enumerate() {
        if idx > 0 {
            spans.push(Span::styled("  |  ", desc_style));
        }
        match segment {
            FooterSegment::Plain(value) => {
                spans.push(Span::styled(value, desc_style));
            }
            FooterSegment::Shortcut { key, desc } => {
                spans.push(Span::styled(format!("{key} "), key_style));
                spans.push(Span::styled(desc, desc_style));
            }
        }
    }

    Line::from(spans)
}

fn session_list_footer_line(app: &App, width: u16) -> Line<'static> {
    let mut segments = vec![
        FooterSegment::Plain(format!(
            "sessions {}/{}",
            app.page_count(),
            app.session_count()
        )),
        FooterSegment::Plain(format!("page {}/{}", app.page + 1, app.total_pages())),
        FooterSegment::Plain(format!("order:{}", app.session_sort_order_label())),
        FooterSegment::Shortcut {
            key: "j/k".to_string(),
            desc: "move".to_string(),
        },
        FooterSegment::Shortcut {
            key: "Enter".to_string(),
            desc: "open".to_string(),
        },
        FooterSegment::Shortcut {
            key: "/".to_string(),
            desc: "search".to_string(),
        },
        FooterSegment::Shortcut {
            key: "m".to_string(),
            desc: "layout".to_string(),
        },
        FooterSegment::Shortcut {
            key: "a".to_string(),
            desc: "tool(alias)".to_string(),
        },
        FooterSegment::Shortcut {
            key: "t".to_string(),
            desc: "tool".to_string(),
        },
        FooterSegment::Shortcut {
            key: "o".to_string(),
            desc: "order".to_string(),
        },
        FooterSegment::Shortcut {
            key: "r".to_string(),
            desc: "range".to_string(),
        },
        FooterSegment::Shortcut {
            key: "R".to_string(),
            desc: "repo search".to_string(),
        },
        FooterSegment::Shortcut {
            key: "Tab/S-Tab".to_string(),
            desc: "view".to_string(),
        },
        FooterSegment::Shortcut {
            key: "1/2/3".to_string(),
            desc: "tabs".to_string(),
        },
        FooterSegment::Shortcut {
            key: "?".to_string(),
            desc: "help".to_string(),
        },
    ];
    if app.searching {
        segments.insert(
            0,
            FooterSegment::Plain(format!("search: {}|", app.search_query)),
        );
    }
    if app.total_pages() > 1 {
        segments.insert(
            2,
            FooterSegment::Shortcut {
                key: "PgUp/PgDn".to_string(),
                desc: "page".to_string(),
            },
        );
    }
    render_footer_segments(segments, width)
}

fn session_detail_footer_line(app: &App, width: u16) -> Line<'static> {
    let mut segments: Vec<FooterSegment> = match app.detail_view_mode {
        DetailViewMode::Linear => {
            let mut shortcuts = vec![
                FooterSegment::Shortcut {
                    key: "j/k".to_string(),
                    desc: "nav".to_string(),
                },
                FooterSegment::Shortcut {
                    key: "g/G".to_string(),
                    desc: "head/tail".to_string(),
                },
                FooterSegment::Shortcut {
                    key: "u/U".to_string(),
                    desc: "user jump".to_string(),
                },
                FooterSegment::Shortcut {
                    key: "n/N".to_string(),
                    desc: "type jump".to_string(),
                },
                FooterSegment::Shortcut {
                    key: "h/l".to_string(),
                    desc: "scroll".to_string(),
                },
                FooterSegment::Shortcut {
                    key: "1-0".to_string(),
                    desc: "filter".to_string(),
                },
            ];
            if selected_event_supports_diff_toggle(app) {
                shortcuts.push(FooterSegment::Shortcut {
                    key: "d".to_string(),
                    desc: "diff toggle".to_string(),
                });
            }
            shortcuts.push(FooterSegment::Shortcut {
                key: "?".to_string(),
                desc: "help".to_string(),
            });
            shortcuts
        }
        DetailViewMode::Turn => vec![
            FooterSegment::Shortcut {
                key: "j/k".to_string(),
                desc: "pane".to_string(),
            },
            FooterSegment::Shortcut {
                key: "n/N".to_string(),
                desc: "turn jump".to_string(),
            },
            FooterSegment::Shortcut {
                key: "g/G".to_string(),
                desc: "head/tail".to_string(),
            },
            FooterSegment::Shortcut {
                key: "h/l".to_string(),
                desc: "scroll".to_string(),
            },
            FooterSegment::Shortcut {
                key: "Space/Enter".to_string(),
                desc: "raw".to_string(),
            },
            FooterSegment::Shortcut {
                key: "p".to_string(),
                desc: "prompt".to_string(),
            },
            FooterSegment::Shortcut {
                key: "v".to_string(),
                desc: "linear".to_string(),
            },
            FooterSegment::Shortcut {
                key: "1-0".to_string(),
                desc: "filter".to_string(),
            },
            FooterSegment::Shortcut {
                key: "?".to_string(),
                desc: "help".to_string(),
            },
        ],
    };

    if let Some(session) = app.selected_session() {
        let visible = app.get_visible_events(session);
        let event_total = visible.len();
        if event_total > 0 {
            let selected_index = app.detail_event_index.min(event_total - 1);
            let event_idx = selected_index + 1;
            segments.push(FooterSegment::Plain(format!(
                "event {event_idx}/{event_total}"
            )));

            let turns = extract_visible_turns(&visible);
            if !turns.is_empty() {
                let turn_idx = match app.detail_view_mode {
                    DetailViewMode::Turn => app.turn_index.min(turns.len() - 1) + 1,
                    DetailViewMode::Linear => turns
                        .iter()
                        .position(|turn| {
                            selected_index >= turn.start_display_index
                                && selected_index <= turn.end_display_index
                        })
                        .map(|idx| idx + 1)
                        .unwrap_or(1),
                };
                segments.push(FooterSegment::Plain(format!(
                    "turn {turn_idx}/{}",
                    turns.len()
                )));
            }

            if let Some(selected_event) = visible.get(event_idx - 1).map(|row| row.event()) {
                let baseline = session
                    .events
                    .first()
                    .map(|event| event.timestamp)
                    .unwrap_or(session.context.created_at);
                let elapsed = selected_event.timestamp.signed_duration_since(baseline);
                segments.push(FooterSegment::Plain(format!(
                    "elapsed {}",
                    format_elapsed_compact(elapsed)
                )));
            }
        }
    }

    segments.push(FooterSegment::Plain(format!(
        "follow:{}",
        app.detail_follow_status_label()
    )));

    render_footer_segments(segments, width)
}

fn selected_event_supports_diff_toggle(app: &App) -> bool {
    let Some(session) = app.selected_session() else {
        return false;
    };
    let visible = app.get_visible_events(session);
    if visible.is_empty() {
        return false;
    }
    let idx = app.detail_event_index.min(visible.len() - 1);
    matches!(
        visible[idx].event().event_type,
        EventType::FileEdit { diff: Some(_), .. }
    )
}

fn format_elapsed_compact(elapsed: ChronoDuration) -> String {
    let total = elapsed.num_seconds().max(0);
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours}h{minutes:02}m")
    } else if minutes > 0 {
        format!("{minutes}m{seconds:02}s")
    } else {
        format!("{seconds}s")
    }
}

fn settings_footer_line(app: &App) -> Line<'static> {
    let key_style = Style::new().fg(Theme::TEXT_KEY).bold();
    let desc_style = Style::new().fg(Theme::TEXT_KEY_DESC);

    if app.editing_field {
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

fn handoff_footer_line(width: u16) -> Line<'static> {
    let segments = vec![
        FooterSegment::Shortcut {
            key: "1/2/3".to_string(),
            desc: "tabs".to_string(),
        },
        FooterSegment::Shortcut {
            key: "j/k".to_string(),
            desc: "move".to_string(),
        },
        FooterSegment::Shortcut {
            key: "Space".to_string(),
            desc: "pick/unpick".to_string(),
        },
        FooterSegment::Shortcut {
            key: "Enter".to_string(),
            desc: "preview".to_string(),
        },
        FooterSegment::Shortcut {
            key: "g".to_string(),
            desc: "generate handoff".to_string(),
        },
        FooterSegment::Shortcut {
            key: "s".to_string(),
            desc: "save artifact".to_string(),
        },
        FooterSegment::Shortcut {
            key: "r".to_string(),
            desc: "refresh artifact".to_string(),
        },
        FooterSegment::Shortcut {
            key: "Esc".to_string(),
            desc: "back".to_string(),
        },
        FooterSegment::Shortcut {
            key: "?".to_string(),
            desc: "help".to_string(),
        },
    ];
    render_footer_segments(segments, width)
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
        .find(|event| matches!(event.event_type, EventType::UserMessage))
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
        UploadPhase::Uploading => 4,
        UploadPhase::Done => popup.results.len() as u16 + 3,
    };
    let popup_height = (content_lines + 2).min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    frame.render_widget(Clear, popup_area);

    let title = match &popup.phase {
        UploadPhase::Uploading => " Publish Session ",
        UploadPhase::Done => " Upload Results ",
    };

    let block = Theme::block_accent().title(title);
    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let key_style = Style::new().fg(Theme::TEXT_KEY);
    let desc_style = Style::new().fg(Theme::TEXT_KEY_DESC);

    let mut lines = Vec::new();

    match &popup.phase {
        UploadPhase::Uploading => {
            lines.push(Line::from(vec![
                Span::styled("  Target: ", Style::new().fg(Theme::TEXT_MUTED)),
                Span::styled(
                    popup.target_name.as_str(),
                    Style::new().fg(Theme::TEXT_PRIMARY),
                ),
            ]));
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
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![
                Span::styled(" Esc ", key_style),
                Span::styled("cancel", desc_style),
            ]));
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
        build_server_status_spans, compact_line, event_filter_hotkey_spans, event_first_text,
        handoff_footer_line, latest_output_preview, latest_prompt_preview, render,
        session_detail_footer_line, session_list_footer_line, settings_footer_line,
        should_show_footer,
    };
    use crate::app::{
        App, ConnectionContext, DetailViewMode, EventFilter, ServerInfo, ServerStatus, View,
    };
    use crate::theme::Theme;
    use chrono::Utc;
    use opensession_core::trace::{Agent, Content, ContentBlock, Event, EventType, Session};
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::style::Color;
    use ratatui::Terminal;
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

    fn line_has_colored_span(line: &ratatui::text::Line<'_>, needle: &str, color: Color) -> bool {
        line.spans
            .iter()
            .any(|span| span.content.as_ref().contains(needle) && span.style.fg == Some(color))
    }

    fn buffer_to_string(buffer: &Buffer) -> String {
        let area = *buffer.area();
        let mut out = String::new();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                out.push_str(buffer[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
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
    fn latest_prompt_preview_ignores_control_user_events() {
        let session = make_session(vec![
            make_event(
                EventType::UserMessage,
                "<instructions>system control message</instructions>",
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
    fn footer_is_visible_in_main_views_without_flash_message() {
        let mut app = App::new(Vec::new());
        assert!(should_show_footer(&app));

        app.view = View::SessionList;
        assert!(should_show_footer(&app));

        app.view = View::SessionDetail;
        assert!(should_show_footer(&app));

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
    fn session_list_footer_line_shows_search_input_while_searching() {
        let mut app = App::new(Vec::new());
        app.searching = true;
        app.search_query = "rollout".to_string();

        let text = spans_to_text(&session_list_footer_line(&app, 180).spans);
        assert!(text.contains("search: rollout|"));
    }

    #[test]
    fn handoff_footer_line_shows_handoff_shortcuts() {
        let text = spans_to_text(&handoff_footer_line(160).spans);
        assert!(text.contains("1/2/3"));
        assert!(text.contains("j/k"));
        assert!(text.contains("Space"));
        assert!(text.contains("Enter"));
        assert!(text.contains("generate"));
        assert!(text.contains("Esc"));
        assert!(text.contains("help"));
    }

    #[test]
    fn session_list_footer_highlights_shortcut_keys() {
        let app = App::new(Vec::new());
        let line = session_list_footer_line(&app, 220);
        assert!(line_has_colored_span(&line, "j/k", Theme::TEXT_KEY));
        assert!(line_has_colored_span(&line, "move", Theme::TEXT_KEY_DESC));
        assert!(line_has_colored_span(&line, "o", Theme::TEXT_KEY));
        assert!(line_has_colored_span(&line, "order", Theme::TEXT_KEY_DESC));
    }

    #[test]
    fn session_detail_footer_highlights_shortcut_keys() {
        let session = make_session(vec![
            make_event(EventType::UserMessage, "prompt"),
            make_event(EventType::AgentMessage, "answer"),
        ]);
        let mut app = App::new(vec![session]);
        app.view = View::SessionDetail;
        app.enter_detail_for_startup();
        let line = session_detail_footer_line(&app, 260);
        assert!(line_has_colored_span(&line, "j/k", Theme::TEXT_KEY));
        assert!(line_has_colored_span(&line, "nav", Theme::TEXT_KEY_DESC));
    }

    #[test]
    fn handoff_footer_highlights_shortcut_keys() {
        let line = handoff_footer_line(220);
        assert!(line_has_colored_span(&line, "j/k", Theme::TEXT_KEY));
        assert!(line_has_colored_span(&line, "move", Theme::TEXT_KEY_DESC));
    }

    #[test]
    fn detail_footer_line_includes_event_turn_elapsed_and_filters() {
        let mut session = make_session(vec![
            make_event(EventType::UserMessage, "prompt"),
            make_event(EventType::AgentMessage, "answer"),
        ]);
        session.context.created_at = Utc::now() - chrono::Duration::seconds(90);

        let mut app = App::new(vec![session]);
        app.view = View::SessionDetail;
        app.enter_detail_for_startup();
        app.detail_event_index = 1;

        let text = spans_to_text(&session_detail_footer_line(&app, 300).spans);
        assert!(text.contains("event"));
        assert!(text.contains("turn"));
        assert!(text.contains("elapsed"));
        assert!(text.contains("follow:"));
        assert!(text.contains("g/G"));
        assert!(text.contains("u/U"));
        assert!(text.contains("n/N"));
        assert!(text.contains("1-0 filter"));
        assert!(!text.contains("d diff toggle"));
    }

    #[test]
    fn detail_footer_line_shows_diff_toggle_for_file_edit_event() {
        let mut session = make_session(vec![
            make_event(EventType::UserMessage, "prompt"),
            make_event(
                EventType::FileEdit {
                    path: "src/main.rs".to_string(),
                    diff: Some("- old\n+ new".to_string()),
                },
                "edit",
            ),
        ]);
        session.context.created_at = Utc::now() - chrono::Duration::seconds(90);

        let mut app = App::new(vec![session]);
        app.view = View::SessionDetail;
        app.enter_detail_for_startup();
        app.detail_event_index = 1;

        let text = spans_to_text(&session_detail_footer_line(&app, 360).spans);
        assert!(text.contains("d diff toggle"));
    }

    #[test]
    fn detail_footer_line_turn_mode_hides_linear_only_shortcuts() {
        let session = make_session(vec![
            make_event(EventType::UserMessage, "prompt"),
            make_event(EventType::AgentMessage, "answer"),
        ]);
        let mut app = App::new(vec![session]);
        app.view = View::SessionDetail;
        app.enter_detail_for_startup();
        app.detail_view_mode = DetailViewMode::Turn;

        let text = spans_to_text(&session_detail_footer_line(&app, 420).spans);
        assert!(text.contains("Space/Enter"));
        assert!(text.contains("turn jump"));
        assert!(text.contains("v linear"));
        assert!(!text.contains("u/U"));
        assert!(!text.contains("d diff toggle"));
    }

    #[test]
    fn detail_header_no_longer_mentions_always_expanded() {
        let session = make_session(vec![make_event(EventType::UserMessage, "prompt")]);
        let mut app = App::new(vec![session]);
        app.enter_detail_for_startup();

        let backend = TestBackend::new(140, 24);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| {
                render(frame, &mut app);
            })
            .expect("draw");
        let text = buffer_to_string(terminal.backend().buffer());
        assert!(!text.contains("always expanded"));
        assert!(!text.contains("mode:"));
        assert!(!text.contains("active:"));
        assert!(text.contains("[1]All [2]User [3]Agent"));
    }

    #[test]
    fn event_filter_hotkey_spans_highlight_selected_filter_instead_of_active_label() {
        let mut app = App::new(Vec::new());
        app.event_filters = std::collections::HashSet::from([EventFilter::User]);

        let spans = event_filter_hotkey_spans(&app);
        let user = spans
            .iter()
            .find(|span| span.content.as_ref() == "[2]User")
            .expect("user filter token");
        let all = spans
            .iter()
            .find(|span| span.content.as_ref() == "[1]All")
            .expect("all filter token");

        assert_eq!(user.style.bg, Some(Theme::ACCENT_BLUE));
        assert_eq!(all.style.bg, None);
    }

    #[test]
    fn session_list_header_hides_local_connection_badge() {
        let mut app = App::new(Vec::new());
        app.view = View::SessionList;
        app.connection_ctx = ConnectionContext::Local;

        let backend = TestBackend::new(140, 24);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| {
                render(frame, &mut app);
            })
            .expect("draw");
        let text = buffer_to_string(terminal.backend().buffer());

        assert!(!text.contains("LOCAL"));
        assert!(text.contains("Local"));
    }

    #[test]
    fn session_list_header_shows_server_connection_badge() {
        let mut app = App::new(Vec::new());
        app.view = View::SessionList;
        app.connection_ctx = ConnectionContext::Server {
            url: "http://localhost:8787".to_string(),
        };

        let backend = TestBackend::new(140, 24);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| {
                render(frame, &mut app);
            })
            .expect("draw");
        let text = buffer_to_string(terminal.backend().buffer());

        assert!(text.contains("SERVER"));
    }

    #[test]
    fn handoff_header_shows_scope_and_selection_counts() {
        let mut session = make_session(vec![make_event(EventType::UserMessage, "prompt")]);
        session.session_id = "handoff-1".to_string();
        session.context.title = Some("Handoff polish".to_string());

        let mut app = App::new(vec![session]);
        app.view = View::Handoff;
        app.handoff_selected_session_id = Some("handoff-1".to_string());
        app.handoff_selected_session_ids = vec!["handoff-1".to_string()];

        let backend = TestBackend::new(140, 24);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| {
                render(frame, &mut app);
            })
            .expect("draw");
        let text = buffer_to_string(terminal.backend().buffer());

        assert!(text.contains("Handoff"));
        assert!(text.contains("Local"));
        assert!(text.contains("1 candidates"));
        assert!(text.contains("picked 1"));
        assert!(text.contains("preview 1"));
    }
}
