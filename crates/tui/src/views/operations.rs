use crate::app::App;
use crate::theme::Theme;
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let block = Theme::block_dim().padding(Theme::PADDING_CARD);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let daemon_on = app.startup_status.daemon_pid.is_some();
    let watch_path_count = app.daemon_config.watchers.custom_paths.len();

    let mut lines = vec![
        Line::from(Span::styled(
            "── Operations ──",
            Style::new().fg(Theme::ACCENT_BLUE).bold(),
        )),
        Line::raw(""),
        Line::from(vec![
            Span::styled(
                "  Daemon:                ",
                Style::new().fg(Theme::TEXT_SECONDARY),
            ),
            Span::styled(
                if daemon_on {
                    format!(
                        "ON (pid {})",
                        app.startup_status.daemon_pid.unwrap_or_default()
                    )
                } else {
                    "OFF".to_string()
                },
                if daemon_on {
                    Style::new().fg(Theme::ACCENT_GREEN)
                } else {
                    Style::new().fg(Theme::TEXT_MUTED)
                },
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "  Capture Policy:        ",
                Style::new().fg(Theme::TEXT_SECONDARY),
            ),
            Span::styled(
                if daemon_on {
                    "ON (forced: Session End)".to_string()
                } else {
                    "OFF (Manual only)".to_string()
                },
                Style::new().fg(Theme::TEXT_PRIMARY),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "  Watch Path Roots:      ",
                Style::new().fg(Theme::TEXT_SECONDARY),
            ),
            Span::styled(
                watch_path_count.to_string(),
                Style::new().fg(Theme::TEXT_PRIMARY),
            ),
        ]),
    ];
    if watch_path_count > 0 {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "  First configured roots:",
            Style::new().fg(Theme::TEXT_HINT),
        )));
        for path in app.daemon_config.watchers.custom_paths.iter().take(3) {
            lines.push(Line::from(vec![
                Span::raw("    - "),
                Span::styled(path, Style::new().fg(Theme::TEXT_MUTED)),
            ]));
        }
        if watch_path_count > 3 {
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled(
                    format!("... and {} more", watch_path_count - 3),
                    Style::new().fg(Theme::TEXT_MUTED),
                ),
            ]));
        }
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "Actions: d=daemon on/off  r=refresh status  4=Settings",
        Style::new().fg(Theme::TEXT_HINT),
    )));
    lines.push(Line::from(Span::styled(
        "Edit watch paths and publish behavior in Settings > Capture & Sync",
        Style::new().fg(Theme::TEXT_MUTED),
    )));

    frame.render_widget(Paragraph::new(lines), inner);
}

#[cfg(test)]
mod tests {
    use super::render;
    use crate::app::App;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::Terminal;

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

    fn render_text(app: &App, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| render(frame, app, frame.area()))
            .expect("draw");
        buffer_to_string(terminal.backend().buffer())
    }

    #[test]
    fn render_shows_daemon_on_with_pid() {
        let mut app = App::new(vec![]);
        app.startup_status.daemon_pid = Some(4242);
        let text = render_text(&app, 100, 30);
        assert!(text.contains("Daemon:"));
        assert!(text.contains("ON (pid 4242)"));
    }

    #[test]
    fn render_shows_daemon_off_when_pid_missing() {
        let app = App::new(vec![]);
        let text = render_text(&app, 100, 30);
        assert!(text.contains("Daemon:"));
        assert!(text.contains("OFF"));
        assert!(text.contains("Actions: d=daemon on/off"));
    }
}
