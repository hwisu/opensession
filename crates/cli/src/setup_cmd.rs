use anyhow::{bail, Context, Result};
use clap::Args;
use opensession_daemon::hooks::{install_hooks, list_installed_hooks, HookType};
use opensession_git_native::{branch_ledger_ref, extract_git_context, resolve_ledger_branch};
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Args)]
pub struct SetupArgs {
    /// Show setup status only.
    #[arg(long)]
    pub check: bool,
    /// Print hidden ledger ref for a branch name (internal use for hooks).
    #[arg(long, hide = true)]
    pub print_ledger_ref: Option<String>,
}

pub fn run(args: SetupArgs) -> Result<()> {
    if let Some(branch) = args.print_ledger_ref {
        println!("{}", branch_ledger_ref(&branch));
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
    let shim_path = install_cli_shim()?;
    let installed = install_hooks(repo_root, HookType::all())?;
    if installed.is_empty() {
        println!("No hooks installed.");
        return Ok(());
    }

    println!("Installed hooks in {}:", repo_root.display());
    for hook in installed {
        println!("  - {}", hook.filename());
    }
    println!("opensession shim: {}", shim_path.display());
    print_daemon_status()?;

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

    match shim_path() {
        Ok(path) => {
            let status = if path.exists() { "present" } else { "missing" };
            println!("opensession shim: {} ({status})", path.display());
        }
        Err(err) => {
            println!("opensession shim: unavailable ({err})");
        }
    }
    print_daemon_status()?;

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

fn shim_path() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .context("HOME environment variable is not set; cannot resolve shim path")?;
    Ok(PathBuf::from(home)
        .join(".local")
        .join("share")
        .join("opensession")
        .join("bin")
        .join("opensession"))
}

fn install_cli_shim() -> Result<PathBuf> {
    let shim = shim_path()?;
    let exe = std::env::current_exe().context("resolve current opensession executable path")?;
    if let Some(parent) = shim.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create shim directory {}", parent.display()))?;
    }

    let existing_matches = if shim.exists() {
        std::fs::canonicalize(&shim).ok() == std::fs::canonicalize(&exe).ok()
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
        std::os::unix::fs::symlink(&exe, &shim)
            .with_context(|| format!("create shim symlink {}", shim.display()))?;
    }

    #[cfg(not(unix))]
    {
        std::fs::copy(&exe, &shim)
            .with_context(|| format!("create shim copy {}", shim.display()))?;
    }

    Ok(shim)
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
}
