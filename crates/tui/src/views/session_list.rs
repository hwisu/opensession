use crate::app::{App, ListLayout, ViewMode};
use crate::theme::{self, Theme};
use chrono::{DateTime, Datelike, Local, Utc};
use opensession_local_db::LocalSessionRow;
use ratatui::prelude::*;
use ratatui::widgets::{List, ListItem, Paragraph};
use std::path::PathBuf;
use std::sync::OnceLock;

pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    match &app.view_mode {
        ViewMode::Local => match app.list_layout {
            ListLayout::Single => render_local_single(frame, app, area),
            ListLayout::ByUser => render_local_multi_column(frame, app, area),
        },
        _ => render_db(frame, app, area),
    }
}

/// Render the original local session list (from parsed Session objects).
fn render_local_single(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.filtered_sessions.is_empty() {
        let msg = if app.sessions.is_empty() {
            if app.startup_status.config_exists {
                "No sessions found. Make sure you have Claude Code sessions in ~/.claude/projects/"
            } else {
                "No sessions yet. You can keep browsing locally, then configure sync in Settings > Workspace."
            }
        } else if app.has_active_session_filters() {
            "No sessions match the current filters."
        } else {
            "No sessions in this view."
        };
        render_empty(frame, area, msg, app);
        return;
    }

    let page_range = app.page_range();
    let agent_counts = &app.session_max_active_agents;
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
            let max_agents = agent_counts
                .get(&session.session_id)
                .copied()
                .unwrap_or_else(|| if session.events.is_empty() { 0 } else { 1 });
            let date = format_relative_datetime(session.context.created_at);

            // Line 1: icon + title + actor
            let mut line1_spans = vec![
                Span::styled(
                    theme::tool_icon(tool),
                    Style::new().fg(theme::tool_color(tool)).bold(),
                ),
                Span::raw(" "),
                Span::styled(
                    truncate(title, 70),
                    Style::new().fg(Theme::TEXT_PRIMARY).bold(),
                ),
            ];
            if let Some(actor) = local_actor_label(session) {
                line1_spans.push(Span::styled(
                    format!("  {actor}"),
                    Style::new().fg(theme::user_color(&actor)).bold(),
                ));
            }
            let line1 = Line::from(line1_spans);

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
                Span::styled(
                    format!("{max_agents} agents"),
                    Style::new().fg(Theme::ACCENT_PURPLE),
                ),
                Span::styled("  ", Style::new().fg(Color::DarkGray)),
                Span::styled(duration, Style::new().fg(Color::Cyan)),
            ]);

            // Line 3: empty spacer
            let line3 = Line::raw("");

            ListItem::new(vec![line1, line2, line3])
        })
        .collect();

    let title = list_title(app);
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

fn render_local_multi_column(frame: &mut Frame, app: &mut App, area: Rect) {
    let total_cols = app.column_users.len();
    let (start_col, col_count) = column_viewport(area.width, total_cols, app.column_focus);
    if col_count == 0 {
        render_local_single(frame, app, area);
        return;
    }

    let [header_area, columns_area] = if area.height >= 3 {
        Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(area)
    } else {
        [Rect::new(area.x, area.y, area.width, 0), area]
    };
    if header_area.height > 0 {
        render_multi_column_header(frame, app, header_area, start_col, col_count);
    }

    let constraints: Vec<Constraint> = (0..col_count)
        .map(|_| Constraint::Ratio(1, col_count as u32))
        .collect();
    let columns = Layout::horizontal(constraints).split(columns_area);

    for (visible_idx, col_idx) in (start_col..start_col + col_count).enumerate() {
        let Some(label) = app.column_users.get(col_idx).cloned() else {
            continue;
        };
        let is_focused = col_idx == app.column_focus;
        let color = column_group_color(&label);
        let indices = app.column_session_indices(&label);
        let agent_counts = &app.session_max_active_agents;

        let items: Vec<ListItem> = indices
            .iter()
            .filter_map(|&abs_idx| {
                let &session_idx = app.filtered_sessions.get(abs_idx)?;
                let session = app.sessions.get(session_idx)?;
                let max_agents = agent_counts.get(&session.session_id).copied();
                Some(local_session_to_compact_item(session, max_agents))
            })
            .collect();

        let block = if is_focused {
            Theme::block_accent()
        } else {
            Theme::block_dim()
        }
        .title(format!(
            " {}/{} {} ({}) ",
            col_idx + 1,
            total_cols,
            label,
            indices.len()
        ))
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
        frame.render_stateful_widget(list, columns[visible_idx], state);
    }
}

/// Render DB-backed session list (Team or Repo views).
fn render_db(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.db_sessions.is_empty() {
        let msg = if app.has_active_session_filters() {
            "No sessions match the current filters."
        } else {
            "No sessions in this view."
        };
        render_empty(frame, area, msg, app);
        return;
    }

    match app.list_layout {
        ListLayout::Single => render_db_single(frame, app, area),
        ListLayout::ByUser => render_db_multi_column(frame, app, area),
    }
}

fn render_db_single(frame: &mut Frame, app: &mut App, area: Rect) {
    let page_range = app.page_range();
    let agent_counts = &app.session_max_active_agents;
    let items: Vec<ListItem> = app.db_sessions[page_range]
        .iter()
        .map(|row| {
            let max_agents = agent_counts.get(&row.id).copied();
            db_row_to_list_item(row, max_agents)
        })
        .collect();

    let title = list_title(app);
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
    let total_cols = app.column_users.len();
    let (start_col, col_count) = column_viewport(area.width, total_cols, app.column_focus);
    if col_count == 0 {
        render_db_single(frame, app, area);
        return;
    }

    let [header_area, columns_area] = if area.height >= 3 {
        Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(area)
    } else {
        [Rect::new(area.x, area.y, area.width, 0), area]
    };
    if header_area.height > 0 {
        render_multi_column_header(frame, app, header_area, start_col, col_count);
    }

    let constraints: Vec<Constraint> = (0..col_count)
        .map(|_| Constraint::Ratio(1, col_count as u32))
        .collect();
    let columns = Layout::horizontal(constraints).split(columns_area);

    for (visible_idx, col_idx) in (start_col..start_col + col_count).enumerate() {
        let Some(user) = app.column_users.get(col_idx).cloned() else {
            continue;
        };
        let is_focused = col_idx == app.column_focus;
        let color = column_group_color(&user);

        let indices = app.column_session_indices(&user);
        let agent_counts = &app.session_max_active_agents;
        let items: Vec<ListItem> = indices
            .iter()
            .map(|&idx| {
                let row = &app.db_sessions[idx];
                let max_agents = agent_counts.get(&row.id).copied();
                db_row_to_compact_item(row, max_agents)
            })
            .collect();

        let block = if is_focused {
            Theme::block_accent()
        } else {
            Theme::block_dim()
        }
        .title(format!(
            " {}/{} {} ({}) ",
            col_idx + 1,
            total_cols,
            user,
            indices.len()
        ))
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
        frame.render_stateful_widget(list, columns[visible_idx], state);
    }
}

fn column_group_color(label: &str) -> Color {
    let count = label
        .split_whitespace()
        .next()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(1);
    match count {
        1 => Theme::TEXT_SECONDARY,
        2 => Theme::ACCENT_BLUE,
        3 => Theme::ACCENT_CYAN,
        4 => Theme::ACCENT_TEAL,
        _ => Theme::ACCENT_PURPLE,
    }
}

fn local_session_to_compact_item(
    session: &opensession_core::trace::Session,
    max_agents: Option<usize>,
) -> ListItem<'static> {
    let title = session
        .context
        .title
        .as_deref()
        .unwrap_or(&session.session_id);
    let tool = &session.agent.tool;
    let date = format_relative_datetime(session.context.created_at);
    let agents = max_agents
        .map(|count| format!("{count} agents"))
        .unwrap_or_else(|| "? agents".to_string());

    let mut line1_spans = vec![
        Span::styled(
            theme::tool_icon(tool),
            Style::new().fg(theme::tool_color(tool)).bold(),
        ),
        Span::styled(truncate(title, 40), Style::new().fg(Theme::TEXT_PRIMARY)),
    ];
    if let Some(actor) = local_actor_label(session) {
        line1_spans.push(Span::styled(
            format!("  {actor}"),
            Style::new().fg(theme::user_color(&actor)).bold(),
        ));
    }
    let line1 = Line::from(line1_spans);

    let line2 = Line::from(vec![
        Span::raw("   "),
        Span::styled(date, Style::new().fg(Color::DarkGray)),
        Span::styled("  ", Style::new()),
        Span::styled(
            format!("{} msgs", session.stats.user_message_count),
            Style::new().fg(Color::Green),
        ),
        Span::styled("  ", Style::new()),
        Span::styled(agents, Style::new().fg(Theme::ACCENT_PURPLE)),
    ]);

    let line3 = Line::raw("");
    ListItem::new(vec![line1, line2, line3])
}

/// Compact list item for multi-column view (no nickname, shorter).
fn db_row_to_compact_item(row: &LocalSessionRow, max_agents: Option<usize>) -> ListItem<'static> {
    let title = row.title.as_deref().unwrap_or(&row.id);
    let tool = &row.tool;
    let date = format_relative_date_str(&row.created_at);
    let agents = max_agents
        .map(|count| format!("{count} agents"))
        .unwrap_or_else(|| "? agents".to_string());

    let mut line1_spans = vec![
        Span::styled(
            theme::tool_icon(tool),
            Style::new().fg(theme::tool_color(tool)).bold(),
        ),
        Span::styled(truncate(title, 40), Style::new().fg(Theme::TEXT_PRIMARY)),
    ];
    if let Some(actor) = actor_label(row) {
        line1_spans.push(Span::styled(
            format!("  {actor}"),
            Style::new().fg(theme::user_color(&actor)).bold(),
        ));
    }
    let line1 = Line::from(line1_spans);

    let line2 = Line::from(vec![
        Span::raw("   "),
        Span::styled(date, Style::new().fg(Color::DarkGray)),
        Span::styled("  ", Style::new()),
        Span::styled(
            format!("{} msgs", row.user_message_count),
            Style::new().fg(Color::Green),
        ),
        Span::styled("  ", Style::new()),
        Span::styled(agents, Style::new().fg(Theme::ACCENT_PURPLE)),
    ]);

    let line3 = Line::raw("");

    ListItem::new(vec![line1, line2, line3])
}

fn db_row_to_list_item(row: &LocalSessionRow, max_agents: Option<usize>) -> ListItem<'static> {
    let title = row.title.as_deref().unwrap_or(&row.id);
    let tool = &row.tool;
    let model = display_model(row);
    let msgs = row.user_message_count;
    let events = row.event_count;
    let duration = format_duration(row.duration_seconds as u64);
    let date = format_relative_date_str(&row.created_at);
    let agents = max_agents
        .map(|count| format!("{count} agents"))
        .unwrap_or_else(|| "? agents".to_string());

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
        Span::styled(model, Style::new().fg(Color::Blue)),
        Span::styled("  ", Style::new().fg(Color::DarkGray)),
        Span::styled(format!("{msgs} msgs"), Style::new().fg(Color::Green)),
        Span::styled("  ", Style::new().fg(Color::DarkGray)),
        Span::styled(format!("{events} events"), Style::new().fg(Color::Yellow)),
        Span::styled("  ", Style::new().fg(Color::DarkGray)),
        Span::styled(agents, Style::new().fg(Theme::ACCENT_PURPLE)),
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

fn local_actor_label(session: &opensession_core::trace::Session) -> Option<String> {
    let attrs = &session.context.attributes;
    if let Some(nick) = attrs
        .get("nickname")
        .or_else(|| attrs.get("user_nickname"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Some(format!("@{}", truncate(nick, 18)));
    }
    if let Some(uid) = attrs
        .get("user_id")
        .or_else(|| attrs.get("uid"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Some(format!("id:{}", truncate(uid, 10)));
    }
    attrs
        .get("originator")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|originator| truncate(originator, 18))
}

fn display_model(row: &LocalSessionRow) -> String {
    if let Some(model) = row
        .agent_model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty() && !model.eq_ignore_ascii_case("unknown"))
    {
        return model.to_string();
    }
    if row.tool.eq_ignore_ascii_case("codex") {
        if let Some(model) = codex_model_fallback() {
            return model;
        }
    }
    row.agent_model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .unwrap_or("-")
        .to_string()
}

fn codex_model_fallback() -> Option<String> {
    static CACHED_MODEL: OnceLock<Option<String>> = OnceLock::new();
    CACHED_MODEL
        .get_or_init(|| {
            let config_path = codex_config_path()?;
            let config_text = std::fs::read_to_string(config_path).ok()?;
            parse_codex_config_model(&config_text)
        })
        .clone()
}

fn codex_config_path() -> Option<PathBuf> {
    if let Ok(codex_home) = std::env::var("CODEX_HOME") {
        let codex_home = codex_home.trim();
        if !codex_home.is_empty() {
            return Some(PathBuf::from(codex_home).join("config.toml"));
        }
    }
    let home = std::env::var("HOME").ok()?;
    let home = home.trim();
    if home.is_empty() {
        return None;
    }
    Some(PathBuf::from(home).join(".codex").join("config.toml"))
}

fn parse_codex_config_model(config_toml: &str) -> Option<String> {
    let value: toml::Value = toml::from_str(config_toml).ok()?;
    let active_profile = value
        .get("profile")
        .or_else(|| value.get("default_profile"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(profile) = active_profile {
        if let Some(model) = value
            .get("profiles")
            .and_then(|profiles| profiles.get(profile))
            .and_then(|profile| profile.get("model"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Some(model.to_string());
        }
    }
    value
        .get("model")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn render_empty(frame: &mut Frame, area: Rect, msg: &str, app: &App) {
    let title = list_title(app);
    let block = Theme::block_dim().title(title).padding(Theme::PADDING_CARD);
    let paragraph = ratatui::widgets::Paragraph::new(msg)
        .block(block)
        .style(Style::new().fg(Color::DarkGray));
    frame.render_widget(paragraph, area);
}

fn list_title(app: &App) -> String {
    let mut base = match &app.view_mode {
        ViewMode::Local => " Sessions ".to_string(),
        ViewMode::Team(tid) => format!(" Team: {} ", truncate(tid, 30)),
        ViewMode::Repo(repo) => format!(" Repo: {} ", truncate(repo, 40)),
    };
    if let Some(tool) = app.active_tool_filter() {
        base.push_str(&format!("[tool:{tool}] "));
    }
    if !app.is_default_time_range() {
        base.push_str(&format!("[range:{}] ", app.session_time_range_label()));
    }
    if !app.is_default_sort() {
        base.push_str(&format!("[sort:{}] ", app.session_sort_label()));
    }
    if app.list_layout == ListLayout::ByUser {
        let total_cols = app.column_users.len();
        if total_cols == 0 {
            base.push_str("[group:agent-count(desc)] [cols:0] ");
        } else {
            base.push_str(&format!("[group:agent-count(desc)] [cols:{total_cols}] "));
        }
    }
    base
}

const MIN_MULTI_COLUMN_WIDTH: usize = 28;

fn column_viewport(area_width: u16, total_cols: usize, focus_col: usize) -> (usize, usize) {
    if total_cols == 0 {
        return (0, 0);
    }
    let width = usize::from(area_width).max(1);
    let max_visible = (width / MIN_MULTI_COLUMN_WIDTH).max(1);
    let visible = total_cols.min(max_visible);
    let focus = focus_col.min(total_cols - 1);
    let mut start = focus.saturating_add(1).saturating_sub(visible);
    let max_start = total_cols.saturating_sub(visible);
    if start > max_start {
        start = max_start;
    }
    (start, visible)
}

fn render_multi_column_header(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    start_col: usize,
    visible_cols: usize,
) {
    let total_cols = app.column_users.len();
    let overflow = total_cols.saturating_sub(visible_cols);
    let mut text = list_title(app).trim().to_string();
    if total_cols > 0 {
        let start = start_col + 1;
        let end = start_col + visible_cols;
        text.push_str(&format!("  view:{start}-{end}/{total_cols}"));
    }
    if overflow > 0 {
        text.push_str(&format!("  hidden:{overflow}"));
    }

    let paragraph = Paragraph::new(text).style(Style::new().fg(Theme::TEXT_MUTED));
    frame.render_widget(paragraph, area);
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

#[cfg(test)]
mod tests {
    use super::{column_viewport, list_title, MIN_MULTI_COLUMN_WIDTH};
    use crate::app::{App, ListLayout, ViewMode};

    #[test]
    fn list_title_includes_column_count_for_multi_column_layout() {
        let mut app = App::new(vec![]);
        app.view_mode = ViewMode::Local;
        app.list_layout = ListLayout::ByUser;
        app.column_users = vec![
            "4 agents".to_string(),
            "3 agents".to_string(),
            "2 agents".to_string(),
            "1 agent".to_string(),
            "5 agents".to_string(),
        ];

        let title = list_title(&app);
        assert!(title.contains("[group:agent-count(desc)]"));
        assert!(title.contains("[cols:5]"));
    }

    #[test]
    fn column_viewport_tracks_focused_column() {
        let width = (MIN_MULTI_COLUMN_WIDTH * 3) as u16;
        let (start, visible) = column_viewport(width, 7, 0);
        assert_eq!((start, visible), (0, 3));

        let (start, visible) = column_viewport(width, 7, 5);
        assert_eq!((start, visible), (3, 3));
    }
}
