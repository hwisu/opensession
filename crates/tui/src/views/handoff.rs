use crate::app::App;
use crate::theme::Theme;
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let block = Theme::block_dim()
        .title(" Handoff ")
        .padding(Theme::PADDING_CARD);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let selected_session_id = app
        .selected_session()
        .map(|session| session.session_id.clone())
        .or_else(|| app.selected_db_session().map(|row| row.id.clone()))
        .unwrap_or_else(|| "(no session selected)".to_string());

    let selected_source_path = app
        .resolve_selected_source_path()
        .map(|path| path.to_string_lossy().into_owned());

    let base = base_handoff_command(selected_source_path.as_deref());

    let mut lines = vec![
        Line::from(Span::styled(
            "Execution-contract handoff (v2 default)",
            Style::new().fg(Theme::ACCENT_BLUE).bold(),
        )),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Selected session: ", Style::new().fg(Theme::TEXT_SECONDARY)),
            Span::styled(selected_session_id, Style::new().fg(Theme::TEXT_PRIMARY)),
        ]),
        Line::from(vec![
            Span::styled("Selected source:  ", Style::new().fg(Theme::TEXT_SECONDARY)),
            Span::styled(
                selected_source_path.unwrap_or_else(|| "(unresolved, using --last)".to_string()),
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
            Span::styled(base.clone(), Style::new().fg(Theme::TEXT_KEY_DESC)),
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
                format!("{base} --format stream --validate"),
                Style::new().fg(Theme::TEXT_KEY_DESC),
            ),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            "Validation semantics",
            Style::new().fg(Theme::ACCENT_BLUE).bold(),
        )),
        Line::from("  - --validate: report findings, exit 0"),
        Line::from("  - --validate --strict: non-zero on findings"),
        Line::raw(""),
        Line::from(Span::styled(
            "Local index scope",
            Style::new().fg(Theme::ACCENT_BLUE).bold(),
        )),
        Line::from("  - local-db: local index/cache only (metadata, sync state, HEAD refs, timeline cache)"),
        Line::from("  - default path: v2 handoff + git-native"),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Enter ", Style::new().fg(Theme::TEXT_KEY).bold()),
            Span::styled("open selected session detail", Style::new().fg(Theme::TEXT_KEY_DESC)),
            Span::styled("  Esc ", Style::new().fg(Theme::TEXT_KEY).bold()),
            Span::styled("back to Sessions", Style::new().fg(Theme::TEXT_KEY_DESC)),
        ]),
    ];

    // Keep layout stable on very small terminals.
    if inner.height == 0 {
        return;
    }
    let max_lines = inner.height as usize;
    if lines.len() > max_lines {
        lines.truncate(max_lines);
    }

    frame.render_widget(Paragraph::new(lines), inner);
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

#[cfg(test)]
mod tests {
    use super::{base_handoff_command, shell_quote};

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
}
