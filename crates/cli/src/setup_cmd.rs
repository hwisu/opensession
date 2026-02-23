use anyhow::{bail, Context, Result};
use clap::Args;
use opensession_daemon::hooks::{install_hooks, list_installed_hooks, HookType};
use opensession_git_native::branch_ledger_ref;
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
    let installed = install_hooks(repo_root, HookType::all())?;
    if installed.is_empty() {
        println!("No hooks installed.");
        return Ok(());
    }

    println!("Installed hooks in {}:", repo_root.display());
    for hook in installed {
        println!("  - {}", hook.filename());
    }

    if let Ok(branch) = current_branch(repo_root) {
        let ledger = branch_ledger_ref(&branch);
        println!("current branch: {branch}");
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

    let branch = current_branch(repo_root)?;
    if branch.trim().is_empty() {
        bail!("could not resolve current branch");
    }
    let ledger = branch_ledger_ref(&branch);
    println!("current branch: {branch}");
    println!("expected ledger ref: {ledger}");
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_ledger_ref_matches_helper() {
        let got = branch_ledger_ref("feature/abc");
        assert_eq!(got, "refs/opensession/branches/ZmVhdHVyZS9hYmM");
    }
}
