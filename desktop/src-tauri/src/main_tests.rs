use super::{
    DesktopSessionListQuery, SearchMode, build_local_filter_with_mode,
    build_vector_chunks_for_session, cosine_similarity, desktop_ask_session_changes,
    desktop_change_reader_tts, desktop_get_contract_version, desktop_get_runtime_settings,
    desktop_get_session_detail, desktop_get_session_raw, desktop_list_sessions,
    desktop_read_session_changes, desktop_summary_batch_run, desktop_summary_batch_status,
    desktop_update_runtime_settings, extract_vector_lines, force_refresh_discovery_tools,
    map_link_type, normalize_launch_route, normalize_session_body_to_hail_jsonl,
    require_non_empty_request_field, session_summary_from_local_row,
    validate_vector_preflight_ready,
};
use crate::app::handoff::{
    artifact_path_for_hash, build_handoff_artifact_record, canonicalize_summaries,
    desktop_share_session_quick, parse_cli_quick_share_response, validate_pin_alias,
};
use crate::app::session_query::split_search_mode;
use opensession_api::{
    DesktopChangeQuestionRequest, DesktopChangeReadRequest, DesktopChangeReaderScope,
    DesktopChangeReaderTtsRequest, DesktopChangeReaderVoiceProvider, DesktopQuickShareRequest,
    DesktopRuntimeChangeReaderSettingsUpdate, DesktopRuntimeChangeReaderVoiceSettingsUpdate,
    DesktopRuntimeLifecycleSettingsUpdate, DesktopRuntimeSettingsUpdateRequest,
    DesktopRuntimeSummaryBatchSettingsUpdate, DesktopRuntimeSummaryPromptSettingsUpdate,
    DesktopRuntimeSummaryProviderSettingsUpdate, DesktopRuntimeSummaryResponseSettingsUpdate,
    DesktopRuntimeSummarySettingsUpdate, DesktopRuntimeSummaryStorageSettingsUpdate,
    DesktopSummaryBatchExecutionMode, DesktopSummaryBatchScope, DesktopSummaryBatchState,
    DesktopSummaryBatchStatusResponse, DesktopSummaryOutputShape, DesktopSummaryProviderId,
    DesktopSummaryResponseStyle, DesktopSummarySourceMode, DesktopSummaryStorageBackend,
    DesktopSummaryTriggerMode, DesktopVectorInstallState, DesktopVectorPreflightResponse,
    DesktopVectorSearchProvider,
};
use opensession_core::handoff::HandoffSummary;
use opensession_core::trace::{Agent, Content, Event, EventType, Session as HailSession};
use opensession_git_native::{NativeGitStorage, SUMMARY_LEDGER_REF, SessionSummaryLedgerRecord};
use opensession_local_db::git::GitContext;
use opensession_local_db::{LocalDb, VectorIndexJobRow};
use opensession_runtime_config::DaemonConfig;
use opensession_summary::DEFAULT_SUMMARY_PROMPT_TEMPLATE_V2;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod change_reader;
mod handoff;
mod runtime_settings;
mod session_access;
mod summary_batch;
mod vector;

static TEST_ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

fn set_env_for_test(key: &str, value: impl AsRef<std::ffi::OsStr>) {
    // SAFETY: desktop tests mutate process environment only while holding TEST_ENV_LOCK,
    // which serializes the affected test cases and avoids concurrent environment access.
    unsafe { std::env::set_var(key, value) };
}

fn remove_env_for_test(key: &str) {
    // SAFETY: desktop tests mutate process environment only while holding TEST_ENV_LOCK,
    // which serializes the affected test cases and avoids concurrent environment access.
    unsafe { std::env::remove_var(key) };
}

struct EnvVarGuard {
    key: String,
    previous: Option<String>,
}

impl EnvVarGuard {
    fn set(key: &str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let previous = std::env::var(key).ok();
        set_env_for_test(key, value);
        Self {
            key: key.to_string(),
            previous,
        }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            set_env_for_test(&self.key, previous);
        } else {
            remove_env_for_test(&self.key);
        }
    }
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be monotonic")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("{prefix}-{unique}"));
    std::fs::create_dir_all(&path).expect("create test temp dir");
    path
}

fn init_test_git_repo(path: &Path) {
    let output = Command::new("git")
        .arg("init")
        .arg("--quiet")
        .arg(path)
        .output()
        .expect("run git init");
    assert!(
        output.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn build_test_session(session_id: &str) -> HailSession {
    let mut session = HailSession::new(
        session_id.to_string(),
        Agent {
            provider: "openai".to_string(),
            model: "gpt-5".to_string(),
            tool: "codex".to_string(),
            tool_version: None,
        },
    );
    session.events.push(Event {
        event_id: "evt-1".to_string(),
        timestamp: chrono::Utc::now(),
        event_type: EventType::UserMessage,
        task_id: None,
        content: Content::text("desktop test message"),
        duration_ms: None,
        attributes: std::collections::HashMap::new(),
    });
    session.recompute_stats();
    session
}

fn summary_settings_update_with_backend(
    backend: DesktopSummaryStorageBackend,
) -> DesktopRuntimeSummarySettingsUpdate {
    DesktopRuntimeSummarySettingsUpdate {
        provider: DesktopRuntimeSummaryProviderSettingsUpdate {
            id: DesktopSummaryProviderId::Disabled,
            endpoint: String::new(),
            model: String::new(),
        },
        prompt: DesktopRuntimeSummaryPromptSettingsUpdate {
            template: DEFAULT_SUMMARY_PROMPT_TEMPLATE_V2.to_string(),
        },
        response: DesktopRuntimeSummaryResponseSettingsUpdate {
            style: DesktopSummaryResponseStyle::Standard,
            shape: DesktopSummaryOutputShape::Layered,
        },
        storage: DesktopRuntimeSummaryStorageSettingsUpdate {
            trigger: DesktopSummaryTriggerMode::OnSessionSave,
            backend,
        },
        source_mode: DesktopSummarySourceMode::SessionOnly,
        batch: DesktopRuntimeSummaryBatchSettingsUpdate {
            execution_mode: DesktopSummaryBatchExecutionMode::Manual,
            scope: DesktopSummaryBatchScope::All,
            recent_days: 30,
        },
    }
}

fn manual_summary_batch_settings(
    backend: DesktopSummaryStorageBackend,
) -> DesktopRuntimeSummarySettingsUpdate {
    DesktopRuntimeSummarySettingsUpdate {
        storage: DesktopRuntimeSummaryStorageSettingsUpdate {
            trigger: DesktopSummaryTriggerMode::Manual,
            backend: backend.clone(),
        },
        ..summary_settings_update_with_backend(backend)
    }
}

fn sample_row() -> opensession_local_db::LocalSessionRow {
    opensession_local_db::LocalSessionRow {
        id: "s1".to_string(),
        source_path: Some("/tmp/s1.hail.jsonl".to_string()),
        sync_status: "local_only".to_string(),
        last_synced_at: None,
        user_id: None,
        nickname: None,
        team_id: Some("personal".to_string()),
        tool: "codex".to_string(),
        agent_provider: Some("openai".to_string()),
        agent_model: Some("gpt-5".to_string()),
        title: Some("Sample".to_string()),
        description: Some("sample session".to_string()),
        tags: Some("tag1,tag2".to_string()),
        created_at: "2026-03-03T00:00:00Z".to_string(),
        uploaded_at: None,
        message_count: 5,
        user_message_count: 2,
        task_count: 1,
        event_count: 20,
        duration_seconds: 120,
        total_input_tokens: 100,
        total_output_tokens: 40,
        git_remote: None,
        git_branch: None,
        git_commit: None,
        git_repo_name: None,
        pr_number: None,
        pr_url: None,
        working_directory: None,
        files_modified: None,
        files_read: None,
        has_errors: false,
        max_active_agents: 1,
        is_auxiliary: false,
    }
}

fn wait_for_summary_batch_completion(
    started: DesktopSummaryBatchStatusResponse,
) -> DesktopSummaryBatchStatusResponse {
    let mut final_state = started;
    for _ in 0..40 {
        final_state = desktop_summary_batch_status().expect("read batch status");
        if !matches!(final_state.state, DesktopSummaryBatchState::Running) {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    final_state
}
