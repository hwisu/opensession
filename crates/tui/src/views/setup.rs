use crate::app::{App, SetupMode};
use crate::config::SettingField;
use crate::theme::Theme;
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

const SETUP_FIELDS: [SettingField; 4] = [
    SettingField::ServerUrl,
    SettingField::ApiKey,
    SettingField::TeamId,
    SettingField::Nickname,
];

const FIELD_LABELS: [&str; 4] = ["Server URL", "API Key", "Team ID", "Nickname"];

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let [title_area, tab_area, form_area, hint_area] = Layout::vertical([
        Constraint::Length(5),
        Constraint::Length(1),
        Constraint::Length(14),
        Constraint::Fill(1),
    ])
    .areas(area);

    // ── Title ─────────────────────────────────────────────────────────
    let title_block = Theme::block().padding(ratatui::widgets::Padding::new(2, 2, 0, 0));
    let title = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("opensession", Style::new().fg(Theme::ACCENT_ORANGE).bold()),
            Span::styled(
                " — Initial Setup",
                Style::new().fg(Theme::TEXT_PRIMARY).bold(),
            ),
        ]),
        Line::from(Span::styled(
            "Configure your server connection to get started.",
            Style::new().fg(Color::DarkGray),
        )),
    ])
    .block(title_block);
    frame.render_widget(title, title_area);

    // ── Tab bar ──────────────────────────────────────────────────────
    let apikey_style = if app.setup_mode == SetupMode::ApiKey {
        Style::new().fg(Color::Black).bg(Theme::ACCENT_BLUE).bold()
    } else {
        Style::new().fg(Theme::TEXT_SECONDARY)
    };
    let login_style = if app.setup_mode == SetupMode::Login {
        Style::new().fg(Color::Black).bg(Theme::ACCENT_BLUE).bold()
    } else {
        Style::new().fg(Theme::TEXT_SECONDARY)
    };
    let tab_line = Line::from(vec![
        Span::raw("  "),
        Span::styled(" API Key ", apikey_style),
        Span::raw("  "),
        Span::styled(" Email Login ", login_style),
        Span::raw("  "),
        Span::styled("(Tab to switch)", Style::new().fg(Color::DarkGray)),
    ]);
    frame.render_widget(Paragraph::new(tab_line), tab_area);

    // ── Form ─────────────────────────────────────────────────────────
    match app.setup_mode {
        SetupMode::ApiKey => render_apikey_form(frame, app, form_area),
        SetupMode::Login => render_login_form(frame, app, form_area),
    }

    // ── Hints ─────────────────────────────────────────────────────────
    let key_style = Style::new().fg(Theme::TEXT_KEY);
    let desc_style = Style::new().fg(Theme::TEXT_KEY_DESC);
    let mut hint_lines = vec![Line::raw("")];

    match app.setup_mode {
        SetupMode::ApiKey => {
            hint_lines.push(Line::from(vec![
                Span::styled(" j/k ", key_style),
                Span::styled("navigate  ", desc_style),
                Span::styled("Enter ", key_style),
                Span::styled("edit  ", desc_style),
                Span::styled("s ", key_style),
                Span::styled("save  ", desc_style),
                Span::styled("Tab ", key_style),
                Span::styled("switch  ", desc_style),
                Span::styled("Esc ", key_style),
                Span::styled("skip", desc_style),
            ]));
        }
        SetupMode::Login => {
            hint_lines.push(Line::from(vec![
                Span::styled(" j/k ", key_style),
                Span::styled("navigate  ", desc_style),
                Span::styled("Enter ", key_style),
                Span::styled("edit  ", desc_style),
                Span::styled("l ", key_style),
                Span::styled("login  ", desc_style),
                Span::styled("Tab ", key_style),
                Span::styled("switch  ", desc_style),
                Span::styled("Esc ", key_style),
                Span::styled("skip", desc_style),
            ]));
        }
    }

    // Flash message
    if let Some((ref msg, level)) = app.flash_message {
        use crate::app::FlashLevel;
        let color = match level {
            FlashLevel::Success => Theme::ACCENT_GREEN,
            FlashLevel::Error => Theme::ACCENT_RED,
            FlashLevel::Info => Theme::ACCENT_BLUE,
        };
        hint_lines.push(Line::raw(""));
        hint_lines.push(Line::from(Span::styled(
            format!("  {msg}"),
            Style::new().fg(color),
        )));
    }

    let hints = Paragraph::new(hint_lines);
    frame.render_widget(hints, hint_area);
}

fn render_apikey_form(frame: &mut Frame, app: &App, area: Rect) {
    let form_block = Theme::block_dim()
        .title(" Configuration ")
        .padding(ratatui::widgets::Padding::new(2, 2, 0, 0));
    let inner = form_block.inner(area);
    frame.render_widget(form_block, area);

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
            Style::new().fg(Theme::TEXT_PRIMARY).bold()
        } else {
            Style::new().fg(Theme::TEXT_SECONDARY)
        };

        let value_text = if is_editing {
            format!("{}|", app.edit_buffer)
        } else {
            field.display_value(&app.daemon_config)
        };

        let value_style = if is_editing {
            Style::new().fg(Theme::ACCENT_YELLOW)
        } else if is_selected {
            Style::new().fg(Theme::TEXT_PRIMARY)
        } else {
            Style::new().fg(Theme::FIELD_VALUE)
        };

        // Label line
        lines.push(Line::from(vec![
            Span::styled(format!(" {} ", pointer), pointer_style),
            Span::styled(format!("{:<14}", label), label_style),
        ]));

        // Value line (indented)
        let bg = if is_selected {
            Style::new().bg(Theme::BG_SURFACE)
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
}

fn render_login_form(frame: &mut Frame, app: &App, area: Rect) {
    let form_block = Theme::block_dim()
        .title(" Email Login ")
        .padding(ratatui::widgets::Padding::new(2, 2, 0, 0));
    let inner = form_block.inner(area);
    frame.render_widget(form_block, area);

    let login = &app.login_state;
    let fields: [(&str, &str, bool); 2] = [
        ("Email", &login.email, false),
        ("Password", &login.password, true),
    ];

    let mut lines = Vec::new();
    for (i, (label, value, is_password)) in fields.iter().enumerate() {
        let is_selected = i == login.field_index;
        let is_editing = is_selected && login.editing;

        let pointer = if is_selected { ">" } else { " " };
        let pointer_style = if is_selected {
            Style::new().fg(Color::Cyan).bold()
        } else {
            Style::new().fg(Color::DarkGray)
        };

        let label_style = if is_selected {
            Style::new().fg(Theme::TEXT_PRIMARY).bold()
        } else {
            Style::new().fg(Theme::TEXT_SECONDARY)
        };

        let display_value = if is_editing {
            if *is_password {
                format!("{}|", "*".repeat(app.edit_buffer.len()))
            } else {
                format!("{}|", app.edit_buffer)
            }
        } else if value.is_empty() {
            "(not set)".to_string()
        } else if *is_password {
            "*".repeat(value.len())
        } else {
            value.to_string()
        };

        let value_style = if is_editing {
            Style::new().fg(Theme::ACCENT_YELLOW)
        } else if is_selected {
            Style::new().fg(Theme::TEXT_PRIMARY)
        } else {
            Style::new().fg(Theme::FIELD_VALUE)
        };

        // Label line
        lines.push(Line::from(vec![
            Span::styled(format!(" {} ", pointer), pointer_style),
            Span::styled(format!("{:<14}", label), label_style),
        ]));

        // Value line
        let bg = if is_selected {
            Style::new().bg(Theme::BG_SURFACE)
        } else {
            Style::new()
        };
        lines.push(
            Line::from(vec![
                Span::raw("     "),
                Span::styled(display_value, value_style),
            ])
            .style(bg),
        );

        lines.push(Line::raw(""));
    }

    // Status message
    if let Some(ref status) = login.status {
        lines.push(Line::raw(""));
        let color = if login.loading {
            Theme::ACCENT_BLUE
        } else if status.starts_with("Error") || status.starts_with("Failed") {
            Theme::ACCENT_RED
        } else {
            Theme::ACCENT_GREEN
        };
        lines.push(Line::from(Span::styled(
            format!("  {status}"),
            Style::new().fg(color),
        )));
    }

    let form_paragraph = Paragraph::new(lines);
    frame.render_widget(form_paragraph, inner);
}
