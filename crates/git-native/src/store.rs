use std::path::Path;

use gix::object::tree::EntryKind;
use gix::ObjectId;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::GitStorageError;
    use crate::test_utils::{init_test_repo, run_git};
    use crate::{branch_ledger_ref, ops};

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
