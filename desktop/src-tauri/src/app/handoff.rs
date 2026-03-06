use super::change_reader::require_non_empty_request_field;
use opensession_api::{
    DesktopHandoffBuildRequest, DesktopHandoffBuildResponse, DesktopQuickShareRequest,
    DesktopQuickShareResponse,
};
use opensession_core::handoff::{HandoffSummary, validate_handoff_summaries};
use opensession_core::object_store::{
    find_repo_root, global_store_root, sha256_hex, store_local_object,
};
use opensession_core::session::working_directory;
use opensession_core::source_uri::SourceUri;
use opensession_core::trace::Session as HailSession;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{
    DesktopApiResult, desktop_error, enum_label, load_normalized_session_body, load_runtime_config,
    open_local_db,
};

const HANDOFF_RECORD_VERSION: &str = "v1";
const HANDOFF_LATEST_PIN_ALIAS: &str = "latest";

fn normalize_optional_remote(value: Option<String>) -> Option<String> {
    value
        .map(|raw| raw.trim().to_string())
        .and_then(|trimmed| (!trimmed.is_empty()).then_some(trimmed))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DesktopHandoffArtifactRecord {
    version: String,
    sha256: String,
    created_at: String,
    source_uris: Vec<String>,
    canonical_jsonl: String,
    raw_sessions: Vec<HailSession>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    summary_meta: Option<DesktopHandoffSummaryMeta>,
    #[serde(default)]
    validation_reports: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DesktopHandoffSummaryMeta {
    session_default_view: String,
    summary_source_mode: String,
    summary_provider: String,
}

#[derive(Debug, Deserialize)]
struct CliQuickSharePayload {
    uri: String,
    source_uri: String,
    remote: String,
    push_cmd: String,
    pushed: bool,
    #[serde(default)]
    auto_push_consent: bool,
}

pub(crate) fn parse_cli_quick_share_response(
    stdout: &str,
) -> DesktopApiResult<DesktopQuickShareResponse> {
    let payload: CliQuickSharePayload = serde_json::from_str(stdout).map_err(|error| {
        desktop_error(
            "desktop.quick_share_parse_failed",
            500,
            "failed to decode quick-share response from CLI",
            Some(json!({ "cause": error.to_string(), "stdout": stdout })),
        )
    })?;
    Ok(DesktopQuickShareResponse {
        source_uri: payload.source_uri,
        shared_uri: payload.uri,
        remote: payload.remote,
        push_cmd: payload.push_cmd,
        pushed: payload.pushed,
        auto_push_consent: payload.auto_push_consent,
    })
}

pub(crate) fn canonicalize_summaries(summaries: &[HandoffSummary]) -> DesktopApiResult<String> {
    let mut sorted = summaries
        .iter()
        .map(|summary| {
            serde_json::to_value(summary)
                .map(|value| (summary.source_session_id.clone(), value))
                .map_err(|error| {
                    desktop_error(
                        "desktop.handoff_serialize_failed",
                        500,
                        "failed to serialize handoff summary",
                        Some(json!({ "cause": error.to_string() })),
                    )
                })
        })
        .collect::<DesktopApiResult<Vec<_>>>()?;
    sorted.sort_by(|left, right| left.0.cmp(&right.0));

    let mut out = String::new();
    for (_session_id, value) in sorted {
        let line = serde_json::to_string(&value).map_err(|error| {
            desktop_error(
                "desktop.handoff_serialize_failed",
                500,
                "failed to serialize canonical handoff line",
                Some(json!({ "cause": error.to_string() })),
            )
        })?;
        out.push_str(&line);
        out.push('\n');
    }
    Ok(out)
}

fn artifact_root_for_cwd(cwd: &Path) -> DesktopApiResult<PathBuf> {
    if let Some(repo_root) = find_repo_root(cwd) {
        return Ok(repo_root.join(".opensession").join("artifacts"));
    }
    let global_objects_root = global_store_root().map_err(|error| {
        desktop_error(
            "desktop.handoff_store_unavailable",
            500,
            "failed to resolve global object store",
            Some(json!({ "cause": error.to_string() })),
        )
    })?;
    let parent = global_objects_root.parent().ok_or_else(|| {
        desktop_error(
            "desktop.handoff_store_unavailable",
            500,
            "invalid global object store path",
            Some(json!({ "path": global_objects_root })),
        )
    })?;
    Ok(parent.join("artifacts"))
}

fn is_valid_sha256(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(crate) fn artifact_path_for_hash(root: &Path, hash: &str) -> DesktopApiResult<PathBuf> {
    if !is_valid_sha256(hash) {
        return Err(desktop_error(
            "desktop.handoff_invalid_hash",
            400,
            "invalid artifact hash",
            Some(json!({ "hash": hash })),
        ));
    }
    Ok(root
        .join("sha256")
        .join(&hash[0..2])
        .join(&hash[2..4])
        .join(format!("{hash}.json")))
}

pub(crate) fn validate_pin_alias(alias: &str) -> DesktopApiResult<()> {
    let trimmed = alias.trim();
    if trimmed.is_empty() {
        return Err(desktop_error(
            "desktop.handoff_invalid_alias",
            400,
            "pin alias cannot be empty",
            Some(json!({ "alias": alias })),
        ));
    }
    if !trimmed
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'_' || byte == b'-')
    {
        return Err(desktop_error(
            "desktop.handoff_invalid_alias",
            400,
            "pin alias contains invalid characters",
            Some(json!({ "alias": alias })),
        ));
    }
    Ok(())
}

fn pin_path_for_alias(root: &Path, alias: &str) -> DesktopApiResult<PathBuf> {
    validate_pin_alias(alias)?;
    Ok(root.join("pins").join(alias))
}

fn store_handoff_artifact_record(
    record: &DesktopHandoffArtifactRecord,
    cwd: &Path,
) -> DesktopApiResult<()> {
    let root = artifact_root_for_cwd(cwd)?;
    let path = artifact_path_for_hash(&root, &record.sha256)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            desktop_error(
                "desktop.handoff_store_failed",
                500,
                "failed to prepare handoff artifact directory",
                Some(json!({ "cause": error.to_string(), "path": parent })),
            )
        })?;
    }
    if !path.exists() {
        let bytes = serde_json::to_vec_pretty(record).map_err(|error| {
            desktop_error(
                "desktop.handoff_store_failed",
                500,
                "failed to serialize handoff artifact record",
                Some(json!({ "cause": error.to_string() })),
            )
        })?;
        std::fs::write(&path, bytes).map_err(|error| {
            desktop_error(
                "desktop.handoff_store_failed",
                500,
                "failed to store handoff artifact",
                Some(json!({ "cause": error.to_string(), "path": path })),
            )
        })?;
    }
    Ok(())
}

fn set_handoff_pin(alias: &str, hash: &str, cwd: &Path) -> DesktopApiResult<()> {
    validate_pin_alias(alias)?;
    if !is_valid_sha256(hash) {
        return Err(desktop_error(
            "desktop.handoff_invalid_hash",
            400,
            "invalid artifact hash",
            Some(json!({ "hash": hash })),
        ));
    }

    let root = artifact_root_for_cwd(cwd)?;
    let pin_path = pin_path_for_alias(&root, alias)?;
    if let Some(parent) = pin_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            desktop_error(
                "desktop.handoff_store_failed",
                500,
                "failed to prepare handoff pin directory",
                Some(json!({ "cause": error.to_string(), "path": parent })),
            )
        })?;
    }

    std::fs::write(&pin_path, format!("{hash}\n")).map_err(|error| {
        desktop_error(
            "desktop.handoff_store_failed",
            500,
            "failed to write handoff pin alias",
            Some(json!({ "cause": error.to_string(), "path": pin_path, "alias": alias })),
        )
    })
}

pub(crate) fn build_handoff_artifact_record(
    normalized_session: &str,
    session: HailSession,
    pin_latest: bool,
    cwd: &Path,
) -> DesktopApiResult<DesktopHandoffBuildResponse> {
    let summaries = vec![HandoffSummary::from_session(&session)];
    let reports = validate_handoff_summaries(&summaries);
    let has_error_level = reports.iter().any(|report| {
        report
            .findings
            .iter()
            .any(|finding| finding.severity == "error")
    });
    if has_error_level {
        return Err(desktop_error(
            "desktop.handoff_validation_failed",
            422,
            "handoff validation failed with error-level findings",
            Some(json!({ "reports": reports })),
        ));
    }

    let canonical_jsonl = canonicalize_summaries(&summaries)?;
    let artifact_hash = sha256_hex(canonical_jsonl.as_bytes());

    let source_object =
        store_local_object(normalized_session.as_bytes(), cwd).map_err(|error| {
            desktop_error(
                "desktop.handoff_store_failed",
                500,
                "failed to store canonical source object for handoff",
                Some(json!({ "cause": error.to_string() })),
            )
        })?;

    let validation_reports = reports
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            desktop_error(
                "desktop.handoff_serialize_failed",
                500,
                "failed to serialize handoff validation report",
                Some(json!({ "cause": error.to_string() })),
            )
        })?;

    let mut deduped_source_uris = BTreeSet::new();
    deduped_source_uris.insert(source_object.uri.to_string());
    let runtime = load_runtime_config().unwrap_or_default();

    let record = DesktopHandoffArtifactRecord {
        version: HANDOFF_RECORD_VERSION.to_string(),
        sha256: artifact_hash.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
        source_uris: deduped_source_uris.into_iter().collect(),
        canonical_jsonl,
        raw_sessions: vec![session],
        summary_meta: Some(DesktopHandoffSummaryMeta {
            session_default_view: enum_label(&runtime.daemon.session_default_view),
            summary_source_mode: enum_label(&runtime.summary.source_mode),
            summary_provider: enum_label(&runtime.summary.provider.id),
        }),
        validation_reports,
    };
    store_handoff_artifact_record(&record, cwd)?;

    if pin_latest {
        set_handoff_pin(HANDOFF_LATEST_PIN_ALIAS, &artifact_hash, cwd)?;
    }

    let artifact_uri = SourceUri::Artifact {
        sha256: artifact_hash,
    }
    .to_string();
    let download_file_name = artifact_uri
        .strip_prefix("os://artifact/")
        .map(|hash| format!("handoff-{hash}.jsonl"));

    Ok(DesktopHandoffBuildResponse {
        artifact_uri,
        pinned_alias: pin_latest.then_some(HANDOFF_LATEST_PIN_ALIAS.to_string()),
        download_file_name,
        download_content: Some(record.canonical_jsonl),
    })
}

#[tauri::command]
pub(crate) fn desktop_build_handoff(
    request: DesktopHandoffBuildRequest,
) -> DesktopApiResult<DesktopHandoffBuildResponse> {
    let session_id = require_non_empty_request_field(
        &request.session_id,
        "desktop.handoff_invalid_request",
        "session_id",
    )?;

    let db = open_local_db()?;
    let normalized_session = load_normalized_session_body(&db, &session_id)?;
    let session = HailSession::from_jsonl(&normalized_session).map_err(|error| {
        desktop_error(
            "desktop.handoff_parse_failed",
            422,
            "failed to decode normalized session for handoff build",
            Some(json!({ "cause": error.to_string(), "session_id": session_id })),
        )
    })?;
    let cwd = std::env::current_dir().map_err(|error| {
        desktop_error(
            "desktop.handoff_store_unavailable",
            500,
            "failed to resolve current workspace directory",
            Some(json!({ "cause": error.to_string() })),
        )
    })?;

    build_handoff_artifact_record(&normalized_session, session, request.pin_latest, &cwd)
}

#[tauri::command]
pub(crate) fn desktop_share_session_quick(
    request: DesktopQuickShareRequest,
) -> DesktopApiResult<DesktopQuickShareResponse> {
    let session_id = require_non_empty_request_field(
        &request.session_id,
        "desktop.quick_share_invalid_request",
        "session_id",
    )?;

    let db = open_local_db()?;
    let normalized_session = load_normalized_session_body(&db, &session_id)?;
    let session = HailSession::from_jsonl(&normalized_session).map_err(|error| {
        desktop_error(
            "desktop.quick_share_parse_failed",
            422,
            "failed to decode normalized session for quick share",
            Some(json!({ "cause": error.to_string(), "session_id": session_id })),
        )
    })?;

    let command_cwd = working_directory(&session)
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .or_else(|| std::env::current_dir().ok())
        .ok_or_else(|| {
            desktop_error(
                "desktop.quick_share_cwd_unavailable",
                500,
                "failed to resolve command working directory",
                Some(json!({ "session_id": session_id })),
            )
        })?;

    let source_object =
        store_local_object(normalized_session.as_bytes(), &command_cwd).map_err(|error| {
            desktop_error(
                "desktop.quick_share_source_store_failed",
                500,
                "failed to store normalized source object for quick share",
                Some(json!({ "cause": error.to_string(), "session_id": session_id })),
            )
        })?;

    let mut command = Command::new("opensession");
    command
        .arg("share")
        .arg(source_object.uri.to_string())
        .arg("--quick")
        .arg("--json")
        .current_dir(&command_cwd);
    if let Some(remote) = normalize_optional_remote(request.remote) {
        command.arg("--remote").arg(remote);
    }

    let output = command.output().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            return desktop_error(
                "desktop.quick_share_cli_missing",
                501,
                "opensession CLI is unavailable. Install/enable the CLI bundle and retry.",
                Some(json!({ "cause": error.to_string() })),
            );
        }
        desktop_error(
            "desktop.quick_share_spawn_failed",
            500,
            "failed to start opensession CLI quick-share command",
            Some(json!({ "cause": error.to_string() })),
        )
    })?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(desktop_error(
            "desktop.quick_share_failed",
            422,
            "quick-share command failed",
            Some(json!({
                "session_id": session_id,
                "source_uri": source_object.uri.to_string(),
                "stdout": stdout,
                "stderr": stderr,
            })),
        ));
    }

    let stdout = String::from_utf8(output.stdout).map_err(|error| {
        desktop_error(
            "desktop.quick_share_invalid_utf8",
            500,
            "quick-share command returned non-UTF8 output",
            Some(json!({ "cause": error.to_string() })),
        )
    })?;
    parse_cli_quick_share_response(&stdout)
}
