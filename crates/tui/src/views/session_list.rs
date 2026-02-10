use crate::app::{App, ViewMode};
use opensession_local_db::LocalSessionRow;
use ratatui::prelude::*;
use ratatui::widgets::{Block, List, ListItem, Padding};

pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    match &app.view_mode {
        ViewMode::Local => render_local(frame, app, area),
        _ => render_db(frame, app, area),
    }
}

/// Render the original local session list (from parsed Session objects).
fn render_local(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.filtered_sessions.is_empty() {
        let msg = if app.sessions.is_empty() {
            "No sessions found. Make sure you have Claude Code sessions in ~/.claude/projects/"
        } else {
            "No sessions match your search query."
        };
        render_empty(frame, area, msg, &app.view_mode);
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

            // Line 3: empty spacer
            let line3 = Line::raw("");

            ListItem::new(vec![line1, line2, line3])
        })
        .collect();

    let title = list_title(&app.view_mode);
    let list = List::new(items)
        .block(
            Block::bordered()
                .title(title)
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

/// Render DB-backed session list (Team or Repo views).
fn render_db(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.db_sessions.is_empty() {
        render_empty(frame, area, "No sessions in this view.", &app.view_mode);
        return;
    }

    let items: Vec<ListItem> = app
        .db_sessions
        .iter()
        .map(|row| db_row_to_list_item(row))
        .collect();

    let title = list_title(&app.view_mode);
    let list = List::new(items)
        .block(
            Block::bordered()
                .title(title)
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

fn db_row_to_list_item(row: &LocalSessionRow) -> ListItem<'static> {
    let title = row.title.as_deref().unwrap_or(&row.id);
    let tool = &row.tool;
    let model = row.agent_model.as_deref().unwrap_or("-");
    let msgs = row.message_count;
    let events = row.event_count;
    let duration = format_duration(row.duration_seconds as u64);
    let date = &row.created_at;

    // Sync status icon
    let sync_icon = match row.sync_status.as_str() {
        "local_only" => Span::styled(" L ", Style::new().fg(Color::Yellow)),
        "synced" => Span::styled(" S ", Style::new().fg(Color::Green)),
        "remote_only" => Span::styled(" R ", Style::new().fg(Color::Cyan)),
        _ => Span::styled(" ? ", Style::new().fg(Color::DarkGray)),
    };

    // Line 1: tool icon + sync icon + title + nickname
    let mut line1_spans = vec![
        Span::styled(
            tool_icon(tool),
            Style::new().fg(tool_color(tool)).bold(),
        ),
        sync_icon,
        Span::styled(
            truncate(title, 60),
            Style::new().fg(Color::White).bold(),
        ),
    ];
    if let Some(ref nick) = row.nickname {
        line1_spans.push(Span::styled(
            format!("  @{nick}"),
            Style::new().fg(Color::Rgb(140, 145, 160)),
        ));
    }
    let line1 = Line::from(line1_spans);

    // Line 2: date, model, stats, git info
    let date_display = if date.len() > 16 { &date[5..16] } else { date };
    let mut line2_spans = vec![
        Span::raw("   "),
        Span::styled(date_display.to_string(), Style::new().fg(Color::DarkGray)),
        Span::styled("  ", Style::new().fg(Color::DarkGray)),
        Span::styled(model.to_string(), Style::new().fg(Color::Blue)),
        Span::styled("  ", Style::new().fg(Color::DarkGray)),
        Span::styled(format!("{msgs} msgs"), Style::new().fg(Color::Green)),
        Span::styled("  ", Style::new().fg(Color::DarkGray)),
        Span::styled(format!("{events} events"), Style::new().fg(Color::Yellow)),
        Span::styled("  ", Style::new().fg(Color::DarkGray)),
        Span::styled(duration, Style::new().fg(Color::Cyan)),
    ];
    // Git branch info
    if let Some(ref branch) = row.git_branch {
        line2_spans.push(Span::styled("  ", Style::new().fg(Color::DarkGray)));
        line2_spans.push(Span::styled(
            truncate(branch, 20),
            Style::new().fg(Color::Magenta),
        ));
    }
    let line2 = Line::from(line2_spans);

    let line3 = Line::raw("");

    ListItem::new(vec![line1, line2, line3])
}

fn render_empty(frame: &mut Frame, area: Rect, msg: &str, mode: &ViewMode) {
    let title = list_title(mode);
    let block = Block::bordered()
        .title(title)
        .padding(Padding::new(2, 2, 1, 1))
        .border_style(Style::new().fg(Color::DarkGray));
    let paragraph = ratatui::widgets::Paragraph::new(msg)
        .block(block)
        .style(Style::new().fg(Color::DarkGray));
    frame.render_widget(paragraph, area);
}

fn list_title(mode: &ViewMode) -> String {
    match mode {
        ViewMode::Local => " Sessions ".to_string(),
        ViewMode::Team(tid) => format!(" Team: {} ", truncate(tid, 30)),
        ViewMode::Repo(repo) => format!(" Repo: {} ", truncate(repo, 40)),
    }
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
