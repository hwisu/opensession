use std::path::{Path, PathBuf};

use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit, RefLog};
use gix::{ObjectId, Repository};

use crate::error::{GitStorageError, Result};

/// Wrap any gix-compatible error into [`GitStorageError::Gix`].
pub fn gix_err(e: impl std::error::Error + Send + Sync + 'static) -> GitStorageError {
    GitStorageError::Gix(Box::new(e))
}

/// Open a git repository at `repo_path`.
///
/// Returns [`GitStorageError::NotARepo`] when `.git` is absent.
pub fn open_repo(repo_path: &Path) -> Result<Repository> {
    let repo = gix::open(repo_path).map_err(|e| {
        if repo_path.join(".git").exists() {
            gix_err(e)
        } else {
            GitStorageError::NotARepo(repo_path.to_path_buf())
        }
    })?;
    Ok(repo)
}

/// Find the tip commit of a ref, returning `None` if the ref doesn't exist.
pub fn find_ref_tip<'r>(repo: &'r Repository, ref_name: &str) -> Result<Option<gix::Id<'r>>> {
    match repo.try_find_reference(ref_name).map_err(gix_err)? {
        Some(reference) => {
            let id = reference.into_fully_peeled_id().map_err(gix_err)?;
            Ok(Some(id))
        }
        None => Ok(None),
    }
}

/// Get the tree [`ObjectId`] from a commit.
pub fn commit_tree_id(repo: &Repository, commit_id: ObjectId) -> Result<ObjectId> {
    let commit = repo
        .find_object(commit_id)
        .map_err(gix_err)?
        .try_into_commit()
        .map_err(gix_err)?;
    let tree_id = commit.tree_id().map_err(gix_err)?;
    Ok(tree_id.detach())
}

/// Build the default OpenSession committer/author signature.
pub fn make_signature() -> gix::actor::Signature {
    gix::actor::Signature {
        name: "opensession".into(),
        email: "cli@opensession.io".into(),
        time: gix::date::Time::now_local_or_utc(),
    }
}

/// Create a commit on `ref_name`, optionally with a parent.
///
/// The ref is updated atomically — it must either not exist (when `parent` is
/// `None`) or point to `parent` (when `Some`).
pub fn create_commit(
    repo: &Repository,
    ref_name: &str,
    tree_id: ObjectId,
    parent: Option<ObjectId>,
    message: &str,
) -> Result<ObjectId> {
    let sig = make_signature();
    let parents: Vec<ObjectId> = parent.into_iter().collect();

    let commit = gix::objs::Commit {
        message: message.into(),
        tree: tree_id,
        author: sig.clone(),
        committer: sig,
        encoding: None,
        parents: parents.clone().into(),
        extra_headers: Default::default(),
    };

    let commit_id = repo.write_object(&commit).map_err(gix_err)?.detach();

    let expected = match parents.first() {
        Some(p) => PreviousValue::ExistingMustMatch(gix::refs::Target::Object(*p)),
        None => PreviousValue::MustNotExist,
    };

    repo.edit_references([RefEdit {
        change: Change::Update {
            log: LogChange {
                mode: RefLog::AndReference,
                force_create_reflog: false,
                message: message.into(),
            },
            expected,
            new: gix::refs::Target::Object(commit_id),
        },
        name: ref_name
            .try_into()
            .map_err(|e: gix::validate::reference::name::Error| gix_err(e))?,
        deref: false,
    }])
    .map_err(gix_err)?;

    Ok(commit_id)
}

/// Replace a ref target with `new_tip`, requiring the current tip to match
/// `expected_tip`.
pub fn replace_ref_tip(
    repo: &Repository,
    ref_name: &str,
    expected_tip: ObjectId,
    new_tip: ObjectId,
    message: &str,
) -> Result<()> {
    repo.edit_references([RefEdit {
        change: Change::Update {
            log: LogChange {
                mode: RefLog::AndReference,
                force_create_reflog: false,
                message: message.into(),
            },
            expected: PreviousValue::ExistingMustMatch(gix::refs::Target::Object(expected_tip)),
            new: gix::refs::Target::Object(new_tip),
        },
        name: ref_name
            .try_into()
            .map_err(|e: gix::validate::reference::name::Error| gix_err(e))?,
        deref: false,
    }])
    .map_err(gix_err)?;
    Ok(())
}

/// Delete a ref, requiring it currently points to `expected_tip`.
pub fn delete_ref(repo: &Repository, ref_name: &str, expected_tip: ObjectId) -> Result<()> {
    repo.edit_references([RefEdit {
        change: Change::Delete {
            expected: PreviousValue::ExistingMustMatch(gix::refs::Target::Object(expected_tip)),
            log: RefLog::AndReference,
        },
        name: ref_name
            .try_into()
            .map_err(|e: gix::validate::reference::name::Error| gix_err(e))?,
        deref: false,
    }])
    .map_err(gix_err)?;
    Ok(())
}

/// Find the git repository root by walking up from `from` looking for `.git`.
pub fn find_repo_root(from: &Path) -> Option<PathBuf> {
    let mut dir = from.to_path_buf();
    loop {
        if dir.join(".git").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::init_test_repo;

    #[test]
    fn test_find_repo_root() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("myrepo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let subdir = repo.join("src").join("deep");
        std::fs::create_dir_all(&subdir).unwrap();

        assert_eq!(find_repo_root(&subdir), Some(repo.clone()));
        assert_eq!(find_repo_root(&repo), Some(repo));

        let no_repo = tmp.path().join("norope");
        std::fs::create_dir_all(&no_repo).unwrap();
        assert_eq!(find_repo_root(&no_repo), None);
    }

    #[test]
    fn test_open_repo_success() {
        let tmp = tempfile::tempdir().unwrap();
        init_test_repo(tmp.path());

        let repo = open_repo(tmp.path());
        assert!(repo.is_ok(), "expected Ok, got: {}", repo.unwrap_err());
    }

    #[test]
    fn test_open_repo_not_a_repo() {
        let tmp = tempfile::tempdir().unwrap();
        // Don't init git — just a bare directory
        let err = open_repo(tmp.path()).unwrap_err();
        assert!(
            matches!(err, GitStorageError::NotARepo(_)),
            "expected NotARepo, got: {err}"
        );
    }

    #[test]
    fn test_find_ref_tip_missing() {
        let tmp = tempfile::tempdir().unwrap();
        init_test_repo(tmp.path());

        let repo = gix::open(tmp.path()).unwrap();
        let tip = find_ref_tip(&repo, "refs/heads/nonexistent").unwrap();
        assert!(tip.is_none());
    }

    #[test]
    fn test_find_ref_tip_exists() {
        let tmp = tempfile::tempdir().unwrap();
        init_test_repo(tmp.path());

        let repo = gix::open(tmp.path()).unwrap();
        // init_test_repo creates a commit on "main"
        let tip = find_ref_tip(&repo, "refs/heads/main").unwrap();
        assert!(tip.is_some(), "expected Some(id) for refs/heads/main");
    }

    #[test]
    fn test_commit_tree_id() {
        let tmp = tempfile::tempdir().unwrap();
        init_test_repo(tmp.path());

        let repo = gix::open(tmp.path()).unwrap();
        let tip = find_ref_tip(&repo, "refs/heads/main")
            .unwrap()
            .expect("main should exist");

        let tree_id = commit_tree_id(&repo, tip.detach()).unwrap();
        // The tree should be a valid object
        let tree = repo.find_tree(tree_id);
        assert!(tree.is_ok(), "tree_id should point to a valid tree object");
    }

    #[test]
    fn test_create_commit_no_parent() {
        let tmp = tempfile::tempdir().unwrap();
        init_test_repo(tmp.path());

        let repo = gix::open(tmp.path()).unwrap();
        let empty_tree = ObjectId::empty_tree(repo.object_hash());

        let commit_id = create_commit(
            &repo,
            "refs/heads/orphan-test",
            empty_tree,
            None,
            "orphan commit",
        )
        .unwrap();

        // The ref should now exist and point to our commit
        let tip = find_ref_tip(&repo, "refs/heads/orphan-test")
            .unwrap()
            .expect("orphan-test ref should exist");
        assert_eq!(tip.detach(), commit_id);
    }

    #[test]
    fn test_create_commit_with_parent() {
        let tmp = tempfile::tempdir().unwrap();
        init_test_repo(tmp.path());

        let repo = gix::open(tmp.path()).unwrap();
        let empty_tree = ObjectId::empty_tree(repo.object_hash());

        // Create first (orphan) commit
        let first_id = create_commit(
            &repo,
            "refs/heads/chain-test",
            empty_tree,
            None,
            "first commit",
        )
        .unwrap();

        // Create second commit with first as parent
        let second_id = create_commit(
            &repo,
            "refs/heads/chain-test",
            empty_tree,
            Some(first_id),
            "second commit",
        )
        .unwrap();

        assert_ne!(first_id, second_id);

        // Ref should now point to the second commit
        let tip = find_ref_tip(&repo, "refs/heads/chain-test")
            .unwrap()
            .expect("chain-test ref should exist");
        assert_eq!(tip.detach(), second_id);
    }

    #[test]
    fn test_delete_ref() {
        let tmp = tempfile::tempdir().unwrap();
        init_test_repo(tmp.path());

        let repo = gix::open(tmp.path()).unwrap();
        let empty_tree = ObjectId::empty_tree(repo.object_hash());

        // Create a ref
        let commit_id = create_commit(
            &repo,
            "refs/heads/to-delete",
            empty_tree,
            None,
            "will be deleted",
        )
        .unwrap();

        // Confirm it exists
        assert!(find_ref_tip(&repo, "refs/heads/to-delete")
            .unwrap()
            .is_some());

        // Delete it
        delete_ref(&repo, "refs/heads/to-delete", commit_id).unwrap();

        // Now it should be gone
        let tip = find_ref_tip(&repo, "refs/heads/to-delete").unwrap();
        assert!(tip.is_none(), "ref should be deleted");
    }

    #[test]
    fn test_replace_ref_tip() {
        let tmp = tempfile::tempdir().unwrap();
        init_test_repo(tmp.path());

        let repo = gix::open(tmp.path()).unwrap();
        let empty_tree = ObjectId::empty_tree(repo.object_hash());

        let first_id = create_commit(
            &repo,
            "refs/heads/replace-test",
            empty_tree,
            None,
            "first commit",
        )
        .unwrap();

        let second_id = create_commit(
            &repo,
            "refs/heads/replace-test-next",
            empty_tree,
            None,
            "second commit",
        )
        .unwrap();

        replace_ref_tip(
            &repo,
            "refs/heads/replace-test",
            first_id,
            second_id,
            "replace tip",
        )
        .unwrap();

        let tip = find_ref_tip(&repo, "refs/heads/replace-test")
            .unwrap()
            .expect("replace-test should exist");
        assert_eq!(tip.detach(), second_id);
    }
}
