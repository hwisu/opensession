use crossterm::event::KeyCode;
use opensession_core::trace::{EventType, Session};
use opensession_local_db::{LocalDb, LocalSessionFilter, LocalSessionRow};
use ratatui::widgets::ListState;
use std::collections::HashSet;
use std::sync::Arc;

use crate::config::{self, DaemonConfig, SettingField};

/// Which screen the user is viewing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    SessionList,
    SessionDetail,
    Setup,
    Settings,
}

/// Active event type filter options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventFilter {
    Messages,
    ToolCalls,
    Thinking,
    FileOps,
    Shell,
    All,
}

#[derive(Debug, Clone)]
pub struct ServerInfo {
    pub url: String,
    pub status: ServerStatus,
    pub last_upload: Option<String>,
}

#[derive(Debug, Clone)]
pub enum ServerStatus {
    Online(String), // version
    Offline,
    Unknown,
}

/// View mode selector — what set of sessions to display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewMode {
    /// Show local sessions only (file-parsed, original behaviour).
    Local,
    /// Show all sessions for the given team (includes remote_only from sync).
    Team(String),
    /// Show sessions grouped by a specific git repo name.
    Repo(String),
}

impl std::fmt::Display for ViewMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ViewMode::Local => write!(f, "Local"),
            ViewMode::Team(t) => write!(f, "Team: {t}"),
            ViewMode::Repo(r) => write!(f, "Repo: {r}"),
        }
    }
}

/// Startup status information shown in the header.
#[derive(Debug, Clone, Default)]
pub struct StartupStatus {
    pub sessions_cached: usize,
    pub repos_detected: usize,
    pub daemon_pid: Option<u32>,
    pub config_exists: bool,
}

pub struct App {
    // Sessions loaded from file parsing (original mode)
    pub sessions: Vec<Session>,
    pub filtered_sessions: Vec<usize>,
    pub view: View,

    // Session list state
    pub list_state: ListState,
    pub search_query: String,
    pub searching: bool,

    // Session detail state
    pub detail_scroll: u16,
    pub detail_event_index: usize,
    pub collapsed_tasks: HashSet<String>,
    pub event_filter: EventFilter,

    // Server connection info
    pub server_info: Option<ServerInfo>,

    // ── Local DB + view mode ──────────────────────────────────────
    pub db: Option<Arc<LocalDb>>,
    pub view_mode: ViewMode,
    /// DB-backed session list (for Team/Repo views).
    pub db_sessions: Vec<LocalSessionRow>,
    /// Available repos for Repo view cycling.
    pub repos: Vec<String>,
    /// Current repo index when cycling.
    pub repo_index: usize,
    /// Team ID from config (if any).
    pub team_id: Option<String>,

    // ── Config + Settings ─────────────────────────────────────────
    pub daemon_config: DaemonConfig,
    pub startup_status: StartupStatus,
    /// Index of selected field in settings/setup (among selectable items).
    pub settings_index: usize,
    /// Whether we're editing a text/number field inline.
    pub editing_field: bool,
    /// Buffer for inline text editing.
    pub edit_buffer: String,
    /// Whether settings have unsaved changes.
    pub config_dirty: bool,
    /// Transient message shown after save, etc.
    pub flash_message: Option<String>,
}

impl App {
    pub fn new(sessions: Vec<Session>) -> Self {
        let filtered: Vec<usize> = (0..sessions.len()).collect();
        let mut list_state = ListState::default();
        if !sessions.is_empty() {
            list_state.select(Some(0));
        }

        Self {
            sessions,
            filtered_sessions: filtered,
            view: View::SessionList,
            list_state,
            search_query: String::new(),
            searching: false,
            detail_scroll: 0,
            detail_event_index: 0,
            collapsed_tasks: HashSet::new(),
            event_filter: EventFilter::All,
            server_info: None,
            db: None,
            view_mode: ViewMode::Local,
            db_sessions: Vec::new(),
            repos: Vec::new(),
            repo_index: 0,
            team_id: None,
            daemon_config: DaemonConfig::default(),
            startup_status: StartupStatus::default(),
            settings_index: 0,
            editing_field: false,
            edit_buffer: String::new(),
            config_dirty: false,
            flash_message: None,
        }
    }

    /// Returns true if the app should quit.
    pub fn handle_key(&mut self, key: KeyCode) -> bool {
        // Clear flash message on any key press
        self.flash_message = None;

        if self.searching {
            return self.handle_search_key(key);
        }

        match self.view {
            View::SessionList => self.handle_list_key(key),
            View::SessionDetail => self.handle_detail_key(key),
            View::Setup => self.handle_setup_key(key),
            View::Settings => self.handle_settings_key(key),
        }
    }

    fn handle_search_key(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Esc => {
                self.searching = false;
                self.search_query.clear();
                self.apply_filter();
            }
            KeyCode::Enter => {
                self.searching = false;
                self.apply_filter();
            }
            KeyCode::Backspace => {
                self.search_query.pop();
                self.apply_filter();
            }
            KeyCode::Char(c) => {
                self.search_query.push(c);
                self.apply_filter();
            }
            _ => {}
        }
        false
    }

    fn handle_list_key(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Char('q') => return true,
            KeyCode::Char('j') | KeyCode::Down => self.list_next(),
            KeyCode::Char('k') | KeyCode::Up => self.list_prev(),
            KeyCode::Char('G') | KeyCode::End => self.list_end(),
            KeyCode::Char('g') | KeyCode::Home => self.list_start(),
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => self.enter_detail(),
            KeyCode::Char('/') => {
                self.searching = true;
            }
            KeyCode::Tab => self.cycle_view_mode(),
            KeyCode::Char('s') => {
                self.settings_index = 0;
                self.editing_field = false;
                self.config_dirty = false;
                self.view = View::Settings;
            }
            _ => {}
        }
        false
    }

    fn handle_detail_key(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Char('q') | KeyCode::Esc | KeyCode::Char('h') | KeyCode::Left => {
                self.view = View::SessionList;
                self.detail_scroll = 0;
                self.detail_event_index = 0;
            }
            KeyCode::Char('j') | KeyCode::Down => self.detail_next_event(),
            KeyCode::Char('k') | KeyCode::Up => self.detail_prev_event(),
            KeyCode::Char('G') | KeyCode::End => self.detail_end(),
            KeyCode::Char('g') | KeyCode::Home => {
                self.detail_event_index = 0;
                self.detail_scroll = 0;
            }
            KeyCode::Enter | KeyCode::Tab => self.toggle_task_fold(),
            KeyCode::Char('f') => self.cycle_event_filter(),
            KeyCode::Char('1') => self.set_event_filter(EventFilter::All),
            KeyCode::Char('2') => self.set_event_filter(EventFilter::Messages),
            KeyCode::Char('3') => self.set_event_filter(EventFilter::ToolCalls),
            KeyCode::Char('4') => self.set_event_filter(EventFilter::Thinking),
            KeyCode::Char('5') => self.set_event_filter(EventFilter::FileOps),
            KeyCode::Char('6') => self.set_event_filter(EventFilter::Shell),
            _ => {}
        }
        false
    }

    // ── Setup key handler ─────────────────────────────────────────────

    fn handle_setup_key(&mut self, key: KeyCode) -> bool {
        // Setup uses the same settings items but only shows the first 4 fields
        // (ServerUrl, ApiKey, TeamId, Nickname)
        const SETUP_FIELD_COUNT: usize = 4;
        let setup_fields = [
            SettingField::ServerUrl,
            SettingField::ApiKey,
            SettingField::TeamId,
            SettingField::Nickname,
        ];

        if self.editing_field {
            match key {
                KeyCode::Esc => {
                    self.editing_field = false;
                    self.edit_buffer.clear();
                }
                KeyCode::Enter => {
                    if let Some(&field) = setup_fields.get(self.settings_index) {
                        field.set_value(&mut self.daemon_config, &self.edit_buffer);
                    }
                    self.editing_field = false;
                    self.edit_buffer.clear();
                }
                KeyCode::Backspace => {
                    self.edit_buffer.pop();
                }
                KeyCode::Char(c) => {
                    self.edit_buffer.push(c);
                }
                _ => {}
            }
            return false;
        }

        match key {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.view = View::SessionList;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if self.settings_index + 1 < SETUP_FIELD_COUNT {
                    self.settings_index += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.settings_index = self.settings_index.saturating_sub(1);
            }
            KeyCode::Enter => {
                if let Some(&field) = setup_fields.get(self.settings_index) {
                    self.edit_buffer = field.raw_value(&self.daemon_config);
                    self.editing_field = true;
                }
            }
            KeyCode::Char('s') => {
                self.save_config();
            }
            _ => {}
        }
        false
    }

    // ── Settings key handler ──────────────────────────────────────────

    fn handle_settings_key(&mut self, key: KeyCode) -> bool {
        let field_count = config::selectable_field_count();

        if self.editing_field {
            match key {
                KeyCode::Esc => {
                    self.editing_field = false;
                    self.edit_buffer.clear();
                }
                KeyCode::Enter => {
                    if let Some(field) = config::nth_selectable_field(self.settings_index) {
                        field.set_value(&mut self.daemon_config, &self.edit_buffer);
                        self.config_dirty = true;
                    }
                    self.editing_field = false;
                    self.edit_buffer.clear();
                }
                KeyCode::Backspace => {
                    self.edit_buffer.pop();
                }
                KeyCode::Char(c) => {
                    self.edit_buffer.push(c);
                }
                _ => {}
            }
            return false;
        }

        match key {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.view = View::SessionList;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if self.settings_index + 1 < field_count {
                    self.settings_index += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.settings_index = self.settings_index.saturating_sub(1);
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(field) = config::nth_selectable_field(self.settings_index) {
                    if field.is_toggle() {
                        field.toggle(&mut self.daemon_config);
                        self.config_dirty = true;
                    } else if field.is_enum() {
                        field.cycle_enum(&mut self.daemon_config);
                        self.config_dirty = true;
                    } else {
                        // Text or number — enter edit mode
                        self.edit_buffer = field.raw_value(&self.daemon_config);
                        self.editing_field = true;
                    }
                }
            }
            KeyCode::Char('s') => {
                self.save_config();
            }
            _ => {}
        }
        false
    }

    fn save_config(&mut self) {
        match config::save_daemon_config(&self.daemon_config) {
            Ok(()) => {
                self.config_dirty = false;
                self.flash_message = Some("Config saved to daemon.toml".to_string());
                // Update team_id in case it changed
                let tid = &self.daemon_config.identity.team_id;
                self.team_id = if tid.is_empty() { None } else { Some(tid.clone()) };
            }
            Err(e) => {
                self.flash_message = Some(format!("Save failed: {e}"));
            }
        }
    }

    fn cycle_event_filter(&mut self) {
        self.event_filter = match self.event_filter {
            EventFilter::All => EventFilter::Messages,
            EventFilter::Messages => EventFilter::ToolCalls,
            EventFilter::ToolCalls => EventFilter::Thinking,
            EventFilter::Thinking => EventFilter::FileOps,
            EventFilter::FileOps => EventFilter::Shell,
            EventFilter::Shell => EventFilter::All,
        };
        self.detail_event_index = 0;
    }

    fn set_event_filter(&mut self, filter: EventFilter) {
        self.event_filter = filter;
        self.detail_event_index = 0;
    }

    // ── View mode cycling ──────────────────────────────────────────

    fn cycle_view_mode(&mut self) {
        let next = match &self.view_mode {
            ViewMode::Local => {
                if let Some(ref tid) = self.team_id {
                    ViewMode::Team(tid.clone())
                } else if !self.repos.is_empty() {
                    self.repo_index = 0;
                    ViewMode::Repo(self.repos[0].clone())
                } else {
                    return; // nothing to cycle to
                }
            }
            ViewMode::Team(_) => {
                if !self.repos.is_empty() {
                    self.repo_index = 0;
                    ViewMode::Repo(self.repos[0].clone())
                } else {
                    ViewMode::Local
                }
            }
            ViewMode::Repo(_) => {
                // Cycle through repos, then back to Local
                if self.repo_index + 1 < self.repos.len() {
                    self.repo_index += 1;
                    ViewMode::Repo(self.repos[self.repo_index].clone())
                } else {
                    ViewMode::Local
                }
            }
        };
        self.view_mode = next;
        self.reload_db_sessions();
        self.list_state.select(if self.session_count() > 0 {
            Some(0)
        } else {
            None
        });
    }

    /// Reload db_sessions for the current view_mode.
    pub fn reload_db_sessions(&mut self) {
        let Some(ref db) = self.db else { return };
        let filter = match &self.view_mode {
            ViewMode::Local => return, // Local mode uses self.sessions
            ViewMode::Team(tid) => LocalSessionFilter {
                team_id: Some(tid.clone()),
                ..Default::default()
            },
            ViewMode::Repo(repo) => LocalSessionFilter {
                git_repo_name: Some(repo.clone()),
                ..Default::default()
            },
        };
        match db.list_sessions(&filter) {
            Ok(rows) => self.db_sessions = rows,
            Err(e) => {
                eprintln!("DB error: {e}");
                self.db_sessions.clear();
            }
        }
    }

    /// Total visible session count for current view mode.
    pub fn session_count(&self) -> usize {
        match &self.view_mode {
            ViewMode::Local => self.filtered_sessions.len(),
            _ => self.db_sessions.len(),
        }
    }

    /// Returns true if the detail view should use DB data (no parsed Session available).
    pub fn is_db_view(&self) -> bool {
        !matches!(self.view_mode, ViewMode::Local)
    }

    // ── List navigation ─────────────────────────────────────────────────

    fn list_next(&mut self) {
        let count = self.session_count();
        if count == 0 {
            return;
        }
        let i = self
            .list_state
            .selected()
            .map(|i| (i + 1).min(count - 1))
            .unwrap_or(0);
        self.list_state.select(Some(i));
    }

    fn list_prev(&mut self) {
        let i = self
            .list_state
            .selected()
            .map(|i| i.saturating_sub(1))
            .unwrap_or(0);
        self.list_state.select(Some(i));
    }

    fn list_end(&mut self) {
        let count = self.session_count();
        if count > 0 {
            self.list_state.select(Some(count - 1));
        }
    }

    fn list_start(&mut self) {
        if self.session_count() > 0 {
            self.list_state.select(Some(0));
        }
    }

    fn enter_detail(&mut self) {
        if let Some(selected) = self.list_state.selected() {
            if selected < self.session_count() {
                // For DB views, we currently only enter detail if there's a matching parsed session.
                // Remote-only sessions would need body download (future work).
                if self.is_db_view() {
                    // For now, only enter detail for local-backed sessions
                    return;
                }
                self.view = View::SessionDetail;
                self.detail_scroll = 0;
                self.detail_event_index = 0;
                self.event_filter = EventFilter::All;
            }
        }
    }

    // ── Detail navigation ───────────────────────────────────────────────

    fn detail_next_event(&mut self) {
        if let Some(session) = self.selected_session() {
            let visible = self.visible_event_count(session);
            if visible > 0 && self.detail_event_index < visible - 1 {
                self.detail_event_index += 1;
            }
        }
    }

    fn detail_prev_event(&mut self) {
        self.detail_event_index = self.detail_event_index.saturating_sub(1);
    }

    fn detail_end(&mut self) {
        if let Some(session) = self.selected_session() {
            let visible = self.visible_event_count(session);
            if visible > 0 {
                self.detail_event_index = visible - 1;
            }
        }
    }

    fn toggle_task_fold(&mut self) {
        let task_id = if let Some(session) = self.selected_session() {
            let visible_events = self.get_visible_events(session);
            visible_events
                .get(self.detail_event_index)
                .and_then(|e| e.task_id.clone())
        } else {
            None
        };

        if let Some(tid) = task_id {
            if self.collapsed_tasks.contains(&tid) {
                self.collapsed_tasks.remove(&tid);
            } else {
                self.collapsed_tasks.insert(tid);
            }
        }
    }

    // ── Helpers ─────────────────────────────────────────────────────────

    pub fn selected_session(&self) -> Option<&Session> {
        self.list_state
            .selected()
            .and_then(|i| self.filtered_sessions.get(i))
            .and_then(|&idx| self.sessions.get(idx))
    }

    /// Get the selected DB session row (for Team/Repo views).
    #[allow(dead_code)]
    pub fn selected_db_session(&self) -> Option<&LocalSessionRow> {
        self.list_state
            .selected()
            .and_then(|i| self.db_sessions.get(i))
    }

    pub fn matches_event_filter(&self, event_type: &EventType) -> bool {
        match self.event_filter {
            EventFilter::All => true,
            EventFilter::Messages => matches!(
                event_type,
                EventType::UserMessage | EventType::AgentMessage | EventType::SystemMessage
            ),
            EventFilter::ToolCalls => matches!(
                event_type,
                EventType::ToolCall { .. }
                    | EventType::ToolResult { .. }
                    | EventType::FileRead { .. }
                    | EventType::CodeSearch { .. }
                    | EventType::FileSearch { .. }
            ),
            EventFilter::Thinking => matches!(event_type, EventType::Thinking),
            EventFilter::FileOps => matches!(
                event_type,
                EventType::FileEdit { .. }
                    | EventType::FileCreate { .. }
                    | EventType::FileDelete { .. }
                    | EventType::FileRead { .. }
            ),
            EventFilter::Shell => matches!(event_type, EventType::ShellCommand { .. }),
        }
    }

    pub fn get_visible_events<'a>(
        &self,
        session: &'a Session,
    ) -> Vec<&'a opensession_core::trace::Event> {
        session
            .events
            .iter()
            .filter(|e| self.matches_event_filter(&e.event_type))
            .collect()
    }

    fn visible_event_count(&self, session: &Session) -> usize {
        session
            .events
            .iter()
            .filter(|e| self.matches_event_filter(&e.event_type))
            .count()
    }

    fn apply_filter(&mut self) {
        let query = self.search_query.to_lowercase();
        if query.is_empty() {
            self.filtered_sessions = (0..self.sessions.len()).collect();
        } else {
            self.filtered_sessions = self
                .sessions
                .iter()
                .enumerate()
                .filter(|(_, s)| {
                    let title = s.context.title.as_deref().unwrap_or("").to_lowercase();
                    let tool = s.agent.tool.to_lowercase();
                    let model = s.agent.model.to_lowercase();
                    let sid = s.session_id.to_lowercase();
                    let tags = s.context.tags.join(" ").to_lowercase();

                    title.contains(&query)
                        || tool.contains(&query)
                        || model.contains(&query)
                        || sid.contains(&query)
                        || tags.contains(&query)
                })
                .map(|(i, _)| i)
                .collect();
        }

        // Reset selection
        if self.filtered_sessions.is_empty() {
            self.list_state.select(None);
        } else {
            self.list_state.select(Some(0));
        }
    }
}
