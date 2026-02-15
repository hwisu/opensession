use crate::app::App;
use crate::theme::Theme;
use opensession_api::TeamResponse;
use ratatui::prelude::*;
use ratatui::widgets::{List, ListItem, Paragraph};

pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.is_local_mode() {
        let block = Theme::block_dim()
            .title(" Collaboration ")
            .padding(Theme::PADDING_CARD);
        let msg = Paragraph::new(
            "Collaboration is unavailable in local mode.\nConfigure server/team in Settings > Workspace.",
        )
        .block(block)
        .style(Style::new().fg(Theme::TEXT_MUTED));
        frame.render_widget(msg, area);
        return;
    }

    if app.teams_loading {
        let block = Theme::block_dim()
            .title(" Collaboration ")
            .padding(Theme::PADDING_CARD);
        let msg = Paragraph::new("Loading teams...")
            .block(block)
            .style(Style::new().fg(Theme::ACCENT_BLUE));
        frame.render_widget(msg, area);
        return;
    }

    if app.teams.is_empty() {
        let block = Theme::block_dim()
            .title(" Collaboration ")
            .padding(Theme::PADDING_CARD);
        let msg = Paragraph::new("No teams yet. Press 'n' to create one. Press 'i' for inbox.")
            .block(block)
            .style(Style::new().fg(Color::DarkGray));
        frame.render_widget(msg, area);
        return;
    }

    let items: Vec<ListItem> = app
        .teams
        .iter()
        .map(|team| team_to_list_item(team))
        .collect();

    let list = List::new(items)
        .block(Theme::block_dim().title(format!(" Teams ({}) ", app.teams.len())))
        .highlight_style(
            Style::new()
                .bg(Theme::BG_SURFACE)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(" > ")
        .highlight_spacing(ratatui::widgets::HighlightSpacing::Always);

    frame.render_stateful_widget(list, area, &mut app.teams_list_state);
}

fn team_to_list_item(team: &TeamResponse) -> ListItem<'static> {
    let visibility = if team.is_public { "public" } else { "private" };
    let vis_color = if team.is_public {
        Theme::ACCENT_GREEN
    } else {
        Theme::ACCENT_PURPLE
    };

    let line1 = Line::from(vec![
        Span::styled(
            team.name.clone(),
            Style::new().fg(Theme::TEXT_PRIMARY).bold(),
        ),
        Span::styled("  ", Style::new()),
        Span::styled(visibility, Style::new().fg(vis_color)),
    ]);

    let date_display = if team.created_at.len() > 10 {
        &team.created_at[..10]
    } else {
        &team.created_at
    };
    let desc = team
        .description
        .as_deref()
        .unwrap_or("")
        .chars()
        .take(50)
        .collect::<String>();

    let mut line2_spans = vec![
        Span::styled("   ", Style::new()),
        Span::styled(date_display.to_string(), Style::new().fg(Color::DarkGray)),
    ];
    if !desc.is_empty() {
        line2_spans.push(Span::styled("  ", Style::new()));
        line2_spans.push(Span::styled(desc, Style::new().fg(Theme::TEXT_SECONDARY)));
    }
    let line2 = Line::from(line2_spans);

    let line3 = Line::raw("");

    ListItem::new(vec![line1, line2, line3])
}

#[cfg(test)]
mod tests {
    use super::render;
    use crate::app::{App, ConnectionContext};
    use opensession_api::TeamResponse;
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

    fn team(name: &str, desc: Option<&str>) -> TeamResponse {
        TeamResponse {
            id: format!("team-{name}"),
            name: name.to_string(),
            description: desc.map(str::to_string),
            is_public: true,
            created_by: "u1".to_string(),
            created_at: "2026-02-15T12:34:56Z".to_string(),
        }
    }

    #[test]
    fn local_mode_shows_collaboration_unavailable_message() {
        let mut app = App::new(vec![]);
        let text = render_text(&mut app, 100, 30);
        assert!(text.contains("Collaboration is unavailable in local mode"));
    }

    #[test]
    fn loading_state_shows_loading_message() {
        let mut app = App::new(vec![]);
        app.connection_ctx = ConnectionContext::CloudTeam {
            team_name: "demo".to_string(),
        };
        app.teams_loading = true;
        let text = render_text(&mut app, 100, 30);
        assert!(text.contains("Loading teams"));
    }

    #[test]
    fn empty_state_shows_create_hint() {
        let mut app = App::new(vec![]);
        app.connection_ctx = ConnectionContext::CloudTeam {
            team_name: "demo".to_string(),
        };
        let text = render_text(&mut app, 100, 30);
        assert!(text.contains("No teams yet"));
        assert!(text.contains("Press 'n' to create one"));
    }

    #[test]
    fn list_state_renders_team_name_visibility_and_date() {
        let mut app = App::new(vec![]);
        app.connection_ctx = ConnectionContext::CloudTeam {
            team_name: "demo".to_string(),
        };
        app.teams = vec![team("alpha", Some("Team Alpha description"))];
        app.teams_list_state.select(Some(0));

        let text = render_text(&mut app, 100, 30);
        assert!(text.contains("Teams (1)"));
        assert!(text.contains("alpha"));
        assert!(text.contains("public"));
        assert!(text.contains("2026-02-15"));
        assert!(!text.contains("12:34:56"));
    }

    #[test]
    fn list_state_truncates_long_description() {
        let long_desc =
            "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZLONG-TAIL-TRUNCATE";
        let mut app = App::new(vec![]);
        app.connection_ctx = ConnectionContext::CloudTeam {
            team_name: "demo".to_string(),
        };
        app.teams = vec![team("alpha", Some(long_desc))];
        app.teams_list_state.select(Some(0));

        let text = render_text(&mut app, 120, 30);
        let expected = long_desc.chars().take(50).collect::<String>();
        assert!(text.contains(&expected));
        assert!(!text.contains(long_desc));
    }
}
