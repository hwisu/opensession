use crate::{PublishMode, SessionDefaultView};

pub const CONFIG_FILE_NAME: &str = "opensession.toml";

pub(crate) fn default_true() -> bool {
    true
}

pub(crate) fn default_false() -> bool {
    false
}

pub(crate) fn default_debounce() -> u64 {
    5
}

pub(crate) fn default_max_retries() -> u32 {
    3
}

pub(crate) fn default_health_check_interval() -> u64 {
    300
}

pub(crate) fn default_realtime_debounce_ms() -> u64 {
    500
}

pub(crate) fn default_detail_realtime_preview_enabled() -> bool {
    false
}

pub(crate) fn default_detail_auto_expand_selected_event() -> bool {
    true
}

pub(crate) fn default_session_default_view() -> SessionDefaultView {
    SessionDefaultView::Full
}

pub(crate) fn default_publish_on() -> PublishMode {
    PublishMode::Manual
}

pub(crate) fn default_git_retention_keep_days() -> u32 {
    30
}

pub(crate) fn default_git_retention_interval_secs() -> u64 {
    86_400
}

pub(crate) fn default_summary_endpoint() -> String {
    "http://127.0.0.1:11434".to_string()
}

pub(crate) fn default_vector_endpoint() -> String {
    "http://127.0.0.1:11434".to_string()
}

pub(crate) fn default_vector_model() -> String {
    "bge-m3".to_string()
}

pub(crate) fn default_vector_chunk_size_lines() -> u16 {
    12
}

pub(crate) fn default_vector_chunk_overlap_lines() -> u16 {
    3
}

pub(crate) fn default_vector_top_k_chunks() -> u16 {
    30
}

pub(crate) fn default_vector_top_k_sessions() -> u16 {
    20
}

pub(crate) fn default_change_reader_max_context_chars() -> u32 {
    12_000
}

pub(crate) fn default_change_reader_voice_model() -> String {
    "gpt-4o-mini-tts".to_string()
}

pub(crate) fn default_change_reader_voice_name() -> String {
    "alloy".to_string()
}

pub(crate) fn default_summary_batch_recent_days() -> u16 {
    30
}

pub(crate) fn default_lifecycle_session_ttl_days() -> u32 {
    30
}

pub(crate) fn default_lifecycle_summary_ttl_days() -> u32 {
    30
}

pub(crate) fn default_lifecycle_cleanup_interval_secs() -> u64 {
    3_600
}

pub(crate) fn default_server_url() -> String {
    "https://opensession.io".to_string()
}

pub(crate) fn default_nickname() -> String {
    "user".to_string()
}

pub(crate) fn default_exclude_patterns() -> Vec<String> {
    vec![
        "*.env".to_string(),
        "*secret*".to_string(),
        "*credential*".to_string(),
    ]
}

pub const DEFAULT_WATCH_PATHS: &[&str] = &[
    "~/.claude/projects",
    "~/.codex/sessions",
    "~/.local/share/opencode/storage/session",
    "~/.cline/data/tasks",
    "~/.local/share/amp/threads",
    "~/.gemini/tmp",
    "~/Library/Application Support/Cursor/User",
    "~/.config/Cursor/User",
];

pub fn default_watch_paths() -> Vec<String> {
    DEFAULT_WATCH_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect()
}
