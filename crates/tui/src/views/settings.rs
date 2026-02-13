use crate::app::{App, SettingsSection};
use crate::config::{GitStorageMethod, SettingField, SettingItem, SETTINGS_LAYOUT};
use crate::theme::Theme;
use ratatui::prelude::*;
use ratatui::widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    match app.settings_section {
        SettingsSection::Profile => render_profile(frame, app, area),
        SettingsSection::Account => render_account(frame, app, area),
        SettingsSection::DaemonConfig => render_daemon_config(frame, app, area),
    }
}

// ── Profile section (read-only) ──────────────────────────────────────────

fn render_profile(frame: &mut Frame, app: &App, area: Rect) {
    let block = Theme::block_dim().padding(Theme::PADDING_CARD);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.profile_loading {
        let msg = Paragraph::new("Loading profile...").style(Style::new().fg(Theme::ACCENT_BLUE));
        frame.render_widget(msg, inner);
        return;
    }

    let mut lines = vec![
        Line::from(Span::styled(
            "── Profile ──",
            Style::new().fg(Theme::ACCENT_BLUE).bold(),
        )),
        Line::raw(""),
    ];

    if let Some(ref profile) = app.profile {
        let fields: Vec<(&str, String)> = vec![
            ("Nickname", profile.nickname.clone()),
            (
                "Email",
                profile.email.clone().unwrap_or_else(|| "-".to_string()),
            ),
            (
                "API Key",
                if profile.api_key.len() > 12 {
                    format!("{}...", &profile.api_key[..12])
                } else {
                    profile.api_key.clone()
                },
            ),
            (
                "OAuth",
                if profile.oauth_providers.is_empty() {
                    "None".to_string()
                } else {
                    profile
                        .oauth_providers
                        .iter()
                        .map(|p| p.display_name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                },
            ),
            (
                "GitHub",
                profile
                    .github_username
                    .clone()
                    .unwrap_or_else(|| "-".to_string()),
            ),
        ];

        for (label, value) in &fields {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {:<16}", label),
                    Style::new().fg(Theme::TEXT_SECONDARY),
                ),
                Span::styled(value.clone(), Style::new().fg(Theme::TEXT_PRIMARY)),
            ]));
        }

        lines.push(Line::raw(""));
        let url_hint = if app.daemon_config.server.url.is_empty() {
            "  (read-only — edit via web UI)".to_string()
        } else {
            format!(
                "  (read-only — edit at {}/settings)",
                app.daemon_config.server.url
            )
        };
        lines.push(Line::from(Span::styled(
            url_hint,
            Style::new().fg(Theme::TEXT_HINT),
        )));
    } else if let Some(ref err) = app.profile_error {
        lines.push(Line::from(vec![
            Span::styled("  Status:  ", Style::new().fg(Theme::TEXT_SECONDARY)),
            Span::styled(
                format!("Failed to load profile: {}", err),
                Style::new().fg(Theme::ACCENT_RED),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  Server:  ", Style::new().fg(Theme::TEXT_SECONDARY)),
            Span::styled(
                &app.daemon_config.server.url,
                Style::new().fg(Theme::TEXT_PRIMARY),
            ),
        ]));
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "  Press 'r' to retry, or check API key in Config tab.",
            Style::new().fg(Theme::TEXT_HINT),
        )));
    } else if app.daemon_config.server.api_key.is_empty() {
        lines.push(Line::from(Span::styled(
            "  API key not set. Go to Config tab to configure.",
            Style::new().fg(Theme::TEXT_SECONDARY),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "  Press 'r' to load profile.",
            Style::new().fg(Theme::TEXT_SECONDARY),
        )));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

// ── Account section (API key, password change) ───────────────────────────

fn render_account(frame: &mut Frame, app: &App, area: Rect) {
    let block = Theme::block_dim().padding(Theme::PADDING_CARD);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Server URL display
    let display_url = app
        .daemon_config
        .server
        .url
        .trim_start_matches("https://")
        .trim_start_matches("http://");

    let mut lines = vec![
        Line::from(vec![
            Span::styled("  Server:         ", Style::new().fg(Theme::TEXT_SECONDARY)),
            Span::styled(display_url, Style::new().fg(Theme::ACCENT_BLUE)),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            "── API Key ──",
            Style::new().fg(Theme::ACCENT_BLUE).bold(),
        )),
        Line::raw(""),
    ];

    // Current API key (masked)
    let key_display = if app.daemon_config.server.api_key.is_empty() {
        "(not set)".to_string()
    } else {
        let key = &app.daemon_config.server.api_key;
        let visible = key.len().min(8);
        format!(
            "{}...{}",
            &key[..visible],
            &key[key.len().saturating_sub(4)..]
        )
    };

    lines.push(Line::from(vec![
        Span::styled("  API Key:        ", Style::new().fg(Theme::TEXT_SECONDARY)),
        Span::styled(key_display, Style::new().fg(Theme::TEXT_PRIMARY)),
    ]));
    lines.push(Line::from(Span::styled(
        "  Press 'r' to regenerate",
        Style::new().fg(Theme::TEXT_HINT),
    )));
    lines.push(Line::raw(""));

    // Password change form
    let has_oauth = app
        .profile
        .as_ref()
        .is_some_and(|p| !p.oauth_providers.is_empty());
    let pw_title = if has_oauth {
        "── Change Password (or set initial password) ──"
    } else {
        "── Change Password ──"
    };
    lines.push(Line::from(Span::styled(
        pw_title,
        Style::new().fg(Theme::ACCENT_BLUE).bold(),
    )));
    lines.push(Line::raw(""));

    let pw_fields = [
        (
            "Current Password",
            mask_password(&app.password_form.current),
        ),
        (
            "New Password",
            mask_password(&app.password_form.new_password),
        ),
        (
            "Confirm Password",
            mask_password(&app.password_form.confirm),
        ),
    ];

    for (i, (label, display)) in pw_fields.iter().enumerate() {
        let is_selected = app.settings_index == i;
        let is_editing = is_selected && app.password_form.editing;

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
            format!("{}|", "*".repeat(app.edit_buffer.len()))
        } else if display.is_empty() {
            "(empty)".to_string()
        } else {
            display.to_string()
        };

        let value_style = if is_editing {
            Style::new().fg(Theme::ACCENT_YELLOW)
        } else {
            Style::new().fg(Theme::FIELD_VALUE)
        };

        let bg = if is_selected {
            Style::new().bg(Theme::BG_SURFACE)
        } else {
            Style::new()
        };

        lines.push(
            Line::from(vec![
                Span::styled(format!(" {} ", pointer), pointer_style),
                Span::styled(format!("{:<22}", label), label_style),
                Span::styled(value_text, value_style),
            ])
            .style(bg),
        );

        // OAuth hint for Current Password field
        if i == 0 && is_selected && has_oauth {
            lines.push(Line::from(vec![
                Span::raw("     "),
                Span::styled(
                    "(leave empty if no password set)",
                    Style::new().fg(Theme::TEXT_HINT),
                ),
            ]));
        }
    }

    // Submit button
    let submit_selected = app.settings_index == 3;
    let submit_bg = if submit_selected {
        Style::new().bg(Theme::BG_SURFACE)
    } else {
        Style::new()
    };
    let submit_style = if submit_selected {
        Style::new().fg(Theme::ACCENT_BLUE).bold()
    } else {
        Style::new().fg(Theme::TEXT_HINT)
    };
    lines.push(Line::raw(""));
    lines.push(
        Line::from(vec![
            Span::styled(
                if submit_selected { " > " } else { "   " },
                if submit_selected {
                    Style::new().fg(Color::Cyan).bold()
                } else {
                    Style::new()
                },
            ),
            Span::styled("[Submit Password Change]", submit_style),
        ])
        .style(submit_bg),
    );

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

fn mask_password(s: &str) -> String {
    if s.is_empty() {
        String::new()
    } else {
        "*".repeat(s.len())
    }
}

// ── DaemonConfig section (existing settings) ─────────────────────────────

fn render_daemon_config(frame: &mut Frame, app: &App, area: Rect) {
    let body_block = Theme::block_dim().padding(Theme::PADDING_COMPACT);
    let inner = body_block.inner(area);
    frame.render_widget(body_block, area);

    let daemon_running = app.startup_status.daemon_pid.is_some();

    let mut lines = Vec::new();
    let mut selectable_idx: usize = 0;

    for item in SETTINGS_LAYOUT.iter() {
        match item {
            SettingItem::Header(title) => {
                if !lines.is_empty() {
                    lines.push(Line::raw(""));
                }

                // Check if this section should be grayed out when daemon is off
                let is_daemon_dependent = matches!(*title, "Daemon" | "Watchers");
                let header_style = if is_daemon_dependent && !daemon_running {
                    Style::new().fg(Theme::TEXT_DISABLED)
                } else {
                    Style::new().fg(Theme::ACCENT_BLUE).bold()
                };

                let mut header_spans = vec![Span::styled(format!("── {} ──", title), header_style)];

                // Show daemon status + start/stop hint
                if *title == "Daemon" {
                    if daemon_running {
                        header_spans.push(Span::styled(
                            "  [d: stop]",
                            Style::new().fg(Theme::ACCENT_RED),
                        ));
                    } else {
                        header_spans
                            .push(Span::styled("  (off)", Style::new().fg(Theme::TEXT_MUTED)));
                        header_spans.push(Span::styled(
                            "  [d: start]",
                            Style::new().fg(Theme::ACCENT_GREEN),
                        ));
                    }
                } else if is_daemon_dependent && !daemon_running {
                    header_spans.push(Span::styled(
                        "  (daemon off)",
                        Style::new().fg(Theme::TEXT_MUTED),
                    ));
                }

                lines.push(Line::from(header_spans));
                lines.push(Line::raw(""));
            }
            SettingItem::Field {
                field,
                label,
                description,
                dependency_hint,
            } => {
                let is_selected = selectable_idx == app.settings_index;
                let is_editing = is_selected && app.editing_field;

                let is_git_token_disabled = *field == SettingField::GitStorageToken
                    && app.daemon_config.git_storage.method == GitStorageMethod::None;
                let dimmed =
                    is_git_token_disabled || (dependency_hint.is_some() && !daemon_running);

                let pointer = if is_selected { "\u{25b8}" } else { " " };
                let pointer_style = if dimmed {
                    Style::new().fg(Theme::TEXT_DIMMER)
                } else if is_selected {
                    Style::new().fg(Color::Cyan).bold()
                } else {
                    Style::new().fg(Color::DarkGray)
                };

                let label_style = if dimmed {
                    Style::new().fg(Theme::TEXT_DISABLED)
                } else if is_selected {
                    Style::new().fg(Theme::TEXT_PRIMARY).bold()
                } else {
                    Style::new().fg(Theme::TEXT_SECONDARY)
                };

                let value_text = if is_editing {
                    format!("{}|", app.edit_buffer)
                } else {
                    field.display_value(&app.daemon_config)
                };

                let value_style = if dimmed {
                    Style::new().fg(Theme::TEXT_DIMMER)
                } else if is_editing {
                    Style::new().fg(Theme::ACCENT_YELLOW)
                } else if field.is_toggle() {
                    let on = matches!(field.display_value(&app.daemon_config).as_str(), "ON");
                    let s = if on {
                        Style::new().fg(Theme::TOGGLE_ON)
                    } else {
                        Style::new().fg(Theme::TOGGLE_OFF)
                    };
                    if is_selected {
                        s.underlined()
                    } else {
                        s
                    }
                } else if field.is_enum() {
                    let s = Style::new().fg(Theme::ACCENT_PURPLE);
                    if is_selected {
                        s.underlined()
                    } else {
                        s
                    }
                } else if is_selected {
                    Style::new().fg(Theme::TEXT_PRIMARY).underlined()
                } else {
                    Style::new().fg(Theme::FIELD_VALUE)
                };

                let bg = if is_selected {
                    Style::new().bg(Theme::BG_SURFACE)
                } else {
                    Style::new()
                };

                // Type hint for editing
                let type_hint = if is_selected && !is_editing && !dimmed {
                    if field.is_toggle() {
                        Span::styled("  [Enter: toggle]", Style::new().fg(Theme::TEXT_HINT))
                    } else if field.is_enum() {
                        Span::styled("  [Enter: cycle]", Style::new().fg(Theme::TEXT_HINT))
                    } else {
                        Span::styled("  [Enter: edit]", Style::new().fg(Theme::TEXT_HINT))
                    }
                } else {
                    Span::raw("")
                };

                lines.push(
                    Line::from(vec![
                        Span::styled(format!(" {} ", pointer), pointer_style),
                        Span::styled(format!("{:<22}", label), label_style),
                        Span::styled(value_text, value_style),
                        type_hint,
                    ])
                    .style(bg),
                );

                // Description / dependency hint below the field
                if is_selected || dimmed {
                    let desc_text = if dimmed {
                        dependency_hint.unwrap_or(description)
                    } else {
                        description
                    };
                    let desc_style = if dimmed {
                        Style::new().fg(Theme::ACCENT_YELLOW)
                    } else {
                        Style::new().fg(Theme::TEXT_HINT)
                    };
                    lines.push(Line::from(vec![
                        Span::raw("     "),
                        Span::styled(desc_text, desc_style),
                    ]));

                    // Show detailed descriptions for Git Storage Method
                    if is_selected && *field == SettingField::GitStorageMethod {
                        let detail_style = Style::new().fg(Theme::TEXT_MUTED);
                        for detail in [
                            "\u{00b7} Platform API \u{2014} Push via GitHub/GitLab REST API (needs token)",
                            "\u{00b7} Native       \u{2014} Write git objects directly (local git required)",
                            "\u{00b7} None         \u{2014} Server-only, no git backup",
                        ] {
                            lines.push(Line::from(vec![
                                Span::raw("     "),
                                Span::styled(detail, detail_style),
                            ]));
                        }
                    }

                    // Show exclude patterns when Privacy fields are selected
                    if is_selected
                        && matches!(field, SettingField::StripPaths | SettingField::StripEnvVars)
                    {
                        let patterns = app.daemon_config.privacy.exclude_patterns.join(", ");
                        if !patterns.is_empty() {
                            lines.push(Line::from(vec![
                                Span::raw("     "),
                                Span::styled(
                                    format!("Exclude patterns: {}", patterns),
                                    Style::new().fg(Theme::TEXT_MUTED),
                                ),
                            ]));
                        }
                    }
                }

                selectable_idx += 1;
            }
        }
    }

    // Calculate scroll
    let visible_height = inner.height as usize;
    let current_line = find_selected_line(&lines, app.settings_index);
    let scroll = if current_line >= visible_height {
        current_line.saturating_sub(visible_height / 2)
    } else {
        0
    };

    let paragraph = Paragraph::new(lines.clone()).scroll((scroll as u16, 0));
    frame.render_widget(paragraph, inner);

    // Scrollbar
    let total_lines = lines.len();
    if total_lines > visible_height {
        let mut scrollbar_state = ScrollbarState::new(total_lines).position(scroll);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .thumb_style(Style::new().fg(Theme::TEXT_MUTED));
        frame.render_stateful_widget(
            scrollbar,
            area.inner(Margin {
                vertical: 1,
                horizontal: 0,
            }),
            &mut scrollbar_state,
        );
    }
}

/// Find the approximate line index for the nth selectable field.
fn find_selected_line(lines: &[Line], selected_idx: usize) -> usize {
    let mut sel_count = 0usize;
    for (line_idx, line) in lines.iter().enumerate() {
        // A selectable line starts with " > " or "   " and has a label
        let text = line.to_string();
        if text.starts_with(" > ")
            || (text.starts_with("   ")
                && text.len() > 5
                && !text.trim().is_empty()
                && !text.contains("──"))
        {
            if sel_count == selected_idx {
                return line_idx;
            }
            sel_count += 1;
        }
    }
    0
}
