use crate::{DesktopApiResult, desktop_error};
use opensession_local_store::global_store_root;
use serde_json::json;
use std::fs;
use std::path::PathBuf;

fn desktop_launch_route_path() -> DesktopApiResult<PathBuf> {
    let store_root = global_store_root().map_err(|error| {
        desktop_error(
            "desktop.launch_route_root_unavailable",
            500,
            "failed to resolve OpenSession home directory",
            Some(json!({ "cause": error.to_string() })),
        )
    })?;
    let opensession_root = store_root.parent().ok_or_else(|| {
        desktop_error(
            "desktop.launch_route_root_invalid",
            500,
            "invalid OpenSession global store path",
            Some(json!({ "store_root": store_root.to_string_lossy() })),
        )
    })?;
    Ok(opensession_root.join("desktop").join("launch-route"))
}

pub(crate) fn normalize_launch_route(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || !trimmed.starts_with('/') || trimmed.starts_with("//") {
        return None;
    }
    if trimmed.chars().any(|ch| ch.is_control()) {
        return None;
    }
    Some(trimmed.to_string())
}

#[tauri::command]
pub(crate) fn desktop_take_launch_route() -> DesktopApiResult<Option<String>> {
    let path = desktop_launch_route_path()?;
    if !path.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(&path).map_err(|error| {
        desktop_error(
            "desktop.launch_route_read_failed",
            500,
            "failed to read desktop launch route",
            Some(json!({ "cause": error.to_string(), "path": path.to_string_lossy() })),
        )
    })?;
    let _ = fs::remove_file(&path);
    Ok(normalize_launch_route(&contents))
}
