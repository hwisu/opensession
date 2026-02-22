use std::path::Path;

use gix::object::tree::EntryKind;
use gix::ObjectId;
use tracing::{debug, info};

use crate::error::Result;
use crate::ops::{self, gix_err};
use crate::{SESSIONS_BRANCH, SESSIONS_REF};

/// Git-native session storage using gix.
///
/// Stores session data (HAIL JSONL + metadata JSON) as blobs on an orphan
/// branch (`opensession/sessions`) without touching the working directory.
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
    /// Store a session in the git repository at `repo_path`.
    ///
    /// Creates the orphan branch if it doesn't exist, then adds/updates blobs
    /// for the HAIL JSONL and metadata JSON under `v1/<prefix>/<id>.*`.
    ///
    /// Returns the relative path within the branch (e.g. `v1/ab/abcdef.hail.jsonl`).
    pub fn store(
        &self,
        repo_path: &Path,
        session_id: &str,
        hail_jsonl: &[u8],
        meta_json: &[u8],
    ) -> Result<String> {
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
        let tip = ops::find_ref_tip(&repo, SESSIONS_BRANCH)?;
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

        let new_tree_id = editor.write().map_err(gix_err)?.detach();

        debug!(tree = %new_tree_id, "Built new tree");

        let parent = tip.map(|id| id.detach());
        let message = format!("session: {session_id}");
        let commit_id = ops::create_commit(&repo, SESSIONS_REF, new_tree_id, parent, &message)?;

        info!(
            session_id,
            commit = %commit_id,
            "Stored session on {SESSIONS_BRANCH}"
        );

        Ok(hail_path)
    }

    /// Prune expired sessions from the sessions branch by age (days).
    ///
    /// This rewrites `opensession/sessions` to a new orphan commit containing
    /// only currently retained paths.
    pub fn prune_by_age(&self, repo_path: &Path, keep_days: u32) -> Result<PruneStats> {
        let repo = ops::open_repo(repo_path)?;
        let tip = match ops::find_ref_tip(&repo, SESSIONS_BRANCH)? {
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
        ops::replace_ref_tip(&repo, SESSIONS_REF, tip, new_tip, &message)?;

        info!(
            keep_days,
            expired_sessions = expired.len(),
            old_tip = %tip,
            new_tip = %new_tip,
            "Pruned expired sessions on {SESSIONS_BRANCH}"
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
    use crate::{ops, SESSIONS_BRANCH};

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
    fn test_store() {
        let tmp = tempfile::tempdir().unwrap();
        init_test_repo(tmp.path());

        let storage = NativeGitStorage;
        let hail = b"{\"event\":\"test\"}\n";
        let meta = b"{\"title\":\"Test Session\"}";

        // Store
        let rel_path = storage
            .store(tmp.path(), "abc123-def456", hail, meta)
            .expect("store failed");
        assert_eq!(rel_path, "v1/ab/abc123-def456.hail.jsonl");

        // Verify branch exists
        let output = run_git(tmp.path(), &["branch", "--list", SESSIONS_BRANCH]);
        let branches = String::from_utf8_lossy(&output.stdout);
        assert!(
            branches.contains("opensession/sessions"),
            "branch not found: {branches}"
        );
    }

    #[test]
    fn test_not_a_repo() {
        let tmp = tempfile::tempdir().unwrap();
        // Don't init git repo
        let storage = NativeGitStorage;
        let err = storage
            .store(tmp.path(), "test", b"data", b"meta")
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
    fn test_prune_by_age_no_branch() {
        let tmp = tempfile::tempdir().unwrap();
        init_test_repo(tmp.path());

        let storage = NativeGitStorage;
        let stats = storage
            .prune_by_age(tmp.path(), 30)
            .expect("prune should work");
        assert_eq!(stats, PruneStats::default());
    }

    #[test]
    fn test_prune_by_age_rewrites_and_removes_expired_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        init_test_repo(tmp.path());

        let storage = NativeGitStorage;
        storage
            .store(tmp.path(), "abc123-def456", b"{\"event\":\"one\"}\n", b"{}")
            .expect("store should succeed");
        storage
            .store(tmp.path(), "ff0011-xyz", b"{\"event\":\"two\"}\n", b"{}")
            .expect("store should succeed");

        let repo = gix::open(tmp.path()).unwrap();
        let before_tip = ops::find_ref_tip(&repo, SESSIONS_BRANCH)
            .unwrap()
            .expect("sessions branch should exist")
            .detach();

        let stats = storage
            .prune_by_age(tmp.path(), 0)
            .expect("prune should work");
        assert!(stats.rewritten);
        assert_eq!(stats.expired_sessions, 2);

        let repo = gix::open(tmp.path()).unwrap();
        let after_tip = ops::find_ref_tip(&repo, SESSIONS_BRANCH)
            .unwrap()
            .expect("sessions branch should exist")
            .detach();
        assert_ne!(before_tip, after_tip, "tip should be rewritten");

        let commit = repo.find_commit(after_tip).unwrap();
        assert_eq!(
            commit.parent_ids().count(),
            0,
            "retention rewrite should produce orphan commit"
        );

        let output = run_git(tmp.path(), &["ls-tree", "-r", SESSIONS_BRANCH]);
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
        storage
            .store(tmp.path(), "abc123-def456", b"{\"event\":\"one\"}\n", b"{}")
            .expect("store should succeed");

        let repo = gix::open(tmp.path()).unwrap();
        let before_tip = ops::find_ref_tip(&repo, SESSIONS_BRANCH)
            .unwrap()
            .expect("sessions branch should exist")
            .detach();

        let stats = storage
            .prune_by_age(tmp.path(), 36500)
            .expect("prune should work");
        assert!(
            !stats.rewritten,
            "no prune should occur for very long retention"
        );
        assert_eq!(stats.expired_sessions, 0);
        assert_eq!(stats.scanned_sessions, 1);

        let repo = gix::open(tmp.path()).unwrap();
        let after_tip = ops::find_ref_tip(&repo, SESSIONS_BRANCH)
            .unwrap()
            .expect("sessions branch should exist")
            .detach();
        assert_eq!(before_tip, after_tip);
    }
}
