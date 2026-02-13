use crossterm::event::KeyCode;
use opensession_api::{
    InvitationResponse, MemberResponse, TeamDetailResponse, TeamResponse, UserSettingsResponse,
};
use opensession_core::trace::{Event, EventType, Session};
use opensession_local_db::{LocalDb, LocalSessionFilter, LocalSessionRow};
use ratatui::widgets::ListState;
use std::collections::HashSet;
use std::sync::Arc;

use crate::async_ops::{AsyncCommand, CommandResult};
use crate::config::{self, DaemonConfig, GitStorageMethod, SettingField};
pub use crate::views::modal::{ConfirmAction, InputAction, Modal};

/// A display-level event for the timeline. Wraps real events with collapse/summary info.
#[derive(Debug, Clone)]
pub enum DisplayEvent<'a> {
    /// A single normal event.
    Single(&'a Event),
    /// A collapsed group of consecutive similar events.
    Collapsed {
        first: &'a Event,
        count: u32,
        kind: String,
    },
    /// A sub-agent task collapsed into a summary line.
    TaskSummary {
        event: &'a Event,
        summary: String,
        inner_count: u32,
    },
}

impl<'a> DisplayEvent<'a> {
    pub fn event(&self) -> &'a Event {
        match self {
            DisplayEvent::Single(e) => e,
            DisplayEvent::Collapsed { first, .. } => first,
            DisplayEvent::TaskSummary { event, .. } => event,
        }
    }
}

/// Returns a grouping key for consecutive-collapse. Same key = can be grouped.
fn consecutive_group_key(event_type: &EventType) -> Option<String> {
    match event_type {
        EventType::FileRead { .. } => Some("FileRead".to_string()),
        EventType::CodeSearch { .. } => Some("CodeSearch".to_string()),
        EventType::FileSearch { .. } => Some("FileSearch".to_string()),
        EventType::WebSearch { .. } => Some("WebSearch".to_string()),
        EventType::WebFetch { .. } => Some("WebFetch".to_string()),
        EventType::ToolResult { .. } => Some("ToolResult".to_string()),
        _ => None,
    }
}

/// Which screen the user is viewing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    SessionList,
    SessionDetail,
    Setup,
    Settings,
    Teams,
    TeamDetail,
    Invitations,
    Help,
}

/// Top-level tab navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Sessions,
    Teams,
    Invitations,
    Settings,
}

/// Focus section within TeamDetail view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamDetailFocus {
    Info,
    Members,
    Invite,
}

/// Settings sub-section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSection {
    Profile,
    Account,
    DaemonConfig,
}

/// Password change form state.
#[derive(Default)]
pub struct PasswordForm {
    pub field_index: usize, // 0=current, 1=new, 2=confirm
    pub current: String,
    pub new_password: String,
    pub confirm: String,
    pub editing: bool,
}

/// Setup sub-mode: API key entry vs email/password login.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupMode {
    ApiKey,
    Login,
}

/// State for the email/password login form.
#[derive(Default)]
pub struct LoginState {
    pub field_index: usize, // 0=email, 1=password
    pub email: String,
    pub password: String,
    pub editing: bool,
    pub status: Option<String>,
    pub loading: bool,
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

/// Flash message severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlashLevel {
    Success,
    Error,
    Info,
}

/// How sub-agent (Task) events are displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskViewMode {
    /// Collapse task into a summary line (default, matches Web "summary-start").
    Summary,
    /// Show all events in chronological order (matches Web "chronological").
    Detail,
}

/// Layout for the session list (single vs multi-column by user).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListLayout {
    #[default]
    Single,
    ByUser,
}

/// How the session detail timeline is displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailViewMode {
    /// Events in chronological order (default).
    Linear,
    /// Side-by-side user/agent turn view.
    Turn,
}

/// A single conversational turn: user prompt + agent response.
pub struct Turn<'a> {
    pub user_events: Vec<&'a Event>,
    pub agent_events: Vec<&'a Event>,
}

/// Extract turns from visible events. Each UserMessage starts a new turn.
pub fn extract_turns<'a>(events: &[DisplayEvent<'a>]) -> Vec<Turn<'a>> {
    let mut turns = Vec::new();
    let mut current_user: Vec<&'a Event> = Vec::new();
    let mut current_agent: Vec<&'a Event> = Vec::new();

    for de in events {
        let event = de.event();
        if matches!(event.event_type, EventType::UserMessage) {
            if !current_user.is_empty() || !current_agent.is_empty() {
                turns.push(Turn {
                    user_events: std::mem::take(&mut current_user),
                    agent_events: std::mem::take(&mut current_agent),
                });
            }
            current_user.push(event);
        } else {
            current_agent.push(event);
        }
    }

    if !current_user.is_empty() || !current_agent.is_empty() {
        turns.push(Turn {
            user_events: current_user,
            agent_events: current_agent,
        });
    }

    turns
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

/// Connection context — determines the badge and available features.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionContext {
    /// No server configured — local-only usage.
    Local,
    /// Connected to a local/Docker server.
    Docker { url: String },
    /// Connected to opensession.io (or cloud), personal mode.
    CloudPersonal,
    /// Connected to opensession.io (or cloud), team mode.
    CloudTeam { team_name: String },
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
    pub event_filters: HashSet<EventFilter>,
    pub task_view_mode: TaskViewMode,
    pub collapse_consecutive: bool,
    pub expanded_events: HashSet<usize>,
    pub detail_view_mode: DetailViewMode,
    pub turn_index: usize,
    pub turn_agent_scroll: u16,
    pub turn_line_offsets: Vec<u16>,
    pub expanded_turns: HashSet<usize>,

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

    // ── Tool filter ──────────────────────────────────────────────
    pub tool_filter: Option<String>,
    pub available_tools: Vec<String>,

    // ── Pagination ───────────────────────────────────────────────
    pub page: usize,
    pub per_page: usize,

    // ── Multi-column layout ──────────────────────────────────────
    pub list_layout: ListLayout,
    pub column_focus: usize,
    pub column_list_states: Vec<ListState>,
    pub column_users: Vec<String>,

    // ── Connection context ────────────────────────────────────────
    pub connection_ctx: ConnectionContext,

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
    pub flash_message: Option<(String, FlashLevel)>,

    // ── Setup login ──────────────────────────────────────────────
    pub setup_mode: SetupMode,
    pub login_state: LoginState,

    // ── Upload popup / Modal ────────────────────────────────────
    pub upload_popup: Option<UploadPopup>,
    pub modal: Option<Modal>,

    // ── Tab navigation ───────────────────────────────────────────
    pub active_tab: Tab,
    pub pending_command: Option<AsyncCommand>,

    // ── Teams ────────────────────────────────────────────────────
    pub teams: Vec<TeamResponse>,
    pub teams_list_state: ListState,
    pub teams_loading: bool,

    // ── Team detail ──────────────────────────────────────────────
    pub team_detail: Option<TeamDetailResponse>,
    pub team_members: Vec<MemberResponse>,
    pub team_members_list_state: ListState,
    pub team_detail_focus: TeamDetailFocus,
    pub invite_email: String,
    pub invite_editing: bool,
    /// Team ID of the team currently being viewed in detail.
    pub viewing_team_id: Option<String>,

    // ── Invitations ──────────────────────────────────────────────
    pub invitations: Vec<InvitationResponse>,
    pub invitations_list_state: ListState,
    pub invitations_loading: bool,

    // ── Profile / Account (Settings enhancement) ─────────────────
    pub settings_section: SettingsSection,
    pub profile: Option<UserSettingsResponse>,
    pub profile_loading: bool,
    pub profile_error: Option<String>,
    pub password_form: PasswordForm,

    // ── Deferred health check ──────────────────────────────────
    pub health_check_done: bool,

    // ── Background loading ───────────────────────────────────
    pub loading_sessions: bool,
}

/// State for the upload team-selection popup.
pub struct UploadPopup {
    pub teams: Vec<TeamInfo>,
    pub selected: usize,
    pub checked: Vec<bool>,
    pub status: Option<String>,
    pub phase: UploadPhase,
    pub results: Vec<(String, Result<String, String>)>,
    pub git_storage_ready: bool,
}

pub enum UploadPhase {
    FetchingTeams,
    SelectTeam,
    Uploading,
    Done,
}

pub struct TeamInfo {
    pub id: String,
    pub name: String,
    pub is_personal: bool,
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
            event_filters: HashSet::from([EventFilter::All]),
            task_view_mode: TaskViewMode::Summary,
            collapse_consecutive: false,
            expanded_events: HashSet::new(),
            detail_view_mode: DetailViewMode::Linear,
            turn_index: 0,
            turn_agent_scroll: 0,
            turn_line_offsets: Vec::new(),
            expanded_turns: HashSet::new(),
            server_info: None,
            db: None,
            view_mode: ViewMode::Local,
            db_sessions: Vec::new(),
            repos: Vec::new(),
            repo_index: 0,
            team_id: None,
            tool_filter: None,
            available_tools: Vec::new(),
            page: 0,
            per_page: 50,
            list_layout: ListLayout::default(),
            column_focus: 0,
            column_list_states: Vec::new(),
            column_users: Vec::new(),
            connection_ctx: ConnectionContext::Local,
            daemon_config: DaemonConfig::default(),
            startup_status: StartupStatus::default(),
            settings_index: 0,
            editing_field: false,
            edit_buffer: String::new(),
            config_dirty: false,
            flash_message: None,
            setup_mode: SetupMode::ApiKey,
            login_state: LoginState::default(),
            upload_popup: None,
            modal: None,
            active_tab: Tab::Sessions,
            pending_command: None,
            teams: Vec::new(),
            teams_list_state: ListState::default(),
            teams_loading: false,
            team_detail: None,
            team_members: Vec::new(),
            team_members_list_state: ListState::default(),
            team_detail_focus: TeamDetailFocus::Info,
            invite_email: String::new(),
            invite_editing: false,
            viewing_team_id: None,
            invitations: Vec::new(),
            invitations_list_state: ListState::default(),
            invitations_loading: false,
            settings_section: SettingsSection::DaemonConfig,
            profile: None,
            profile_loading: false,
            profile_error: None,
            password_form: PasswordForm::default(),
            health_check_done: false,
            loading_sessions: false,
        }
    }

    /// Returns true if the app should quit.
    pub fn handle_key(&mut self, key: KeyCode) -> bool {
        // Clear flash message on any key press
        self.flash_message = None;

        // Modal intercepts all keys when active
        if self.modal.is_some() {
            return self.handle_modal_key(key);
        }

        // Upload popup intercepts keys when active
        if self.upload_popup.is_some() {
            return self.handle_upload_popup_key(key);
        }

        if self.searching {
            return self.handle_search_key(key);
        }

        // Help overlay — `?` from any non-editing state
        if matches!(key, KeyCode::Char('?'))
            && !self.editing_field
            && !self.invite_editing
            && !self.password_form.editing
            && !self.searching
            && !matches!(self.view, View::Setup)
        {
            if self.view == View::Help {
                self.view = View::SessionList;
                self.active_tab = Tab::Sessions;
            } else {
                self.view = View::Help;
            }
            return false;
        }

        // Global tab switching (only when not in detail/setup/editing/searching)
        if !matches!(
            self.view,
            View::SessionDetail | View::Setup | View::TeamDetail | View::Help
        ) && !self.editing_field
            && !self.invite_editing
            && !self.password_form.editing
        {
            match key {
                KeyCode::Char('1') => {
                    self.switch_tab(Tab::Sessions);
                    return false;
                }
                KeyCode::Char('2') => {
                    self.switch_tab(Tab::Teams);
                    return false;
                }
                KeyCode::Char('3') => {
                    self.switch_tab(Tab::Invitations);
                    return false;
                }
                KeyCode::Char('4') => {
                    self.switch_tab(Tab::Settings);
                    return false;
                }
                _ => {}
            }
        }

        match self.view {
            View::SessionList => self.handle_list_key(key),
            View::SessionDetail => self.handle_detail_key(key),
            View::Setup => self.handle_setup_key(key),
            View::Settings => self.handle_settings_key(key),
            View::Teams => self.handle_teams_key(key),
            View::TeamDetail => self.handle_team_detail_key(key),
            View::Invitations => self.handle_invitations_key(key),
            View::Help => {
                // Any key exits help
                self.view = View::SessionList;
                self.active_tab = Tab::Sessions;
                false
            }
        }
    }

    fn switch_tab(&mut self, tab: Tab) {
        if self.active_tab == tab {
            return;
        }
        self.active_tab = tab;
        match tab {
            Tab::Sessions => {
                self.view = View::SessionList;
            }
            Tab::Teams => {
                self.view = View::Teams;
                if self.teams.is_empty() && !self.teams_loading {
                    self.teams_loading = true;
                    self.pending_command = Some(AsyncCommand::FetchTeams);
                }
            }
            Tab::Invitations => {
                self.view = View::Invitations;
                if self.invitations.is_empty() && !self.invitations_loading {
                    self.invitations_loading = true;
                    self.pending_command = Some(AsyncCommand::FetchInvitations);
                }
            }
            Tab::Settings => {
                self.view = View::Settings;
                self.settings_index = 0;
                self.editing_field = false;
                if self.profile.is_none() && !self.profile_loading {
                    self.profile_loading = true;
                    self.pending_command = Some(AsyncCommand::FetchProfile);
                }
            }
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
        // ByUser multi-column mode
        if self.list_layout == ListLayout::ByUser && self.is_db_view() {
            return self.handle_multi_column_key(key);
        }

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
            KeyCode::Char('m') => self.toggle_list_layout(),
            KeyCode::Char('p') => {
                // Open upload popup — only when connected to a server
                if matches!(self.connection_ctx, ConnectionContext::Local) {
                    self.flash_info("No server configured");
                } else if self.list_state.selected().is_some() {
                    let gs = &self.daemon_config.git_storage;
                    let git_storage_ready =
                        gs.method == GitStorageMethod::PlatformApi && !gs.token.is_empty();
                    self.upload_popup = Some(UploadPopup {
                        teams: Vec::new(),
                        selected: 0,
                        checked: Vec::new(),
                        status: Some("Fetching teams...".to_string()),
                        phase: UploadPhase::FetchingTeams,
                        results: Vec::new(),
                        git_storage_ready,
                    });
                }
            }
            KeyCode::Char('f') => {
                if self.is_db_view() {
                    self.cycle_tool_filter();
                }
            }
            KeyCode::Char(']') => self.next_page(),
            KeyCode::Char('[') => self.prev_page(),
            KeyCode::Char('d') => {
                if self.is_db_view() {
                    if let Some(row) = self.selected_db_session().cloned() {
                        if row.sync_status == "local_only" {
                            self.flash_info("Local-only session — delete from filesystem");
                        } else {
                            self.modal = Some(Modal::Confirm {
                                title: "Delete Session".to_string(),
                                message: format!(
                                    "Delete \"{}\"?",
                                    row.title.as_deref().unwrap_or(&row.id)
                                ),
                                action: ConfirmAction::DeleteSession {
                                    session_id: row.id.clone(),
                                },
                            });
                        }
                    }
                }
            }
            _ => {}
        }
        false
    }

    fn handle_multi_column_key(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Char('q') => return true,
            KeyCode::Char('h') | KeyCode::Left => {
                if self.column_focus > 0 {
                    self.column_focus -= 1;
                }
            }
            KeyCode::Char('l') | KeyCode::Right => {
                if self.column_focus + 1 < self.column_users.len() {
                    self.column_focus += 1;
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let user = self.column_users[self.column_focus].clone();
                let count = self.column_session_indices(&user).len();
                if let Some(state) = self.column_list_states.get_mut(self.column_focus) {
                    if count > 0 {
                        let current = state.selected().unwrap_or(0);
                        state.select(Some((current + 1).min(count - 1)));
                    }
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(state) = self.column_list_states.get_mut(self.column_focus) {
                    let current = state.selected().unwrap_or(0);
                    state.select(Some(current.saturating_sub(1)));
                }
            }
            KeyCode::Enter => {
                // Open the selected session from the focused column
                if let Some(user) = self.column_users.get(self.column_focus).cloned() {
                    let indices = self.column_session_indices(&user);
                    if let Some(state) = self.column_list_states.get(self.column_focus) {
                        if let Some(sel) = state.selected() {
                            if let Some(&db_idx) = indices.get(sel) {
                                // Set the main list_state to this db index so enter_detail works
                                self.list_state.select(Some(db_idx));
                                self.enter_detail();
                            }
                        }
                    }
                }
            }
            KeyCode::Char('m') => self.toggle_list_layout(),
            KeyCode::Tab => self.cycle_view_mode(),
            _ => {}
        }
        false
    }

    fn handle_detail_key(&mut self, key: KeyCode) -> bool {
        // Turn mode has its own key handling
        if self.detail_view_mode == DetailViewMode::Turn {
            return self.handle_turn_key(key);
        }

        match key {
            KeyCode::Char('q') | KeyCode::Esc | KeyCode::Char('h') | KeyCode::Left => {
                self.view = View::SessionList;
                self.detail_scroll = 0;
                self.detail_event_index = 0;
                self.detail_view_mode = DetailViewMode::Linear;
            }
            KeyCode::Char('j') | KeyCode::Down => self.detail_next_event(),
            KeyCode::Char('k') | KeyCode::Up => self.detail_prev_event(),
            KeyCode::Char('G') | KeyCode::End => self.detail_end(),
            KeyCode::Char('g') | KeyCode::Home => {
                self.detail_event_index = 0;
                self.detail_scroll = 0;
            }
            KeyCode::PageDown => self.detail_page_down(),
            KeyCode::PageUp => self.detail_page_up(),
            KeyCode::Enter | KeyCode::Char(' ') => self.toggle_expanded(),
            KeyCode::Tab => self.toggle_task_fold(),
            KeyCode::Char('u') => self.jump_to_next_user_message(),
            KeyCode::Char('U') => self.jump_to_prev_user_message(),
            KeyCode::Char('n') => self.jump_to_next_same_type(),
            KeyCode::Char('N') => self.jump_to_prev_same_type(),
            KeyCode::Char('v') => {
                self.detail_view_mode = DetailViewMode::Turn;
                self.sync_linear_to_turn();
            }
            KeyCode::Char('1') => self.toggle_event_filter(EventFilter::All),
            KeyCode::Char('2') => self.toggle_event_filter(EventFilter::Messages),
            KeyCode::Char('3') => self.toggle_event_filter(EventFilter::ToolCalls),
            KeyCode::Char('4') => self.toggle_event_filter(EventFilter::Thinking),
            KeyCode::Char('5') => self.toggle_event_filter(EventFilter::FileOps),
            KeyCode::Char('6') => self.toggle_event_filter(EventFilter::Shell),
            KeyCode::Char('t') => self.toggle_task_view_mode(),
            KeyCode::Char('c') => {
                self.collapse_consecutive = !self.collapse_consecutive;
                self.detail_event_index = 0;
            }
            _ => {}
        }
        false
    }

    // ── Setup key handler ─────────────────────────────────────────────

    fn handle_setup_key(&mut self, key: KeyCode) -> bool {
        match self.setup_mode {
            SetupMode::ApiKey => self.handle_setup_apikey_key(key),
            SetupMode::Login => self.handle_setup_login_key(key),
        }
    }

    fn handle_setup_apikey_key(&mut self, key: KeyCode) -> bool {
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
                    // Auto-save and transition when API key is set
                    if !self.daemon_config.server.api_key.is_empty() {
                        self.save_config();
                        self.connection_ctx = Self::derive_connection_ctx(&self.daemon_config);
                        self.view = View::SessionList;
                        self.active_tab = Tab::Sessions;
                    }
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
                // Auto-save if API key has been set
                if !self.daemon_config.server.api_key.is_empty() {
                    self.save_config();
                    self.connection_ctx = Self::derive_connection_ctx(&self.daemon_config);
                }
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
            KeyCode::Tab => {
                self.setup_mode = SetupMode::Login;
                self.settings_index = 0;
                self.editing_field = false;
                self.edit_buffer.clear();
            }
            _ => {}
        }
        false
    }

    fn handle_setup_login_key(&mut self, key: KeyCode) -> bool {
        if self.login_state.loading {
            return false; // block input while loading
        }

        if self.login_state.editing {
            match key {
                KeyCode::Esc => {
                    self.login_state.editing = false;
                }
                KeyCode::Enter => {
                    // Save the edit buffer into the appropriate field
                    match self.login_state.field_index {
                        0 => self.login_state.email = self.edit_buffer.clone(),
                        1 => self.login_state.password = self.edit_buffer.clone(),
                        _ => {}
                    }
                    self.login_state.editing = false;
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
                if self.login_state.field_index < 1 {
                    self.login_state.field_index += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.login_state.field_index = self.login_state.field_index.saturating_sub(1);
            }
            KeyCode::Enter => {
                // Enter edit mode for current field
                self.edit_buffer = match self.login_state.field_index {
                    0 => self.login_state.email.clone(),
                    1 => self.login_state.password.clone(),
                    _ => String::new(),
                };
                self.login_state.editing = true;
            }
            KeyCode::Char('l') => {
                // Trigger login
                if !self.login_state.email.is_empty() && !self.login_state.password.is_empty() {
                    self.login_state.loading = true;
                    self.login_state.status = Some("Logging in...".to_string());
                }
            }
            KeyCode::Tab => {
                self.setup_mode = SetupMode::ApiKey;
                self.settings_index = 0;
                self.editing_field = false;
                self.edit_buffer.clear();
            }
            _ => {}
        }
        false
    }

    // ── Upload popup key handler ─────────────────────────────────────

    fn handle_upload_popup_key(&mut self, key: KeyCode) -> bool {
        let popup = self.upload_popup.as_mut().unwrap();
        match &popup.phase {
            UploadPhase::FetchingTeams | UploadPhase::Uploading => {
                // Only allow escape while loading
                if matches!(key, KeyCode::Esc) {
                    self.upload_popup = None;
                }
            }
            UploadPhase::SelectTeam => match key {
                KeyCode::Esc => {
                    self.upload_popup = None;
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    if !popup.teams.is_empty() && popup.selected + 1 < popup.teams.len() {
                        popup.selected += 1;
                    }
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    popup.selected = popup.selected.saturating_sub(1);
                }
                KeyCode::Char(' ') => {
                    // Toggle check on current item
                    let is_personal = popup
                        .teams
                        .get(popup.selected)
                        .is_some_and(|t| t.is_personal);
                    if is_personal && !popup.git_storage_ready {
                        popup.status = Some(
                            "Git storage required for personal upload (Settings > Git Storage)"
                                .into(),
                        );
                    } else if let Some(c) = popup.checked.get_mut(popup.selected) {
                        *c = !*c;
                        popup.status = None;
                    }
                }
                KeyCode::Char('a') => {
                    // Toggle all: if any checked, uncheck all; else check all
                    let any_checked = popup.checked.iter().any(|&c| c);
                    let new_val = !any_checked;
                    for (i, c) in popup.checked.iter_mut().enumerate() {
                        let is_personal = popup.teams.get(i).is_some_and(|t| t.is_personal);
                        if new_val && is_personal && !popup.git_storage_ready {
                            *c = false;
                        } else {
                            *c = new_val;
                        }
                    }
                }
                KeyCode::Enter => {
                    let checked_count = popup.checked.iter().filter(|&&c| c).count();
                    if checked_count > 0 {
                        popup.phase = UploadPhase::Uploading;
                        popup.results.clear();
                        popup.status = Some("Uploading...".to_string());
                    }
                }
                _ => {}
            },
            UploadPhase::Done => {
                // Any key dismisses
                self.upload_popup = None;
            }
        }
        false
    }

    // ── Settings key handler ──────────────────────────────────────────

    fn handle_settings_key(&mut self, key: KeyCode) -> bool {
        // Password form editing
        if self.password_form.editing {
            match key {
                KeyCode::Esc => {
                    self.password_form.editing = false;
                    self.edit_buffer.clear();
                }
                KeyCode::Enter => {
                    match self.password_form.field_index {
                        0 => self.password_form.current = self.edit_buffer.clone(),
                        1 => self.password_form.new_password = self.edit_buffer.clone(),
                        2 => self.password_form.confirm = self.edit_buffer.clone(),
                        _ => {}
                    }
                    self.password_form.editing = false;
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

        // DaemonConfig field editing
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
                if self.config_dirty {
                    self.modal = Some(Modal::Confirm {
                        title: "Unsaved Changes".to_string(),
                        message: "You have unsaved changes. Save before leaving?".to_string(),
                        action: ConfirmAction::SaveChanges,
                    });
                } else {
                    self.view = View::SessionList;
                    self.active_tab = Tab::Sessions;
                }
            }
            KeyCode::Char(']') => {
                // Next settings section
                self.settings_section = match self.settings_section {
                    SettingsSection::Profile => SettingsSection::Account,
                    SettingsSection::Account => SettingsSection::DaemonConfig,
                    SettingsSection::DaemonConfig => SettingsSection::Profile,
                };
                self.settings_index = 0;
            }
            KeyCode::Char('[') => {
                // Previous settings section
                self.settings_section = match self.settings_section {
                    SettingsSection::Profile => SettingsSection::DaemonConfig,
                    SettingsSection::Account => SettingsSection::Profile,
                    SettingsSection::DaemonConfig => SettingsSection::Account,
                };
                self.settings_index = 0;
            }
            _ => {
                // Delegate to section-specific handling
                match self.settings_section {
                    SettingsSection::Profile => {
                        // Profile is read-only, only r to refresh
                        if matches!(key, KeyCode::Char('r')) {
                            self.profile_loading = true;
                            self.pending_command = Some(AsyncCommand::FetchProfile);
                        }
                    }
                    SettingsSection::Account => {
                        self.handle_account_settings_key(key);
                    }
                    SettingsSection::DaemonConfig => {
                        self.handle_daemon_config_key(key);
                    }
                }
            }
        }
        false
    }

    fn handle_account_settings_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Char('j') | KeyCode::Down => {
                if self.settings_index < 4 {
                    // 0..4: current/new/confirm/submit + api_key_regen
                    self.settings_index += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.settings_index = self.settings_index.saturating_sub(1);
            }
            KeyCode::Enter => {
                match self.settings_index {
                    0..=2 => {
                        // Enter password field edit mode
                        self.password_form.field_index = self.settings_index;
                        self.edit_buffer = match self.settings_index {
                            0 => self.password_form.current.clone(),
                            1 => self.password_form.new_password.clone(),
                            2 => self.password_form.confirm.clone(),
                            _ => String::new(),
                        };
                        self.password_form.editing = true;
                    }
                    3 => {
                        // Submit password change
                        if self.password_form.new_password != self.password_form.confirm {
                            self.flash_error("Passwords do not match");
                        } else if self.password_form.new_password.is_empty() {
                            self.flash_error("New password is required");
                        } else {
                            self.pending_command = Some(AsyncCommand::ChangePassword {
                                current: self.password_form.current.clone(),
                                new_password: self.password_form.new_password.clone(),
                            });
                            self.password_form = PasswordForm::default();
                        }
                    }
                    _ => {}
                }
            }
            KeyCode::Char('r') => {
                // Regenerate API key — confirm
                self.modal = Some(Modal::Confirm {
                    title: "Regenerate API Key".to_string(),
                    message: "This will invalidate your current API key.".to_string(),
                    action: ConfirmAction::RegenerateApiKey,
                });
            }
            _ => {}
        }
    }

    fn handle_daemon_config_key(&mut self, key: KeyCode) {
        let field_count = config::selectable_field_count();
        let daemon_running = self.startup_status.daemon_pid.is_some();

        match key {
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
                    // Block editing daemon-dependent fields when daemon is off
                    if !daemon_running && Self::is_daemon_dependent_field(field) {
                        self.flash_info("Daemon is not running");
                        return;
                    }
                    // Block editing Git Storage Token when method is None
                    if field == SettingField::GitStorageToken
                        && self.daemon_config.git_storage.method == GitStorageMethod::None
                    {
                        self.flash_info("Set Git Storage method first");
                        return;
                    }
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
            KeyCode::Char('d') => {
                self.toggle_daemon();
            }
            _ => {}
        }
    }

    fn toggle_daemon(&mut self) {
        if self.startup_status.daemon_pid.is_some() {
            self.stop_daemon();
        } else {
            self.start_daemon();
        }
    }

    fn start_daemon(&mut self) {
        match std::process::Command::new("opensession-daemon")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(_) => {
                std::thread::sleep(std::time::Duration::from_millis(500));
                self.startup_status.daemon_pid = config::daemon_pid();
                if self.startup_status.daemon_pid.is_some() {
                    self.flash_success("Daemon started");
                } else {
                    self.flash_error("Daemon started but PID not found");
                }
            }
            Err(e) => {
                self.flash_error(format!("Failed to start daemon: {e}"));
            }
        }
    }

    fn stop_daemon(&mut self) {
        if let Some(pid) = self.startup_status.daemon_pid {
            let _ = std::process::Command::new("kill")
                .arg(pid.to_string())
                .status();
            std::thread::sleep(std::time::Duration::from_millis(300));
            self.startup_status.daemon_pid = config::daemon_pid();
            if self.startup_status.daemon_pid.is_none() {
                self.flash_success("Daemon stopped");
            } else {
                self.flash_error("Daemon may still be running");
            }
        }
    }

    fn is_daemon_dependent_field(field: SettingField) -> bool {
        matches!(
            field,
            SettingField::AutoPublish
                | SettingField::PublishMode
                | SettingField::DebounceSecs
                | SettingField::HealthCheckSecs
                | SettingField::MaxRetries
                | SettingField::WatchClaudeCode
                | SettingField::WatchOpenCode
                | SettingField::WatchGoose
                | SettingField::WatchAider
                | SettingField::WatchCursor
        )
    }

    // ── Teams key handler ─────────────────────────────────────────────

    fn handle_teams_key(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Char('q') => return true,
            KeyCode::Esc => {
                self.switch_tab(Tab::Sessions);
                return false;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if !self.teams.is_empty() {
                    let i = self
                        .teams_list_state
                        .selected()
                        .map(|i| (i + 1).min(self.teams.len() - 1))
                        .unwrap_or(0);
                    self.teams_list_state.select(Some(i));
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let i = self
                    .teams_list_state
                    .selected()
                    .map(|i| i.saturating_sub(1))
                    .unwrap_or(0);
                self.teams_list_state.select(Some(i));
            }
            KeyCode::Enter => {
                if let Some(idx) = self.teams_list_state.selected() {
                    if let Some(team) = self.teams.get(idx) {
                        let team_id = team.id.clone();
                        self.viewing_team_id = Some(team_id.clone());
                        self.team_detail = None;
                        self.team_members.clear();
                        self.team_members_list_state = ListState::default();
                        self.team_detail_focus = TeamDetailFocus::Info;
                        self.invite_email.clear();
                        self.invite_editing = false;
                        self.view = View::TeamDetail;
                        self.pending_command = Some(AsyncCommand::FetchTeamDetail(team_id));
                    }
                }
            }
            KeyCode::Char('n') => {
                // Open create-team modal
                self.edit_buffer.clear();
                self.modal = Some(Modal::TextInput {
                    title: "Create Team".to_string(),
                    label: "Team Name".to_string(),
                    action: InputAction::CreateTeam,
                });
            }
            KeyCode::Char('r') => {
                self.teams_loading = true;
                self.pending_command = Some(AsyncCommand::FetchTeams);
            }
            _ => {}
        }
        false
    }

    // ── Team detail key handler ──────────────────────────────────────

    fn handle_team_detail_key(&mut self, key: KeyCode) -> bool {
        // If editing invite email
        if self.invite_editing {
            match key {
                KeyCode::Esc => {
                    self.invite_editing = false;
                }
                KeyCode::Enter => {
                    if !self.invite_email.is_empty() {
                        if let Some(ref tid) = self.viewing_team_id {
                            self.pending_command = Some(AsyncCommand::InviteMember {
                                team_id: tid.clone(),
                                email: self.invite_email.clone(),
                            });
                        }
                        self.invite_editing = false;
                    }
                }
                KeyCode::Backspace => {
                    self.invite_email.pop();
                }
                KeyCode::Char(c) => {
                    self.invite_email.push(c);
                }
                _ => {}
            }
            return false;
        }

        match key {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.view = View::Teams;
                self.active_tab = Tab::Teams;
            }
            KeyCode::Tab => {
                self.team_detail_focus = match self.team_detail_focus {
                    TeamDetailFocus::Info => TeamDetailFocus::Members,
                    TeamDetailFocus::Members => TeamDetailFocus::Invite,
                    TeamDetailFocus::Invite => TeamDetailFocus::Info,
                };
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if matches!(self.team_detail_focus, TeamDetailFocus::Members)
                    && !self.team_members.is_empty()
                {
                    let i = self
                        .team_members_list_state
                        .selected()
                        .map(|i| (i + 1).min(self.team_members.len() - 1))
                        .unwrap_or(0);
                    self.team_members_list_state.select(Some(i));
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if matches!(self.team_detail_focus, TeamDetailFocus::Members) {
                    let i = self
                        .team_members_list_state
                        .selected()
                        .map(|i| i.saturating_sub(1))
                        .unwrap_or(0);
                    self.team_members_list_state.select(Some(i));
                }
            }
            KeyCode::Char('d') => {
                // Remove member — show confirm modal
                if matches!(self.team_detail_focus, TeamDetailFocus::Members) {
                    if let Some(idx) = self.team_members_list_state.selected() {
                        if let Some(member) = self.team_members.get(idx) {
                            if let Some(ref tid) = self.viewing_team_id {
                                self.modal = Some(Modal::Confirm {
                                    title: "Remove Member".to_string(),
                                    message: format!("Remove @{} from team?", member.nickname),
                                    action: ConfirmAction::RemoveMember {
                                        team_id: tid.clone(),
                                        user_id: member.user_id.clone(),
                                    },
                                });
                            }
                        }
                    }
                }
            }
            KeyCode::Enter => {
                if matches!(self.team_detail_focus, TeamDetailFocus::Invite) {
                    self.invite_editing = true;
                }
            }
            KeyCode::Char('r') => {
                // Refresh team detail
                if let Some(ref tid) = self.viewing_team_id {
                    self.pending_command = Some(AsyncCommand::FetchTeamDetail(tid.clone()));
                }
            }
            _ => {}
        }
        false
    }

    // ── Invitations key handler ──────────────────────────────────────

    fn handle_invitations_key(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Char('q') => return true,
            KeyCode::Esc => {
                self.switch_tab(Tab::Sessions);
                return false;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if !self.invitations.is_empty() {
                    let i = self
                        .invitations_list_state
                        .selected()
                        .map(|i| (i + 1).min(self.invitations.len() - 1))
                        .unwrap_or(0);
                    self.invitations_list_state.select(Some(i));
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let i = self
                    .invitations_list_state
                    .selected()
                    .map(|i| i.saturating_sub(1))
                    .unwrap_or(0);
                self.invitations_list_state.select(Some(i));
            }
            KeyCode::Char('a') => {
                // Accept invitation
                if let Some(idx) = self.invitations_list_state.selected() {
                    if let Some(inv) = self.invitations.get(idx) {
                        if inv.status == opensession_api::InvitationStatus::Pending {
                            let id = inv.id.clone();
                            self.pending_command = Some(AsyncCommand::AcceptInvitation(id));
                        }
                    }
                }
            }
            KeyCode::Char('d') => {
                // Decline invitation — confirm modal
                if let Some(idx) = self.invitations_list_state.selected() {
                    if let Some(inv) = self.invitations.get(idx) {
                        if inv.status == opensession_api::InvitationStatus::Pending {
                            self.modal = Some(Modal::Confirm {
                                title: "Decline Invitation".to_string(),
                                message: format!("Decline invitation to {}?", inv.team_name),
                                action: ConfirmAction::DeclineInvitation(inv.id.clone()),
                            });
                        }
                    }
                }
            }
            KeyCode::Char('r') => {
                self.invitations_loading = true;
                self.pending_command = Some(AsyncCommand::FetchInvitations);
            }
            _ => {}
        }
        false
    }

    // ── Modal key handler ────────────────────────────────────────────

    fn handle_modal_key(&mut self, key: KeyCode) -> bool {
        let modal = match self.modal.take() {
            Some(m) => m,
            None => return false,
        };

        match modal {
            Modal::Confirm { action, .. } => match key {
                KeyCode::Char('y') | KeyCode::Enter => {
                    // Execute the confirmed action
                    match action {
                        ConfirmAction::RemoveMember { team_id, user_id } => {
                            self.pending_command =
                                Some(AsyncCommand::RemoveMember { team_id, user_id });
                        }
                        ConfirmAction::RegenerateApiKey => {
                            self.pending_command = Some(AsyncCommand::RegenerateApiKey);
                        }
                        ConfirmAction::DeclineInvitation(id) => {
                            self.pending_command = Some(AsyncCommand::DeclineInvitation(id));
                        }
                        ConfirmAction::DeleteSession { session_id } => {
                            self.pending_command = Some(AsyncCommand::DeleteSession { session_id });
                        }
                        ConfirmAction::SaveChanges => {
                            self.save_config();
                            self.view = View::SessionList;
                            self.active_tab = Tab::Sessions;
                        }
                    }
                    // modal already taken
                }
                KeyCode::Char('n') => {
                    // For SaveChanges: discard changes and exit
                    if matches!(action, ConfirmAction::SaveChanges) {
                        self.daemon_config = config::load_daemon_config();
                        self.config_dirty = false;
                        self.view = View::SessionList;
                        self.active_tab = Tab::Sessions;
                    }
                    // For other modals: same as cancel
                }
                KeyCode::Esc => {
                    // Cancel — modal already removed, stay in current view
                }
                _ => {
                    // Put modal back
                    self.modal = Some(Modal::Confirm {
                        title: String::new(),
                        message: String::new(),
                        action,
                    });
                }
            },
            Modal::TextInput { action, .. } => match key {
                KeyCode::Esc => {
                    self.edit_buffer.clear();
                }
                KeyCode::Enter => {
                    let value = self.edit_buffer.clone();
                    self.edit_buffer.clear();
                    if !value.is_empty() {
                        match action {
                            InputAction::CreateTeam => {
                                self.pending_command =
                                    Some(AsyncCommand::CreateTeam { name: value });
                            }
                        }
                    }
                }
                KeyCode::Backspace => {
                    self.edit_buffer.pop();
                    self.modal = Some(Modal::TextInput {
                        title: String::new(),
                        label: String::new(),
                        action,
                    });
                }
                KeyCode::Char(c) => {
                    self.edit_buffer.push(c);
                    self.modal = Some(Modal::TextInput {
                        title: String::new(),
                        label: String::new(),
                        action,
                    });
                }
                _ => {
                    self.modal = Some(Modal::TextInput {
                        title: String::new(),
                        label: String::new(),
                        action,
                    });
                }
            },
        }
        false
    }

    // ── Apply async command result ────────────────────────────────────

    pub fn apply_command_result(&mut self, result: CommandResult) {
        match result {
            CommandResult::Login(Ok((api_key, nickname))) => {
                self.daemon_config.server.api_key = api_key;
                self.daemon_config.identity.nickname = nickname;
                self.config_dirty = true;
                self.save_config();
                self.connection_ctx = Self::derive_connection_ctx(&self.daemon_config);
                self.login_state.loading = false;
                self.login_state.status = Some("Login successful!".to_string());
                self.view = View::SessionList;
                self.active_tab = Tab::Sessions;
            }
            CommandResult::Login(Err(e)) => {
                self.login_state.loading = false;
                self.login_state.status = Some(format!("Error: {e}"));
            }

            CommandResult::UploadTeams(Ok(teams)) => {
                let allows_public = self.selected_session_allows_public();
                if let Some(ref mut popup) = self.upload_popup {
                    let teams = if allows_public {
                        teams
                    } else {
                        teams.into_iter().filter(|t| !t.is_personal).collect()
                    };
                    let len = teams.len();
                    popup.teams = teams;
                    popup.checked = vec![false; len];
                    popup.phase = UploadPhase::SelectTeam;
                    popup.status = None;
                }
            }
            CommandResult::UploadTeams(Err(e)) => {
                if let Some(ref mut popup) = self.upload_popup {
                    popup.status = Some(format!("Error: {e}"));
                    popup.phase = UploadPhase::Done;
                }
            }

            CommandResult::UploadDone(result) => {
                if result.is_ok() {
                    // Mark synced on any successful upload
                    if let Some(session) = self.selected_session() {
                        let sid = session.session_id.clone();
                        if let Some(ref db) = self.db {
                            let _ = db.mark_synced(&sid);
                        }
                    }
                }

                if let Some(ref mut popup) = self.upload_popup {
                    match result {
                        Ok((team_name, url)) => {
                            popup.results.push((team_name, Ok(url)));
                        }
                        Err((team_name, e)) => {
                            popup.results.push((team_name, Err(e)));
                        }
                    }

                    // Check if there are more checked teams to upload
                    let uploaded_names: Vec<_> =
                        popup.results.iter().map(|(name, _)| name.clone()).collect();
                    let has_remaining = popup
                        .teams
                        .iter()
                        .enumerate()
                        .any(|(i, t)| popup.checked[i] && !uploaded_names.contains(&t.name));

                    if has_remaining {
                        // Stay in Uploading — main loop will dispatch next
                        let done = popup.results.len();
                        let total = popup.checked.iter().filter(|&&c| c).count();
                        popup.status = Some(format!("Uploading... ({done}/{total})"));
                    } else {
                        popup.phase = UploadPhase::Done;
                        popup.status = None;
                    }
                }
            }

            CommandResult::Teams(Ok(teams)) => {
                self.teams_loading = false;
                self.teams = teams;
                if !self.teams.is_empty() && self.teams_list_state.selected().is_none() {
                    self.teams_list_state.select(Some(0));
                }
            }
            CommandResult::Teams(Err(e)) => {
                self.teams_loading = false;
                self.flash_error(format!("Error: {e}"));
            }

            CommandResult::TeamDetail(Ok(detail)) => {
                let team_id = detail.team.id.clone();
                self.team_detail = Some(detail);
                // Also fetch members
                self.pending_command = Some(AsyncCommand::FetchMembers(team_id));
            }
            CommandResult::TeamDetail(Err(e)) => {
                self.flash_error(format!("Error: {e}"));
            }

            CommandResult::Members(Ok(members)) => {
                self.team_members = members;
                if !self.team_members.is_empty()
                    && self.team_members_list_state.selected().is_none()
                {
                    self.team_members_list_state.select(Some(0));
                }
            }
            CommandResult::Members(Err(e)) => {
                self.flash_error(format!("Error: {e}"));
            }

            CommandResult::Invitations(Ok(invs)) => {
                self.invitations_loading = false;
                self.invitations = invs;
                if !self.invitations.is_empty() && self.invitations_list_state.selected().is_none()
                {
                    self.invitations_list_state.select(Some(0));
                }
            }
            CommandResult::Invitations(Err(e)) => {
                self.invitations_loading = false;
                self.flash_error(format!("Error: {e}"));
            }

            CommandResult::Profile(Ok(profile)) => {
                self.profile_loading = false;
                self.profile = Some(profile);
                self.profile_error = None;
            }
            CommandResult::Profile(Err(e)) => {
                self.profile_loading = false;
                self.profile_error = Some(e.clone());
                self.flash_error(format!("Error: {e}"));
            }

            CommandResult::ApiKey(Ok(resp)) => {
                self.daemon_config.server.api_key = resp.api_key;
                self.config_dirty = true;
                self.save_config();
                self.flash_success("API key regenerated and saved");
            }
            CommandResult::ApiKey(Err(e)) => {
                self.flash_error(format!("Error: {e}"));
            }

            CommandResult::ServerSessions(Ok(resp)) => {
                self.flash_success(format!(
                    "Loaded {} sessions from server",
                    resp.sessions.len()
                ));
            }
            CommandResult::ServerSessions(Err(e)) => {
                self.flash_error(format!("Error: {e}"));
            }

            CommandResult::DeleteSession(Ok(session_id)) => {
                if let Some(ref db) = self.db {
                    let _ = db.delete_session(&session_id);
                }
                self.db_sessions.retain(|r| r.id != session_id);
                self.sessions.retain(|s| s.session_id != session_id);
                // Fix selection
                let count = self.page_count();
                if count == 0 {
                    self.list_state.select(None);
                } else if let Some(sel) = self.list_state.selected() {
                    if sel >= count {
                        self.list_state.select(Some(count - 1));
                    }
                }
                self.flash_success("Session deleted");
            }
            CommandResult::DeleteSession(Err(e)) => {
                self.flash_error(format!("Delete failed: {e}"));
            }

            CommandResult::GenericOk(Ok(msg)) => {
                self.flash_success(msg);
                // Refresh relevant data after mutations
                match self.view {
                    View::Teams => {
                        self.teams_loading = true;
                        self.pending_command = Some(AsyncCommand::FetchTeams);
                    }
                    View::TeamDetail => {
                        if let Some(ref tid) = self.viewing_team_id {
                            self.pending_command = Some(AsyncCommand::FetchTeamDetail(tid.clone()));
                        }
                    }
                    View::Invitations => {
                        self.invitations_loading = true;
                        self.pending_command = Some(AsyncCommand::FetchInvitations);
                    }
                    _ => {}
                }
            }
            CommandResult::GenericOk(Err(e)) => {
                self.flash_error(format!("Error: {e}"));
            }
        }
    }

    pub fn flash_success(&mut self, msg: impl Into<String>) {
        self.flash_message = Some((msg.into(), FlashLevel::Success));
    }

    pub fn flash_error(&mut self, msg: impl Into<String>) {
        self.flash_message = Some((msg.into(), FlashLevel::Error));
    }

    pub fn flash_info(&mut self, msg: impl Into<String>) {
        self.flash_message = Some((msg.into(), FlashLevel::Info));
    }

    pub fn save_config(&mut self) {
        match config::save_daemon_config(&self.daemon_config) {
            Ok(()) => {
                self.config_dirty = false;
                self.flash_success("Config saved to daemon.toml");
                // Update team_id in case it changed
                let tid = &self.daemon_config.identity.team_id;
                self.team_id = if tid.is_empty() {
                    None
                } else {
                    Some(tid.clone())
                };
                // Re-derive connection context
                self.connection_ctx = Self::derive_connection_ctx(&self.daemon_config);
            }
            Err(e) => {
                self.flash_error(format!("Save failed: {e}"));
            }
        }
    }

    /// Derive the connection context from the current daemon config.
    pub fn derive_connection_ctx(config: &DaemonConfig) -> ConnectionContext {
        if config.server.api_key.is_empty() {
            return ConnectionContext::Local;
        }
        let url = config.server.url.to_lowercase();
        let is_local = url.contains("localhost")
            || url.contains("127.0.0.1")
            || url.contains("192.168.")
            || url.contains("10.")
            || url.contains("172.16.");
        if is_local {
            return ConnectionContext::Docker {
                url: config.server.url.clone(),
            };
        }
        if config.identity.team_id.is_empty() {
            ConnectionContext::CloudPersonal
        } else {
            ConnectionContext::CloudTeam {
                team_name: config.identity.team_id.clone(),
            }
        }
    }

    fn toggle_event_filter(&mut self, filter: EventFilter) {
        if filter == EventFilter::All {
            // "All" resets to show everything
            self.event_filters.clear();
            self.event_filters.insert(EventFilter::All);
        } else {
            // Remove "All" when toggling a specific filter
            self.event_filters.remove(&EventFilter::All);
            if self.event_filters.contains(&filter) {
                self.event_filters.remove(&filter);
            } else {
                self.event_filters.insert(filter);
            }
            // If nothing selected, fall back to All
            if self.event_filters.is_empty() {
                self.event_filters.insert(EventFilter::All);
            }
        }
        self.detail_event_index = 0;
    }

    fn toggle_task_view_mode(&mut self) {
        self.task_view_mode = match self.task_view_mode {
            TaskViewMode::Summary => TaskViewMode::Detail,
            TaskViewMode::Detail => TaskViewMode::Summary,
        };
        self.detail_event_index = 0;
    }

    /// Returns true if the currently selected session has sub-agent tasks.
    pub fn session_has_sub_agents(&self) -> bool {
        self.selected_session()
            .map(|s| s.stats.task_count > 0)
            .unwrap_or(false)
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
        self.tool_filter = None;
        self.page = 0;
        self.reload_db_sessions();
        self.list_state.select(if self.session_count() > 0 {
            Some(0)
        } else {
            None
        });
    }

    /// Toggle between Single and ByUser list layout (Team/Repo views only).
    fn toggle_list_layout(&mut self) {
        if matches!(self.view_mode, ViewMode::Local) {
            return;
        }
        match self.list_layout {
            ListLayout::Single => {
                self.list_layout = ListLayout::ByUser;
                self.rebuild_columns();
            }
            ListLayout::ByUser => {
                self.list_layout = ListLayout::Single;
            }
        }
    }

    /// Group db_sessions by user nickname for multi-column view.
    fn rebuild_columns(&mut self) {
        use std::collections::BTreeMap;
        let mut by_user: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (idx, row) in self.db_sessions.iter().enumerate() {
            let key = row.nickname.clone().unwrap_or_else(|| "you".into());
            by_user.entry(key).or_default().push(idx);
        }
        self.column_users = by_user.keys().cloned().collect();
        self.column_list_states = vec![ListState::default(); self.column_users.len()];
        for s in &mut self.column_list_states {
            s.select(Some(0));
        }
        self.column_focus = 0;
    }

    /// Get the indices of db_sessions for a given column user.
    pub fn column_session_indices(&self, user: &str) -> Vec<usize> {
        self.db_sessions
            .iter()
            .enumerate()
            .filter(|(_, row)| row.nickname.as_deref().unwrap_or("you") == user)
            .map(|(i, _)| i)
            .collect()
    }

    /// Reload db_sessions for the current view_mode.
    pub fn reload_db_sessions(&mut self) {
        let Some(ref db) = self.db else { return };
        let filter = match &self.view_mode {
            ViewMode::Local => return, // Local mode uses self.sessions
            ViewMode::Team(tid) => LocalSessionFilter {
                team_id: Some(tid.clone()),
                tool: self.tool_filter.clone(),
                ..Default::default()
            },
            ViewMode::Repo(repo) => LocalSessionFilter {
                git_repo_name: Some(repo.clone()),
                tool: self.tool_filter.clone(),
                ..Default::default()
            },
        };
        match db.list_sessions(&filter) {
            Ok(rows) => {
                self.db_sessions = rows;
                self.rebuild_available_tools();
            }
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

    // ── Pagination ──────────────────────────────────────────────────────

    /// Total pages for current session count.
    pub fn total_pages(&self) -> usize {
        let total = self.session_count();
        if total == 0 {
            1
        } else {
            total.div_ceil(self.per_page)
        }
    }

    /// Index range for the current page.
    pub fn page_range(&self) -> std::ops::Range<usize> {
        let total = self.session_count();
        let start = self.page * self.per_page;
        let end = (start + self.per_page).min(total);
        start..end
    }

    /// Number of items on the current page.
    pub fn page_count(&self) -> usize {
        self.page_range().len()
    }

    fn next_page(&mut self) {
        if self.page + 1 < self.total_pages() {
            self.page += 1;
            self.list_state.select(Some(0));
        }
    }

    fn prev_page(&mut self) {
        if self.page > 0 {
            self.page -= 1;
            self.list_state.select(Some(0));
        }
    }

    // ── Tool filter ─────────────────────────────────────────────────────

    /// Rebuild the list of available tools from the current db_sessions.
    /// Only updates when no tool filter is active (to keep the list stable while cycling).
    pub fn rebuild_available_tools(&mut self) {
        if self.tool_filter.is_some() {
            return; // Keep existing list while filtering
        }
        let mut tools: Vec<String> = self
            .db_sessions
            .iter()
            .map(|r| r.tool.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        tools.sort();
        self.available_tools = tools;
    }

    fn cycle_tool_filter(&mut self) {
        if self.available_tools.is_empty() {
            self.tool_filter = None;
            return;
        }
        self.tool_filter = match &self.tool_filter {
            None => Some(self.available_tools[0].clone()),
            Some(current) => {
                if let Some(pos) = self.available_tools.iter().position(|t| t == current) {
                    if pos + 1 < self.available_tools.len() {
                        Some(self.available_tools[pos + 1].clone())
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        };
        self.page = 0;
        self.reload_db_sessions();
        self.list_state.select(if self.session_count() > 0 {
            Some(0)
        } else {
            None
        });
    }

    // ── List navigation ─────────────────────────────────────────────────

    fn list_next(&mut self) {
        let count = self.page_count();
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
        let count = self.page_count();
        if count > 0 {
            self.list_state.select(Some(count - 1));
        }
    }

    fn list_start(&mut self) {
        if self.page_count() > 0 {
            self.list_state.select(Some(0));
        }
    }

    fn enter_detail(&mut self) {
        if let Some(selected) = self.list_state.selected() {
            if selected < self.page_count() {
                // For DB views, only enter if we have a matching parsed session
                if self.is_db_view() && self.selected_session().is_none() {
                    let url = &self.daemon_config.server.url;
                    let abs_idx = self.page * self.per_page + selected;
                    if let Some(row) = self.db_sessions.get(abs_idx) {
                        self.flash_info(format!(
                            "Remote-only session — view at {}/sessions/{}",
                            url, row.id
                        ));
                    } else {
                        self.flash_info("Remote-only session — not available locally");
                    }
                    return;
                }
                self.view = View::SessionDetail;
                self.detail_scroll = 0;
                self.detail_event_index = 0;
                self.event_filters = HashSet::from([EventFilter::All]);
                self.expanded_events.clear();
                self.detail_view_mode = DetailViewMode::Linear;
                self.turn_index = 0;
                self.turn_agent_scroll = 0;
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

    fn detail_page_down(&mut self) {
        if let Some(session) = self.selected_session() {
            let visible = self.visible_event_count(session);
            if visible > 0 {
                self.detail_event_index = (self.detail_event_index + 10).min(visible - 1);
            }
        }
    }

    fn detail_page_up(&mut self) {
        self.detail_event_index = self.detail_event_index.saturating_sub(10);
    }

    fn toggle_task_fold(&mut self) {
        let task_id = if let Some(session) = self.selected_session() {
            let visible_events = self.get_visible_events(session);
            visible_events
                .get(self.detail_event_index)
                .and_then(|e| e.event().task_id.clone())
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

    fn toggle_expanded(&mut self) {
        let idx = self.detail_event_index;
        if self.expanded_events.contains(&idx) {
            self.expanded_events.remove(&idx);
        } else {
            self.expanded_events.insert(idx);
        }
    }

    fn handle_turn_key(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Char('q') => {
                self.view = View::SessionList;
                self.detail_scroll = 0;
                self.detail_event_index = 0;
                self.detail_view_mode = DetailViewMode::Linear;
            }
            KeyCode::Esc | KeyCode::Char('v') | KeyCode::Char('h') | KeyCode::Left => {
                self.detail_view_mode = DetailViewMode::Linear;
                self.sync_turn_to_linear();
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.turn_agent_scroll = self.turn_agent_scroll.saturating_add(1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.turn_agent_scroll = self.turn_agent_scroll.saturating_sub(1);
            }
            KeyCode::Char('J') | KeyCode::Char('n') => self.turn_next(),
            KeyCode::Char('K') | KeyCode::Char('N') => self.turn_prev(),
            KeyCode::Char('g') | KeyCode::Home => {
                self.turn_index = 0;
                self.turn_agent_scroll = 0;
            }
            KeyCode::Char('G') | KeyCode::End => {
                if let Some(session) = self.selected_session().cloned() {
                    let visible = self.get_visible_events(&session);
                    let turns = extract_turns(&visible);
                    self.turn_index = turns.len().saturating_sub(1);
                    if let Some(&offset) = self.turn_line_offsets.get(self.turn_index) {
                        self.turn_agent_scroll = offset;
                    } else {
                        self.turn_agent_scroll = 0;
                    }
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                let idx = self.turn_index;
                if self.expanded_turns.contains(&idx) {
                    self.expanded_turns.remove(&idx);
                } else {
                    self.expanded_turns.insert(idx);
                }
            }
            _ => {}
        }
        false
    }

    fn turn_next(&mut self) {
        if let Some(session) = self.selected_session().cloned() {
            let visible = self.get_visible_events(&session);
            let turns = extract_turns(&visible);
            if self.turn_index + 1 < turns.len() {
                self.turn_index += 1;
                if let Some(&offset) = self.turn_line_offsets.get(self.turn_index) {
                    self.turn_agent_scroll = offset;
                } else {
                    self.turn_agent_scroll = 0;
                }
            }
        }
    }

    fn turn_prev(&mut self) {
        if self.turn_index > 0 {
            self.turn_index -= 1;
            if let Some(&offset) = self.turn_line_offsets.get(self.turn_index) {
                self.turn_agent_scroll = offset;
            } else {
                self.turn_agent_scroll = 0;
            }
        }
    }

    fn sync_linear_to_turn(&mut self) {
        if let Some(session) = self.selected_session().cloned() {
            let visible = self.get_visible_events(&session);
            let turns = extract_turns(&visible);
            let mut event_count = 0;
            for (ti, turn) in turns.iter().enumerate() {
                let turn_size = turn.user_events.len() + turn.agent_events.len();
                if event_count + turn_size > self.detail_event_index {
                    self.turn_index = ti;
                    self.turn_agent_scroll = 0;
                    return;
                }
                event_count += turn_size;
            }
            self.turn_index = turns.len().saturating_sub(1);
            self.turn_agent_scroll = 0;
        }
    }

    fn sync_turn_to_linear(&mut self) {
        if let Some(session) = self.selected_session().cloned() {
            let visible = self.get_visible_events(&session);
            let turns = extract_turns(&visible);
            let mut event_count = 0;
            for (ti, turn) in turns.iter().enumerate() {
                if ti == self.turn_index {
                    self.detail_event_index = event_count;
                    return;
                }
                event_count += turn.user_events.len() + turn.agent_events.len();
            }
        }
    }

    fn jump_to_next_same_type(&mut self) {
        if let Some(session) = self.selected_session().cloned() {
            let visible = self.get_visible_events(&session);
            if visible.is_empty() {
                return;
            }
            let current = self.detail_event_index.min(visible.len() - 1);
            let target_disc = std::mem::discriminant(&visible[current].event().event_type);
            for (i, de) in visible.iter().enumerate().skip(current + 1) {
                if std::mem::discriminant(&de.event().event_type) == target_disc {
                    self.detail_event_index = i;
                    return;
                }
            }
        }
    }

    fn jump_to_prev_same_type(&mut self) {
        if let Some(session) = self.selected_session().cloned() {
            let visible = self.get_visible_events(&session);
            if visible.is_empty() {
                return;
            }
            let current = self.detail_event_index.min(visible.len() - 1);
            if current == 0 {
                return;
            }
            let target_disc = std::mem::discriminant(&visible[current].event().event_type);
            for (i, de) in visible.iter().enumerate().take(current).rev() {
                if std::mem::discriminant(&de.event().event_type) == target_disc {
                    self.detail_event_index = i;
                    return;
                }
            }
        }
    }

    fn jump_to_next_user_message(&mut self) {
        if let Some(session) = self.selected_session().cloned() {
            let visible = self.get_visible_events(&session);
            let start = self.detail_event_index + 1;
            for (offset, de) in visible.iter().skip(start).enumerate() {
                if matches!(de.event().event_type, EventType::UserMessage) {
                    self.detail_event_index = start + offset;
                    return;
                }
            }
        }
    }

    fn jump_to_prev_user_message(&mut self) {
        if self.detail_event_index == 0 {
            return;
        }
        if let Some(session) = self.selected_session().cloned() {
            let visible = self.get_visible_events(&session);
            for (i, de) in visible
                .iter()
                .enumerate()
                .take(self.detail_event_index)
                .rev()
            {
                if matches!(de.event().event_type, EventType::UserMessage) {
                    self.detail_event_index = i;
                    return;
                }
            }
        }
    }

    // ── Helpers ─────────────────────────────────────────────────────────

    pub fn selected_session(&self) -> Option<&Session> {
        if self.is_db_view() {
            // In DB view, match by session_id against parsed sessions
            let abs_idx = self
                .list_state
                .selected()
                .map(|i| self.page * self.per_page + i)?;
            let db_row = self.db_sessions.get(abs_idx)?;
            self.sessions.iter().find(|s| s.session_id == db_row.id)
        } else {
            let abs_idx = self
                .list_state
                .selected()
                .map(|i| self.page * self.per_page + i)?;
            self.filtered_sessions
                .get(abs_idx)
                .and_then(|&idx| self.sessions.get(idx))
        }
    }

    /// Get nickname of the currently selected session (Team/Repo views only).
    pub fn selected_session_nickname(&self) -> Option<&str> {
        if self.is_db_view() {
            let abs_idx = self
                .list_state
                .selected()
                .map(|i| self.page * self.per_page + i)?;
            self.db_sessions
                .get(abs_idx)
                .and_then(|row| row.nickname.as_deref())
        } else {
            None
        }
    }

    /// Get the selected DB session row (for Team/Repo views).
    pub fn selected_db_session(&self) -> Option<&LocalSessionRow> {
        let abs_idx = self
            .list_state
            .selected()
            .map(|i| self.page * self.per_page + i)?;
        self.db_sessions.get(abs_idx)
    }

    /// Check if the selected session's repo allows public upload.
    /// Returns `false` only if the repo has `.opensession/config.toml` with `allow_public = false`.
    pub fn selected_session_allows_public(&self) -> bool {
        let session = match self.selected_session() {
            Some(s) => s,
            None => return true,
        };
        let cwd = session
            .context
            .attributes
            .get("cwd")
            .or_else(|| session.context.attributes.get("working_directory"))
            .and_then(|v| v.as_str());
        let cwd = match cwd {
            Some(c) => c,
            None => return true,
        };
        let repo_root = match opensession_git_native::ops::find_repo_root(std::path::Path::new(cwd))
        {
            Some(r) => r,
            None => return true,
        };

        // Read allow_public from per-repo config (shared + local)
        for name in &["config.toml", "config.local.toml"] {
            let path = repo_root.join(".opensession").join(name);
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(val) = toml::from_str::<toml::Value>(&content) {
                    if let Some(false) = val.get("allow_public").and_then(|v| v.as_bool()) {
                        return false;
                    }
                }
            }
        }
        true
    }

    pub fn matches_event_filter(&self, event_type: &EventType) -> bool {
        if self.event_filters.contains(&EventFilter::All) {
            return true;
        }
        for f in &self.event_filters {
            let matches = match f {
                EventFilter::All => true,
                EventFilter::Messages => matches!(
                    event_type,
                    EventType::UserMessage | EventType::AgentMessage | EventType::SystemMessage
                ),
                EventFilter::ToolCalls => matches!(
                    event_type,
                    EventType::ToolCall { .. } | EventType::ToolResult { .. }
                ),
                EventFilter::Thinking => matches!(event_type, EventType::Thinking),
                EventFilter::FileOps => matches!(
                    event_type,
                    EventType::FileEdit { .. }
                        | EventType::FileCreate { .. }
                        | EventType::FileDelete { .. }
                        | EventType::FileRead { .. }
                        | EventType::CodeSearch { .. }
                        | EventType::FileSearch { .. }
                ),
                EventFilter::Shell => matches!(event_type, EventType::ShellCommand { .. }),
            };
            if matches {
                return true;
            }
        }
        false
    }

    pub fn get_visible_events<'a>(&self, session: &'a Session) -> Vec<DisplayEvent<'a>> {
        // Step 1: filter by event type
        let filtered: Vec<&'a Event> = session
            .events
            .iter()
            .filter(|e| self.matches_event_filter(&e.event_type))
            .collect();

        // Step 2: apply task view mode (collapse sub-agent tasks)
        let after_task = self.apply_task_view_mode(&filtered);

        // Step 3: collapse consecutive similar actions
        if self.collapse_consecutive {
            Self::collapse_consecutive_events(after_task)
        } else {
            after_task
        }
    }

    fn apply_task_view_mode<'a>(&self, events: &[&'a Event]) -> Vec<DisplayEvent<'a>> {
        match self.task_view_mode {
            TaskViewMode::Detail => events.iter().map(|e| DisplayEvent::Single(e)).collect(),
            TaskViewMode::Summary => {
                let mut result: Vec<DisplayEvent<'a>> = Vec::new();
                let mut i = 0;
                while i < events.len() {
                    if let EventType::TaskStart { ref title } = events[i].event_type {
                        let task_id = events[i].task_id.clone();
                        let start_idx = i;
                        let mut inner_count = 0u32;
                        let mut end_summary: Option<String> = None;
                        i += 1;
                        // Collect until matching TaskEnd
                        while i < events.len() {
                            if let EventType::TaskEnd { ref summary } = events[i].event_type {
                                if events[i].task_id == task_id {
                                    end_summary = summary.clone().or_else(|| Some(String::new()));
                                    i += 1;
                                    break;
                                }
                            }
                            inner_count += 1;
                            i += 1;
                        }
                        let label = title.as_deref().unwrap_or("task");
                        let summary_text = end_summary
                            .map(|s| {
                                if s.is_empty() {
                                    format!("{} ({} events)", label, inner_count)
                                } else {
                                    format!("{}: {} ({} events)", label, s, inner_count)
                                }
                            })
                            .unwrap_or_else(|| format!("{} ({} events)", label, inner_count));
                        result.push(DisplayEvent::TaskSummary {
                            event: events[start_idx],
                            summary: summary_text,
                            inner_count,
                        });
                    } else {
                        result.push(DisplayEvent::Single(events[i]));
                        i += 1;
                    }
                }
                result
            }
        }
    }

    fn collapse_consecutive_events<'a>(events: Vec<DisplayEvent<'a>>) -> Vec<DisplayEvent<'a>> {
        let mut result: Vec<DisplayEvent<'a>> = Vec::new();
        let mut i = 0;
        while i < events.len() {
            let group_kind = match &events[i] {
                DisplayEvent::Single(e) => consecutive_group_key(&e.event_type),
                _ => None,
            };
            if let Some(kind) = group_kind {
                // Collect consecutive events of same kind
                let start = i;
                let mut items: Vec<&'a Event> = Vec::new();
                while i < events.len() {
                    if let DisplayEvent::Single(e) = &events[i] {
                        if consecutive_group_key(&e.event_type).as_deref() == Some(&kind) {
                            items.push(e);
                            i += 1;
                            continue;
                        }
                    }
                    break;
                }
                if items.len() > 1 {
                    result.push(DisplayEvent::Collapsed {
                        first: items[0],
                        count: items.len() as u32,
                        kind: kind.clone(),
                    });
                } else {
                    result.push(events[start].clone());
                }
            } else {
                result.push(events[i].clone());
                i += 1;
            }
        }
        result
    }

    fn visible_event_count(&self, session: &Session) -> usize {
        self.get_visible_events(session).len()
    }

    fn apply_filter(&mut self) {
        let query = self.search_query.to_lowercase();
        self.page = 0;

        match &self.view_mode {
            ViewMode::Local => {
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

                if self.filtered_sessions.is_empty() {
                    self.list_state.select(None);
                } else {
                    self.list_state.select(Some(0));
                }
            }
            ViewMode::Team(tid) => {
                if let Some(ref db) = self.db {
                    let filter = LocalSessionFilter {
                        team_id: Some(tid.clone()),
                        tool: self.tool_filter.clone(),
                        search: if query.is_empty() { None } else { Some(query) },
                        ..Default::default()
                    };
                    self.db_sessions = db.list_sessions(&filter).unwrap_or_default();
                }
                self.list_state.select(if self.db_sessions.is_empty() {
                    None
                } else {
                    Some(0)
                });
            }
            ViewMode::Repo(repo) => {
                if let Some(ref db) = self.db {
                    let filter = LocalSessionFilter {
                        git_repo_name: Some(repo.clone()),
                        tool: self.tool_filter.clone(),
                        search: if query.is_empty() { None } else { Some(query) },
                        ..Default::default()
                    };
                    self.db_sessions = db.list_sessions(&filter).unwrap_or_default();
                }
                self.list_state.select(if self.db_sessions.is_empty() {
                    None
                } else {
                    Some(0)
                });
            }
        }
    }
}
