use crate::app::App;
use crate::config::SettingField;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Padding, Paragraph};

const SETUP_FIELDS: [SettingField; 4] = [
    SettingField::ServerUrl,
    SettingField::ApiKey,
    SettingField::TeamId,
    SettingField::Nickname,
];

const FIELD_LABELS: [&str; 4] = ["Server URL", "API Key", "Team ID", "Nickname"];

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let [title_area, form_area, hint_area] = Layout::vertical([
        Constraint::Length(5),
        Constraint::Length(SETUP_FIELDS.len() as u16 * 3 + 2),
        Constraint::Fill(1),
    ])
    .areas(area);

    // ── Title ─────────────────────────────────────────────────────────
    let title_block = Block::bordered()
        .border_style(Style::new().fg(Color::Rgb(60, 65, 80)))
        .padding(Padding::new(2, 2, 0, 0));
    let title = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                "opensession",
                Style::new().fg(Color::Rgb(217, 119, 80)).bold(),
            ),
            Span::styled(
                " — Initial Setup",
                Style::new().fg(Color::White).bold(),
            ),
        ]),
        Line::from(Span::styled(
            "Configure your server connection to get started.",
            Style::new().fg(Color::DarkGray),
        )),
    ])
    .block(title_block);
    frame.render_widget(title, title_area);

    // ── Form fields ───────────────────────────────────────────────────
    let form_block = Block::bordered()
        .title(" Configuration ")
        .border_style(Style::new().fg(Color::DarkGray))
        .padding(Padding::new(2, 2, 0, 0));
    let inner = form_block.inner(form_area);
    frame.render_widget(form_block, form_area);

    let mut lines = Vec::new();
    for (i, (&field, &label)) in SETUP_FIELDS.iter().zip(FIELD_LABELS.iter()).enumerate() {
        let is_selected = i == app.settings_index;
        let is_editing = is_selected && app.editing_field;

        let pointer = if is_selected { ">" } else { " " };
        let pointer_style = if is_selected {
            Style::new().fg(Color::Cyan).bold()
        } else {
            Style::new().fg(Color::DarkGray)
        };

        let label_style = if is_selected {
            Style::new().fg(Color::White).bold()
        } else {
            Style::new().fg(Color::Rgb(140, 145, 160))
        };

        let value_text = if is_editing {
            format!("{}|", app.edit_buffer)
        } else {
            field.display_value(&app.daemon_config)
        };

        let value_style = if is_editing {
            Style::new().fg(Color::Rgb(220, 180, 60))
        } else if is_selected {
            Style::new().fg(Color::White)
        } else {
            Style::new().fg(Color::Rgb(100, 105, 120))
        };

        // Label line
        lines.push(Line::from(vec![
            Span::styled(format!(" {} ", pointer), pointer_style),
            Span::styled(format!("{:<14}", label), label_style),
        ]));

        // Value line (indented)
        let bg = if is_selected {
            Style::new().bg(Color::Rgb(30, 35, 50))
        } else {
            Style::new()
        };
        lines.push(
            Line::from(vec![
                Span::raw("     "),
                Span::styled(value_text, value_style),
            ])
            .style(bg),
        );

        // Spacer
        lines.push(Line::raw(""));
    }

    let form_paragraph = Paragraph::new(lines);
    frame.render_widget(form_paragraph, inner);

    // ── Hints ─────────────────────────────────────────────────────────
    let mut hint_lines = vec![
        Line::raw(""),
        Line::from(vec![
            Span::styled(" j/k ", Style::new().fg(Color::Rgb(140, 145, 160))),
            Span::styled("navigate  ", Style::new().fg(Color::DarkGray)),
            Span::styled("Enter ", Style::new().fg(Color::Rgb(140, 145, 160))),
            Span::styled("edit  ", Style::new().fg(Color::DarkGray)),
            Span::styled("s ", Style::new().fg(Color::Rgb(140, 145, 160))),
            Span::styled("save  ", Style::new().fg(Color::DarkGray)),
            Span::styled("Esc ", Style::new().fg(Color::Rgb(140, 145, 160))),
            Span::styled("skip", Style::new().fg(Color::DarkGray)),
        ]),
    ];

    // Flash message
    if let Some(ref msg) = app.flash_message {
        hint_lines.push(Line::raw(""));
        hint_lines.push(Line::from(Span::styled(
            format!("  {msg}"),
            Style::new().fg(Color::Rgb(80, 200, 120)),
        )));
    }

    let hints = Paragraph::new(hint_lines);
    frame.render_widget(hints, hint_area);
}
