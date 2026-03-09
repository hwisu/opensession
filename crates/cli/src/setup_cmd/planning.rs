use super::doctor;
use super::shims::{ShimInstallPlan, ShimPaths, shim_action_label};
use super::{FANOUT_MODE_GIT_CONFIG_KEY, FanoutMode, OpenTargetInstallPlan, SetupProfile};
use crate::hooks::{HookInstallAction, HookInstallPlan, HookInstallReport};
use crate::open_target::{OpenTarget, read_repo_open_target, write_repo_open_target};
use anyhow::{Context, Result, bail};
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::process::Command;

pub(super) fn print_setup_plan(
    repo_root: &Path,
    fanout_plan: super::FanoutInstallPlan,
    open_target_plan: OpenTargetInstallPlan,
    profile: SetupProfile,
    hook_plans: &[HookInstallPlan],
    shim_plans: &[ShimInstallPlan],
    yes: bool,
) {
    println!("repo: {}", repo_root.display());
    println!("setup plan:");
    println!("  - profile: {}", profile.as_str());
    println!("  - fanout mode: {}", fanout_plan.summary());
    println!("  - open target: {}", open_target_plan.summary(profile));
    for plan in hook_plans {
        println!(
            "  - hook {}: {} ({})",
            plan.hook_type.filename(),
            hook_action_summary(plan.action),
            plan.hook_path.display()
        );
        if let Some(backup_path) = &plan.backup_path {
            println!("    original hook saved as: {}", backup_path.display());
            println!(
                "    restore: mv '{}' '{}'",
                backup_path.display(),
                plan.hook_path.display()
            );
        }
    }
    for plan in shim_plans {
        println!(
            "  - shim {}: {} ({})",
            plan.name,
            shim_action_label(plan.action),
            plan.path.display()
        );
    }
    if yes {
        println!("  - confirmation: skipped (--yes)");
    } else {
        println!("  - confirmation: required (use --yes to skip)");
    }
}

pub(super) fn print_applied_setup(
    repo_root: &Path,
    fanout_mode: FanoutMode,
    open_target: OpenTarget,
    hook_reports: &[HookInstallReport],
    shim_plans: &[ShimInstallPlan],
    shim_paths: &ShimPaths,
) {
    println!("Applied setup in {}:", repo_root.display());
    println!("  - fanout mode: {}", fanout_mode.as_str());
    println!("  - open target: {}", open_target.as_str());
    for report in hook_reports {
        println!(
            "  - hook {}: {} ({})",
            report.hook_type.filename(),
            hook_action_summary(report.action),
            report.hook_path.display()
        );
        if report.backup_created {
            if let Some(backup_path) = &report.backup_path {
                println!("    original hook saved as: {}", backup_path.display());
                println!(
                    "    restore: mv '{}' '{}'",
                    backup_path.display(),
                    report.hook_path.display()
                );
            }
        }
    }

    for plan in shim_plans {
        let path = match plan.name {
            "opensession" => &shim_paths.opensession,
            _ => &shim_paths.ops,
        };
        println!(
            "  - shim {}: {} ({})",
            plan.name,
            shim_action_label(plan.action),
            path.display()
        );
    }
}

fn hook_action_summary(action: HookInstallAction) -> &'static str {
    match action {
        HookInstallAction::InstallNew => "install",
        HookInstallAction::ReplaceManaged => "refresh",
        HookInstallAction::BackupAndReplace => "preserve-original+replace",
    }
}

pub(super) fn ensure_fanout_mode(
    repo_root: &Path,
    requested: Option<FanoutMode>,
    interactive: bool,
) -> Result<FanoutMode> {
    if let Some(mode) = requested {
        write_fanout_mode(repo_root, mode)?;
        println!("fanout mode set: {}", mode.as_str());
        return Ok(mode);
    }
    if let Some(mode) = read_fanout_mode(repo_root)? {
        return Ok(mode);
    }

    if !interactive {
        bail!(
            "fanout mode is not configured for this repository.\nnext: run `{}`",
            doctor::suggested_doctor_command(
                FanoutMode::HiddenRef,
                OpenTarget::Web,
                SetupProfile::Local
            )
        );
    }

    let mode = prompt_fanout_mode()?;
    write_fanout_mode(repo_root, mode)?;
    println!("fanout mode initialized: {}", mode.as_str());
    Ok(mode)
}

pub(super) fn ensure_open_target(
    repo_root: &Path,
    requested: Option<OpenTarget>,
    interactive: bool,
    profile: SetupProfile,
) -> Result<OpenTarget> {
    if let Some(target) = requested {
        write_repo_open_target(repo_root, target)?;
        println!("open target set: {}", target.as_str());
        return Ok(target);
    }
    if let Some(target) = read_repo_open_target(repo_root)? {
        return Ok(target);
    }

    let default_target = default_open_target_for_profile(profile);
    let target = if interactive {
        let selected = prompt_open_target(default_target, profile)?;
        println!("open target initialized: {}", selected.as_str());
        selected
    } else {
        println!(
            "open target defaulted: {} (non-interactive)",
            default_target.as_str()
        );
        default_target
    };
    write_repo_open_target(repo_root, target)?;
    Ok(target)
}

pub(super) fn default_open_target_for_profile(profile: SetupProfile) -> OpenTarget {
    match profile {
        SetupProfile::Local => OpenTarget::Web,
        SetupProfile::App => OpenTarget::App,
    }
}

pub(super) fn read_fanout_mode(repo_root: &Path) -> Result<Option<FanoutMode>> {
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

pub(super) fn write_fanout_mode(repo_root: &Path, mode: FanoutMode) -> Result<()> {
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
        bail!(
            "fanout mode prompt requires an interactive terminal.\nnext: run `{}`",
            doctor::suggested_doctor_command(
                FanoutMode::HiddenRef,
                OpenTarget::Web,
                SetupProfile::Local
            )
        );
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

pub(super) fn parse_fanout_choice(input: &str) -> Option<FanoutMode> {
    FanoutMode::parse(input)
}

fn prompt_open_target(default_target: OpenTarget, profile: SetupProfile) -> Result<OpenTarget> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!(
            "open target prompt requires an interactive terminal.\nnext: run `{}`",
            doctor::suggested_doctor_command(FanoutMode::HiddenRef, default_target, profile)
        );
    }

    println!("Choose OpenSession review opener for this repository:");
    println!("  1) app");
    println!("  2) web");
    println!("  default: {}", default_target.as_str());
    print!("select [1/2]: ");
    io::stdout().flush().context("flush stdout")?;

    let mut line = String::new();
    io::stdin().read_line(&mut line).context("read selection")?;
    Ok(parse_open_target_choice(&line).unwrap_or(default_target))
}

pub(super) fn parse_open_target_choice(input: &str) -> Option<OpenTarget> {
    OpenTarget::parse(input)
}
