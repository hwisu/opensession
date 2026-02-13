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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::GitStorageError;
    use crate::test_utils::init_test_repo;
    use crate::SESSIONS_BRANCH;

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
        let output = std::process::Command::new("git")
            .args(["branch", "--list", SESSIONS_BRANCH])
            .current_dir(tmp.path())
            .output()
            .unwrap();
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
}
