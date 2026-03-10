use anyhow::Result;
use opensession_core::Session;
use opensession_git_native::PruneStats;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{debug, info, warn};

use crate::repo_registry::RepoRegistry;

pub(super) fn run_git_retention_once(registry: &RepoRegistry, keep_days: u32) -> Result<()> {
    let repo_roots = registry.repo_roots();
    if repo_roots.is_empty() {
        debug!("Git retention: no tracked repositories");
        return Ok(());
    }

    let storage = opensession_git_native::NativeGitStorage;
    for repo_root in repo_roots {
        let refs = list_branch_ledger_refs(&repo_root);
        if refs.is_empty() {
            continue;
        }
        for ref_name in refs {
            let prune_result = storage.prune_by_age_at_ref(&repo_root, &ref_name, keep_days);
            match prune_result {
                Ok(PruneStats {
                    scanned_sessions,
                    expired_sessions,
                    rewritten,
                }) => {
                    if rewritten {
                        info!(
                            repo = %repo_root.display(),
                            ref_name,
                            keep_days,
                            scanned_sessions,
                            expired_sessions,
                            "Git retention: pruned expired sessions"
                        );
                    } else {
                        debug!(
                            repo = %repo_root.display(),
                            ref_name,
                            keep_days,
                            scanned_sessions,
                            "Git retention: no expired sessions"
                        );
                    }
                }
                Err(error) => {
                    warn!(
                        repo = %repo_root.display(),
                        ref_name,
                        keep_days,
                        "Git retention failed: {error}"
                    );
                }
            }
        }
    }

    Ok(())
}

fn list_branch_ledger_refs(repo_root: &Path) -> Vec<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("for-each-ref")
        .arg("--format=%(refname)")
        .arg(opensession_git_native::BRANCH_LEDGER_REF_PREFIX)
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn commit_shas_from_reflog(repo_root: &Path, start_ts: i64, end_ts: i64) -> Vec<String> {
    let git_dir_output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("rev-parse")
        .arg("--git-dir")
        .output();
    let Ok(git_dir_output) = git_dir_output else {
        return Vec::new();
    };
    if !git_dir_output.status.success() {
        return Vec::new();
    }
    let git_dir = String::from_utf8_lossy(&git_dir_output.stdout)
        .trim()
        .to_string();
    if git_dir.is_empty() {
        return Vec::new();
    }
    let git_dir_path = if Path::new(&git_dir).is_absolute() {
        PathBuf::from(git_dir)
    } else {
        repo_root.join(git_dir)
    };
    let reflog_path = git_dir_path.join("logs").join("HEAD");
    let raw = std::fs::read_to_string(&reflog_path);
    let Ok(raw) = raw else {
        return Vec::new();
    };

    let mut seen = HashSet::new();
    let mut commits = Vec::new();
    for line in raw.lines() {
        let Some((left, _msg)) = line.split_once('\t') else {
            continue;
        };
        let mut pieces = left.split_whitespace();
        let _old = pieces.next();
        let new = pieces.next();
        let Some(new_sha) = new else {
            continue;
        };
        if new_sha.len() < 7 || !new_sha.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        let mut tail = left.split_whitespace().rev();
        let _tz = tail.next();
        let ts_raw = tail.next();
        let Some(ts_raw) = ts_raw else {
            continue;
        };
        let Ok(ts) = ts_raw.parse::<i64>() else {
            continue;
        };
        if ts < start_ts || ts > end_ts {
            continue;
        }
        if seen.insert(new_sha.to_string()) {
            commits.push(new_sha.to_string());
        }
    }
    commits
}

pub(super) fn collect_commit_shas_for_session(repo_root: &Path, session: &Session) -> Vec<String> {
    let created = session.context.created_at.timestamp();
    let updated = session.context.updated_at.timestamp();
    let start = created.min(updated);
    let end = created.max(updated);

    let mut commits = commit_shas_from_reflog(repo_root, start, end);
    if commits.is_empty() {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo_root)
            .arg("rev-parse")
            .arg("HEAD")
            .output();
        if let Ok(output) = output {
            if output.status.success() {
                let head = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !head.is_empty() {
                    commits.push(head);
                }
            }
        }
    }
    commits
}
