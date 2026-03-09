use super::{COMMIT_HINT_GRACE_SECONDS, SYNC_BRANCH_COMMITS_MAX, SYNC_MAX_CANDIDATES};
use anyhow::{Context, Result};
use opensession_core::Session;
use opensession_core::sanitize::{SanitizeConfig, sanitize_session};
use opensession_core::session::{
    GitMeta, build_git_storage_meta_json_with_git, is_auxiliary_session, working_directory,
};
use opensession_git_native::{NativeGitStorage, extract_git_context};
use opensession_parser_discovery::discover_sessions;
use opensession_parsers::ParserRegistry;
use opensession_runtime_config::DaemonConfig;
use std::cmp::Reverse;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
struct SessionCandidate {
    path: PathBuf,
    modified: std::time::SystemTime,
}

fn collect_recent_candidates() -> Vec<SessionCandidate> {
    let mut candidates = Vec::new();
    for location in discover_sessions() {
        for path in location.paths {
            let Ok(metadata) = std::fs::metadata(&path) else {
                continue;
            };
            let Ok(modified) = metadata.modified() else {
                continue;
            };
            candidates.push(SessionCandidate { path, modified });
        }
    }

    candidates.sort_by_key(|candidate| Reverse(candidate.modified));
    candidates.into_iter().take(SYNC_MAX_CANDIDATES).collect()
}

fn same_repo_root(left: &Path, right: &Path) -> bool {
    let left = std::fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf());
    let right = std::fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf());
    left == right
}

fn parse_session_candidate(path: &Path) -> Option<Session> {
    match ParserRegistry::default().parse_path(path) {
        Ok(Some(session)) => {
            if working_directory(&session).is_some() {
                Some(session)
            } else {
                std::fs::read_to_string(path)
                    .ok()
                    .and_then(|content| Session::from_jsonl(&content).ok())
            }
        }
        Ok(None) | Err(_) => std::fs::read_to_string(path)
            .ok()
            .and_then(|content| Session::from_jsonl(&content).ok()),
    }
}

fn normalize_commit_hint(commit_hint: Option<String>) -> Option<String> {
    commit_hint
        .and_then(|raw| {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .filter(|sha| sha != "0000000000000000000000000000000000000000")
}

fn list_branch_commits(repo_root: &Path, branch: &str, max_count: usize) -> HashSet<String> {
    let rev = format!("refs/heads/{branch}");
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("rev-list")
        .arg("--max-count")
        .arg(max_count.to_string())
        .arg(rev)
        .output();
    let Ok(output) = output else {
        return HashSet::new();
    };
    if !output.status.success() {
        return HashSet::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn commit_time_unix(repo_root: &Path, commit: &str) -> Option<i64> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("show")
        .arg("-s")
        .arg("--format=%ct")
        .arg(commit)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    raw.parse::<i64>().ok()
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
    let raw = std::fs::read_to_string(reflog_path);
    let Ok(raw) = raw else {
        return Vec::new();
    };

    let mut seen = HashSet::new();
    let mut commits = Vec::new();
    for line in raw.lines() {
        let Some((left, _)) = line.split_once('\t') else {
            continue;
        };
        let mut parts = left.split_whitespace();
        let _old = parts.next();
        let Some(new_sha) = parts.next() else {
            continue;
        };
        if new_sha.len() < 7 || !new_sha.chars().all(|ch| ch.is_ascii_hexdigit()) {
            continue;
        }
        let mut tail = left.split_whitespace().rev();
        let _tz = tail.next();
        let Some(ts_raw) = tail.next() else {
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

fn session_commit_links(
    repo_root: &Path,
    branch_commits: &HashSet<String>,
    session: &Session,
    commit_hint: Option<&str>,
) -> Vec<String> {
    let created = session.context.created_at.timestamp();
    let updated = session.context.updated_at.timestamp();
    let (start, end) = if created <= updated {
        (created, updated)
    } else {
        (updated, created)
    };
    let mut commits = commit_shas_from_reflog(repo_root, start, end)
        .into_iter()
        .filter(|sha| branch_commits.contains(sha))
        .collect::<Vec<_>>();

    if let Some(hint) = commit_hint {
        if branch_commits.contains(hint) && !commits.iter().any(|sha| sha == hint) {
            if let Some(hint_ts) = commit_time_unix(repo_root, hint) {
                let window_start = start.saturating_sub(COMMIT_HINT_GRACE_SECONDS);
                let window_end = end.saturating_add(COMMIT_HINT_GRACE_SECONDS);
                if hint_ts >= window_start && hint_ts <= window_end {
                    commits.push(hint.to_string());
                }
            }
        }
    }

    commits
}

fn load_daemon_config() -> DaemonConfig {
    let Ok(path) = opensession_paths::runtime_config_path() else {
        return DaemonConfig::default();
    };
    let Ok(content) = std::fs::read_to_string(path) else {
        return DaemonConfig::default();
    };
    toml::from_str(&content).unwrap_or_default()
}

pub(super) fn sync_branch_session_to_hidden_ledger(
    repo_root: &Path,
    branch: &str,
    commit_hint: Option<String>,
) -> Result<()> {
    let candidates = collect_recent_candidates();
    if candidates.is_empty() {
        return Ok(());
    }

    let config = load_daemon_config();
    let branch_commits = list_branch_commits(repo_root, branch, SYNC_BRANCH_COMMITS_MAX);
    if branch_commits.is_empty() {
        return Ok(());
    }
    let commit_hint = normalize_commit_hint(commit_hint);
    let mut synced_any = false;
    let mut seen_sessions = HashSet::new();

    for candidate in candidates {
        let Some(mut session) = parse_session_candidate(&candidate.path) else {
            continue;
        };
        let Some(cwd) = working_directory(&session).map(str::to_owned) else {
            continue;
        };
        let Some(session_repo) = opensession_git_native::ops::find_repo_root(Path::new(&cwd))
        else {
            continue;
        };
        if !same_repo_root(repo_root, &session_repo) {
            continue;
        }
        if is_auxiliary_session(&session) {
            continue;
        }

        if config
            .privacy
            .exclude_tools
            .iter()
            .any(|tool| tool.eq_ignore_ascii_case(&session.agent.tool))
        {
            continue;
        }
        if !seen_sessions.insert(session.session_id.clone()) {
            continue;
        }

        let commit_shas =
            session_commit_links(repo_root, &branch_commits, &session, commit_hint.as_deref());
        if commit_shas.is_empty() {
            continue;
        }

        sanitize_session(
            &mut session,
            &SanitizeConfig {
                strip_paths: config.privacy.strip_paths,
                strip_env_vars: config.privacy.strip_env_vars,
                exclude_patterns: config.privacy.exclude_patterns.clone(),
            },
        );

        let git_ctx = extract_git_context(&cwd);
        let meta = build_git_storage_meta_json_with_git(
            &session,
            Some(&GitMeta {
                remote: git_ctx.remote.clone(),
                repo_name: git_ctx.repo_name.clone(),
                branch: Some(branch.to_string()),
                head: commit_hint
                    .clone()
                    .or_else(|| commit_shas.last().cloned())
                    .or(git_ctx.commit.clone()),
                commits: commit_shas.clone(),
            }),
        );
        let hail = session
            .to_jsonl()
            .context("serialize session to canonical HAIL JSONL")?;

        NativeGitStorage.store_session_at_ref(
            repo_root,
            &opensession_git_native::branch_ledger_ref(branch),
            &session.session_id,
            hail.as_bytes(),
            &meta,
            &commit_shas,
        )?;
        synced_any = true;
    }

    let _ = synced_any;
    Ok(())
}
