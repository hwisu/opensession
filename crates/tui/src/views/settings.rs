use crate::app::App;
use crate::config::{SettingItem, SETTINGS_LAYOUT};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Padding, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let [header_area, body_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Fill(1),
        Constraint::Length(2),
    ])
    .areas(area);

    // ── Header ────────────────────────────────────────────────────────
    let dirty_mark = if app.config_dirty { " *" } else { "" };
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            " Settings ",
            Style::new().fg(Color::White).bold(),
        ),
        Span::styled(
            "(daemon.toml)",
            Style::new().fg(Color::DarkGray),
        ),
        Span::styled(dirty_mark, Style::new().fg(Color::Rgb(220, 180, 60))),
    ]))
    .block(Block::bordered().border_style(Style::new().fg(Color::Rgb(60, 65, 80))));
    frame.render_widget(header, header_area);

    // ── Body: settings list ───────────────────────────────────────────
    let body_block = Block::bordered()
        .border_style(Style::new().fg(Color::DarkGray))
        .padding(Padding::new(1, 1, 0, 0));
    let inner = body_block.inner(body_area);
    frame.render_widget(body_block, body_area);

    let mut lines = Vec::new();
    let mut selectable_idx: usize = 0;

    for item in SETTINGS_LAYOUT.iter() {
        match item {
            SettingItem::Header(title) => {
                if !lines.is_empty() {
                    lines.push(Line::raw(""));
                }
                let bar = "─".repeat(title.len() + 4);
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("── {} ──", title),
                        Style::new().fg(Color::Rgb(100, 180, 240)).bold(),
                    ),
                    Span::styled(
                        bar.chars().skip(title.len() + 6).collect::<String>(),
                        Style::new().fg(Color::Rgb(40, 45, 60)),
                    ),
                ]));
                lines.push(Line::raw(""));
            }
            SettingItem::Field { field, label } => {
                let is_selected = selectable_idx == app.settings_index;
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
                } else if field.is_toggle() {
                    let on = match field.display_value(&app.daemon_config).as_str() {
                        "ON" => true,
                        _ => false,
                    };
                    if on {
                        Style::new().fg(Color::Rgb(80, 200, 120))
                    } else {
                        Style::new().fg(Color::Rgb(220, 80, 80))
                    }
                } else if field.is_enum() {
                    Style::new().fg(Color::Rgb(180, 140, 220))
                } else if is_selected {
                    Style::new().fg(Color::White)
                } else {
                    Style::new().fg(Color::Rgb(100, 105, 120))
                };

                let bg = if is_selected {
                    Style::new().bg(Color::Rgb(30, 35, 50))
                } else {
                    Style::new()
                };

                // Type hint for editing
                let type_hint = if is_selected && !is_editing {
                    if field.is_toggle() {
                        Span::styled("  [Enter: toggle]", Style::new().fg(Color::Rgb(60, 65, 80)))
                    } else if field.is_enum() {
                        Span::styled("  [Enter: cycle]", Style::new().fg(Color::Rgb(60, 65, 80)))
                    } else {
                        Span::styled("  [Enter: edit]", Style::new().fg(Color::Rgb(60, 65, 80)))
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
            .thumb_style(Style::new().fg(Color::Rgb(80, 85, 100)));
        frame.render_stateful_widget(
            scrollbar,
            body_area.inner(Margin {
                vertical: 1,
                horizontal: 0,
            }),
            &mut scrollbar_state,
        );
    }

    // ── Footer ────────────────────────────────────────────────────────
    let mut footer_lines = vec![Line::from(vec![
        Span::styled(" j/k ", Style::new().fg(Color::Rgb(140, 145, 160))),
        Span::styled("navigate  ", Style::new().fg(Color::DarkGray)),
        Span::styled("Enter ", Style::new().fg(Color::Rgb(140, 145, 160))),
        Span::styled("edit  ", Style::new().fg(Color::DarkGray)),
        Span::styled("s ", Style::new().fg(Color::Rgb(140, 145, 160))),
        Span::styled("save  ", Style::new().fg(Color::DarkGray)),
        Span::styled("Esc ", Style::new().fg(Color::Rgb(140, 145, 160))),
        Span::styled("back", Style::new().fg(Color::DarkGray)),
    ])];

    if let Some(ref msg) = app.flash_message {
        footer_lines.push(Line::from(Span::styled(
            format!(" {msg}"),
            Style::new().fg(Color::Rgb(80, 200, 120)),
        )));
    }

    let footer = Paragraph::new(footer_lines);
    frame.render_widget(footer, footer_area);
}

/// Find the approximate line index for the nth selectable field.
fn find_selected_line(lines: &[Line], selected_idx: usize) -> usize {
    let mut sel_count = 0usize;
    for (line_idx, line) in lines.iter().enumerate() {
        // A selectable line starts with " > " or "   " and has a label
        let text = line.to_string();
        if text.starts_with(" > ") || (text.starts_with("   ") && text.len() > 5 && !text.trim().is_empty() && !text.contains("──")) {
            if sel_count == selected_idx {
                return line_idx;
            }
            sel_count += 1;
        }
    }
    0
}
