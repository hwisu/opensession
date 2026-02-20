use crate::app::{App, HandoffCandidate};
use crate::theme::{self, Theme};
use opensession_core::handoff::HandoffSummary;
use opensession_parsers::all_parsers;
use ratatui::prelude::*;
use ratatui::widgets::{List, ListItem, ListState, Paragraph};

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let block = Theme::block()
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

    let [picker_area, preview_area] = if inner.width >= 160 {
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).areas(inner)
    } else {
        Layout::horizontal([Constraint::Percentage(42), Constraint::Percentage(58)]).areas(inner)
    };
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
            let is_selected = global_idx == selected_idx;
            let is_picked = selected_session_ids
                .iter()
                .any(|session_id| session_id == &candidate.session_id);
            let picked_order = selected_session_ids
                .iter()
                .position(|session_id| session_id == &candidate.session_id)
                .map(|idx| idx + 1);

            let title_width = area.width.saturating_sub(16) as usize;
            let wrapped_title = wrap_title_lines(&candidate.title, title_width.max(16), 2);
            let title_first = wrapped_title
                .first()
                .cloned()
                .unwrap_or_else(|| "(untitled)".to_string());
            let title_second = wrapped_title.get(1).cloned().unwrap_or_default();

            let line1_title_style = if is_selected {
                Style::new().fg(Theme::TEXT_PRIMARY).bold()
            } else {
                Style::new().fg(Theme::TEXT_CONTENT).bold()
            };
            let badge = if let Some(order) = picked_order {
                Span::styled(
                    format!(" {:>2} ", order),
                    Style::new().fg(Color::Black).bg(Theme::ACCENT_GREEN).bold(),
                )
            } else {
                Span::styled(" .. ", Style::new().fg(Theme::TEXT_MUTED))
            };

            let line1 = Line::from(vec![
                badge,
                Span::raw(" "),
                Span::styled(
                    theme::tool_icon(&candidate.tool),
                    Style::new().fg(theme::tool_color(&candidate.tool)).bold(),
                ),
                Span::raw(" "),
                Span::styled(title_first, line1_title_style),
            ]);

            let line2 = if !title_second.is_empty() {
                Line::from(vec![
                    Span::raw("      "),
                    Span::styled(title_second, Style::new().fg(Theme::TEXT_PRIMARY)),
                ])
            } else {
                Line::from(vec![
                    Span::raw("      "),
                    Span::styled(
                        truncate(&candidate.session_id, title_width.max(16)),
                        if is_picked {
                            Style::new().fg(Theme::ACCENT_GREEN)
                        } else {
                            Style::new().fg(Theme::TEXT_MUTED)
                        },
                    ),
                ])
            };

            let line3 = candidate_picker_meta_line(candidate);
            ListItem::new(vec![line1, line2, line3])
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(selected_idx.saturating_sub(start)));

    let picked_count = candidates
        .iter()
        .filter(|candidate| {
            selected_session_ids
                .iter()
                .any(|session_id| session_id == &candidate.session_id)
        })
        .count();
    let title = if candidates.len() > end.saturating_sub(start) {
        format!(
            " Sessions {}-{} / {} · picked {} ",
            start + 1,
            end,
            candidates.len(),
            picked_count
        )
    } else {
        format!(
            " Sessions ({}) · picked {} ",
            candidates.len(),
            picked_count
        )
    };

    let list = List::new(items)
        .block(Theme::block_dim().title(title))
        .highlight_style(
            Style::new()
                .bg(Theme::BG_SURFACE)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(" > ")
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
    let per_item_rows = 3usize;
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

fn candidate_picker_meta_line(candidate: &HandoffCandidate) -> Line<'static> {
    let model = if candidate.model.trim().is_empty() {
        "unknown".to_string()
    } else {
        truncate(candidate.model.trim(), 26)
    };
    let timestamp = candidate.created_at.format("%m-%d %H:%M").to_string();
    let msgs = format!("{} msgs", candidate.message_count);
    let events = format!("{} ev", candidate.event_count);
    Line::from(vec![
        Span::raw("      "),
        Span::styled(model, Style::new().fg(Theme::ACCENT_BLUE)),
        Span::styled(" · ", Style::new().fg(Theme::TEXT_MUTED)),
        Span::styled(timestamp, Style::new().fg(Theme::TEXT_SECONDARY)),
        Span::styled(" · ", Style::new().fg(Theme::TEXT_MUTED)),
        Span::styled(msgs, Style::new().fg(Color::Green)),
        Span::styled(" · ", Style::new().fg(Theme::TEXT_MUTED)),
        Span::styled(events, Style::new().fg(Color::Yellow)),
    ])
}

fn wrap_title_lines(value: &str, max_width: usize, max_lines: usize) -> Vec<String> {
    if max_width == 0 || max_lines == 0 {
        return Vec::new();
    }

    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return vec!["(untitled)".to_string()];
    }

    let words: Vec<&str> = normalized.split(' ').collect();
    let mut idx = 0usize;
    let mut lines = Vec::new();
    let mut current = String::new();

    while idx < words.len() {
        let word = words[idx];
        let candidate = if current.is_empty() {
            word.to_string()
        } else {
            format!("{current} {word}")
        };

        if candidate.chars().count() <= max_width {
            current = candidate;
            idx += 1;
            continue;
        }

        if current.is_empty() {
            lines.push(truncate(word, max_width));
            idx += 1;
        } else {
            lines.push(current);
            current = String::new();
        }

        if lines.len() >= max_lines {
            break;
        }
    }

    if lines.len() < max_lines && !current.is_empty() {
        lines.push(current);
    }

    if idx < words.len() && !lines.is_empty() {
        let last_idx = lines.len() - 1;
        let last_line = lines[last_idx].clone();
        let mut compact = truncate(&last_line, max_width);
        if !compact.ends_with("...") && compact.chars().count() + 3 <= max_width {
            compact.push_str("...");
        } else if !compact.ends_with("...") {
            compact = truncate(&compact, max_width);
        }
        lines[last_idx] = compact;
    }

    lines
}

fn preview_lines(app: &App, selected_candidates: &[HandoffCandidate]) -> Vec<Line<'static>> {
    let (payload_rows, warnings) = payload_preview_rows(selected_candidates);
    let warning_count = warnings.len();
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Selection ", Style::new().fg(Theme::TEXT_SECONDARY)),
            Span::styled(
                format!("{} session(s)", selected_candidates.len()),
                Style::new().fg(Theme::TEXT_PRIMARY).bold(),
            ),
            Span::styled("  ", Style::new()),
            Span::styled(
                " merge ",
                Style::new().fg(Color::Black).bg(Theme::ACCENT_BLUE).bold(),
            ),
            Span::styled(" ", Style::new()),
            Span::styled("time_asc", Style::new().fg(Theme::ACCENT_BLUE)),
            Span::styled("  ", Style::new()),
            Span::styled("warnings ", Style::new().fg(Theme::TEXT_SECONDARY)),
            Span::styled(
                warning_count.to_string(),
                if warning_count == 0 {
                    Style::new().fg(Theme::ACCENT_GREEN)
                } else {
                    Style::new().fg(Theme::ACCENT_YELLOW).bold()
                },
            ),
        ]),
        Line::raw(""),
    ];

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "Payload snapshot (JSON array artifact)",
        Style::new().fg(Theme::ACCENT_BLUE).bold(),
    )));
    if payload_rows.is_empty() {
        lines.push(Line::from(Span::styled(
            "(unavailable: source session file not resolvable)",
            Style::new().fg(Theme::TEXT_MUTED),
        )));
    } else {
        let preview_limit = 4usize;
        let extra_count = payload_rows.len().saturating_sub(preview_limit);
        for (idx, row) in payload_rows.into_iter().take(preview_limit).enumerate() {
            lines.push(Line::from(Span::styled(
                format!("{:>2}.", idx + 1),
                Style::new().fg(Theme::TEXT_SECONDARY),
            )));
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled(
                    theme::tool_icon(&row.tool),
                    Style::new().fg(theme::tool_color(&row.tool)).bold(),
                ),
                Span::raw(" "),
                Span::styled(
                    truncate(&row.model, 34),
                    Style::new().fg(Theme::ACCENT_BLUE),
                ),
                Span::styled("  ", Style::new()),
                Span::styled(
                    format!("{}s", row.duration_seconds),
                    Style::new().fg(Theme::ACCENT_CYAN),
                ),
                Span::styled("  ", Style::new()),
                Span::styled(row.short_session_id(), Style::new().fg(Theme::TEXT_MUTED)),
            ]));
            lines.push(Line::from(vec![
                Span::raw("      "),
                Span::styled(
                    truncate(&format!("objective: {}", row.objective_or_missing()), 96),
                    Style::new().fg(Theme::TEXT_SECONDARY),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::raw("      "),
                Span::styled(
                    format!("next_actions: {}", row.next_actions_count),
                    Style::new().fg(Theme::TEXT_MUTED),
                ),
            ]));
        }
        if extra_count > 0 {
            lines.push(Line::from(Span::styled(
                format!("... +{extra_count} more session(s)"),
                Style::new().fg(Theme::TEXT_MUTED),
            )));
        }
    }
    for warning in warnings {
        lines.push(Line::from(vec![
            Span::styled("! ", Style::new().fg(Theme::ACCENT_YELLOW).bold()),
            Span::styled(truncate(&warning, 96), Style::new().fg(Theme::TEXT_MUTED)),
        ]));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "Artifact status",
        Style::new().fg(Theme::ACCENT_BLUE).bold(),
    )));
    if let Some((artifact_id, stale, reasons)) = app.handoff_last_artifact_status() {
        lines.push(Line::from(vec![
            Span::styled("id ", Style::new().fg(Theme::TEXT_SECONDARY)),
            Span::styled(artifact_id, Style::new().fg(Theme::TEXT_PRIMARY)),
            Span::styled(" ", Style::new()),
            Span::styled(
                if stale { " STALE " } else { " FRESH " },
                if stale {
                    Style::new().fg(Color::Black).bg(Theme::ACCENT_RED).bold()
                } else {
                    Style::new().fg(Color::Black).bg(Theme::ACCENT_GREEN).bold()
                },
            ),
        ]));
        for reason in reasons.into_iter().take(3) {
            lines.push(Line::from(vec![
                Span::styled("  - ", Style::new().fg(Theme::ACCENT_YELLOW)),
                Span::styled(truncate(&reason, 96), Style::new().fg(Theme::TEXT_MUTED)),
            ]));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "none saved in this TUI session",
            Style::new().fg(Theme::TEXT_MUTED),
        )));
    }

    lines
}

#[derive(Debug, Clone)]
struct PayloadPreviewRow {
    session_id: String,
    tool: String,
    model: String,
    objective: Option<String>,
    duration_seconds: u64,
    next_actions_count: usize,
}

impl PayloadPreviewRow {
    fn short_session_id(&self) -> String {
        truncate(&self.session_id, 28)
    }

    fn objective_or_missing(&self) -> String {
        self.objective
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| "(missing)".to_string())
    }
}

fn payload_preview_rows(candidates: &[HandoffCandidate]) -> (Vec<PayloadPreviewRow>, Vec<String>) {
    let parsers = all_parsers();
    let mut rows = Vec::new();
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
                    None
                } else {
                    Some(summary.objective)
                };
                rows.push(PayloadPreviewRow {
                    session_id: summary.source_session_id,
                    tool: summary.tool,
                    model: summary.model,
                    objective,
                    duration_seconds: summary.duration_seconds,
                    next_actions_count: summary.execution_contract.next_actions.len(),
                });
            }
            Err(err) => warnings.push(format!(
                "failed to parse {} ({}): {err}",
                candidate.session_id,
                path.display()
            )),
        }
    }

    (rows, warnings)
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
        candidate_picker_meta_line, candidate_window, preview_lines, truncate, wrap_title_lines,
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
    fn candidate_picker_meta_line_includes_model_and_counts() {
        let candidate = sample_candidate();
        let line = candidate_picker_meta_line(&candidate);
        let meta = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(meta.contains("gpt-5-codex"));
        assert!(meta.contains("7 msgs"));
        assert!(meta.contains("12 ev"));
    }

    #[test]
    fn wrap_title_lines_wraps_to_two_rows() {
        let wrapped = wrap_title_lines(
            "Need environment conventions for implementation and runtime safety",
            24,
            2,
        );
        assert_eq!(wrapped.len(), 2);
        assert!(wrapped[0].chars().count() <= 24);
        assert!(wrapped[1].chars().count() <= 24);
    }

    #[test]
    fn preview_lines_show_payload_unavailable_when_source_is_missing() {
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
        assert!(text.contains("Payload snapshot (JSON array artifact)"));
        assert!(text.contains("(unavailable: source session file not resolvable)"));
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
