use crate::app::App;
use crate::theme::Theme;
use opensession_api::InvitationResponse;
use ratatui::prelude::*;
use ratatui::widgets::{List, ListItem, Paragraph};

pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.invitations_loading {
        let block = Theme::block_dim()
            .title(" Inbox ")
            .padding(Theme::PADDING_CARD);
        let msg = Paragraph::new("Loading inbox...")
            .block(block)
            .style(Style::new().fg(Theme::ACCENT_BLUE));
        frame.render_widget(msg, area);
        return;
    }

    if app.invitations.is_empty() {
        let block = Theme::block_dim()
            .title(" Inbox ")
            .padding(Theme::PADDING_CARD);
        let msg =
            Paragraph::new("Inbox is empty. Invitations and collaboration updates appear here.")
                .block(block)
                .style(Style::new().fg(Color::DarkGray));
        frame.render_widget(msg, area);
        return;
    }

    let items: Vec<ListItem> = app
        .invitations
        .iter()
        .map(|inv| invitation_to_list_item(inv))
        .collect();

    let list = List::new(items)
        .block(Theme::block_dim().title(format!(" Inbox ({}) ", app.invitations.len())))
        .highlight_style(
            Style::new()
                .bg(Theme::BG_SURFACE)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(" > ")
        .highlight_spacing(ratatui::widgets::HighlightSpacing::Always);

    frame.render_stateful_widget(list, area, &mut app.invitations_list_state);
}

fn invitation_to_list_item(inv: &InvitationResponse) -> ListItem<'static> {
    use opensession_api::{InvitationStatus, TeamRole};

    let role_color = if inv.role == TeamRole::Admin {
        Theme::ACCENT_YELLOW
    } else {
        Theme::TEXT_SECONDARY
    };

    let status_color = match inv.status {
        InvitationStatus::Pending => Theme::ACCENT_YELLOW,
        InvitationStatus::Accepted => Theme::ACCENT_GREEN,
        InvitationStatus::Declined => Theme::ACCENT_RED,
    };

    let line1 = Line::from(vec![
        Span::styled(
            inv.team_name.clone(),
            Style::new().fg(Theme::TEXT_PRIMARY).bold(),
        ),
        Span::styled("  ", Style::new()),
        Span::styled(inv.role.as_str().to_string(), Style::new().fg(role_color)),
        Span::styled("  ", Style::new()),
        Span::styled(
            inv.status.as_str().to_string(),
            Style::new().fg(status_color),
        ),
    ]);

    let date_display = if inv.created_at.len() > 10 {
        inv.created_at[..10].to_string()
    } else {
        inv.created_at.clone()
    };

    let line2 = Line::from(vec![
        Span::styled("   ", Style::new()),
        Span::styled(
            format!("from @{}", inv.invited_by_nickname),
            Style::new().fg(Theme::TEXT_SECONDARY),
        ),
        Span::styled("  ", Style::new()),
        Span::styled(date_display, Style::new().fg(Color::DarkGray)),
    ]);

    let line3 = Line::raw("");

    ListItem::new(vec![line1, line2, line3])
}
