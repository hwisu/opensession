use std::path::Path;
use std::process::Command;

use gix::object::tree::EntryKind;
use gix::ObjectId;

use crate::error::{GitStorageError, Result};
use crate::ops::{self, gix_err};
use crate::HANDOFF_ARTIFACTS_REF_PREFIX;

const ARTIFACT_BLOB_PATH: &str = "artifact.json";

pub fn artifact_ref_name(artifact_id: &str) -> String {
    format!("{HANDOFF_ARTIFACTS_REF_PREFIX}/{}", artifact_id.trim())
}

pub fn store_handoff_artifact(
    repo_path: &Path,
    artifact_id: &str,
    artifact_json: &[u8],
) -> Result<String> {
    let repo = ops::open_repo(repo_path)?;
    let hash_kind = repo.object_hash();
    let ref_name = artifact_ref_name(artifact_id);

    let artifact_blob = repo.write_blob(artifact_json).map_err(gix_err)?.detach();

    let tip = ops::find_ref_tip(&repo, &ref_name)?;
    let base_tree_id = match &tip {
        Some(commit_id) => ops::commit_tree_id(&repo, commit_id.detach())?,
        None => ObjectId::empty_tree(hash_kind),
    };

    let mut editor = repo.edit_tree(base_tree_id).map_err(gix_err)?;
    editor
        .upsert(ARTIFACT_BLOB_PATH, EntryKind::Blob, artifact_blob)
        .map_err(gix_err)?;
    let new_tree_id = editor.write().map_err(gix_err)?.detach();

    let parent = tip.map(|id| id.detach());
    let message = format!("handoff-artifact: {artifact_id}");
    let _ = ops::create_commit(&repo, &ref_name, new_tree_id, parent, &message)?;
    Ok(ref_name)
}

pub fn load_handoff_artifact(repo_path: &Path, id_or_ref: &str) -> Result<Vec<u8>> {
    let _ = ops::open_repo(repo_path)?;
    let ref_name = normalize_ref_name(id_or_ref);

    let output = git_cmd(
        repo_path,
        &["show", &format!("{ref_name}:{ARTIFACT_BLOB_PATH}")],
    )?;
    if !output.status.success() {
        return Err(GitStorageError::NotFound(ref_name));
    }
    Ok(output.stdout)
}

pub fn list_handoff_artifact_refs(repo_path: &Path) -> Result<Vec<String>> {
    let _ = ops::open_repo(repo_path)?;
    let output = git_cmd(
        repo_path,
        &[
            "for-each-ref",
            "--format=%(refname)",
            HANDOFF_ARTIFACTS_REF_PREFIX,
        ],
    )?;
    if !output.status.success() {
        return Err(GitStorageError::Other(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    let mut refs = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    refs.sort();
    Ok(refs)
}

fn normalize_ref_name(value: &str) -> String {
    if value.starts_with("refs/") {
        value.to_string()
    } else {
        artifact_ref_name(value)
    }
}

fn git_cmd(repo_path: &Path, args: &[&str]) -> Result<std::process::Output> {
    Command::new("git")
        .args(args)
        .current_dir(repo_path)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .output()
        .map_err(GitStorageError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops;
    use crate::test_utils::init_test_repo;

    #[test]
    fn store_list_and_load_handoff_artifact() {
        let tmp = tempfile::tempdir().expect("tempdir");
        init_test_repo(tmp.path());

        let ref_name = store_handoff_artifact(tmp.path(), "artifact-1", br#"{"ok":true}"#)
            .expect("store artifact");
        assert_eq!(
            ref_name,
            "refs/opensession/handoff/artifacts/artifact-1".to_string()
        );

        let refs = list_handoff_artifact_refs(tmp.path()).expect("list refs");
        assert_eq!(refs, vec![ref_name.clone()]);

        let bytes = load_handoff_artifact(tmp.path(), "artifact-1").expect("load artifact");
        assert_eq!(String::from_utf8_lossy(&bytes), "{\"ok\":true}");
    }

    #[test]
    fn storing_existing_artifact_updates_tip() {
        let tmp = tempfile::tempdir().expect("tempdir");
        init_test_repo(tmp.path());

        let ref_name =
            store_handoff_artifact(tmp.path(), "artifact-2", br#"{"rev":1}"#).expect("store");
        let repo = gix::open(tmp.path()).expect("open");
        let first_tip = ops::find_ref_tip(&repo, &ref_name)
            .expect("tip")
            .expect("ref exists")
            .detach();

        store_handoff_artifact(tmp.path(), "artifact-2", br#"{"rev":2}"#).expect("store update");
        let repo = gix::open(tmp.path()).expect("open");
        let second_tip = ops::find_ref_tip(&repo, &ref_name)
            .expect("tip")
            .expect("ref exists")
            .detach();

        assert_ne!(first_tip, second_tip);
        let bytes = load_handoff_artifact(tmp.path(), "artifact-2").expect("load");
        assert_eq!(String::from_utf8_lossy(&bytes), "{\"rev\":2}");
    }

    #[test]
    fn load_missing_artifact_returns_not_found() {
        let tmp = tempfile::tempdir().expect("tempdir");
        init_test_repo(tmp.path());

        let err = load_handoff_artifact(tmp.path(), "missing-artifact").unwrap_err();
        assert!(matches!(err, GitStorageError::NotFound(_)));
    }
}
