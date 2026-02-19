use crate::theme::Theme;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Clear, Paragraph};

/// Kinds of modal overlay.
pub enum Modal {
    /// Confirmation dialog.
    Confirm {
        title: String,
        message: String,
        action: ConfirmAction,
    },
}

/// What happens when a Confirm modal is accepted.
#[derive(Clone)]
pub enum ConfirmAction {
    RegenerateApiKey,
    /// Settings: save unsaved changes and exit.
    SaveChanges,
    /// Delete a session from the server and local DB.
    DeleteSession {
        session_id: String,
    },
}

/// Render the current modal overlay on top of everything.
pub fn render(frame: &mut Frame, modal: &Modal, _edit_buffer: &str) {
    let area = frame.area();
    let popup_width = 50u16.min(area.width.saturating_sub(4));
    let popup_height = 10u16.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    frame.render_widget(Clear, popup_area);

    let key_style = Style::new().fg(Theme::TEXT_KEY);
    let desc_style = Style::new().fg(Theme::TEXT_KEY_DESC);

    let Modal::Confirm {
        title,
        message,
        action,
    } = modal;

    let block = Block::bordered()
        .border_type(ratatui::widgets::BorderType::Rounded)
        .title(format!(" {} ", title))
        .border_style(Style::new().fg(Theme::ACCENT_YELLOW));
    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let hint_line = if matches!(action, ConfirmAction::SaveChanges) {
        // 3-way: y=save+exit, n=discard+exit, Esc=cancel
        Line::from(vec![
            Span::styled("  y ", key_style),
            Span::styled("save  ", desc_style),
            Span::styled("n ", key_style),
            Span::styled("discard  ", desc_style),
            Span::styled("Esc ", key_style),
            Span::styled("cancel", desc_style),
        ])
    } else {
        Line::from(vec![
            Span::styled("  y/Enter ", key_style),
            Span::styled("confirm  ", desc_style),
            Span::styled("n/Esc ", key_style),
            Span::styled("cancel", desc_style),
        ])
    };

    let lines = vec![
        Line::raw(""),
        Line::from(Span::styled(
            format!("  {}", message),
            Style::new().fg(Theme::TEXT_PRIMARY),
        )),
        Line::raw(""),
        hint_line,
    ];
    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}
