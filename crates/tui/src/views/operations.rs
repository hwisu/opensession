use crate::app::App;
use crate::theme::Theme;
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let block = Theme::block_dim().padding(Theme::PADDING_CARD);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let daemon_on = app.startup_status.daemon_pid.is_some();
    let stream_write = &app.daemon_config.daemon.stream_write;

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
                "  Realtime Publish:      ",
                Style::new().fg(Theme::TEXT_SECONDARY),
            ),
            Span::styled(
                format!(
                    "{} / {}",
                    on_off(app.daemon_config.daemon.auto_publish),
                    app.daemon_config.daemon.publish_on.display()
                ),
                Style::new().fg(Theme::TEXT_PRIMARY),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "  Publish Debounce:      ",
                Style::new().fg(Theme::TEXT_SECONDARY),
            ),
            Span::styled(
                format!("{}s", app.daemon_config.daemon.debounce_secs),
                Style::new().fg(Theme::TEXT_PRIMARY),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "  Realtime Poll:         ",
                Style::new().fg(Theme::TEXT_SECONDARY),
            ),
            Span::styled(
                format!("{}ms", app.daemon_config.daemon.realtime_debounce_ms),
                Style::new().fg(Theme::TEXT_PRIMARY),
            ),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            "── Capture Watchers ──",
            Style::new().fg(Theme::ACCENT_BLUE).bold(),
        )),
        Line::raw(""),
        Line::from(vec![
            Span::styled(
                "  Claude Code:           ",
                Style::new().fg(Theme::TEXT_SECONDARY),
            ),
            Span::styled(
                on_off(app.daemon_config.watchers.claude_code),
                Style::new().fg(Theme::TEXT_PRIMARY),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "  OpenCode:              ",
                Style::new().fg(Theme::TEXT_SECONDARY),
            ),
            Span::styled(
                on_off(app.daemon_config.watchers.opencode),
                Style::new().fg(Theme::TEXT_PRIMARY),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "  Cursor:                ",
                Style::new().fg(Theme::TEXT_SECONDARY),
            ),
            Span::styled(
                on_off(app.daemon_config.watchers.cursor),
                Style::new().fg(Theme::TEXT_PRIMARY),
            ),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            "── Neglect Live Session Rules ──",
            Style::new().fg(Theme::ACCENT_BLUE).bold(),
        )),
        Line::from(Span::styled(
            "  Rule: if session.agent.tool matches (case-insensitive), Detail Live + LLM Summary are ignored",
            Style::new().fg(Theme::TEXT_MUTED),
        )),
        Line::raw(""),
    ];

    for (label, key) in [
        ("claude-code", "claude-code"),
        ("codex", "codex"),
        ("cursor", "cursor"),
        ("gemini", "gemini"),
        ("opencode", "opencode"),
    ] {
        let enabled = stream_write.iter().any(|v| v.eq_ignore_ascii_case(key));
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {:<21}", label),
                Style::new().fg(Theme::TEXT_SECONDARY),
            ),
            Span::styled(on_off(enabled), Style::new().fg(Theme::TEXT_PRIMARY)),
        ]));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "Actions: d=daemon on/off  s=save config  r=refresh status  4=Settings",
        Style::new().fg(Theme::TEXT_HINT),
    )));

    frame.render_widget(Paragraph::new(lines), inner);
}

fn on_off(on: bool) -> &'static str {
    if on {
        "ON"
    } else {
        "OFF"
    }
}
