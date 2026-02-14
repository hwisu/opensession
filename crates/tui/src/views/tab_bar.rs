use crate::app::{Tab, View};
use crate::theme::Theme;
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

pub fn render(frame: &mut Frame, active: &Tab, view: &View, area: Rect, local_mode: bool) {
    let tabs = [
        (Tab::Sessions, "1:Sessions", "Sessions"),
        (Tab::Collaboration, "2:Collaboration", "Collaboration"),
        (Tab::Operations, "3:Operations", "Operations"),
        (Tab::Settings, "4:Settings", "Settings"),
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
