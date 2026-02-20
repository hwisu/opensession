use crate::app::{App, HandoffCandidate};
use crate::theme::Theme;
use opensession_core::handoff::HandoffSummary;
use opensession_parsers::all_parsers;
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
    let candidates = app.handoff_candidates();
    let selected_idx = selected_handoff_index(app, &candidates);
    render_picker(
        frame,
        &candidates,
        selected_idx,
        &app.handoff_selected_session_ids,
        picker_area,
    );
    render_preview(frame, app, &candidates, selected_idx, preview_area);
}

fn render_picker(
    frame: &mut Frame,
    candidates: &[HandoffCandidate],
    selected_idx: Option<usize>,
    selected_session_ids: &[String],
    area: Rect,
) {
    if candidates.is_empty() {
        frame.render_widget(
            Paragraph::new("No handoff candidates in current scope.")
                .block(Theme::block_dim().title(" Sessions ")),
            area,
        );
        return;
    }

    let selected_idx = selected_idx.unwrap_or(0);
    let max_items = visible_candidate_capacity(area);
    let (start, end) = candidate_window(candidates.len(), selected_idx, max_items);

    let items: Vec<ListItem> = candidates
        .iter()
        .skip(start)
        .take(end.saturating_sub(start))
        .enumerate()
        .map(|(idx, candidate)| {
            let global_idx = start + idx;
            let marker = if global_idx == selected_idx { ">" } else { " " };
            let picked = if selected_session_ids
                .iter()
                .any(|session_id| session_id == &candidate.session_id)
            {
                "[x]"
            } else {
                "[ ]"
            };
            let title = truncate(candidate.title.as_str(), 44);
            let meta = candidate_picker_meta(marker, picked, candidate);
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
    state.select(Some(selected_idx.saturating_sub(start)));

    let title = if candidates.len() > end.saturating_sub(start) {
        format!(" Sessions {}-{} / {} ", start + 1, end, candidates.len())
    } else {
        " Sessions ".to_string()
    };

    let list = List::new(items)
        .block(Theme::block_dim().title(title))
        .highlight_style(
            Style::new()
                .bg(Theme::BG_SURFACE)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(" ")
        .highlight_spacing(ratatui::widgets::HighlightSpacing::Always);
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_preview(
    frame: &mut Frame,
    app: &App,
    candidates: &[HandoffCandidate],
    selected_idx: Option<usize>,
    area: Rect,
) {
    let effective_candidates = effective_candidates(app, candidates, selected_idx);
    if effective_candidates.is_empty() {
        frame.render_widget(
            Paragraph::new("Select a session in picker (j/k).")
                .block(Theme::block_dim().title(" Preview ")),
            area,
        );
        return;
    }

    let mut sorted = effective_candidates;
    sorted.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.session_id.cmp(&right.session_id))
    });

    let mut lines = preview_lines(app, &sorted);
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

fn effective_candidates(
    app: &App,
    candidates: &[HandoffCandidate],
    selected_idx: Option<usize>,
) -> Vec<HandoffCandidate> {
    let selected = app.handoff_effective_candidates();
    if !selected.is_empty() {
        return selected;
    }
    selected_idx
        .and_then(|idx| candidates.get(idx))
        .cloned()
        .into_iter()
        .collect::<Vec<_>>()
}

fn selected_handoff_index(app: &App, candidates: &[HandoffCandidate]) -> Option<usize> {
    if candidates.is_empty() {
        return None;
    }
    if let Some(selected_id) = app.handoff_selected_session_id.as_deref() {
        if let Some(index) = candidates
            .iter()
            .position(|candidate| candidate.session_id == selected_id)
        {
            return Some(index);
        }
    }
    Some(0)
}

fn visible_candidate_capacity(area: Rect) -> usize {
    let usable_rows = area.height.saturating_sub(2) as usize;
    let per_item_rows = 2usize;
    (usable_rows / per_item_rows).max(1)
}

fn candidate_window(total: usize, selected_idx: usize, max_items: usize) -> (usize, usize) {
    if total == 0 {
        return (0, 0);
    }
    let capped_items = max_items.max(1).min(total);
    let mut start = selected_idx.saturating_sub(capped_items / 2);
    let max_start = total.saturating_sub(capped_items);
    if start > max_start {
        start = max_start;
    }
    let end = (start + capped_items).min(total);
    (start, end)
}

fn candidate_agent_label(candidate: &HandoffCandidate) -> String {
    let tool = candidate.tool.trim();
    let model = candidate.model.trim();
    match (tool.is_empty(), model.is_empty()) {
        (false, false) => format!("{tool} / {model}"),
        (false, true) => tool.to_string(),
        (true, false) => model.to_string(),
        (true, true) => "unknown".to_string(),
    }
}

fn candidate_picker_meta(marker: &str, picked: &str, candidate: &HandoffCandidate) -> String {
    format!(
        "{}{} {} · {} · {} msgs · {} ev",
        marker,
        picked,
        candidate_agent_label(candidate),
        candidate.created_at.format("%m-%d %H:%M"),
        candidate.message_count,
        candidate.event_count
    )
}

fn preview_lines(app: &App, selected_candidates: &[HandoffCandidate]) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(Span::styled(
            "Handoff artifact preview (merge_policy=time_asc)",
            Style::new().fg(Theme::ACCENT_BLUE).bold(),
        )),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Selection: ", Style::new().fg(Theme::TEXT_SECONDARY)),
            Span::styled(
                format!("{} session(s)", selected_candidates.len()),
                Style::new().fg(Theme::TEXT_PRIMARY),
            ),
        ]),
    ];

    let (payload_preview, warnings) = payload_preview_jsonl(selected_candidates);
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "Expected payload (jsonl preview)",
        Style::new().fg(Theme::ACCENT_BLUE).bold(),
    )));
    if payload_preview.is_empty() {
        lines.push(Line::from(Span::styled(
            "(unavailable: source session file not resolvable)",
            Style::new().fg(Theme::TEXT_MUTED),
        )));
        lines.push(Line::from(Span::styled(
            truncate(
                &format!("example: {}", fallback_payload_example(selected_candidates)),
                100,
            ),
            Style::new().fg(Theme::TEXT_KEY_DESC),
        )));
    } else {
        let preview_limit = 3usize;
        let extra_count = payload_preview.len().saturating_sub(preview_limit);
        for line in payload_preview.into_iter().take(preview_limit) {
            lines.push(Line::from(Span::styled(
                truncate(&line, 100),
                Style::new().fg(Theme::TEXT_KEY_DESC),
            )));
        }
        if extra_count > 0 {
            lines.push(Line::from(Span::styled(
                format!("... +{extra_count} more line(s)"),
                Style::new().fg(Theme::TEXT_MUTED),
            )));
        }
    }
    for warning in warnings {
        lines.push(Line::from(vec![
            Span::styled("warn: ", Style::new().fg(Theme::ACCENT_YELLOW).bold()),
            Span::styled(truncate(&warning, 94), Style::new().fg(Theme::TEXT_MUTED)),
        ]));
    }

    for (idx, candidate) in selected_candidates.iter().enumerate() {
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::styled(
                format!("S{} ", idx + 1),
                Style::new().fg(Theme::TEXT_SECONDARY),
            ),
            Span::styled(
                truncate(&candidate.session_id, 40),
                Style::new().fg(Theme::TEXT_PRIMARY),
            ),
            Span::styled(
                format!("  {}", candidate.created_at.format("%Y-%m-%d %H:%M:%S")),
                Style::new().fg(Theme::TEXT_MUTED),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::raw("   "),
            Span::styled(
                format!(
                    "{} · {} msgs · {} ev",
                    candidate_agent_label(candidate),
                    candidate.message_count,
                    candidate.event_count
                ),
                Style::new().fg(Theme::TEXT_SECONDARY),
            ),
        ]));
    }

    lines.push(Line::raw(""));
    if let Some((artifact_id, stale, reasons)) = app.handoff_last_artifact_status() {
        lines.push(Line::from(vec![
            Span::styled("Artifact: ", Style::new().fg(Theme::TEXT_SECONDARY)),
            Span::styled(artifact_id, Style::new().fg(Theme::TEXT_PRIMARY)),
            Span::styled("  ", Style::new()),
            Span::styled(
                if stale { "[STALE]" } else { "[FRESH]" },
                if stale {
                    Style::new().fg(Theme::ACCENT_RED).bold()
                } else {
                    Style::new().fg(Theme::ACCENT_GREEN).bold()
                },
            ),
        ]));
        for reason in reasons.into_iter().take(3) {
            lines.push(Line::from(vec![
                Span::styled("  - ", Style::new().fg(Theme::TEXT_SECONDARY)),
                Span::styled(truncate(&reason, 96), Style::new().fg(Theme::TEXT_MUTED)),
            ]));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "Artifact: (none saved in this TUI session)",
            Style::new().fg(Theme::TEXT_MUTED),
        )));
    }

    lines
}

fn payload_preview_jsonl(candidates: &[HandoffCandidate]) -> (Vec<String>, Vec<String>) {
    let parsers = all_parsers();
    let mut lines = Vec::new();
    let mut warnings = Vec::new();

    for candidate in candidates {
        let Some(path) = candidate.source_path.as_ref() else {
            warnings.push(format!("{} has no local source_path", candidate.session_id));
            continue;
        };
        let Some(parser) = parsers.iter().find(|parser| parser.can_parse(path)) else {
            warnings.push(format!(
                "{} unsupported source format: {}",
                candidate.session_id,
                path.display()
            ));
            continue;
        };

        match parser.parse(path) {
            Ok(session) => {
                let summary = HandoffSummary::from_session(&session);
                let objective = if summary.objective_undefined_reason.is_some() {
                    serde_json::Value::Null
                } else {
                    serde_json::Value::String(summary.objective)
                };
                let value = serde_json::json!({
                    "session_id": summary.source_session_id,
                    "tool": summary.tool,
                    "model": summary.model,
                    "objective": objective,
                    "duration_seconds": summary.duration_seconds,
                    "next_actions": summary.execution_contract.next_actions,
                });
                if let Ok(line) = serde_json::to_string(&value) {
                    lines.push(line);
                }
            }
            Err(err) => warnings.push(format!(
                "failed to parse {} ({}): {err}",
                candidate.session_id,
                path.display()
            )),
        }
    }

    (lines, warnings)
}

fn fallback_payload_example(candidates: &[HandoffCandidate]) -> String {
    let candidate = candidates.first();
    let session_id = serde_json::to_string(
        &candidate
            .map(|c| c.session_id.as_str())
            .unwrap_or("session-id"),
    )
    .unwrap_or_else(|_| "\"session-id\"".to_string());
    let tool = serde_json::to_string(&candidate.map(|c| c.tool.as_str()).unwrap_or("unknown"))
        .unwrap_or_else(|_| "\"unknown\"".to_string());
    let model = serde_json::to_string(&candidate.map(|c| c.model.as_str()).unwrap_or("unknown"))
        .unwrap_or_else(|_| "\"unknown\"".to_string());
    format!(
        "{{\"session_id\":{session_id},\"tool\":{tool},\"model\":{model},\"objective\":null,\"duration_seconds\":0,\"next_actions\":[]}}"
    )
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
    use super::{
        candidate_agent_label, candidate_picker_meta, candidate_window, fallback_payload_example,
        preview_lines, truncate,
    };
    use crate::app::{App, HandoffCandidate};
    use chrono::Utc;

    fn sample_candidate() -> HandoffCandidate {
        HandoffCandidate {
            session_id: "ses-1".to_string(),
            title: "Example Session".to_string(),
            tool: "codex".to_string(),
            model: "gpt-5-codex".to_string(),
            created_at: Utc::now(),
            event_count: 12,
            message_count: 7,
            source_path: None,
        }
    }

    #[test]
    fn truncate_adds_ellipsis_for_long_values() {
        assert_eq!(truncate("abcdefghij", 6), "abc...");
    }

    #[test]
    fn candidate_agent_label_formats_tool_and_model() {
        let candidate = sample_candidate();
        assert_eq!(candidate_agent_label(&candidate), "codex / gpt-5-codex");
    }

    #[test]
    fn candidate_picker_meta_includes_agent_label() {
        let candidate = sample_candidate();
        let meta = candidate_picker_meta(">", "[x]", &candidate);
        assert!(meta.contains("codex / gpt-5-codex"));
        assert!(meta.contains("7 msgs"));
        assert!(meta.contains("12 ev"));
    }

    #[test]
    fn fallback_payload_example_includes_core_fields() {
        let candidate = sample_candidate();
        let payload = fallback_payload_example(&[candidate]);
        assert!(payload.contains("\"session_id\""));
        assert!(payload.contains("\"tool\""));
        assert!(payload.contains("\"model\""));
        assert!(payload.contains("\"next_actions\":[]"));
    }

    #[test]
    fn preview_lines_show_jsonl_example_when_source_is_missing() {
        let app = App::new(Vec::new());
        let lines = preview_lines(&app, &[sample_candidate()]);
        let text = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("Expected payload (jsonl preview)"));
        assert!(text.contains("example: {"));
        assert!(text.contains("\"session_id\""));
        assert!(text.contains("\"tool\""));
        assert!(text.contains("\"model\""));
    }

    #[test]
    fn candidate_window_centers_selection_when_possible() {
        assert_eq!(candidate_window(20, 10, 5), (8, 13));
        assert_eq!(candidate_window(20, 1, 5), (0, 5));
        assert_eq!(candidate_window(20, 19, 5), (15, 20));
    }

    #[test]
    fn candidate_window_handles_small_total_and_zero() {
        assert_eq!(candidate_window(3, 1, 10), (0, 3));
        assert_eq!(candidate_window(0, 0, 5), (0, 0));
    }
}
