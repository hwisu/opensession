use crate::app::{App, EventFilter, ServerStatus, View};
use crate::views::{session_detail, session_list};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Paragraph, Tabs};

pub fn render(frame: &mut Frame, app: &mut App) {
    let [header_area, body_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    render_header(frame, app, header_area);

    match app.view {
        View::SessionList => session_list::render(frame, app, body_area),
        View::SessionDetail => session_detail::render(frame, app, body_area),
    }

    render_footer(frame, app, footer_area);
}

fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    match app.view {
        View::SessionList => {
            let block = Block::bordered().border_style(Style::new().fg(Color::Rgb(60, 65, 80)));

            let inner = block.inner(area);
            frame.render_widget(block, area);

            // Left side: title + session count
            let left_spans = vec![
                Span::styled(
                    " opensession ",
                    Style::new().fg(Color::Rgb(217, 119, 80)).bold(),
                ),
                Span::styled("  ", Style::new().fg(Color::DarkGray)),
                Span::styled(
                    format!("{} sessions", app.filtered_sessions.len()),
                    Style::new().fg(Color::Rgb(140, 145, 160)),
                ),
                if !app.search_query.is_empty() {
                    Span::styled(
                        format!("  (filtered from {})", app.sessions.len()),
                        Style::new().fg(Color::DarkGray),
                    )
                } else {
                    Span::raw("")
                },
            ];
            let left_line = Line::from(left_spans);
            let p = Paragraph::new(left_line).alignment(Alignment::Left);
            frame.render_widget(p, inner);

            // Right side: server status
            if let Some(ref info) = app.server_info {
                let status_spans = build_server_status_spans(info);
                let right_line = Line::from(status_spans);
                let p_right = Paragraph::new(right_line).alignment(Alignment::Right);
                frame.render_widget(p_right, inner);
            }
        }
        View::SessionDetail => {
            let filter_titles = ["All", "Messages", "Tools", "Thinking", "Files", "Shell"];
            let selected = match app.event_filter {
                EventFilter::All => 0,
                EventFilter::Messages => 1,
                EventFilter::ToolCalls => 2,
                EventFilter::Thinking => 3,
                EventFilter::FileOps => 4,
                EventFilter::Shell => 5,
            };

            let tabs = Tabs::new(filter_titles)
                .block(Block::bordered().border_style(Style::new().fg(Color::Rgb(60, 65, 80))))
                .select(selected)
                .style(Style::new().fg(Color::Rgb(80, 85, 100)))
                .highlight_style(Style::new().fg(Color::Rgb(100, 180, 240)).bold())
                .divider("  ");

            frame.render_widget(tabs, area);
        }
    }
}

fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    let help = match app.view {
        View::SessionList => {
            if app.searching {
                Line::from(vec![
                    Span::styled(
                        " / ",
                        Style::new()
                            .fg(Color::Black)
                            .bg(Color::Rgb(220, 180, 60))
                            .bold(),
                    ),
                    Span::styled(
                        format!(" {}", app.search_query),
                        Style::new().fg(Color::White),
                    ),
                    Span::styled("_", Style::new().fg(Color::Rgb(220, 180, 60))),
                    Span::styled(
                        "  ESC cancel  Enter confirm",
                        Style::new().fg(Color::DarkGray),
                    ),
                ])
            } else {
                Line::from(vec![
                    Span::styled(" j/k ", Style::new().fg(Color::Rgb(140, 145, 160))),
                    Span::styled("navigate  ", Style::new().fg(Color::DarkGray)),
                    Span::styled("Enter ", Style::new().fg(Color::Rgb(140, 145, 160))),
                    Span::styled("open  ", Style::new().fg(Color::DarkGray)),
                    Span::styled("/ ", Style::new().fg(Color::Rgb(140, 145, 160))),
                    Span::styled("search  ", Style::new().fg(Color::DarkGray)),
                    Span::styled("q ", Style::new().fg(Color::Rgb(140, 145, 160))),
                    Span::styled("quit", Style::new().fg(Color::DarkGray)),
                ])
            }
        }
        View::SessionDetail => Line::from(vec![
            Span::styled(" j/k ", Style::new().fg(Color::Rgb(140, 145, 160))),
            Span::styled("scroll  ", Style::new().fg(Color::DarkGray)),
            Span::styled("1-6 ", Style::new().fg(Color::Rgb(140, 145, 160))),
            Span::styled("filter  ", Style::new().fg(Color::DarkGray)),
            Span::styled("f ", Style::new().fg(Color::Rgb(140, 145, 160))),
            Span::styled("cycle  ", Style::new().fg(Color::DarkGray)),
            Span::styled("Esc ", Style::new().fg(Color::Rgb(140, 145, 160))),
            Span::styled("back", Style::new().fg(Color::DarkGray)),
        ]),
    };

    let paragraph = Paragraph::new(help);
    frame.render_widget(paragraph, area);
}

use crate::app::ServerInfo;

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
                Style::new().fg(Color::Rgb(140, 145, 160)),
            ));
            spans.push(Span::styled(
                format!("online v{} ", version),
                Style::new().fg(Color::Rgb(80, 200, 120)),
            ));
        }
        ServerStatus::Offline => {
            spans.push(Span::styled(
                format!("{} ", display_url),
                Style::new().fg(Color::Rgb(140, 145, 160)),
            ));
            spans.push(Span::styled(
                "offline ",
                Style::new().fg(Color::Rgb(220, 80, 80)),
            ));
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
