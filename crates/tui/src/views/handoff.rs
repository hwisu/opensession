use crate::app::{App, HandoffCandidate};
use crate::theme::Theme;
use ratatui::prelude::*;
use ratatui::widgets::{List, ListItem, ListState, Paragraph};

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let block = Theme::block_dim()
        .title(" Handoff ")
        .padding(Theme::PADDING_CARD);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width < 24 || inner.height < 6 {
        frame.render_widget(
            Paragraph::new("Expand terminal to use handoff picker."),
            inner,
        );
        return;
    }

    let [picker_area, preview_area] =
        Layout::horizontal([Constraint::Percentage(38), Constraint::Percentage(62)]).areas(inner);
    render_picker(frame, app, picker_area);
    render_preview(frame, app, preview_area);
}

fn render_picker(frame: &mut Frame, app: &App, area: Rect) {
    let candidates = app.handoff_candidates();
    if candidates.is_empty() {
        frame.render_widget(
            Paragraph::new("No handoff candidates in current scope.")
                .block(Theme::block_dim().title(" Sessions ")),
            area,
        );
        return;
    }

    let selected_idx = app
        .handoff_selected_session_id
        .as_deref()
        .and_then(|selected_id| {
            candidates
                .iter()
                .position(|candidate| candidate.session_id == selected_id)
        })
        .unwrap_or(0);

    let items: Vec<ListItem> = candidates
        .iter()
        .enumerate()
        .map(|(idx, candidate)| {
            let marker = if idx == selected_idx { ">" } else { " " };
            let title = truncate(candidate.title.as_str(), 44);
            let meta = format!(
                "{} {} · {} msgs · {} ev",
                marker, candidate.tool, candidate.message_count, candidate.event_count
            );
            ListItem::new(vec![
                Line::from(Span::styled(
                    title,
                    Style::new().fg(Theme::TEXT_PRIMARY).bold(),
                )),
                Line::from(Span::styled(meta, Style::new().fg(Theme::TEXT_SECONDARY))),
            ])
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(selected_idx));

    let list = List::new(items)
        .block(Theme::block_dim().title(" Sessions "))
        .highlight_style(
            Style::new()
                .bg(Theme::BG_SURFACE)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(" ")
        .highlight_spacing(ratatui::widgets::HighlightSpacing::Always);
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_preview(frame: &mut Frame, app: &App, area: Rect) {
    let Some(candidate) = app.selected_handoff_candidate() else {
        frame.render_widget(
            Paragraph::new("Select a session in picker (j/k).")
                .block(Theme::block_dim().title(" Preview ")),
            area,
        );
        return;
    };

    let selected_source_path = candidate
        .source_path
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned());
    let base = base_handoff_command(selected_source_path.as_deref());

    let mut lines = preview_lines(&candidate, selected_source_path.as_deref(), &base);
    let max_lines = area.height.saturating_sub(2) as usize;
    if max_lines == 0 {
        return;
    }
    if lines.len() > max_lines {
        lines.truncate(max_lines);
    }

    frame.render_widget(
        Paragraph::new(lines).block(Theme::block_dim().title(" Preview ")),
        area,
    );
}

fn preview_lines(
    candidate: &HandoffCandidate,
    source_path: Option<&str>,
    base: &str,
) -> Vec<Line<'static>> {
    vec![
        Line::from(Span::styled(
            "Execution-contract handoff (v2 default)",
            Style::new().fg(Theme::ACCENT_BLUE).bold(),
        )),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Session: ", Style::new().fg(Theme::TEXT_SECONDARY)),
            Span::styled(
                truncate(candidate.session_id.as_str(), 52),
                Style::new().fg(Theme::TEXT_PRIMARY),
            ),
        ]),
        Line::from(vec![
            Span::styled("Model:   ", Style::new().fg(Theme::TEXT_SECONDARY)),
            Span::styled(
                format!(
                    "{} / {} · {} msgs · {} ev",
                    candidate.tool, candidate.model, candidate.message_count, candidate.event_count
                ),
                Style::new().fg(Theme::TEXT_PRIMARY),
            ),
        ]),
        Line::from(vec![
            Span::styled("Source:  ", Style::new().fg(Theme::TEXT_SECONDARY)),
            Span::styled(
                source_path
                    .map(|path| truncate(path, 72))
                    .unwrap_or_else(|| "(unresolved, using --last)".to_string()),
                Style::new().fg(Theme::TEXT_PRIMARY),
            ),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            "Recommended commands",
            Style::new().fg(Theme::ACCENT_BLUE).bold(),
        )),
        Line::from(vec![
            Span::styled("  1) ", Style::new().fg(Theme::TEXT_KEY).bold()),
            Span::styled(base.to_string(), Style::new().fg(Theme::TEXT_KEY_DESC)),
        ]),
        Line::from(vec![
            Span::styled("  2) ", Style::new().fg(Theme::TEXT_KEY).bold()),
            Span::styled(
                format!("{base} --validate"),
                Style::new().fg(Theme::TEXT_KEY_DESC),
            ),
        ]),
        Line::from(vec![
            Span::styled("  3) ", Style::new().fg(Theme::TEXT_KEY).bold()),
            Span::styled(
                format!("{base} --validate --strict"),
                Style::new().fg(Theme::TEXT_KEY_DESC),
            ),
        ]),
        Line::from(vec![
            Span::styled("  4) ", Style::new().fg(Theme::TEXT_KEY).bold()),
            Span::styled(
                "opensession session handoff --last 6 --populate claude".to_string(),
                Style::new().fg(Theme::TEXT_KEY_DESC),
            ),
        ]),
        Line::from(vec![
            Span::styled("  5) ", Style::new().fg(Theme::TEXT_KEY).bold()),
            Span::styled(
                "opensession session handoff --last HEAD~6 --populate claude:opus-4.6".to_string(),
                Style::new().fg(Theme::TEXT_KEY_DESC),
            ),
        ]),
        Line::raw(""),
        Line::from("Validation semantics"),
        Line::from("  - --validate: report findings, exit 0"),
        Line::from("  - --validate --strict: non-zero on error findings"),
        Line::from("  - execution_contract.parallel_actions: parallelizable work packages"),
        Line::from("  - execution_contract.ordered_steps: ordered timeline with timestamps"),
    ]
}

fn base_handoff_command(source_path: Option<&str>) -> String {
    match source_path {
        Some(path) if !path.trim().is_empty() => {
            format!("opensession session handoff {}", shell_quote(path))
        }
        _ => "opensession session handoff --last".to_string(),
    }
}

fn shell_quote(value: &str) -> String {
    let safe = value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | ':' | '~'));
    if safe {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }
}

fn truncate(value: &str, max: usize) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= max {
        return value.to_string();
    }
    if max <= 3 {
        return ".".repeat(max);
    }
    chars[..max.saturating_sub(3)].iter().collect::<String>() + "..."
}

#[cfg(test)]
mod tests {
    use super::{base_handoff_command, shell_quote, truncate};

    #[test]
    fn base_handoff_command_uses_last_without_path() {
        assert_eq!(
            base_handoff_command(None),
            "opensession session handoff --last"
        );
    }

    #[test]
    fn base_handoff_command_quotes_path_when_needed() {
        let cmd = base_handoff_command(Some("/tmp/hello world/session.jsonl"));
        assert_eq!(
            cmd,
            "opensession session handoff '/tmp/hello world/session.jsonl'"
        );
    }

    #[test]
    fn shell_quote_leaves_safe_values_unquoted() {
        assert_eq!(shell_quote("/tmp/session.jsonl"), "/tmp/session.jsonl");
    }

    #[test]
    fn truncate_adds_ellipsis_for_long_values() {
        assert_eq!(truncate("abcdefghij", 6), "abc...");
    }
}
