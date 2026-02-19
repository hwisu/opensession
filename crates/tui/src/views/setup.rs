use crate::app::{App, SetupScenario, SetupStep};
use crate::config::SettingField;
use crate::theme::Theme;
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

const SETUP_FIELDS: [(SettingField, &str); 3] = [
    (SettingField::ServerUrl, "Server URL"),
    (SettingField::ApiKey, "API Key (personal)"),
    (SettingField::Nickname, "Handle"),
];

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    if app.setup_step == SetupStep::Scenario {
        render_scenario_picker(frame, app, area);
        return;
    }

    let [title_area, form_area, hint_area] = Layout::vertical([
        Constraint::Length(5),
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
            "Finish the required setup for your selected mode.",
            Style::new().fg(Color::DarkGray),
        )),
    ])
    .block(title_block);
    frame.render_widget(title, title_area);

    // ── Form ─────────────────────────────────────────────────────────
    render_apikey_form(frame, app, form_area);

    // ── Hints ─────────────────────────────────────────────────────────
    let key_style = Style::new().fg(Theme::TEXT_KEY);
    let desc_style = Style::new().fg(Theme::TEXT_KEY_DESC);
    let mut hint_lines = vec![Line::raw("")];

    hint_lines.push(Line::from(vec![
        Span::styled(" j/k ", key_style),
        Span::styled("navigate  ", desc_style),
        Span::styled("Enter ", key_style),
        Span::styled("edit  ", desc_style),
        Span::styled("s ", key_style),
        Span::styled("save+continue  ", desc_style),
        Span::styled("Esc ", key_style),
        Span::styled("skip", desc_style),
    ]));

    hint_lines.push(Line::raw(""));
    hint_lines.push(Line::from(Span::styled(
        "Skip now? Configure later in Settings > Web Share.",
        Style::new().fg(Theme::TEXT_HINT),
    )));
    let settings_url = format!(
        "{}/settings",
        app.daemon_config.server.url.trim_end_matches('/')
    );
    hint_lines.push(Line::from(Span::styled(
        format!("Personal API key: {settings_url}"),
        Style::new().fg(Theme::TEXT_HINT),
    )));
    hint_lines.push(Line::from(Span::styled(
        "Issue a personal API key in web settings, then paste it here.",
        Style::new().fg(Theme::TEXT_HINT),
    )));
    if app.setup_scenario == Some(SetupScenario::Public) {
        hint_lines.push(Line::from(Span::styled(
            "Public mode requires Git setup for personal uploads.",
            Style::new().fg(Theme::TEXT_HINT),
        )));
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

fn render_scenario_picker(frame: &mut Frame, app: &App, area: Rect) {
    let [title_area, list_area, hint_area] = Layout::vertical([
        Constraint::Length(6),
        Constraint::Length(12),
        Constraint::Fill(1),
    ])
    .areas(area);

    let title_block = Theme::block().padding(ratatui::widgets::Padding::new(2, 2, 0, 0));
    let title = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("opensession", Style::new().fg(Theme::ACCENT_ORANGE).bold()),
            Span::styled(
                " — Initial Setup",
                Style::new().fg(Theme::TEXT_PRIMARY).bold(),
            ),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            "How do you want to use OpenSession?",
            Style::new().fg(Theme::TEXT_PRIMARY).bold(),
        )),
    ])
    .block(title_block);
    frame.render_widget(title, title_area);

    let options = [
        (
            SetupScenario::Local,
            "Local mode",
            "Browse local sessions only. No cloud setup required.",
        ),
        (
            SetupScenario::Public,
            "Public mode",
            "Auto-publish to personal public feed (Git setup required).",
        ),
    ];

    let list_block = Theme::block_dim()
        .title(" Choose a scenario ")
        .padding(ratatui::widgets::Padding::new(2, 2, 0, 0));
    let inner = list_block.inner(list_area);
    frame.render_widget(list_block, list_area);

    let mut lines = Vec::new();
    for (idx, (_scenario, label, desc)) in options.iter().enumerate() {
        let selected = idx == app.setup_scenario_index;
        let pointer = if selected { ">" } else { " " };
        let pointer_style = if selected {
            Style::new().fg(Color::Cyan).bold()
        } else {
            Style::new().fg(Color::DarkGray)
        };
        let label_style = if selected {
            Style::new().fg(Theme::TEXT_PRIMARY).bold()
        } else {
            Style::new().fg(Theme::TEXT_SECONDARY)
        };
        let desc_style = if selected {
            Style::new().fg(Theme::TEXT_PRIMARY)
        } else {
            Style::new().fg(Theme::TEXT_HINT)
        };
        let bg = if selected {
            Style::new().bg(Theme::BG_SURFACE)
        } else {
            Style::new()
        };

        lines.push(
            Line::from(vec![
                Span::styled(format!(" {} ", pointer), pointer_style),
                Span::styled(*label, label_style),
            ])
            .style(bg),
        );
        lines.push(Line::from(vec![Span::raw("    "), Span::styled(*desc, desc_style)]).style(bg));
        lines.push(Line::raw(""));
    }

    frame.render_widget(Paragraph::new(lines), inner);

    let key_style = Style::new().fg(Theme::TEXT_KEY);
    let desc_style = Style::new().fg(Theme::TEXT_KEY_DESC);
    let hints = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(" j/k ", key_style),
            Span::styled("navigate  ", desc_style),
            Span::styled("Enter ", key_style),
            Span::styled("continue  ", desc_style),
            Span::styled("Esc ", key_style),
            Span::styled("skip", desc_style),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            "You can skip this now and configure it later in Settings > Web Share.",
            Style::new().fg(Theme::TEXT_HINT),
        )),
        Line::from(Span::styled(
            "Config file: ~/.config/opensession/opensession.toml",
            Style::new().fg(Theme::TEXT_HINT),
        )),
    ]);
    frame.render_widget(hints, hint_area);
}

fn render_apikey_form(frame: &mut Frame, app: &App, area: Rect) {
    let form_block = Theme::block_dim()
        .title(" Configuration ")
        .padding(ratatui::widgets::Padding::new(2, 2, 0, 0));
    let inner = form_block.inner(area);
    frame.render_widget(form_block, area);

    let mut lines = Vec::new();
    let fields: &[(SettingField, &str)] = &SETUP_FIELDS;
    for (i, (field, label)) in fields.iter().enumerate() {
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

#[cfg(test)]
mod tests {
    use super::render;
    use crate::app::{App, FlashLevel, SetupScenario, SetupStep};
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

    fn render_text(app: &App, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| render(frame, app, frame.area()))
            .expect("draw");
        buffer_to_string(terminal.backend().buffer())
    }

    #[test]
    fn scenario_step_shows_all_setup_options() {
        let mut app = App::new(vec![]);
        app.setup_step = SetupStep::Scenario;
        let text = render_text(&app, 120, 40);
        assert!(text.contains("How do you want to use OpenSession?"));
        assert!(text.contains("Local mode"));
        assert!(text.contains("Public mode"));
    }

    #[test]
    fn apikey_setup_renders_required_fields() {
        let mut app = App::new(vec![]);
        app.setup_step = SetupStep::Configure;
        app.setup_scenario = Some(SetupScenario::Public);

        let text = render_text(&app, 120, 40);
        assert!(text.contains("Server URL"));
        assert!(text.contains("Handle"));
        assert!(text.contains("Issue a personal API key in web settings"));
        assert!(!text.contains("Email Login"));
    }

    #[test]
    fn apikey_public_scenario_shows_git_hint() {
        let mut app = App::new(vec![]);
        app.setup_step = SetupStep::Configure;
        app.setup_scenario = Some(SetupScenario::Public);

        let text = render_text(&app, 120, 40);
        assert!(text.contains("Public mode requires Git setup"));
    }

    #[test]
    fn configure_step_renders_flash_message() {
        let mut app = App::new(vec![]);
        app.setup_step = SetupStep::Configure;
        app.flash_message = Some(("saved successfully".to_string(), FlashLevel::Success));

        let text = render_text(&app, 120, 40);
        assert!(text.contains("saved successfully"));
    }
}
