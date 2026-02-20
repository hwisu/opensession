use crate::app::{App, DetailViewMode, View};
use crate::theme::Theme;
use ratatui::prelude::*;
use ratatui::widgets::{Clear, Paragraph};

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    // Center the help overlay
    let popup_width = 78u16.min(area.width.saturating_sub(4));
    let popup_height = 22u16.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    frame.render_widget(Clear, popup_area);

    let block = Theme::block_accent()
        .title(" Keyboard Shortcuts ")
        .padding(Theme::PADDING_CARD);
    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let key_style = Style::new().fg(Theme::ACCENT_YELLOW).bold();
    let desc_style = Style::new().fg(Theme::TEXT_CONTENT);
    let header_style = Style::new().fg(Theme::ACCENT_BLUE).bold();
    let context = context_label(app);
    let close_hint_line = Line::from(Span::styled(
        "Press any key (or ?) to close",
        Style::new().fg(Color::DarkGray),
    ));

    let mut lines = vec![Line::from(vec![
        Span::styled("Current Context: ", Style::new().fg(Theme::TEXT_SECONDARY)),
        Span::styled(context, Style::new().fg(Theme::ACCENT_BLUE).bold()),
    ])];

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled("── Global ──", header_style)));
    if allows_tab_switching(app) {
        lines.push(Line::from(vec![
            Span::styled("  1/2/3       ", key_style),
            Span::styled("Switch tabs", desc_style),
        ]));
    }
    lines.push(Line::from(vec![
        Span::styled("  ? / Esc     ", key_style),
        Span::styled("Close help overlay", desc_style),
    ]));

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled("── Current View ──", header_style)));
    lines.extend(current_view_shortcuts(app, key_style, desc_style));
    lines.push(Line::raw(""));
    lines.push(close_hint_line.clone());

    // Keep close hint visible even when the help body exceeds the popup height.
    let max_lines = inner.height as usize;
    if max_lines == 0 {
        return;
    }
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        if let Some(last) = lines.last_mut() {
            *last = close_hint_line;
        }
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

fn context_label(app: &App) -> &'static str {
    match app.view {
        View::SessionList => "Session List",
        View::SessionDetail => {
            if app.detail_view_mode == DetailViewMode::Turn {
                "Session Detail (Turn)"
            } else {
                "Session Detail (Linear)"
            }
        }
        View::Handoff => "Handoff",
        View::Settings => "Settings",
        View::Setup => "Setup",
        View::Help => "Help",
    }
}

fn allows_tab_switching(app: &App) -> bool {
    !matches!(app.view, View::SessionDetail | View::Setup) && !app.focus_detail_view
}

fn current_view_shortcuts(app: &App, key_style: Style, desc_style: Style) -> Vec<Line<'static>> {
    match app.view {
        View::SessionList => vec![
            shortcut_line("  j/k         ", "Move selection", key_style, desc_style),
            shortcut_line(
                "  Enter       ",
                "Open session detail",
                key_style,
                desc_style,
            ),
            shortcut_line("  /           ", "Search sessions", key_style, desc_style),
            shortcut_line(
                "  Tab/S-Tab   ",
                "Cycle Local/Repo view",
                key_style,
                desc_style,
            ),
            shortcut_line(
                "  R           ",
                "Repo search picker",
                key_style,
                desc_style,
            ),
            shortcut_line(
                "  m           ",
                "Toggle multi-column layout",
                key_style,
                desc_style,
            ),
            shortcut_line("  a / t       ", "Cycle tool filter", key_style, desc_style),
            shortcut_line("  r           ", "Cycle time range", key_style, desc_style),
        ],
        View::SessionDetail => {
            if app.detail_view_mode == DetailViewMode::Turn {
                vec![
                    shortcut_line("  j/k         ", "Scroll agent pane", key_style, desc_style),
                    shortcut_line("  n/N         ", "Next/prev turn", key_style, desc_style),
                    shortcut_line("  g/G         ", "First/last turn", key_style, desc_style),
                    shortcut_line("  h/l         ", "Horizontal scroll", key_style, desc_style),
                    shortcut_line("  Space/Enter ", "Toggle raw output", key_style, desc_style),
                    shortcut_line(
                        "  p           ",
                        "Toggle prompt block",
                        key_style,
                        desc_style,
                    ),
                    shortcut_line(
                        "  v           ",
                        "Back to linear mode",
                        key_style,
                        desc_style,
                    ),
                    shortcut_line(
                        "  1-0         ",
                        "Toggle event filters",
                        key_style,
                        desc_style,
                    ),
                ]
            } else {
                let mut lines = vec![
                    shortcut_line("  j/k         ", "Move events", key_style, desc_style),
                    shortcut_line("  g/G         ", "First/last event", key_style, desc_style),
                    shortcut_line("  h/l, ←/→    ", "Horizontal scroll", key_style, desc_style),
                    shortcut_line("  PgDn/PgUp   ", "Jump 10 events", key_style, desc_style),
                    shortcut_line(
                        "  u/U         ",
                        "Next/prev user message",
                        key_style,
                        desc_style,
                    ),
                    shortcut_line(
                        "  n/N         ",
                        "Next/prev same-type event",
                        key_style,
                        desc_style,
                    ),
                    shortcut_line(
                        "  1-0         ",
                        "Toggle event filters",
                        key_style,
                        desc_style,
                    ),
                ];
                if selected_event_supports_diff_toggle(app) {
                    lines.push(shortcut_line(
                        "  d           ",
                        "Toggle diff preview",
                        key_style,
                        desc_style,
                    ));
                }
                lines
            }
        }
        View::Handoff => vec![
            shortcut_line("  j/k         ", "Move candidate", key_style, desc_style),
            shortcut_line(
                "  Space       ",
                "Pick / unpick session",
                key_style,
                desc_style,
            ),
            shortcut_line("  Enter       ", "Refresh preview", key_style, desc_style),
            shortcut_line(
                "  g           ",
                "Generate handoff markdown",
                key_style,
                desc_style,
            ),
            shortcut_line("  s           ", "Save artifact", key_style, desc_style),
            shortcut_line(
                "  r           ",
                "Refresh last artifact",
                key_style,
                desc_style,
            ),
        ],
        View::Settings => vec![
            shortcut_line("  j/k         ", "Move setting", key_style, desc_style),
            shortcut_line(
                "  Enter       ",
                "Edit / cycle value",
                key_style,
                desc_style,
            ),
            shortcut_line("  [/]         ", "Prev/next section", key_style, desc_style),
            shortcut_line("  s           ", "Save settings", key_style, desc_style),
            shortcut_line("  Esc/q       ", "Back to sessions", key_style, desc_style),
        ],
        View::Setup | View::Help => vec![shortcut_line(
            "  Esc/q       ",
            "Close current screen",
            key_style,
            desc_style,
        )],
    }
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
        opensession_core::trace::EventType::FileEdit { diff: Some(_), .. }
    )
}

fn shortcut_line(
    key: &'static str,
    desc: &'static str,
    key_style: Style,
    desc_style: Style,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(key, key_style),
        Span::styled(desc, desc_style),
    ])
}

#[cfg(test)]
mod tests {
    use super::render;
    use crate::app::{App, View};
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
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

    #[test]
    fn render_shows_shortcuts_and_close_hint() {
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut app = App::new(Vec::new());
        app.view = View::SessionList;
        terminal
            .draw(|frame| {
                let area = frame.area();
                render(frame, area, &app);
            })
            .expect("draw");

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(text.contains("Keyboard Shortcuts"));
        assert!(text.contains("Current Context: Session List"));
        assert!(text.contains("Tab/S-Tab"));
        assert!(text.contains("Press any key"));
        assert!(!text.contains("Generate handoff markdown"));
    }

    #[test]
    fn render_handles_small_terminal_area() {
        let backend = TestBackend::new(30, 10);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let app = App::new(Vec::new());
        terminal
            .draw(|frame| {
                render(frame, Rect::new(0, 0, 30, 10), &app);
            })
            .expect("draw");

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(text.contains("Keyboard"));
    }

    #[test]
    fn render_session_detail_help_hides_unrelated_sections() {
        let backend = TestBackend::new(120, 34);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut app = App::new(Vec::new());
        app.view = View::SessionDetail;
        terminal
            .draw(|frame| {
                let area = frame.area();
                render(frame, area, &app);
            })
            .expect("draw");

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(text.contains("Current Context: Session Detail (Linear)"));
        assert!(text.contains("Next/prev user message"));
        assert!(!text.contains("Generate handoff markdown"));
    }
}
