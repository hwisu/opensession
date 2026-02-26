use anyhow::{bail, Context, Result};
use clap::Args;
use opensession_core::sanitize::{sanitize_session, SanitizeConfig};
use opensession_core::session::{build_git_storage_meta_json_with_git, working_directory, GitMeta};
use opensession_core::Session;
use opensession_daemon::hooks::{install_hooks, list_installed_hooks, HookType};
use opensession_git_native::{
    branch_ledger_ref, extract_git_context, resolve_ledger_branch, NativeGitStorage,
};
use opensession_parsers::{discover::discover_sessions, parse_with_default_parsers};
use opensession_runtime_config::{DaemonConfig, CONFIG_FILE_NAME};
use std::cmp::Reverse;
use std::collections::HashSet;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

const FANOUT_MODE_GIT_CONFIG_KEY: &str = "opensession.fanout-mode";
const SYNC_MAX_CANDIDATES: usize = 128;
const SYNC_BRANCH_COMMITS_MAX: usize = 4096;
const COMMIT_HINT_GRACE_SECONDS: i64 = 6 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FanoutMode {
    HiddenRef,
    GitNotes,
}

impl FanoutMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::HiddenRef => "hidden_ref",
            Self::GitNotes => "git_notes",
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "hidden_ref" | "hidden" | "1" => Some(Self::HiddenRef),
            "git_notes" | "notes" | "note" | "2" => Some(Self::GitNotes),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Args)]
pub struct SetupArgs {
    /// Show setup status only.
    #[arg(long)]
    pub check: bool,
    /// Print hidden ledger ref for a branch name (internal use for hooks).
    #[arg(long, hide = true)]
    pub print_ledger_ref: Option<String>,
    /// Print configured fanout mode (`hidden_ref` | `git_notes`) for this repo.
    #[arg(long, hide = true)]
    pub print_fanout_mode: bool,
    /// Ingest the latest local session for this branch into the hidden ledger (internal use).
    #[arg(long, hide = true)]
    pub sync_branch_session: Option<String>,
    /// Commit SHA hint used to improve commit mapping when syncing branch sessions (internal use).
    #[arg(long, hide = true)]
    pub sync_branch_commit: Option<String>,
}

pub fn run(args: SetupArgs) -> Result<()> {
    if let Some(branch) = args.print_ledger_ref {
        println!("{}", branch_ledger_ref(&branch));
        return Ok(());
    }

    if args.sync_branch_session.is_none() && args.sync_branch_commit.is_some() {
        bail!("--sync-branch-commit requires --sync-branch-session");
    }

    if let Some(branch) = args.sync_branch_session {
        let cwd = std::env::current_dir().context("read current directory")?;
        let repo_root = opensession_git_native::ops::find_repo_root(&cwd)
            .ok_or_else(|| anyhow::anyhow!("current directory is not inside a git repository"))?;
        sync_branch_session_to_hidden_ledger(&repo_root, &branch, args.sync_branch_commit)?;
        return Ok(());
    }

    if args.print_fanout_mode {
        let cwd = std::env::current_dir().context("read current directory")?;
        let repo_root = opensession_git_native::ops::find_repo_root(&cwd)
            .ok_or_else(|| anyhow::anyhow!("current directory is not inside a git repository"))?;
        let mode = read_fanout_mode(&repo_root)?.unwrap_or(FanoutMode::HiddenRef);
        println!("{}", mode.as_str());
        return Ok(());
    }

    let cwd = std::env::current_dir().context("read current directory")?;
    let repo_root = opensession_git_native::ops::find_repo_root(&cwd)
        .ok_or_else(|| anyhow::anyhow!("current directory is not inside a git repository"))?;

    if args.check {
        return run_check(&repo_root);
    }
    run_install(&repo_root)
}

fn run_install(repo_root: &PathBuf) -> Result<()> {
    let fanout_mode = ensure_fanout_mode(repo_root)?;
    let shim_paths = install_cli_shims()?;
    let installed = install_hooks(repo_root, HookType::all())?;
    if installed.is_empty() {
        println!("No hooks installed.");
        return Ok(());
    }

    println!("Installed hooks in {}:", repo_root.display());
    for hook in installed {
        println!("  - {}", hook.filename());
    }
    println!("opensession shim: {}", shim_paths.opensession.display());
    println!("ops shim: {}", shim_paths.ops.display());
    println!("fanout mode: {}", fanout_mode.as_str());
    print_daemon_status()?;
    print_review_readiness(repo_root)?;

    if let Ok(branch) = current_branch(repo_root) {
        let ledger_branch = ledger_branch_name(repo_root);
        let ledger = branch_ledger_ref(&ledger_branch);
        println!("current branch: {branch}");
        if branch != ledger_branch {
            println!("ledger branch: {ledger_branch}");
        }
        println!("ledger ref: {ledger}");
    }
    Ok(())
}

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
    match parse_with_default_parsers(path) {
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
    let home = match std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
        Ok(home) => home,
        Err(_) => return DaemonConfig::default(),
    };
    let path = PathBuf::from(home)
        .join(".config")
        .join("opensession")
        .join(CONFIG_FILE_NAME);
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(_) => return DaemonConfig::default(),
    };
    toml::from_str(&content).unwrap_or_default()
}

fn sync_branch_session_to_hidden_ledger(
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
            &branch_ledger_ref(branch),
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

fn run_check(repo_root: &PathBuf) -> Result<()> {
    let installed = list_installed_hooks(repo_root);
    println!("repo: {}", repo_root.display());
    if installed.is_empty() {
        println!("opensession hooks: none");
    } else {
        println!("opensession hooks:");
        for hook in installed {
            println!("  - {}", hook.filename());
        }
    }

    match shim_path("opensession") {
        Ok(path) => {
            let status = if path.exists() { "present" } else { "missing" };
            println!("opensession shim: {} ({status})", path.display());
        }
        Err(err) => {
            println!("opensession shim: unavailable ({err})");
        }
    }
    match shim_path("ops") {
        Ok(path) => {
            let status = if path.exists() { "present" } else { "missing" };
            println!("ops shim: {} ({status})", path.display());
        }
        Err(err) => {
            println!("ops shim: unavailable ({err})");
        }
    }
    let fanout_mode = read_fanout_mode(repo_root)?.unwrap_or(FanoutMode::HiddenRef);
    println!("fanout mode: {}", fanout_mode.as_str());
    print_daemon_status()?;
    print_review_readiness(repo_root)?;

    let branch = current_branch(repo_root)?;
    let ledger_branch = ledger_branch_name(repo_root);
    let ledger = branch_ledger_ref(&ledger_branch);
    println!("current branch: {branch}");
    if branch != ledger_branch {
        println!("ledger branch: {ledger_branch}");
    }
    println!("expected ledger ref: {ledger}");
    Ok(())
}

fn ensure_fanout_mode(repo_root: &std::path::Path) -> Result<FanoutMode> {
    if let Some(mode) = read_fanout_mode(repo_root)? {
        return Ok(mode);
    }

    let mode = prompt_fanout_mode()?;
    write_fanout_mode(repo_root, mode)?;
    println!("fanout mode initialized: {}", mode.as_str());
    Ok(mode)
}

fn read_fanout_mode(repo_root: &std::path::Path) -> Result<Option<FanoutMode>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("config")
        .arg("--local")
        .arg("--get")
        .arg(FANOUT_MODE_GIT_CONFIG_KEY)
        .output()
        .context("read git fanout mode")?;

    if !output.status.success() {
        return Ok(None);
    }

    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if raw.is_empty() {
        return Ok(None);
    }

    Ok(Some(
        FanoutMode::parse(&raw).unwrap_or(FanoutMode::HiddenRef),
    ))
}

fn write_fanout_mode(repo_root: &std::path::Path, mode: FanoutMode) -> Result<()> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("config")
        .arg("--local")
        .arg(FANOUT_MODE_GIT_CONFIG_KEY)
        .arg(mode.as_str())
        .output()
        .context("write git fanout mode")?;
    if !output.status.success() {
        bail!(
            "failed to store fanout mode in git config: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn prompt_fanout_mode() -> Result<FanoutMode> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Ok(FanoutMode::HiddenRef);
    }

    println!("Choose OpenSession fanout mode for this repository:");
    println!("  1) hidden refs (default)");
    println!("  2) git notes");
    print!("select [1/2]: ");
    io::stdout().flush().context("flush stdout")?;

    let mut line = String::new();
    io::stdin().read_line(&mut line).context("read selection")?;
    Ok(parse_fanout_choice(&line).unwrap_or(FanoutMode::HiddenRef))
}

fn parse_fanout_choice(input: &str) -> Option<FanoutMode> {
    FanoutMode::parse(input)
}

fn print_daemon_status() -> Result<()> {
    let pid_path = daemon_pid_path()?;
    let status = daemon_status(&pid_path);
    match status {
        DaemonStatus::Running(pid) => {
            println!("daemon: running (pid {pid})");
        }
        DaemonStatus::NotRunning => {
            println!(
                "daemon: not running (pid file missing: {})",
                pid_path.display()
            );
            println!(
                "daemon hint: run `opensession-daemon` (or `cargo run -p opensession-daemon -- run` in a source checkout)"
            );
        }
        DaemonStatus::StalePid(pid) => {
            println!(
                "daemon: not running (stale pid file: {} -> pid {pid})",
                pid_path.display()
            );
            println!(
                "daemon hint: run `opensession-daemon` (or `cargo run -p opensession-daemon -- run` in a source checkout)"
            );
        }
        DaemonStatus::Unreadable(err) => {
            println!("daemon: status unavailable ({err})");
        }
    }
    Ok(())
}

fn daemon_pid_path() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .context("HOME/USERPROFILE is not set; cannot resolve daemon pid path")?;
    Ok(PathBuf::from(home)
        .join(".config")
        .join("opensession")
        .join("daemon.pid"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DaemonStatus {
    Running(u32),
    NotRunning,
    StalePid(u32),
    Unreadable(String),
}

fn daemon_status(pid_path: &std::path::Path) -> DaemonStatus {
    if !pid_path.exists() {
        return DaemonStatus::NotRunning;
    }

    let pid_raw = match std::fs::read_to_string(pid_path) {
        Ok(raw) => raw,
        Err(err) => return DaemonStatus::Unreadable(format!("read {}: {err}", pid_path.display())),
    };
    let pid = match pid_raw.trim().parse::<u32>() {
        Ok(pid) if pid > 0 => pid,
        Ok(_) | Err(_) => {
            return DaemonStatus::Unreadable(format!(
                "invalid pid content in {}",
                pid_path.display()
            ));
        }
    };

    if process_running(pid) {
        DaemonStatus::Running(pid)
    } else {
        DaemonStatus::StalePid(pid)
    }
}

#[cfg(unix)]
fn process_running(pid: u32) -> bool {
    // kill(pid, 0) does not send a signal; it only checks process existence/permission.
    let rc = unsafe { libc::kill(pid as i32, 0) };
    if rc == 0 {
        return true;
    }
    matches!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::EPERM)
    )
}

#[cfg(not(unix))]
fn process_running(_pid: u32) -> bool {
    false
}

fn ledger_branch_name(repo_root: &std::path::Path) -> String {
    let cwd = repo_root.to_string_lossy().to_string();
    let git_ctx = extract_git_context(&cwd);
    resolve_ledger_branch(git_ctx.branch.as_deref(), git_ctx.commit.as_deref())
}

fn current_branch(repo_root: &PathBuf) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("rev-parse")
        .arg("--abbrev-ref")
        .arg("HEAD")
        .output()
        .context("resolve current git branch")?;
    if !output.status.success() {
        bail!(
            "failed to read current branch: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn shim_path(name: &str) -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .context("HOME environment variable is not set; cannot resolve shim path")?;
    Ok(PathBuf::from(home)
        .join(".local")
        .join("share")
        .join("opensession")
        .join("bin")
        .join(name))
}

#[derive(Debug, Clone)]
struct ShimPaths {
    opensession: PathBuf,
    ops: PathBuf,
}

fn install_cli_shims() -> Result<ShimPaths> {
    let exe = std::env::current_exe().context("resolve current opensession executable path")?;
    let opensession = install_cli_shim("opensession", &exe)?;
    let ops = install_cli_shim("ops", &exe)?;
    Ok(ShimPaths { opensession, ops })
}

fn install_cli_shim(name: &str, exe: &std::path::Path) -> Result<PathBuf> {
    let shim = shim_path(name)?;
    if let Some(parent) = shim.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create shim directory {}", parent.display()))?;
    }

    let existing_matches = if shim.exists() {
        std::fs::canonicalize(&shim).ok() == std::fs::canonicalize(exe).ok()
    } else {
        false
    };
    if existing_matches {
        return Ok(shim);
    }

    if shim.exists() {
        std::fs::remove_file(&shim)
            .with_context(|| format!("remove existing shim {}", shim.display()))?;
    }

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(exe, &shim)
            .with_context(|| format!("create shim symlink {}", shim.display()))?;
    }

    #[cfg(not(unix))]
    {
        std::fs::copy(exe, &shim)
            .with_context(|| format!("create shim copy {}", shim.display()))?;
    }

    Ok(shim)
}

fn print_review_readiness(repo_root: &PathBuf) -> Result<()> {
    let hidden_fanout_ready = list_installed_hooks(repo_root).contains(&HookType::PrePush);

    let remote_hidden_refs = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("for-each-ref")
        .arg("--count=1")
        .arg("--format=%(refname)")
        .arg("refs/remotes/*/opensession/branches")
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| !String::from_utf8_lossy(&out.stdout).trim().is_empty())
        .unwrap_or(false);

    println!(
        "review readiness: hidden-fanout={} hidden-refs={}",
        if hidden_fanout_ready { "ok" } else { "missing" },
        if remote_hidden_refs {
            "present"
        } else {
            "none-fetched"
        }
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn print_ledger_ref_matches_helper() {
        let got = branch_ledger_ref("feature/abc");
        assert_eq!(got, "refs/opensession/branches/ZmVhdHVyZS9hYmM");
    }

    #[test]
    fn daemon_status_reports_not_running_when_pid_file_missing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let status = daemon_status(&tmp.path().join("daemon.pid"));
        assert_eq!(status, DaemonStatus::NotRunning);
    }

    #[test]
    fn daemon_status_reports_unreadable_for_invalid_pid_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let pid_path = tmp.path().join("daemon.pid");
        fs::write(&pid_path, "not-a-pid").expect("write pid");
        let status = daemon_status(&pid_path);
        assert!(matches!(status, DaemonStatus::Unreadable(_)));
    }

    #[test]
    fn parse_fanout_choice_accepts_hidden_ref_aliases() {
        assert_eq!(parse_fanout_choice("1"), Some(FanoutMode::HiddenRef));
        assert_eq!(
            parse_fanout_choice("hidden_ref"),
            Some(FanoutMode::HiddenRef)
        );
        assert_eq!(parse_fanout_choice("hidden"), Some(FanoutMode::HiddenRef));
    }

    #[test]
    fn parse_fanout_choice_accepts_git_notes_aliases() {
        assert_eq!(parse_fanout_choice("2"), Some(FanoutMode::GitNotes));
        assert_eq!(parse_fanout_choice("git_notes"), Some(FanoutMode::GitNotes));
        assert_eq!(parse_fanout_choice("notes"), Some(FanoutMode::GitNotes));
    }

    #[test]
    fn parse_fanout_choice_rejects_unknown_values() {
        assert_eq!(parse_fanout_choice(""), None);
        assert_eq!(parse_fanout_choice("unknown"), None);
    }

    #[test]
    fn write_and_read_fanout_mode_roundtrip() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).expect("create repo dir");
        let init = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .arg("init")
            .output()
            .expect("git init");
        assert!(
            init.status.success(),
            "{}",
            String::from_utf8_lossy(&init.stderr)
        );

        assert_eq!(read_fanout_mode(&repo).expect("read"), None);

        write_fanout_mode(&repo, FanoutMode::GitNotes).expect("write");
        assert_eq!(
            read_fanout_mode(&repo).expect("read"),
            Some(FanoutMode::GitNotes)
        );

        write_fanout_mode(&repo, FanoutMode::HiddenRef).expect("write");
        assert_eq!(
            read_fanout_mode(&repo).expect("read"),
            Some(FanoutMode::HiddenRef)
        );
    }
}
