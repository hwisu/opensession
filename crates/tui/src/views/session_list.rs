use crate::app::App;
use ratatui::prelude::*;
use ratatui::widgets::{Block, List, ListItem, Padding};

pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.filtered_sessions.is_empty() {
        let msg = if app.sessions.is_empty() {
            "No sessions found. Make sure you have Claude Code sessions in ~/.claude/projects/"
        } else {
            "No sessions match your search query."
        };
        let block = Block::bordered()
            .title(" Sessions ")
            .padding(Padding::new(2, 2, 1, 1))
            .border_style(Style::new().fg(Color::DarkGray));
        let paragraph = ratatui::widgets::Paragraph::new(msg)
            .block(block)
            .style(Style::new().fg(Color::DarkGray));
        frame.render_widget(paragraph, area);
        return;
    }

    let items: Vec<ListItem> = app
        .filtered_sessions
        .iter()
        .map(|&idx| {
            let session = &app.sessions[idx];
            let title = session
                .context
                .title
                .as_deref()
                .unwrap_or(&session.session_id);

            let tool = &session.agent.tool;
            let model = &session.agent.model;
            let events = session.stats.event_count;
            let msgs = session.stats.message_count;
            let duration = format_duration(session.stats.duration_seconds);
            let date = session.context.created_at.format("%m/%d %H:%M");

            // Line 1: icon + title
            let line1 = Line::from(vec![
                Span::styled(tool_icon(tool), Style::new().fg(tool_color(tool)).bold()),
                Span::raw(" "),
                Span::styled(truncate(title, 70), Style::new().fg(Color::White).bold()),
            ]);

            // Line 2: metadata with subtle separators
            let line2 = Line::from(vec![
                Span::raw("   "),
                Span::styled(format!("{}", date), Style::new().fg(Color::DarkGray)),
                Span::styled("  ", Style::new().fg(Color::DarkGray)),
                Span::styled(model, Style::new().fg(Color::Blue)),
                Span::styled("  ", Style::new().fg(Color::DarkGray)),
                Span::styled(format!("{} msgs", msgs), Style::new().fg(Color::Green)),
                Span::styled("  ", Style::new().fg(Color::DarkGray)),
                Span::styled(format!("{} events", events), Style::new().fg(Color::Yellow)),
                Span::styled("  ", Style::new().fg(Color::DarkGray)),
                Span::styled(duration, Style::new().fg(Color::Cyan)),
            ]);

            // Line 3: empty spacer for breathing room
            let line3 = Line::raw("");

            ListItem::new(vec![line1, line2, line3])
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::bordered()
                .title(" Sessions ")
                .border_style(Style::new().fg(Color::DarkGray)),
        )
        .highlight_style(
            Style::new()
                .bg(Color::Rgb(30, 35, 50))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(" > ")
        .highlight_spacing(ratatui::widgets::HighlightSpacing::Always);

    frame.render_stateful_widget(list, area, &mut app.list_state);
}

fn tool_icon(tool: &str) -> &'static str {
    match tool {
        "claude-code" => " CC ",
        "codex" => " Cx ",
        "opencode" => " Oc ",
        "cline" => " Cl ",
        "amp" => " Ap ",
        "goose" => " Gs ",
        "aider" => " Ai ",
        "cursor" => " Cr ",
        _ => " ?? ",
    }
}

fn tool_color(tool: &str) -> Color {
    match tool {
        "claude-code" => Color::Rgb(217, 119, 80),
        "codex" => Color::Rgb(16, 185, 129),
        "opencode" => Color::Rgb(245, 158, 11),
        "cline" => Color::Rgb(239, 68, 68),
        "amp" => Color::Rgb(168, 85, 247),
        "goose" => Color::Rgb(200, 180, 80),
        "aider" => Color::Rgb(180, 120, 200),
        "cursor" => Color::Rgb(80, 180, 220),
        _ => Color::White,
    }
}

fn format_duration(seconds: u64) -> String {
    if seconds < 60 {
        format!("{}s", seconds)
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else {
        format!("{}h {}m", seconds / 3600, (seconds % 3600) / 60)
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len.saturating_sub(1)).collect();
        format!("{}…", truncated)
    }
}
