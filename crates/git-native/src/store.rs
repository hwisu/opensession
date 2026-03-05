use std::path::Path;
use std::process::Command;

use gix::object::tree::EntryKind;
use gix::ObjectId;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{debug, info};

use crate::error::Result;
use crate::ops::{self, gix_err};

/// Git-native session storage using gix.
///
/// Stores session data (HAIL JSONL + metadata JSON) as blobs on an explicit
/// hidden ledger ref without touching the working directory.
pub struct NativeGitStorage;

/// Result of a git-native retention prune run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PruneStats {
    /// Number of unique sessions observed while scanning history.
    pub scanned_sessions: usize,
    /// Number of sessions considered expired by retention policy.
    pub expired_sessions: usize,
    /// Whether the sessions ref was rewritten.
    pub rewritten: bool,
}

/// Result of storing a session at an explicit ref.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSessionRecord {
    pub ref_name: String,
    pub commit_id: String,
    pub hail_path: String,
    pub meta_path: String,
}

/// Result of storing a semantic summary at an explicit ref.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSummaryRecord {
    pub ref_name: String,
    pub commit_id: String,
    pub summary_path: String,
    pub meta_path: String,
}

/// Session semantic summary payload persisted in hidden refs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionSummaryLedgerRecord {
    pub session_id: String,
    pub generated_at: String,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub source_kind: String,
    pub generation_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_fingerprint: Option<String>,
    pub summary: serde_json::Value,
    #[serde(default)]
    pub source_details: serde_json::Value,
    #[serde(default)]
    pub diff_tree: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl NativeGitStorage {
    /// Compute the storage path prefix for a session ID.
    /// e.g. session_id "abcdef-1234" → "v1/ab/abcdef-1234"
    fn session_prefix(session_id: &str) -> String {
        let prefix = if session_id.len() >= 2 {
            &session_id[..2]
        } else {
            session_id
        };
        format!("v1/{prefix}/{session_id}")
    }

    fn session_id_from_commit_message(message: &str) -> Option<&str> {
        let first = message.lines().next()?.trim();
        let id = first.strip_prefix("session: ")?.trim();
        if id.is_empty() {
            None
        } else {
            Some(id)
        }
    }

    fn commit_index_path(commit_sha: &str, session_id: &str) -> String {
        format!(
            "v1/index/commits/{}/{}.json",
            sanitize_path_component(commit_sha),
            sanitize_path_component(session_id)
        )
    }

    fn commit_index_payload(
        session_id: &str,
        hail_path: &str,
        meta_path: &str,
    ) -> serde_json::Value {
        json!({
            "session_id": session_id,
            "hail_path": hail_path,
            "meta_path": meta_path,
            "stored_at": chrono::Utc::now().to_rfc3339(),
        })
    }

    fn summary_prefix(session_id: &str) -> String {
        let prefix = if session_id.len() >= 2 {
            &session_id[..2]
        } else {
            session_id
        };
        format!("v1/summaries/{prefix}/{session_id}")
    }

    fn summary_session_id_from_commit_message(message: &str) -> Option<&str> {
        let first = message.lines().next()?.trim();
        let id = first.strip_prefix("summary: ")?.trim();
        if id.is_empty() {
            None
        } else {
            Some(id)
        }
    }
}

fn sanitize_path_component(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Store arbitrary blob content under a specific ref/path without touching the working tree.
///
/// This powers `opensession share --git`, which needs explicit ref/path control.
pub fn store_blob_at_ref(
    repo_path: &Path,
    ref_name: &str,
    rel_path: &str,
    body: &[u8],
    message: &str,
) -> Result<ObjectId> {
    let repo = ops::open_repo(repo_path)?;
    let hash_kind = repo.object_hash();

    let blob = repo.write_blob(body).map_err(gix_err)?.detach();
    let tip = ops::find_ref_tip(&repo, ref_name)?;
    let base_tree_id = match &tip {
        Some(commit_id) => ops::commit_tree_id(&repo, commit_id.detach())?,
        None => ObjectId::empty_tree(hash_kind),
    };

    let mut editor = repo.edit_tree(base_tree_id).map_err(gix_err)?;
    editor
        .upsert(rel_path, EntryKind::Blob, blob)
        .map_err(gix_err)?;
    let new_tree_id = editor.write().map_err(gix_err)?.detach();
    let parent = tip.map(|id| id.detach());
    ops::create_commit(&repo, ref_name, new_tree_id, parent, message)
}

impl NativeGitStorage {
    /// Store a session at an explicit ref name.
    ///
    /// Stores body and metadata blobs plus per-commit index entries:
    /// `v1/index/commits/<sha>/<session_id>.json`.
    pub fn store_session_at_ref(
        &self,
        repo_path: &Path,
        ref_name: &str,
        session_id: &str,
        hail_jsonl: &[u8],
        meta_json: &[u8],
        commit_shas: &[String],
    ) -> Result<StoredSessionRecord> {
        let repo = ops::open_repo(repo_path)?;
        let hash_kind = repo.object_hash();

        // Write blobs
        let hail_blob = repo.write_blob(hail_jsonl).map_err(gix_err)?.detach();
        let meta_blob = repo.write_blob(meta_json).map_err(gix_err)?.detach();

        debug!(
            session_id,
            hail_blob = %hail_blob,
            meta_blob = %meta_blob,
            "Wrote session blobs"
        );

        let prefix = Self::session_prefix(session_id);
        let hail_path = format!("{prefix}.hail.jsonl");
        let meta_path = format!("{prefix}.meta.json");

        // Determine base tree: existing branch tree or empty tree
        let tip = ops::find_ref_tip(&repo, ref_name)?;
        let base_tree_id = match &tip {
            Some(commit_id) => ops::commit_tree_id(&repo, commit_id.detach())?,
            None => ObjectId::empty_tree(hash_kind),
        };

        // Build new tree using editor
        let mut editor = repo.edit_tree(base_tree_id).map_err(gix_err)?;
        editor
            .upsert(&hail_path, EntryKind::Blob, hail_blob)
            .map_err(gix_err)?;
        editor
            .upsert(&meta_path, EntryKind::Blob, meta_blob)
            .map_err(gix_err)?;

        for sha in commit_shas {
            let trimmed = sha.trim();
            if trimmed.is_empty() {
                continue;
            }
            let index_path = Self::commit_index_path(trimmed, session_id);
            let payload = Self::commit_index_payload(session_id, &hail_path, &meta_path);
            let payload_bytes = serde_json::to_vec(&payload)?;
            let payload_blob = repo.write_blob(&payload_bytes).map_err(gix_err)?.detach();
            editor
                .upsert(&index_path, EntryKind::Blob, payload_blob)
                .map_err(gix_err)?;
        }

        let new_tree_id = editor.write().map_err(gix_err)?.detach();

        debug!(tree = %new_tree_id, "Built new tree");

        let parent = tip.map(|id| id.detach());
        let message = format!("session: {session_id}");
        let commit_id = ops::create_commit(&repo, ref_name, new_tree_id, parent, &message)?;

        info!(
            session_id,
            ref_name,
            commit = %commit_id,
            "Stored session on ref"
        );

        Ok(StoredSessionRecord {
            ref_name: ref_name.to_string(),
            commit_id: commit_id.to_string(),
            hail_path,
            meta_path,
        })
    }

    /// Store a session semantic summary at an explicit ref.
    ///
    /// Paths:
    /// - `v1/summaries/<prefix>/<session_id>.summary.json`
    /// - `v1/summaries/<prefix>/<session_id>.summary.meta.json`
    pub fn store_summary_at_ref(
        &self,
        repo_path: &Path,
        ref_name: &str,
        record: &SessionSummaryLedgerRecord,
    ) -> Result<StoredSummaryRecord> {
        let repo = ops::open_repo(repo_path)?;
        let hash_kind = repo.object_hash();
        let prefix = Self::summary_prefix(&record.session_id);
        let summary_path = format!("{prefix}.summary.json");
        let meta_path = format!("{prefix}.summary.meta.json");

        let summary_bytes = serde_json::to_vec(&record.summary)?;
        let summary_blob = repo.write_blob(&summary_bytes).map_err(gix_err)?.detach();

        let meta_payload = json!({
            "session_id": record.session_id,
            "generated_at": record.generated_at,
            "provider": record.provider,
            "model": record.model,
            "source_kind": record.source_kind,
            "generation_kind": record.generation_kind,
            "prompt_fingerprint": record.prompt_fingerprint,
            "source_details": record.source_details,
            "diff_tree": record.diff_tree,
            "error": record.error,
            "updated_at": chrono::Utc::now().to_rfc3339(),
        });
        let meta_bytes = serde_json::to_vec(&meta_payload)?;
        let meta_blob = repo.write_blob(&meta_bytes).map_err(gix_err)?.detach();

        let tip = ops::find_ref_tip(&repo, ref_name)?;
        let base_tree_id = match &tip {
            Some(commit_id) => ops::commit_tree_id(&repo, commit_id.detach())?,
            None => ObjectId::empty_tree(hash_kind),
        };
        let mut editor = repo.edit_tree(base_tree_id).map_err(gix_err)?;
        editor
            .upsert(&summary_path, EntryKind::Blob, summary_blob)
            .map_err(gix_err)?;
        editor
            .upsert(&meta_path, EntryKind::Blob, meta_blob)
            .map_err(gix_err)?;
        let new_tree_id = editor.write().map_err(gix_err)?.detach();
        let parent = tip.map(|id| id.detach());
        let message = format!("summary: {}", record.session_id);
        let commit_id = ops::create_commit(&repo, ref_name, new_tree_id, parent, &message)?;

        Ok(StoredSummaryRecord {
            ref_name: ref_name.to_string(),
            commit_id: commit_id.to_string(),
            summary_path,
            meta_path,
        })
    }

    /// Load a session semantic summary from an explicit ref.
    pub fn load_summary_at_ref(
        &self,
        repo_path: &Path,
        ref_name: &str,
        session_id: &str,
    ) -> Result<Option<SessionSummaryLedgerRecord>> {
        let prefix = Self::summary_prefix(session_id);
        let summary_path = format!("{prefix}.summary.json");
        let meta_path = format!("{prefix}.summary.meta.json");

        let summary_raw = match read_path_from_ref(repo_path, ref_name, &summary_path)? {
            Some(value) => value,
            None => return Ok(None),
        };
        let meta_raw = match read_path_from_ref(repo_path, ref_name, &meta_path)? {
            Some(value) => value,
            None => return Ok(None),
        };

        let summary_value: serde_json::Value = serde_json::from_slice(&summary_raw)?;
        let meta_value: serde_json::Value = serde_json::from_slice(&meta_raw)?;

        Ok(Some(SessionSummaryLedgerRecord {
            session_id: meta_value
                .get("session_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(session_id)
                .to_string(),
            generated_at: meta_value
                .get("generated_at")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            provider: meta_value
                .get("provider")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
            model: meta_value
                .get("model")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            source_kind: meta_value
                .get("source_kind")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
            generation_kind: meta_value
                .get("generation_kind")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
            prompt_fingerprint: meta_value
                .get("prompt_fingerprint")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            summary: summary_value,
            source_details: meta_value
                .get("source_details")
                .cloned()
                .unwrap_or(serde_json::Value::Object(Default::default())),
            diff_tree: meta_value
                .get("diff_tree")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default(),
            error: meta_value
                .get("error")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
        }))
    }

    /// Delete a session semantic summary from an explicit ref.
    ///
    /// Returns true when the ref was rewritten, false when no summary existed.
    pub fn delete_summary_at_ref(
        &self,
        repo_path: &Path,
        ref_name: &str,
        session_id: &str,
    ) -> Result<bool> {
        let repo = ops::open_repo(repo_path)?;
        let tip = match ops::find_ref_tip(&repo, ref_name)? {
            Some(tip) => tip.detach(),
            None => return Ok(false),
        };

        let prefix = Self::summary_prefix(session_id);
        let summary_path = format!("{prefix}.summary.json");
        let meta_path = format!("{prefix}.summary.meta.json");
        let has_summary = read_path_from_ref(repo_path, ref_name, &summary_path)?.is_some();
        let has_meta = read_path_from_ref(repo_path, ref_name, &meta_path)?.is_some();
        if !has_summary && !has_meta {
            return Ok(false);
        }

        let base_tree_id = ops::commit_tree_id(&repo, tip)?;
        let mut editor = repo.edit_tree(base_tree_id).map_err(gix_err)?;
        if has_summary {
            editor.remove(&summary_path).map_err(gix_err)?;
        }
        if has_meta {
            editor.remove(&meta_path).map_err(gix_err)?;
        }

        let new_tree_id = editor.write().map_err(gix_err)?.detach();
        let message = format!("summary-delete: {session_id}");
        let sig = ops::make_signature();
        let commit = gix::objs::Commit {
            message: message.clone().into(),
            tree: new_tree_id,
            author: sig.clone(),
            committer: sig,
            encoding: None,
            parents: Vec::<ObjectId>::new().into(),
            extra_headers: Default::default(),
        };
        let new_tip = repo.write_object(&commit).map_err(gix_err)?.detach();
        ops::replace_ref_tip(&repo, ref_name, tip, new_tip, &message)?;
        Ok(true)
    }

    /// Prune expired semantic summaries from a specific summary ref by age (days).
    pub fn prune_summaries_by_age_at_ref(
        &self,
        repo_path: &Path,
        ref_name: &str,
        keep_days: u32,
    ) -> Result<PruneStats> {
        let repo = ops::open_repo(repo_path)?;
        let tip = match ops::find_ref_tip(&repo, ref_name)? {
            Some(tip) => tip.detach(),
            None => return Ok(PruneStats::default()),
        };

        let cutoff = chrono::Utc::now()
            .timestamp()
            .saturating_sub((keep_days as i64).saturating_mul(24 * 60 * 60));

        let mut latest_seen: std::collections::HashMap<String, i64> =
            std::collections::HashMap::new();
        let mut current = Some(tip);
        while let Some(commit_id) = current {
            let commit = repo.find_commit(commit_id).map_err(gix_err)?;
            let message = String::from_utf8_lossy(commit.message_raw_sloppy().as_ref());
            if let Some(session_id) = Self::summary_session_id_from_commit_message(&message) {
                latest_seen
                    .entry(session_id.to_string())
                    .or_insert(commit.time().map_err(gix_err)?.seconds);
            }
            current = commit.parent_ids().next().map(|id| id.detach());
        }

        let mut expired: Vec<String> = latest_seen
            .iter()
            .filter_map(|(id, ts)| {
                if *ts <= cutoff {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect();
        expired.sort();

        if expired.is_empty() {
            return Ok(PruneStats {
                scanned_sessions: latest_seen.len(),
                expired_sessions: 0,
                rewritten: false,
            });
        }

        let base_tree_id = ops::commit_tree_id(&repo, tip)?;
        let mut editor = repo.edit_tree(base_tree_id).map_err(gix_err)?;
        for session_id in &expired {
            let prefix = Self::summary_prefix(session_id);
            let summary_path = format!("{prefix}.summary.json");
            let meta_path = format!("{prefix}.summary.meta.json");
            editor.remove(&summary_path).map_err(gix_err)?;
            editor.remove(&meta_path).map_err(gix_err)?;
        }

        let new_tree_id = editor.write().map_err(gix_err)?.detach();
        let message = format!(
            "summary-retention-prune: keep_days={keep_days} expired={}",
            expired.len()
        );
        let sig = ops::make_signature();
        let commit = gix::objs::Commit {
            message: message.clone().into(),
            tree: new_tree_id,
            author: sig.clone(),
            committer: sig,
            encoding: None,
            parents: Vec::<ObjectId>::new().into(),
            extra_headers: Default::default(),
        };
        let new_tip = repo.write_object(&commit).map_err(gix_err)?.detach();
        ops::replace_ref_tip(&repo, ref_name, tip, new_tip, &message)?;

        Ok(PruneStats {
            scanned_sessions: latest_seen.len(),
            expired_sessions: expired.len(),
            rewritten: true,
        })
    }

    /// Prune expired sessions from a specific ledger ref by age (days).
    pub fn prune_by_age_at_ref(
        &self,
        repo_path: &Path,
        ref_name: &str,
        keep_days: u32,
    ) -> Result<PruneStats> {
        let repo = ops::open_repo(repo_path)?;
        let tip = match ops::find_ref_tip(&repo, ref_name)? {
            Some(tip) => tip.detach(),
            None => return Ok(PruneStats::default()),
        };

        let cutoff = chrono::Utc::now()
            .timestamp()
            .saturating_sub((keep_days as i64).saturating_mul(24 * 60 * 60));

        // First-parent walk from tip to capture latest-seen timestamp per session.
        let mut latest_seen: std::collections::HashMap<String, i64> =
            std::collections::HashMap::new();
        let mut current = Some(tip);
        while let Some(commit_id) = current {
            let commit = repo.find_commit(commit_id).map_err(gix_err)?;

            let message = String::from_utf8_lossy(commit.message_raw_sloppy().as_ref());
            if let Some(session_id) = Self::session_id_from_commit_message(&message) {
                latest_seen
                    .entry(session_id.to_string())
                    .or_insert(commit.time().map_err(gix_err)?.seconds);
            }

            current = commit.parent_ids().next().map(|id| id.detach());
        }

        let mut expired: Vec<String> = latest_seen
            .iter()
            .filter_map(|(id, ts)| {
                if *ts <= cutoff {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect();
        expired.sort();

        if expired.is_empty() {
            return Ok(PruneStats {
                scanned_sessions: latest_seen.len(),
                expired_sessions: 0,
                rewritten: false,
            });
        }

        let base_tree_id = ops::commit_tree_id(&repo, tip)?;
        let mut editor = repo.edit_tree(base_tree_id).map_err(gix_err)?;
        for session_id in &expired {
            let prefix = Self::session_prefix(session_id);
            let hail_path = format!("{prefix}.hail.jsonl");
            let meta_path = format!("{prefix}.meta.json");
            editor.remove(&hail_path).map_err(gix_err)?;
            editor.remove(&meta_path).map_err(gix_err)?;
        }

        let new_tree_id = editor.write().map_err(gix_err)?.detach();
        let message = format!(
            "retention-prune: keep_days={keep_days} expired={}",
            expired.len()
        );
        let sig = ops::make_signature();
        let commit = gix::objs::Commit {
            message: message.clone().into(),
            tree: new_tree_id,
            author: sig.clone(),
            committer: sig,
            encoding: None,
            parents: Vec::<ObjectId>::new().into(),
            extra_headers: Default::default(),
        };
        let new_tip = repo.write_object(&commit).map_err(gix_err)?.detach();
        ops::replace_ref_tip(&repo, ref_name, tip, new_tip, &message)?;

        info!(
            ref_name,
            keep_days,
            expired_sessions = expired.len(),
            old_tip = %tip,
            new_tip = %new_tip,
            "Pruned expired sessions on ref"
        );

        Ok(PruneStats {
            scanned_sessions: latest_seen.len(),
            expired_sessions: expired.len(),
            rewritten: true,
        })
    }
}

fn read_path_from_ref(repo_path: &Path, ref_name: &str, rel_path: &str) -> Result<Option<Vec<u8>>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("show")
        .arg(format!("{ref_name}:{rel_path}"))
        .output()?;
    if output.status.success() {
        return Ok(Some(output.stdout));
    }
    let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
    if stderr.contains("does not exist")
        || stderr.contains("not in")
        || stderr.contains("unknown revision")
        || stderr.contains("invalid object name")
    {
        return Ok(None);
    }
    Err(crate::error::GitStorageError::Other(format!(
        "failed to read {rel_path} from {ref_name}: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::GitStorageError;
    use crate::test_utils::{init_test_repo, run_git};
    use crate::{branch_ledger_ref, ops};
    use serde_json::json;

    #[test]
    fn test_session_prefix() {
        assert_eq!(
            NativeGitStorage::session_prefix("abcdef-1234"),
            "v1/ab/abcdef-1234"
        );
        assert_eq!(NativeGitStorage::session_prefix("x"), "v1/x/x");
        assert_eq!(NativeGitStorage::session_prefix("ab"), "v1/ab/ab");
    }

    #[test]
    fn test_not_a_repo() {
        let tmp = tempfile::tempdir().unwrap();
        // Don't init git repo
        let storage = NativeGitStorage;
        let ref_name = branch_ledger_ref("main");
        let err = storage
            .store_session_at_ref(tmp.path(), &ref_name, "test", b"data", b"meta", &[])
            .unwrap_err();
        assert!(
            matches!(err, GitStorageError::NotARepo(_)),
            "expected NotARepo, got: {err}"
        );
    }

    #[test]
    fn store_blob_at_ref_writes_requested_path() {
        let tmp = tempfile::tempdir().expect("tempdir");
        init_test_repo(tmp.path());

        let ref_name = "refs/heads/opensession/custom-share";
        let rel_path = "sessions/hash.jsonl";
        store_blob_at_ref(
            tmp.path(),
            ref_name,
            rel_path,
            b"hello",
            "custom share write",
        )
        .expect("store blob at ref");

        let output = run_git(tmp.path(), &["show", &format!("{ref_name}:{rel_path}")]);
        assert_eq!(String::from_utf8_lossy(&output.stdout), "hello");
    }

    #[test]
    fn test_store_session_at_ref_writes_commit_indexes() {
        let tmp = tempfile::tempdir().expect("tempdir");
        init_test_repo(tmp.path());

        let storage = NativeGitStorage;
        let ref_name = branch_ledger_ref("feature/ledger");
        let result = storage
            .store_session_at_ref(
                tmp.path(),
                &ref_name,
                "session-1",
                b"{\"event\":\"one\"}\n",
                b"{\"meta\":1}",
                &["abcd1234".to_string(), "beef5678".to_string()],
            )
            .expect("store at ref");

        assert_eq!(result.ref_name, ref_name);
        assert_eq!(result.hail_path, "v1/se/session-1.hail.jsonl");
        assert_eq!(result.meta_path, "v1/se/session-1.meta.json");
        assert!(!result.commit_id.is_empty());
        run_git(tmp.path(), &["show-ref", "--verify", "--quiet", &ref_name]);

        let first_index = "v1/index/commits/abcd1234/session-1.json";
        let first_output = run_git(tmp.path(), &["show", &format!("{ref_name}:{first_index}")]);
        let parsed: serde_json::Value =
            serde_json::from_slice(&first_output.stdout).expect("valid index payload");
        assert_eq!(parsed["session_id"], "session-1");
        assert_eq!(parsed["hail_path"], "v1/se/session-1.hail.jsonl");
    }

    #[test]
    fn test_store_and_load_summary_at_ref() {
        let tmp = tempfile::tempdir().expect("tempdir");
        init_test_repo(tmp.path());

        let storage = NativeGitStorage;
        let ref_name = "refs/opensession/summaries";
        let record = SessionSummaryLedgerRecord {
            session_id: "session-9".to_string(),
            generated_at: "2026-03-05T00:00:00Z".to_string(),
            provider: "codex_exec".to_string(),
            model: Some("gpt-5".to_string()),
            source_kind: "session_signals".to_string(),
            generation_kind: "provider".to_string(),
            prompt_fingerprint: Some("abc123".to_string()),
            summary: json!({ "changes": "updated", "auth_security": "none detected", "layer_file_changes": [] }),
            source_details: json!({ "repo_root": "/tmp/repo" }),
            diff_tree: vec![json!({"layer":"application","files":[]})],
            error: None,
        };

        let stored = storage
            .store_summary_at_ref(tmp.path(), ref_name, &record)
            .expect("store summary");
        assert_eq!(
            stored.summary_path,
            "v1/summaries/se/session-9.summary.json"
        );
        assert_eq!(
            stored.meta_path,
            "v1/summaries/se/session-9.summary.meta.json"
        );

        let loaded = storage
            .load_summary_at_ref(tmp.path(), ref_name, "session-9")
            .expect("load summary")
            .expect("summary should exist");
        assert_eq!(loaded.session_id, "session-9");
        assert_eq!(loaded.provider, "codex_exec");
        assert_eq!(loaded.model.as_deref(), Some("gpt-5"));
        assert_eq!(loaded.summary["changes"], "updated");
    }

    #[test]
    fn test_load_summary_missing_returns_none() {
        let tmp = tempfile::tempdir().expect("tempdir");
        init_test_repo(tmp.path());
        let storage = NativeGitStorage;

        let loaded = storage
            .load_summary_at_ref(tmp.path(), "refs/opensession/summaries", "missing-session")
            .expect("load summary");
        assert!(loaded.is_none());
    }

    #[test]
    fn test_delete_summary_at_ref_removes_paths() {
        let tmp = tempfile::tempdir().expect("tempdir");
        init_test_repo(tmp.path());

        let storage = NativeGitStorage;
        let ref_name = "refs/opensession/summaries";
        let record = SessionSummaryLedgerRecord {
            session_id: "session-delete".to_string(),
            generated_at: "2026-03-05T00:00:00Z".to_string(),
            provider: "codex_exec".to_string(),
            model: None,
            source_kind: "session_signals".to_string(),
            generation_kind: "provider".to_string(),
            prompt_fingerprint: None,
            summary: json!({ "changes": "x" }),
            source_details: json!({}),
            diff_tree: vec![],
            error: None,
        };
        storage
            .store_summary_at_ref(tmp.path(), ref_name, &record)
            .expect("store summary");

        let rewritten = storage
            .delete_summary_at_ref(tmp.path(), ref_name, "session-delete")
            .expect("delete summary");
        assert!(rewritten);
        assert!(storage
            .load_summary_at_ref(tmp.path(), ref_name, "session-delete")
            .expect("load after delete")
            .is_none());
    }

    #[test]
    fn test_prune_summaries_by_age_rewrites_and_removes_expired_summaries() {
        let tmp = tempfile::tempdir().expect("tempdir");
        init_test_repo(tmp.path());

        let storage = NativeGitStorage;
        let ref_name = "refs/opensession/summaries";
        let record_a = SessionSummaryLedgerRecord {
            session_id: "summary-a".to_string(),
            generated_at: "2026-03-05T00:00:00Z".to_string(),
            provider: "codex_exec".to_string(),
            model: None,
            source_kind: "session_signals".to_string(),
            generation_kind: "provider".to_string(),
            prompt_fingerprint: None,
            summary: json!({ "changes": "a" }),
            source_details: json!({}),
            diff_tree: vec![],
            error: None,
        };
        let record_b = SessionSummaryLedgerRecord {
            session_id: "summary-b".to_string(),
            generated_at: "2026-03-05T00:00:01Z".to_string(),
            provider: "codex_exec".to_string(),
            model: None,
            source_kind: "session_signals".to_string(),
            generation_kind: "provider".to_string(),
            prompt_fingerprint: None,
            summary: json!({ "changes": "b" }),
            source_details: json!({}),
            diff_tree: vec![],
            error: None,
        };
        storage
            .store_summary_at_ref(tmp.path(), ref_name, &record_a)
            .expect("store summary a");
        storage
            .store_summary_at_ref(tmp.path(), ref_name, &record_b)
            .expect("store summary b");

        let stats = storage
            .prune_summaries_by_age_at_ref(tmp.path(), ref_name, 0)
            .expect("prune summaries");
        assert!(stats.rewritten);
        assert_eq!(stats.expired_sessions, 2);
        assert!(storage
            .load_summary_at_ref(tmp.path(), ref_name, "summary-a")
            .expect("load summary a")
            .is_none());
        assert!(storage
            .load_summary_at_ref(tmp.path(), ref_name, "summary-b")
            .expect("load summary b")
            .is_none());
    }

    #[test]
    fn test_prune_by_age_no_branch() {
        let tmp = tempfile::tempdir().unwrap();
        init_test_repo(tmp.path());

        let storage = NativeGitStorage;
        let ref_name = branch_ledger_ref("feature/no-branch");
        let stats = storage
            .prune_by_age_at_ref(tmp.path(), &ref_name, 30)
            .expect("prune should work");
        assert_eq!(stats, PruneStats::default());
    }

    #[test]
    fn test_prune_by_age_rewrites_and_removes_expired_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        init_test_repo(tmp.path());

        let storage = NativeGitStorage;
        let ref_name = branch_ledger_ref("feature/prune-expired");
        storage
            .store_session_at_ref(
                tmp.path(),
                &ref_name,
                "abc123-def456",
                b"{\"event\":\"one\"}\n",
                b"{}",
                &[],
            )
            .expect("store should succeed");
        storage
            .store_session_at_ref(
                tmp.path(),
                &ref_name,
                "ff0011-xyz",
                b"{\"event\":\"two\"}\n",
                b"{}",
                &[],
            )
            .expect("store should succeed");

        let repo = gix::open(tmp.path()).unwrap();
        let before_tip = ops::find_ref_tip(&repo, &ref_name)
            .unwrap()
            .expect("ledger ref should exist")
            .detach();

        let stats = storage
            .prune_by_age_at_ref(tmp.path(), &ref_name, 0)
            .expect("prune should work");
        assert!(stats.rewritten);
        assert_eq!(stats.expired_sessions, 2);

        let repo = gix::open(tmp.path()).unwrap();
        let after_tip = ops::find_ref_tip(&repo, &ref_name)
            .unwrap()
            .expect("ledger ref should exist")
            .detach();
        assert_ne!(before_tip, after_tip, "tip should be rewritten");

        let commit = repo.find_commit(after_tip).unwrap();
        assert_eq!(
            commit.parent_ids().count(),
            0,
            "retention rewrite should produce orphan commit"
        );

        let output = run_git(tmp.path(), &["ls-tree", "-r", &ref_name]);
        let listing = String::from_utf8_lossy(&output.stdout);
        assert!(
            !listing.contains(".hail.jsonl"),
            "expected no retained session blobs after prune: {listing}"
        );
    }

    #[test]
    fn test_prune_by_age_keeps_recent_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        init_test_repo(tmp.path());

        let storage = NativeGitStorage;
        let ref_name = branch_ledger_ref("feature/prune-keep");
        storage
            .store_session_at_ref(
                tmp.path(),
                &ref_name,
                "abc123-def456",
                b"{\"event\":\"one\"}\n",
                b"{}",
                &[],
            )
            .expect("store should succeed");

        let repo = gix::open(tmp.path()).unwrap();
        let before_tip = ops::find_ref_tip(&repo, &ref_name)
            .unwrap()
            .expect("ledger ref should exist")
            .detach();

        let stats = storage
            .prune_by_age_at_ref(tmp.path(), &ref_name, 36500)
            .expect("prune should work");
        assert!(
            !stats.rewritten,
            "no prune should occur for very long retention"
        );
        assert_eq!(stats.expired_sessions, 0);
        assert_eq!(stats.scanned_sessions, 1);

        let repo = gix::open(tmp.path()).unwrap();
        let after_tip = ops::find_ref_tip(&repo, &ref_name)
            .unwrap()
            .expect("ledger ref should exist")
            .detach();
        assert_eq!(before_tip, after_tip);
    }
}
