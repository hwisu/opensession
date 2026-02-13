use crate::app::{App, TeamDetailFocus};
use crate::theme::Theme;
use ratatui::prelude::*;
use ratatui::widgets::{List, ListItem, Paragraph};

pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let [info_area, members_area, invite_area] = Layout::vertical([
        Constraint::Length(5),
        Constraint::Fill(1),
        Constraint::Length(5),
    ])
    .areas(area);

    render_info(frame, app, info_area);
    render_members(frame, app, members_area);
    render_invite(frame, app, invite_area);
}

fn render_info(frame: &mut Frame, app: &App, area: Rect) {
    let is_focused = matches!(app.team_detail_focus, TeamDetailFocus::Info);
    let block = if is_focused {
        Theme::block_accent()
    } else {
        Theme::block_dim()
    };

    let block = block.title(" Team Info ").padding(Theme::PADDING_COMPACT);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if let Some(ref detail) = app.team_detail {
        let visibility = if detail.team.is_public {
            "Public"
        } else {
            "Private"
        };
        let vis_color = if detail.team.is_public {
            Theme::ACCENT_GREEN
        } else {
            Theme::ACCENT_PURPLE
        };
        let desc = detail.team.description.as_deref().unwrap_or("-");

        let lines = vec![
            Line::from(vec![
                Span::styled(
                    &*detail.team.name,
                    Style::new().fg(Theme::TEXT_PRIMARY).bold(),
                ),
                Span::styled("  ", Style::new()),
                Span::styled(visibility, Style::new().fg(vis_color)),
                Span::styled("  ", Style::new()),
                Span::styled(
                    format!("{} members", detail.member_count),
                    Style::new().fg(Theme::TEXT_SECONDARY),
                ),
            ]),
            Line::from(Span::styled(
                desc.to_string(),
                Style::new().fg(Theme::TEXT_SECONDARY),
            )),
        ];
        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, inner);
    } else {
        let msg =
            Paragraph::new("Loading team details...").style(Style::new().fg(Theme::ACCENT_BLUE));
        frame.render_widget(msg, inner);
    }
}

fn render_members(frame: &mut Frame, app: &mut App, area: Rect) {
    let is_focused = matches!(app.team_detail_focus, TeamDetailFocus::Members);
    let block = if is_focused {
        Theme::block_accent()
    } else {
        Theme::block_dim()
    };

    if app.team_members.is_empty() {
        let block = block.title(" Members ").padding(Theme::PADDING_COMPACT);
        let msg = Paragraph::new("No members")
            .block(block)
            .style(Style::new().fg(Color::DarkGray));
        frame.render_widget(msg, area);
        return;
    }

    let items: Vec<ListItem> = app
        .team_members
        .iter()
        .map(|m| {
            let role_color = if m.role == opensession_api::TeamRole::Admin {
                Theme::ACCENT_YELLOW
            } else {
                Theme::TEXT_SECONDARY
            };
            let joined = if m.joined_at.len() > 10 {
                &m.joined_at[..10]
            } else {
                &m.joined_at
            };
            ListItem::new(Line::from(vec![
                Span::styled(&*m.nickname, Style::new().fg(Theme::TEXT_PRIMARY)),
                Span::styled("  ", Style::new()),
                Span::styled(m.role.as_str().to_string(), Style::new().fg(role_color)),
                Span::styled("  ", Style::new()),
                Span::styled(joined.to_string(), Style::new().fg(Color::DarkGray)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(block.title(format!(" Members ({}) ", app.team_members.len())))
        .highlight_style(
            Style::new()
                .bg(Theme::BG_SURFACE)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(" > ")
        .highlight_spacing(ratatui::widgets::HighlightSpacing::Always);

    frame.render_stateful_widget(list, area, &mut app.team_members_list_state);
}

fn render_invite(frame: &mut Frame, app: &App, area: Rect) {
    let is_focused = matches!(app.team_detail_focus, TeamDetailFocus::Invite);
    let block = if is_focused {
        Theme::block_accent()
    } else {
        Theme::block_dim()
    };

    let block = block
        .title(" Invite Member ")
        .padding(Theme::PADDING_COMPACT);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let email_display = if app.invite_editing {
        format!("Email: {}|", app.invite_email)
    } else if app.invite_email.is_empty() {
        "Email: (press Enter to type)".to_string()
    } else {
        format!("Email: {}", app.invite_email)
    };

    let email_style = if app.invite_editing {
        Style::new().fg(Theme::ACCENT_YELLOW)
    } else if is_focused {
        Style::new().fg(Theme::TEXT_PRIMARY)
    } else {
        Style::new().fg(Theme::FIELD_VALUE)
    };

    let lines = vec![
        Line::from(Span::styled(email_display, email_style)),
        Line::from(Span::styled(
            if is_focused {
                "Enter: type/send  Esc: cancel"
            } else {
                ""
            },
            Style::new().fg(Theme::TEXT_HINT),
        )),
    ];

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}
