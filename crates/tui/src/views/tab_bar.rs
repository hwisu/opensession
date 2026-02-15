use crate::app::{Tab, View};
use crate::theme::Theme;
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

pub fn render(frame: &mut Frame, active: &Tab, view: &View, area: Rect, local_mode: bool) {
    let tabs = [
        (Tab::Sessions, "1:Sessions", "Sessions"),
        (Tab::Collaboration, "2:Collaboration", "Collaboration"),
        (Tab::Settings, "3:Settings", "Settings"),
    ];

    // In detail views, hide number prefixes since 1-6 keys are used for event filters
    let hide_numbers = matches!(view, View::SessionDetail | View::TeamDetail);

    let mut spans = vec![Span::styled(" ", Style::new())];

    for (tab, label_numbered, label_plain) in &tabs {
        let is_active = tab == active;
        let label = if hide_numbers {
            label_plain
        } else {
            label_numbered
        };
        let style = if is_active {
            Style::new()
                .fg(Color::Black)
                .bg(Theme::ACCENT_BLUE)
                .bold()
                .add_modifier(Modifier::UNDERLINED)
        } else if local_mode && *tab == Tab::Collaboration {
            Style::new().fg(Theme::TAB_DIM)
        } else if hide_numbers {
            // Dimmer style in detail views where tabs are not directly switchable
            Style::new().fg(Theme::TAB_DIM)
        } else {
            Style::new().fg(Theme::TAB_INACTIVE)
        };

        let text = format!(" {} ", label);

        spans.push(Span::styled(text, style));
        spans.push(Span::styled(" ", Style::new()));
    }

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::render;
    use crate::app::{Tab, View};
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

    fn render_tab_text(active: Tab, view: View, local_mode: bool) -> String {
        let backend = TestBackend::new(120, 3);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| {
                let area = frame.area();
                render(frame, &active, &view, area, local_mode);
            })
            .expect("draw");
        buffer_to_string(terminal.backend().buffer())
    }

    #[test]
    fn session_list_view_shows_numbered_tabs() {
        let text = render_tab_text(Tab::Sessions, View::SessionList, false);
        assert!(text.contains("1:Sessions"));
        assert!(text.contains("2:Collaboration"));
        assert!(text.contains("3:Settings"));
    }

    #[test]
    fn session_detail_view_hides_number_prefixes() {
        let text = render_tab_text(Tab::Sessions, View::SessionDetail, false);
        assert!(text.contains("Sessions"));
        assert!(text.contains("Collaboration"));
        assert!(!text.contains("1:Sessions"));
        assert!(!text.contains("2:Collaboration"));
    }

    #[test]
    fn team_detail_view_hides_number_prefixes() {
        let text = render_tab_text(Tab::Collaboration, View::TeamDetail, false);
        assert!(text.contains("Sessions"));
        assert!(text.contains("Settings"));
        assert!(!text.contains("3:Settings"));
    }
}
