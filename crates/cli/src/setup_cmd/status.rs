use super::FanoutMode;
use super::doctor::{self, DoctorLevel, DoctorSummary};
use super::planning::read_fanout_mode;
use super::shims::shim_path;
use crate::cleanup_cmd::{self, CleanupDoctorLevel};
use crate::hooks::{HookType, list_installed_hooks};
use anyhow::{Context, Result, bail};
use opensession_git_native::{branch_ledger_ref, extract_git_context, resolve_ledger_branch};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) fn run_check(repo_root: &Path) -> Result<()> {
    let colors = doctor::doctor_colors_enabled();
    let mut summary = DoctorSummary::default();
    let installed = list_installed_hooks(repo_root);
    let fanout_mode = read_fanout_mode(repo_root)?.unwrap_or(FanoutMode::HiddenRef);
    let branch = current_branch(repo_root)?;
    let ledger_branch = ledger_branch_name(repo_root);
    let ledger = branch_ledger_ref(&ledger_branch);

    println!("repo: {}", repo_root.display());
    println!("doctor checks:");

    let hooks_summary = if installed.is_empty() {
        "none".to_string()
    } else {
        installed
            .iter()
            .map(HookType::filename)
            .collect::<Vec<_>>()
            .join(", ")
    };
    let hook_level = if installed.is_empty() {
        DoctorLevel::Warn
    } else {
        DoctorLevel::Ok
    };
    doctor::print_doctor_item(colors, hook_level, "opensession hooks", &hooks_summary);
    summary.record(hook_level);

    let mut required_actions = Vec::new();
    let mut optional_actions = Vec::new();

    match shim_path("opensession") {
        Ok(path) => {
            let present = path.exists();
            let level = if present {
                DoctorLevel::Ok
            } else {
                DoctorLevel::Warn
            };
            doctor::print_doctor_item(
                colors,
                level,
                "opensession shim",
                &format!(
                    "{} ({})",
                    path.display(),
                    if present { "present" } else { "missing" }
                ),
            );
            summary.record(level);
            if !present {
                required_actions.push(
                    "run `opensession doctor --fix` to install hooks/shims for this repo"
                        .to_string(),
                );
            }
        }
        Err(err) => {
            doctor::print_doctor_item(
                colors,
                DoctorLevel::Fail,
                "opensession shim",
                &format!("unavailable ({err})"),
            );
            summary.record(DoctorLevel::Fail);
        }
    }

    match shim_path("ops") {
        Ok(path) => {
            let present = path.exists();
            let level = if present {
                DoctorLevel::Ok
            } else {
                DoctorLevel::Info
            };
            doctor::print_doctor_item(
                colors,
                level,
                "ops shim",
                &format!(
                    "{} ({})",
                    path.display(),
                    if present { "present" } else { "missing" }
                ),
            );
            summary.record(level);
            if !present {
                optional_actions.push(
                    "optional: install `ops` shim via `opensession doctor --fix` for alias UX"
                        .to_string(),
                );
            }
        }
        Err(err) => {
            doctor::print_doctor_item(
                colors,
                DoctorLevel::Fail,
                "ops shim",
                &format!("unavailable ({err})"),
            );
            summary.record(DoctorLevel::Fail);
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        doctor::print_doctor_item(
            colors,
            DoctorLevel::Ok,
            "active binary",
            &exe.display().to_string(),
        );
        summary.record(DoctorLevel::Ok);
    }

    doctor::print_doctor_item(colors, DoctorLevel::Ok, "fanout mode", fanout_mode.as_str());
    summary.record(DoctorLevel::Ok);

    let daemon_pid = daemon_pid_path()?;
    let daemon = daemon_status(&daemon_pid);
    let (daemon_level, daemon_summary, daemon_hint) = daemon_status_summary(&daemon, &daemon_pid);
    doctor::print_doctor_item(colors, daemon_level, "daemon", &daemon_summary);
    summary.record(daemon_level);
    if let Some(hint) = daemon_hint {
        doctor::print_doctor_hint(&hint);
        if daemon_level == DoctorLevel::Info {
            optional_actions.push(hint);
        } else {
            required_actions.push(hint);
        }
    }

    let readiness = review_readiness(repo_root);
    let (readiness_level, readiness_summary, readiness_hint) =
        review_readiness_summary(readiness.hidden_fanout_ready, readiness.remote_hidden_refs);
    doctor::print_doctor_item(
        colors,
        readiness_level,
        "review readiness",
        &readiness_summary,
    );
    summary.record(readiness_level);
    if let Some(hint) = readiness_hint {
        doctor::print_doctor_hint(&hint);
        if readiness_level == DoctorLevel::Info {
            optional_actions.push(hint);
        } else {
            required_actions.push(hint);
        }
    }

    let cleanup = cleanup_cmd::doctor_status(repo_root);
    let cleanup_level = match cleanup.level {
        CleanupDoctorLevel::Ok => DoctorLevel::Ok,
        CleanupDoctorLevel::Warn => DoctorLevel::Warn,
    };
    doctor::print_doctor_item(colors, cleanup_level, "cleanup", &cleanup.detail);
    summary.record(cleanup_level);
    if let Some(hint) = cleanup.hint {
        doctor::print_doctor_hint(&hint);
        required_actions.push(hint);
    }

    doctor::print_doctor_item(colors, DoctorLevel::Ok, "current branch", &branch);
    summary.record(DoctorLevel::Ok);
    if branch != ledger_branch {
        doctor::print_doctor_item(colors, DoctorLevel::Info, "ledger branch", &ledger_branch);
        summary.record(DoctorLevel::Info);
        optional_actions.push(
            "optional: branch/ledger mismatch is expected on detached HEAD; verify before sharing"
                .to_string(),
        );
    }
    doctor::print_doctor_item(colors, DoctorLevel::Ok, "expected ledger ref", &ledger);
    summary.record(DoctorLevel::Ok);

    if !required_actions.is_empty() {
        let mut dedup = HashSet::new();
        println!("next actions (recommended):");
        for suggestion in required_actions {
            if dedup.insert(suggestion.clone()) {
                println!("  - {suggestion}");
            }
        }
    }
    if !optional_actions.is_empty() {
        let mut dedup = HashSet::new();
        println!("next actions (optional):");
        for suggestion in optional_actions {
            if dedup.insert(suggestion.clone()) {
                println!("  - {suggestion}");
            }
        }
    }

    if summary.issue_categories() == 0 {
        println!("doctor summary: no blocking issues.");
    } else {
        println!(
            "doctor summary: found issues in {} categories.",
            summary.issue_categories()
        );
    }

    Ok(())
}

pub(super) fn print_daemon_status() -> Result<()> {
    let pid_path = daemon_pid_path()?;
    let status = daemon_status(&pid_path);
    let (_, summary, hint) = daemon_status_summary(&status, &pid_path);
    println!("daemon: {summary}");
    if let Some(hint) = hint {
        println!("daemon hint: {hint}");
    }
    Ok(())
}

pub(super) fn daemon_status_summary(
    status: &DaemonStatus,
    pid_path: &Path,
) -> (DoctorLevel, String, Option<String>) {
    match status {
        DaemonStatus::Running(pid) => (DoctorLevel::Ok, format!("running (pid {pid})"), None),
        DaemonStatus::NotRunning => (
            DoctorLevel::Info,
            format!("not running (pid file missing: {})", pid_path.display()),
            Some(
                "optional: start daemon for auto-capture with `opensession-daemon` (or `cargo run -p opensession-daemon -- run` in a source checkout)"
                    .to_string(),
            ),
        ),
        DaemonStatus::StalePid(pid) => (
            DoctorLevel::Info,
            format!(
                "not running (stale pid file: {} -> pid {pid})",
                pid_path.display()
            ),
            Some(
                "optional: restart daemon for auto-capture with `opensession-daemon` (or `cargo run -p opensession-daemon -- run` in a source checkout)"
                    .to_string(),
            ),
        ),
        DaemonStatus::Unreadable(err) => (
            DoctorLevel::Fail,
            format!("status unavailable ({err})"),
            None,
        ),
    }
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
pub(super) enum DaemonStatus {
    Running(u32),
    NotRunning,
    StalePid(u32),
    Unreadable(String),
}

pub(super) fn daemon_status(pid_path: &Path) -> DaemonStatus {
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
    // SAFETY: kill(pid, 0) does not deliver a signal. It is the standard Unix probe for
    // process existence and permissions, and we pass the parsed PID value directly to libc.
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

pub(super) fn ledger_branch_name(repo_root: &Path) -> String {
    let cwd = repo_root.to_string_lossy().to_string();
    let git_ctx = extract_git_context(&cwd);
    resolve_ledger_branch(git_ctx.branch.as_deref(), git_ctx.commit.as_deref())
}

pub(super) fn current_branch(repo_root: &Path) -> Result<String> {
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

#[derive(Debug, Clone, Copy)]
struct ReviewReadiness {
    hidden_fanout_ready: bool,
    remote_hidden_refs: bool,
}

fn review_readiness(repo_root: &Path) -> ReviewReadiness {
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

    ReviewReadiness {
        hidden_fanout_ready,
        remote_hidden_refs,
    }
}

pub(super) fn review_readiness_summary(
    hidden_fanout_ready: bool,
    remote_hidden_refs: bool,
) -> (DoctorLevel, String, Option<String>) {
    let summary = format!(
        "hidden-fanout={} hidden-refs={}",
        if hidden_fanout_ready { "ok" } else { "missing" },
        if remote_hidden_refs {
            "present"
        } else {
            "none-fetched"
        }
    );

    if hidden_fanout_ready && remote_hidden_refs {
        return (DoctorLevel::Ok, summary, None);
    }

    if !hidden_fanout_ready {
        return (
            DoctorLevel::Warn,
            summary,
            Some("run `opensession doctor --fix` to install the pre-push fanout hook".to_string()),
        );
    }

    (
        DoctorLevel::Info,
        summary,
        Some(
            "optional: fetch hidden refs before local review: `git fetch origin 'refs/opensession/branches/*:refs/remotes/origin/opensession/branches/*'`".to_string(),
        ),
    )
}

pub(super) fn print_review_readiness(repo_root: &Path) -> Result<()> {
    let readiness = review_readiness(repo_root);
    let (_, summary, _) =
        review_readiness_summary(readiness.hidden_fanout_ready, readiness.remote_hidden_refs);
    println!("review readiness: {summary}");
    Ok(())
}
