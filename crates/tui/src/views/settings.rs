use crate::app::App;
use crate::config::{self, SettingField, SettingItem, SettingsGroup};
use crate::theme::Theme;
use ratatui::prelude::*;
use ratatui::widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};

const SESSION_STORAGE_METHOD_DETAILS: [&str; 3] = [
    "· Git-Native (Branch Based) — Store canonical session snapshots as local git branch objects",
    "· SQLite                    — Keep local index/cache metadata for fast local queries",
    "· NOTE                      — SQLite is an index/cache layer, not canonical body storage",
];

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    if let Some(group) = app.settings_section.group() {
        render_daemon_config(frame, app, area, group, app.settings_section.panel_title());
    } else {
        render_account(frame, app, area);
    }
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
        Line::from(Span::styled(
            "── Account Profile ──",
            Style::new().fg(Theme::ACCENT_BLUE).bold(),
        )),
        Line::raw(""),
    ];

    if app.profile_loading {
        lines.push(Line::from(Span::styled(
            "  Loading profile...",
            Style::new().fg(Theme::ACCENT_BLUE),
        )));
    } else if let Some(ref profile) = app.profile {
        let oauth = if profile.oauth_providers.is_empty() {
            "None".to_string()
        } else {
            profile
                .oauth_providers
                .iter()
                .map(|p| p.display_name.clone())
                .collect::<Vec<_>>()
                .join(", ")
        };
        lines.push(Line::from(vec![
            Span::styled("  Handle:         ", Style::new().fg(Theme::TEXT_SECONDARY)),
            Span::styled(
                profile.nickname.as_str(),
                Style::new().fg(Theme::TEXT_PRIMARY),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  Email:          ", Style::new().fg(Theme::TEXT_SECONDARY)),
            Span::styled(
                profile.email.as_deref().unwrap_or("-"),
                Style::new().fg(Theme::TEXT_PRIMARY),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  OAuth:          ", Style::new().fg(Theme::TEXT_SECONDARY)),
            Span::styled(oauth, Style::new().fg(Theme::TEXT_PRIMARY)),
        ]));
    } else if let Some(ref err) = app.profile_error {
        lines.push(Line::from(vec![
            Span::styled("  Profile:        ", Style::new().fg(Theme::TEXT_SECONDARY)),
            Span::styled(
                format!("load failed ({err})"),
                Style::new().fg(Theme::ACCENT_RED),
            ),
        ]));
    } else {
        lines.push(Line::from(Span::styled(
            "  Press 'r' to fetch profile info.",
            Style::new().fg(Theme::TEXT_HINT),
        )));
    }

    lines.extend([
        Line::raw(""),
        Line::from(vec![
            Span::styled("  Server:         ", Style::new().fg(Theme::TEXT_SECONDARY)),
            Span::styled(display_url, Style::new().fg(Theme::ACCENT_BLUE)),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            "── API Key (personal) ──",
            Style::new().fg(Theme::ACCENT_BLUE).bold(),
        )),
        Line::raw(""),
    ]);

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
        "  Press 'g' to regenerate",
        Style::new().fg(Theme::TEXT_HINT),
    )));
    lines.push(Line::from(Span::styled(
        "  (this is your personal key, not a team-only key)",
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

#[cfg(test)]
mod tests {
    use super::{mask_password, render, SESSION_STORAGE_METHOD_DETAILS};
    use crate::app::{App, SettingsSection};
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
    fn mask_password_returns_empty_for_empty_input() {
        assert_eq!(mask_password(""), "");
    }

    #[test]
    fn mask_password_keeps_length_and_hides_content() {
        assert_eq!(mask_password("abc123"), "******");
        assert_ne!(mask_password("abc123"), "abc123");
    }

    #[test]
    fn session_storage_details_are_aligned_with_supported_modes() {
        let details = SESSION_STORAGE_METHOD_DETAILS
            .iter()
            .map(|s| s.to_ascii_lowercase())
            .collect::<Vec<_>>();

        assert!(details.iter().any(|line| line.contains("git-native")));
        assert!(details.iter().any(|line| line.contains("sqlite")));
        assert!(details
            .iter()
            .any(|line| line.contains("index/cache") && line.contains("not canonical")));
        assert!(details.iter().all(|line| !line.contains("platform api")));
    }

    #[test]
    fn web_share_panel_shows_public_git_purpose_and_section_hint() {
        let mut app = App::new(vec![]);
        app.settings_section = SettingsSection::Workspace;
        let text = render_text(&app, 160, 40);

        assert!(text.contains("Web Share = register Public Git target"));
        assert!(text.contains("[/]"));
    }

    #[test]
    fn capture_flow_panel_explains_capture_vs_sync() {
        let mut app = App::new(vec![]);
        app.settings_section = SettingsSection::CaptureSync;
        let text = render_text(&app, 180, 50);

        assert!(text.contains("Capture = collect local events"));
        assert!(text.contains("Sync = upload captured sessions"));
        assert!(text.contains("capture-runtime:off"));
    }
}

// ── DaemonConfig section (existing settings) ─────────────────────────────

fn render_daemon_config(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    section: SettingsGroup,
    section_title: &str,
) {
    let body_block = Theme::block_dim().padding(Theme::PADDING_COMPACT);
    let inner = body_block.inner(area);
    frame.render_widget(body_block, area);

    let daemon_running = app.startup_status.daemon_pid.is_some();
    let section_items = config::section_items(section);

    let mut lines = vec![Line::from(Span::styled(
        format!("── {} ──", section_title),
        Style::new().fg(Theme::ACCENT_BLUE).bold(),
    ))];
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("[/]", Style::new().fg(Theme::ACCENT_YELLOW).bold()),
        Span::styled(
            " move section (left/right bracket keys)",
            Style::new().fg(Theme::TEXT_MUTED),
        ),
    ]));

    match section {
        SettingsGroup::Workspace => {
            lines.push(Line::from(Span::styled(
                "  Web Share = register Public Git target and publish shareable logs",
                Style::new().fg(Theme::TEXT_MUTED),
            )));
        }
        SettingsGroup::CaptureSync => {
            lines.push(Line::from(Span::styled(
                "  Capture = collect local events · Sync = upload captured sessions to share targets",
                Style::new().fg(Theme::TEXT_MUTED),
            )));
        }
        _ => {}
    }

    if section == SettingsGroup::CaptureSync {
        let daemon_status = if daemon_running {
            "capture-runtime:on (d to stop)"
        } else {
            "capture-runtime:off (d to start)"
        };
        lines.push(Line::from(Span::styled(
            format!("  {}", daemon_status),
            if daemon_running {
                Style::new().fg(Theme::ACCENT_GREEN)
            } else {
                Style::new().fg(Theme::TEXT_MUTED)
            },
        )));
    }
    lines.push(Line::raw(""));

    let mut selected_line = 0usize;
    let mut selectable_idx = 0usize;
    let mut rendered_headers = 0usize;

    for item in section_items {
        let (field, label, description, dependency_hint) = match item {
            SettingItem::Header(title) => {
                if rendered_headers > 0 {
                    lines.push(Line::raw(""));
                }
                rendered_headers += 1;
                lines.push(Line::from(vec![
                    Span::styled("  ", Style::new()),
                    Span::styled(
                        format!("{}:", title),
                        Style::new().fg(Theme::ACCENT_BLUE).bold(),
                    ),
                ]));
                continue;
            }
            SettingItem::Field {
                field,
                label,
                description,
                dependency_hint,
            } => (*field, *label, *description, *dependency_hint),
        };

        let is_selected = selectable_idx == app.settings_index;
        selectable_idx += 1;
        if is_selected {
            selected_line = lines.len();
        }
        let is_editing = is_selected && app.editing_field;

        let blocked_reason = app.daemon_config_field_block_reason(field);
        let daemon_hint = !daemon_running
            && matches!(
                field,
                SettingField::DebounceSecs
                    | SettingField::RealtimeDebounceMs
                    | SettingField::HealthCheckSecs
                    | SettingField::MaxRetries
                    | SettingField::WatchPaths
            )
            && dependency_hint.is_some();
        let dimmed = blocked_reason.is_some();

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

        if is_selected || dimmed || daemon_hint {
            let desc_text = if let Some(reason) = blocked_reason {
                reason
            } else if daemon_hint {
                dependency_hint.unwrap_or(description)
            } else {
                description
            };
            let desc_style = if blocked_reason.is_some() {
                Style::new().fg(Theme::ACCENT_YELLOW)
            } else if daemon_hint {
                Style::new().fg(Theme::TEXT_MUTED)
            } else {
                Style::new().fg(Theme::TEXT_HINT)
            };
            lines.push(Line::from(vec![
                Span::raw("     "),
                Span::styled(desc_text, desc_style),
            ]));

            if is_selected && field == SettingField::GitStorageMethod {
                let detail_style = Style::new().fg(Theme::TEXT_MUTED);
                for detail in SESSION_STORAGE_METHOD_DETAILS {
                    lines.push(Line::from(vec![
                        Span::raw("     "),
                        Span::styled(detail, detail_style),
                    ]));
                }
            }

            if is_selected && matches!(field, SettingField::StripPaths | SettingField::StripEnvVars)
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

            if is_selected && field == SettingField::SummaryProvider {
                lines.push(Line::from(vec![
                    Span::raw("     "),
                    Span::styled(
                        "Settings override env when set. Env fallback: ANTHROPIC_API_KEY | OPENAI_API_KEY | GEMINI_API_KEY",
                        Style::new().fg(Theme::TEXT_MUTED),
                    ),
                ]));
            }

            if is_selected && field == SettingField::SummaryCliAgent {
                lines.push(Line::from(vec![
                    Span::raw("     "),
                    Span::styled(
                        "CLI env fallback: OPS_TL_SUM_CLI_BIN, OPS_TL_SUM_CLI_ARGS. Model can be set below.",
                        Style::new().fg(Theme::TEXT_MUTED),
                    ),
                ]));
            }

            if is_selected && field == SettingField::SummaryModel {
                lines.push(Line::from(vec![
                    Span::raw("     "),
                    Span::styled(
                        "Applies to API requests and CLI --model when not already specified in CLI args.",
                        Style::new().fg(Theme::TEXT_MUTED),
                    ),
                ]));
            }

            if is_selected
                && matches!(
                    field,
                    SettingField::SummaryOpenAiCompatEndpoint
                        | SettingField::SummaryOpenAiCompatBase
                        | SettingField::SummaryOpenAiCompatPath
                        | SettingField::SummaryOpenAiCompatStyle
                        | SettingField::SummaryOpenAiCompatApiKey
                        | SettingField::SummaryOpenAiCompatApiKeyHeader
                )
            {
                lines.push(Line::from(vec![
                    Span::raw("     "),
                    Span::styled(
                        "Used by LLM Summary Mode = API:OpenAI-Compatible (or Auto when selected).",
                        Style::new().fg(Theme::TEXT_MUTED),
                    ),
                ]));
            }

            if is_selected && field == SettingField::SummaryEventWindow {
                lines.push(Line::from(vec![
                    Span::raw("     "),
                    Span::styled(
                        "Tip: 0/auto = turn-aware auto segmentation mode",
                        Style::new().fg(Theme::TEXT_MUTED),
                    ),
                ]));
            }

            if is_selected && field == SettingField::SummaryMaxInflight {
                lines.push(Line::from(vec![
                    Span::raw("     "),
                    Span::styled(
                        "Debounce controls pacing; max inflight controls parallelism.",
                        Style::new().fg(Theme::TEXT_MUTED),
                    ),
                ]));
            }

            if is_selected && field == SettingField::RealtimeDebounceMs {
                lines.push(Line::from(vec![
                    Span::raw("     "),
                    Span::styled(
                        "Scope: daemon realtime publish cadence + auto-refresh polling",
                        Style::new().fg(Theme::TEXT_MUTED),
                    ),
                ]));
            }

            if is_selected && field == SettingField::AutoPublish {
                lines.push(Line::from(vec![
                    Span::raw("     "),
                    Span::styled(
                        "ON: daemon running + session-end publish forced. OFF: daemon stopped + manual only.",
                        Style::new().fg(Theme::TEXT_MUTED),
                    ),
                ]));
            }

            if is_selected && field == SettingField::WatchPaths {
                let paths = &app.daemon_config.watchers.custom_paths;
                if paths.is_empty() {
                    lines.push(Line::from(vec![
                        Span::raw("     "),
                        Span::styled(
                            "No paths configured. Add comma-separated paths.",
                            Style::new().fg(Theme::ACCENT_YELLOW),
                        ),
                    ]));
                } else {
                    lines.push(Line::from(vec![
                        Span::raw("     "),
                        Span::styled("Current paths:", Style::new().fg(Theme::TEXT_MUTED)),
                    ]));
                    for p in paths {
                        lines.push(Line::from(vec![
                            Span::raw("       - "),
                            Span::styled(p, Style::new().fg(Theme::TEXT_MUTED)),
                        ]));
                    }
                }
            }

            if is_selected && field == SettingField::DetailRealtimePreviewEnabled {
                lines.push(Line::from(vec![
                    Span::raw("     "),
                    Span::styled(
                        "Scope: Session auto-refresh only (separate from Realtime Publish)",
                        Style::new().fg(Theme::TEXT_MUTED),
                    ),
                ]));
            }

            if is_selected && field == SettingField::DetailAutoExpandSelectedEvent {
                lines.push(Line::from(vec![
                    Span::raw("     "),
                    Span::styled(
                        "ON: selected event always shows preview lines. OFF: Enter/Space to expand manually.",
                        Style::new().fg(Theme::TEXT_MUTED),
                    ),
                ]));
            }

            if is_selected && field == SettingField::CalendarDisplayMode {
                lines.push(Line::from(vec![
                    Span::raw("     "),
                    Span::styled(
                        "smart: recent=relative, older=absolute",
                        Style::new().fg(Theme::TEXT_MUTED),
                    ),
                ]));
            }
        }
    }

    // Calculate scroll
    let visible_height = inner.height as usize;
    let scroll = if selected_line >= visible_height {
        selected_line.saturating_sub(visible_height / 2)
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
