#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;

use app::change_reader::{
    desktop_ask_session_changes, desktop_change_reader_tts, desktop_read_session_changes,
    require_non_empty_request_field,
};
use app::handoff::{desktop_build_handoff, desktop_share_session_quick};
use app::launch_route::desktop_take_launch_route;
#[cfg(test)]
pub(crate) use app::lifecycle_cleanup::desktop_lifecycle_cleanup_status_from_db;
use app::lifecycle_cleanup::{
    desktop_lifecycle_cleanup_status, maybe_start_lifecycle_cleanup_loop,
};
use app::runtime_settings::{
    desktop_detect_summary_provider, desktop_get_runtime_settings, desktop_update_runtime_settings,
};
use app::session_access::{
    desktop_get_session_detail, desktop_get_session_raw, desktop_list_repos, desktop_list_sessions,
};
#[cfg(test)]
use app::session_access::{
    force_refresh_discovery_tools, map_link_type, normalize_session_body_to_hail_jsonl,
    session_summary_from_local_row,
};
pub(crate) use app::session_access::{
    load_normalized_session_body, session_summary_from_local_row_with_score,
};
#[cfg(test)]
use app::session_query::{SearchMode, build_local_filter_with_mode};
use app::session_summary::{
    desktop_get_session_summary, desktop_regenerate_session_summary, desktop_summary_batch_run,
    desktop_summary_batch_status, maybe_start_summary_batch_on_app_start,
};
#[cfg(test)]
use app::vector::{
    build_vector_chunks_for_session, cosine_similarity, extract_vector_lines,
    persist_vector_index_failure_snapshot, rebuild_vector_index_blocking,
    validate_vector_preflight_ready,
};
use app::vector::{
    desktop_search_sessions_vector, desktop_vector_index_rebuild, desktop_vector_index_status,
    desktop_vector_install_model, desktop_vector_preflight,
};
#[cfg(test)]
use app::{
    launch_route::normalize_launch_route,
    lifecycle_cleanup::run_desktop_lifecycle_cleanup_once_with_db,
};
use opensession_api::{
    CapabilitiesResponse, DESKTOP_IPC_CONTRACT_VERSION, DesktopApiError,
    DesktopContractVersionResponse, DesktopSessionListQuery,
    oauth::{AuthProvidersResponse, OAuthProviderInfo},
};
use opensession_local_db::LocalDb;
use opensession_runtime_config::DaemonConfig;
use serde_json::json;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

pub(crate) type DesktopApiResult<T> = Result<T, DesktopApiError>;

const CHANGE_READER_MAX_EVENTS: usize = 180;
const CHANGE_READER_MAX_LINE_CHARS: usize = 220;
static LIFECYCLE_CLEANUP_LOOP_STARTED: LazyLock<Mutex<bool>> = LazyLock::new(|| Mutex::new(false));

pub(crate) fn desktop_error(
    code: &str,
    status: u16,
    message: impl Into<String>,
    details: Option<serde_json::Value>,
) -> DesktopApiError {
    DesktopApiError {
        code: code.to_string(),
        status,
        message: message.into(),
        details,
    }
}

pub(crate) fn enum_label<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value)
        .ok()
        .map(|raw| raw.trim_matches('"').to_string())
        .filter(|raw| !raw.trim().is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

fn open_local_db() -> DesktopApiResult<LocalDb> {
    let custom_path = std::env::var("OPENSESSION_LOCAL_DB_PATH")
        .ok()
        .and_then(|value| {
            let trimmed = value.trim().to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        })
        .map(PathBuf::from);

    let db = if let Some(path) = custom_path {
        LocalDb::open_path(&path)
    } else {
        LocalDb::open()
    };

    db.map_err(|error| {
        desktop_error(
            "desktop.local_db_open_failed",
            500,
            "failed to open local database",
            Some(json!({ "cause": error.to_string() })),
        )
    })
}

fn runtime_config_path() -> DesktopApiResult<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|error| {
            desktop_error(
                "desktop.runtime_config_home_unavailable",
                500,
                "failed to resolve home directory for runtime config",
                Some(json!({ "cause": error.to_string() })),
            )
        })?;
    Ok(PathBuf::from(home)
        .join(".config")
        .join("opensession")
        .join(opensession_runtime_config::CONFIG_FILE_NAME))
}

fn load_runtime_config() -> DesktopApiResult<DaemonConfig> {
    let path = runtime_config_path()?;
    if !path.exists() {
        return Ok(DaemonConfig::default());
    }
    let content = std::fs::read_to_string(&path).map_err(|error| {
        desktop_error(
            "desktop.runtime_config_read_failed",
            500,
            "failed to read runtime config",
            Some(json!({ "cause": error.to_string(), "path": path })),
        )
    })?;
    toml::from_str(&content).map_err(|error| {
        desktop_error(
            "desktop.runtime_config_parse_failed",
            500,
            "failed to parse runtime config",
            Some(json!({ "cause": error.to_string(), "path": path })),
        )
    })
}

fn save_runtime_config(config: &DaemonConfig) -> DesktopApiResult<()> {
    let path = runtime_config_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            desktop_error(
                "desktop.runtime_config_write_failed",
                500,
                "failed to create runtime config directory",
                Some(json!({ "cause": error.to_string(), "path": parent })),
            )
        })?;
    }

    let body = toml::to_string_pretty(config).map_err(|error| {
        desktop_error(
            "desktop.runtime_config_serialize_failed",
            500,
            "failed to serialize runtime config",
            Some(json!({ "cause": error.to_string() })),
        )
    })?;

    std::fs::write(&path, body).map_err(|error| {
        desktop_error(
            "desktop.runtime_config_write_failed",
            500,
            "failed to write runtime config",
            Some(json!({ "cause": error.to_string(), "path": path })),
        )
    })
}

#[tauri::command]
fn desktop_get_capabilities() -> CapabilitiesResponse {
    CapabilitiesResponse::for_runtime(false, false)
}

#[tauri::command]
fn desktop_get_auth_providers() -> AuthProvidersResponse {
    AuthProvidersResponse {
        email_password: false,
        oauth: Vec::<OAuthProviderInfo>::new(),
    }
}

#[tauri::command]
fn desktop_get_contract_version() -> DesktopContractVersionResponse {
    DesktopContractVersionResponse {
        version: DESKTOP_IPC_CONTRACT_VERSION.to_string(),
    }
}

#[tauri::command]
fn desktop_get_docs_markdown() -> String {
    include_str!("../../../docs.md").to_string()
}

fn main() {
    maybe_start_summary_batch_on_app_start();
    maybe_start_lifecycle_cleanup_loop();

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            desktop_get_capabilities,
            desktop_get_auth_providers,
            desktop_get_contract_version,
            desktop_get_docs_markdown,
            desktop_get_runtime_settings,
            desktop_update_runtime_settings,
            desktop_lifecycle_cleanup_status,
            desktop_summary_batch_status,
            desktop_summary_batch_run,
            desktop_detect_summary_provider,
            desktop_vector_preflight,
            desktop_vector_install_model,
            desktop_vector_index_rebuild,
            desktop_vector_index_status,
            desktop_search_sessions_vector,
            desktop_list_sessions,
            desktop_list_repos,
            desktop_get_session_detail,
            desktop_get_session_raw,
            desktop_get_session_summary,
            desktop_regenerate_session_summary,
            desktop_read_session_changes,
            desktop_ask_session_changes,
            desktop_change_reader_tts,
            desktop_take_launch_route,
            desktop_build_handoff,
            desktop_share_session_quick
        ])
        .run(tauri::generate_context!())
        .expect("failed to run opensession desktop app");
}

#[cfg(test)]
mod main_tests;
