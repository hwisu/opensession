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

#[cfg(test)]
mod tests {
    use super::render;
    use crate::app::App;
    use opensession_api::{InvitationResponse, InvitationStatus, TeamRole};
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

    fn render_text(app: &mut App, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| render(frame, app, frame.area()))
            .expect("draw");
        buffer_to_string(terminal.backend().buffer())
    }

    fn invitation(status: InvitationStatus, role: TeamRole) -> InvitationResponse {
        InvitationResponse {
            id: "inv-1".to_string(),
            team_id: "team-1".to_string(),
            team_name: "alpha-team".to_string(),
            email: Some("member@example.com".to_string()),
            oauth_provider: None,
            oauth_provider_username: None,
            invited_by_nickname: "owner".to_string(),
            role,
            status,
            created_at: "2026-02-15T09:08:07Z".to_string(),
        }
    }

    #[test]
    fn loading_state_shows_loading_message() {
        let mut app = App::new(vec![]);
        app.invitations_loading = true;
        let text = render_text(&mut app, 100, 30);
        assert!(text.contains("Loading inbox"));
    }

    #[test]
    fn empty_state_shows_empty_hint() {
        let mut app = App::new(vec![]);
        let text = render_text(&mut app, 100, 30);
        assert!(text.contains("Inbox is empty"));
    }

    #[test]
    fn list_state_renders_team_role_status_and_sender() {
        let mut app = App::new(vec![]);
        app.invitations = vec![invitation(InvitationStatus::Pending, TeamRole::Admin)];
        app.invitations_list_state.select(Some(0));

        let text = render_text(&mut app, 120, 30);
        assert!(text.contains("Inbox (1)"));
        assert!(text.contains("alpha-team"));
        assert!(text.contains("admin"));
        assert!(text.contains("pending"));
        assert!(text.contains("from @owner"));
    }

    #[test]
    fn list_state_truncates_created_at_to_date() {
        let mut app = App::new(vec![]);
        app.invitations = vec![invitation(InvitationStatus::Accepted, TeamRole::Member)];
        app.invitations_list_state.select(Some(0));

        let text = render_text(&mut app, 120, 30);
        assert!(text.contains("2026-02-15"));
        assert!(!text.contains("09:08:07"));
    }

    #[test]
    fn list_state_renders_declined_status() {
        let mut app = App::new(vec![]);
        app.invitations = vec![invitation(InvitationStatus::Declined, TeamRole::Member)];
        app.invitations_list_state.select(Some(0));

        let text = render_text(&mut app, 120, 30);
        assert!(text.contains("declined"));
    }
}
