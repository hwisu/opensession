use crate::setup_cmd::{self, SetupArgs};
use anyhow::{anyhow, bail, Context, Result};
use clap::{Args, ValueEnum};
use std::path::{Path, PathBuf};
use std::process::Command;

const FANOUT_MODE_GIT_CONFIG_KEY: &str = "opensession.fanout-mode";

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DoctorFanoutMode {
    #[value(name = "hidden_ref", alias = "hidden-ref", alias = "hidden")]
    HiddenRef,
    #[value(name = "git_notes", alias = "git-notes", alias = "notes")]
    GitNotes,
}

impl DoctorFanoutMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::HiddenRef => "hidden_ref",
            Self::GitNotes => "git_notes",
        }
    }
}

#[derive(Debug, Clone, Args)]
pub struct DoctorArgs {
    /// Apply recommended setup fixes (hooks/shims/fanout defaults).
    #[arg(long)]
    pub fix: bool,
    /// Set fanout mode before applying fixes.
    #[arg(long, value_enum)]
    pub fanout_mode: Option<DoctorFanoutMode>,
}

pub fn run(args: DoctorArgs) -> Result<()> {
    validate_args(&args)?;

    println!(
        "OpenSession doctor ({})",
        if args.fix { "apply mode" } else { "check mode" }
    );

    if let Some(mode) = args.fanout_mode {
        let repo_root = current_repo_root()?;
        write_fanout_mode(&repo_root, mode)?;
        println!(
            "fanout mode set: {} ({})",
            mode.as_str(),
            repo_root.display()
        );
    }

    setup_cmd::run(SetupArgs {
        check: !args.fix,
        print_ledger_ref: None,
        print_fanout_mode: false,
        sync_branch_session: None,
        sync_branch_commit: None,
    })?;

    if !args.fix {
        println!("hint: run `opensession doctor --fix` to apply recommended setup values.");
    }

    Ok(())
}

fn validate_args(args: &DoctorArgs) -> Result<()> {
    if args.fanout_mode.is_some() && !args.fix {
        bail!("`--fanout-mode` requires `--fix`");
    }
    Ok(())
}

fn current_repo_root() -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("read current directory")?;
    opensession_git_native::ops::find_repo_root(&cwd)
        .ok_or_else(|| anyhow!("current directory is not inside a git repository"))
}

fn write_fanout_mode(repo_root: &Path, mode: DoctorFanoutMode) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_args_rejects_fanout_without_fix() {
        let args = DoctorArgs {
            fix: false,
            fanout_mode: Some(DoctorFanoutMode::HiddenRef),
        };
        let err = validate_args(&args).expect_err("validate");
        assert!(err.to_string().contains("requires `--fix`"));
    }

    #[test]
    fn validate_args_accepts_fix_with_fanout() {
        let args = DoctorArgs {
            fix: true,
            fanout_mode: Some(DoctorFanoutMode::GitNotes),
        };
        validate_args(&args).expect("validate");
    }

    #[test]
    fn write_fanout_mode_persists_local_git_config() {
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

        write_fanout_mode(&repo, DoctorFanoutMode::GitNotes).expect("write fanout mode");
        let get = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .arg("config")
            .arg("--local")
            .arg("--get")
            .arg(FANOUT_MODE_GIT_CONFIG_KEY)
            .output()
            .expect("git config get");
        assert!(
            get.status.success(),
            "{}",
            String::from_utf8_lossy(&get.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&get.stdout).trim(), "git_notes");
    }
}
