use crossterm::event::KeyCode;
use opensession_core::trace::{EventType, Session};
use ratatui::widgets::ListState;
use std::collections::HashSet;

/// Which screen the user is viewing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    SessionList,
    SessionDetail,
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

pub struct App {
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
        }
    }

    /// Returns true if the app should quit.
    pub fn handle_key(&mut self, key: KeyCode) -> bool {
        if self.searching {
            return self.handle_search_key(key);
        }

        match self.view {
            View::SessionList => self.handle_list_key(key),
            View::SessionDetail => self.handle_detail_key(key),
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

    // ── List navigation ─────────────────────────────────────────────────

    fn list_next(&mut self) {
        if self.filtered_sessions.is_empty() {
            return;
        }
        let i = self
            .list_state
            .selected()
            .map(|i| (i + 1).min(self.filtered_sessions.len() - 1))
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
        if !self.filtered_sessions.is_empty() {
            self.list_state
                .select(Some(self.filtered_sessions.len() - 1));
        }
    }

    fn list_start(&mut self) {
        if !self.filtered_sessions.is_empty() {
            self.list_state.select(Some(0));
        }
    }

    fn enter_detail(&mut self) {
        if let Some(selected) = self.list_state.selected() {
            if selected < self.filtered_sessions.len() {
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
