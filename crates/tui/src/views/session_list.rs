use crate::app::{App, ListLayout, ViewMode};
use crate::theme::{self, Theme};
use chrono::{DateTime, Datelike, Local, Utc};
use opensession_local_db::LocalSessionRow;
use ratatui::prelude::*;
use ratatui::widgets::{List, ListItem};

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
            if app.startup_status.config_exists {
                "No sessions found. Make sure you have Claude Code sessions in ~/.claude/projects/"
            } else {
                "No sessions yet. You can keep browsing locally, then configure sync in Settings > Workspace (4)."
            }
        } else {
            "No sessions match your search query."
        };
        render_empty(frame, area, msg, &app.view_mode);
        return;
    }

    let page_range = app.page_range();
    let items: Vec<ListItem> = app.filtered_sessions[page_range]
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
            let msgs = session.stats.user_message_count;
            let duration = format_duration(session.stats.duration_seconds);
            let date = format_relative_datetime(session.context.created_at);

            // Line 1: icon + title
            let line1 = Line::from(vec![
                Span::styled(
                    theme::tool_icon(tool),
                    Style::new().fg(theme::tool_color(tool)).bold(),
                ),
                Span::raw(" "),
                Span::styled(
                    truncate(title, 70),
                    Style::new().fg(Theme::TEXT_PRIMARY).bold(),
                ),
            ]);

            // Line 2: metadata with subtle separators
            let line2 = Line::from(vec![
                Span::raw("   "),
                Span::styled(date, Style::new().fg(Color::DarkGray)),
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
        .block(Theme::block_dim().title(title))
        .highlight_style(
            Style::new()
                .bg(Theme::BG_SURFACE)
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

    match app.list_layout {
        ListLayout::Single => render_db_single(frame, app, area),
        ListLayout::ByUser => render_db_multi_column(frame, app, area),
    }
}

fn render_db_single(frame: &mut Frame, app: &mut App, area: Rect) {
    let page_range = app.page_range();
    let items: Vec<ListItem> = app.db_sessions[page_range]
        .iter()
        .map(|row| db_row_to_list_item(row))
        .collect();

    let title = list_title(&app.view_mode);
    let list = List::new(items)
        .block(Theme::block_dim().title(title))
        .highlight_style(
            Style::new()
                .bg(Theme::BG_SURFACE)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(" > ")
        .highlight_spacing(ratatui::widgets::HighlightSpacing::Always);

    frame.render_stateful_widget(list, area, &mut app.list_state);
}

fn render_db_multi_column(frame: &mut Frame, app: &mut App, area: Rect) {
    let col_count = app.column_users.len().min(4);
    if col_count == 0 {
        render_db_single(frame, app, area);
        return;
    }

    let constraints: Vec<Constraint> = (0..col_count)
        .map(|_| Constraint::Ratio(1, col_count as u32))
        .collect();
    let columns = Layout::horizontal(constraints).split(area);

    for (col_idx, user) in app.column_users.clone().iter().take(col_count).enumerate() {
        let is_focused = col_idx == app.column_focus;
        let color = theme::user_color(user);

        let indices = app.column_session_indices(user);
        let items: Vec<ListItem> = indices
            .iter()
            .map(|&idx| db_row_to_compact_item(&app.db_sessions[idx]))
            .collect();

        let block = if is_focused {
            Theme::block_accent()
        } else {
            Theme::block_dim()
        }
        .title(format!(" @{} ({}) ", user, indices.len()))
        .title_style(Style::new().fg(color).bold());

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::new()
                    .bg(Theme::BG_SURFACE)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ")
            .highlight_spacing(ratatui::widgets::HighlightSpacing::Always);

        let state = app
            .column_list_states
            .get_mut(col_idx)
            .expect("column state missing");
        frame.render_stateful_widget(list, columns[col_idx], state);
    }
}

/// Compact list item for multi-column view (no nickname, shorter).
fn db_row_to_compact_item(row: &LocalSessionRow) -> ListItem<'static> {
    let title = row.title.as_deref().unwrap_or(&row.id);
    let tool = &row.tool;
    let date = format_relative_date_str(&row.created_at);

    let line1 = Line::from(vec![
        Span::styled(
            theme::tool_icon(tool),
            Style::new().fg(theme::tool_color(tool)).bold(),
        ),
        Span::styled(truncate(title, 40), Style::new().fg(Theme::TEXT_PRIMARY)),
    ]);

    let line2 = Line::from(vec![
        Span::raw("   "),
        Span::styled(date, Style::new().fg(Color::DarkGray)),
        Span::styled("  ", Style::new()),
        Span::styled(
            format!("{} msgs", row.user_message_count),
            Style::new().fg(Color::Green),
        ),
    ]);

    let line3 = Line::raw("");

    ListItem::new(vec![line1, line2, line3])
}

fn db_row_to_list_item(row: &LocalSessionRow) -> ListItem<'static> {
    let title = row.title.as_deref().unwrap_or(&row.id);
    let tool = &row.tool;
    let model = row.agent_model.as_deref().unwrap_or("-");
    let msgs = row.user_message_count;
    let events = row.event_count;
    let duration = format_duration(row.duration_seconds as u64);
    let date = format_relative_date_str(&row.created_at);

    // Sync status icon
    let sync_icon = match row.sync_status.as_str() {
        "local_only" => Span::styled(" L ", Style::new().fg(Color::Yellow)),
        "synced" => Span::styled(" S ", Style::new().fg(Color::Green)),
        "remote_only" => Span::styled(" R ", Style::new().fg(Color::Cyan)),
        _ => Span::styled(" ? ", Style::new().fg(Color::DarkGray)),
    };

    // Line 1: color bar + tool icon + sync icon + title + nickname
    let mut line1_spans = Vec::new();
    if let Some(ref nick) = row.nickname {
        let color = theme::user_color(nick);
        line1_spans.push(Span::styled("█", Style::new().fg(color)));
    }
    line1_spans.extend([
        Span::styled(
            theme::tool_icon(tool),
            Style::new().fg(theme::tool_color(tool)).bold(),
        ),
        sync_icon,
        Span::styled(
            truncate(title, 60),
            Style::new().fg(Theme::TEXT_PRIMARY).bold(),
        ),
    ]);
    if let Some(actor) = actor_label(row) {
        let color = theme::user_color(&actor);
        line1_spans.push(Span::styled(
            format!("  {actor}"),
            Style::new().fg(color).bold(),
        ));
    }
    let line1 = Line::from(line1_spans);

    // Line 2: date, model, stats, git info
    let mut line2_spans = vec![
        Span::raw("   "),
        Span::styled(date, Style::new().fg(Color::DarkGray)),
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

fn actor_label(row: &LocalSessionRow) -> Option<String> {
    if let Some(nick) = row.nickname.as_deref().filter(|s| !s.is_empty()) {
        return Some(format!("@{nick}"));
    }
    row.user_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|uid| format!("id:{}", truncate(uid, 10)))
}

fn render_empty(frame: &mut Frame, area: Rect, msg: &str, mode: &ViewMode) {
    let title = list_title(mode);
    let block = Theme::block_dim().title(title).padding(Theme::PADDING_CARD);
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

pub fn truncate(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len.saturating_sub(1)).collect();
        format!("{}…", truncated)
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

/// Format a UTC DateTime as a relative date string.
fn format_relative_datetime(dt: DateTime<Utc>) -> String {
    let local = dt.with_timezone(&Local);
    format_relative_local(local)
}

/// Format an ISO8601 date string as a relative date string.
fn format_relative_date_str(date_str: &str) -> String {
    if let Ok(dt) = DateTime::parse_from_rfc3339(date_str) {
        format_relative_local(dt.with_timezone(&Local))
    } else if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(date_str, "%Y-%m-%dT%H:%M:%S%.f") {
        format_relative_local(dt.and_utc().with_timezone(&Local))
    } else if date_str.len() > 10 {
        // Fallback: show truncated date
        date_str[5..date_str.len().min(16)].to_string()
    } else {
        date_str.to_string()
    }
}

fn format_relative_local(local: DateTime<Local>) -> String {
    let now = Local::now();
    let today = now.date_naive();
    let date = local.date_naive();
    let diff = today.signed_duration_since(date).num_days();

    if diff == 0 {
        // Today → show time only
        local.format("%H:%M").to_string()
    } else if diff == 1 {
        "yesterday".to_string()
    } else if diff <= 7 {
        format!("{}d ago", diff)
    } else if date.year() == today.year() {
        // Same year → MM/DD
        local.format("%m/%d").to_string()
    } else {
        // Different year → YY/MM/DD
        local.format("%y/%m/%d").to_string()
    }
}
