use crate::app::App;
use crate::theme::Theme;
use opensession_api::TeamResponse;
use ratatui::prelude::*;
use ratatui::widgets::{List, ListItem, Paragraph};

pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.teams_loading {
        let block = Theme::block_dim()
            .title(" Teams ")
            .padding(Theme::PADDING_CARD);
        let msg = Paragraph::new("Loading teams...")
            .block(block)
            .style(Style::new().fg(Theme::ACCENT_BLUE));
        frame.render_widget(msg, area);
        return;
    }

    if app.teams.is_empty() {
        let block = Theme::block_dim()
            .title(" Teams ")
            .padding(Theme::PADDING_CARD);
        let msg = Paragraph::new("No teams yet. Press 'n' to create one.")
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
