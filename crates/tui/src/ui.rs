use crate::app::{App, EventFilter, View};
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
            let block = Block::bordered()
                .border_style(Style::new().fg(Color::Rgb(60, 65, 80)));

            let inner = block.inner(area);
            frame.render_widget(block, area);

            let header_line = Line::from(vec![
                Span::styled(
                    " opensession ",
                    Style::new()
                        .fg(Color::Rgb(217, 119, 80))
                        .bold(),
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
            ]);
            let p = Paragraph::new(header_line).alignment(Alignment::Left);
            frame.render_widget(p, inner);
        }
        View::SessionDetail => {
            let filter_titles = [
                "All",
                "Messages",
                "Tools",
                "Thinking",
                "Files",
                "Shell",
            ];
            let selected = match app.event_filter {
                EventFilter::All => 0,
                EventFilter::Messages => 1,
                EventFilter::ToolCalls => 2,
                EventFilter::Thinking => 3,
                EventFilter::FileOps => 4,
                EventFilter::Shell => 5,
            };

            let tabs = Tabs::new(filter_titles)
                .block(
                    Block::bordered()
                        .border_style(Style::new().fg(Color::Rgb(60, 65, 80))),
                )
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
                    Span::styled(" / ", Style::new().fg(Color::Black).bg(Color::Rgb(220, 180, 60)).bold()),
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
