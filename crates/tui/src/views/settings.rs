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
    let group = app
        .settings_section
        .group()
        .unwrap_or(SettingsGroup::Workspace);
    render_daemon_config(frame, app, area, group, app.settings_section.panel_title());
}

#[cfg(test)]
mod tests {
    use super::{render, SESSION_STORAGE_METHOD_DETAILS};
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
    fn web_sync_panel_shows_public_git_purpose_and_section_hint() {
        let mut app = App::new(vec![]);
        app.settings_section = SettingsSection::Workspace;
        let text = render_text(&app, 160, 40);

        assert!(text.contains("Web Sync (Public) = account profile"));
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

    #[test]
    fn git_panel_explains_git_explorer_scope() {
        let mut app = App::new(vec![]);
        app.settings_section = SettingsSection::Git;
        let text = render_text(&app, 180, 40);

        assert!(text.contains("Git Explorer = parse-path"));
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
                "  Web Sync (Public) = account profile + public sync registration",
                Style::new().fg(Theme::TEXT_MUTED),
            )));
            let display_url = app
                .daemon_config
                .server
                .url
                .trim_start_matches("https://")
                .trim_start_matches("http://");
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("Endpoint: ", Style::new().fg(Theme::TEXT_SECONDARY)),
                Span::styled(display_url, Style::new().fg(Theme::ACCENT_BLUE)),
            ]));
            if app.profile_loading {
                lines.push(Line::from(Span::styled(
                    "  Profile: loading...",
                    Style::new().fg(Theme::ACCENT_BLUE),
                )));
            } else if let Some(profile) = app.profile.as_ref() {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled("Profile: ", Style::new().fg(Theme::TEXT_SECONDARY)),
                    Span::styled(
                        format!(
                            "{} ({})",
                            profile.nickname,
                            profile.email.as_deref().unwrap_or("-")
                        ),
                        Style::new().fg(Theme::TEXT_PRIMARY),
                    ),
                ]));
            } else if let Some(err) = app.profile_error.as_ref() {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled("Profile: ", Style::new().fg(Theme::TEXT_SECONDARY)),
                    Span::styled(
                        format!("load failed ({err})"),
                        Style::new().fg(Theme::ACCENT_RED),
                    ),
                ]));
            } else {
                lines.push(Line::from(Span::styled(
                    "  Profile: press 'r' to fetch",
                    Style::new().fg(Theme::TEXT_HINT),
                )));
            }
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("API key: ", Style::new().fg(Theme::TEXT_SECONDARY)),
                Span::styled(
                    if app.daemon_config.server.api_key.is_empty() {
                        "not set".to_string()
                    } else {
                        "set (press 'g' to regenerate)".to_string()
                    },
                    Style::new().fg(Theme::TEXT_PRIMARY),
                ),
            ]));
        }
        SettingsGroup::CaptureSync => {
            lines.push(Line::from(Span::styled(
                "  Capture = collect local events · Sync = upload captured sessions to share targets",
                Style::new().fg(Theme::TEXT_MUTED),
            )));
        }
        SettingsGroup::Git => {
            lines.push(Line::from(Span::styled(
                "  Git Explorer = parse-path + git-native storage settings (for local/public navigation)",
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
            && matches!(field, SettingField::DebounceSecs | SettingField::WatchPaths)
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
