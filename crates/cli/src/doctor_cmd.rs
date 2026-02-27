use crate::setup_cmd::{self, SetupArgs, SetupFanoutMode};
use anyhow::{bail, Result};
use clap::{Args, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DoctorFanoutMode {
    #[value(name = "hidden_ref", alias = "hidden-ref", alias = "hidden")]
    HiddenRef,
    #[value(name = "git_notes", alias = "git-notes", alias = "notes")]
    GitNotes,
}

impl DoctorFanoutMode {
    fn as_setup_mode(self) -> SetupFanoutMode {
        match self {
            Self::HiddenRef => SetupFanoutMode::HiddenRef,
            Self::GitNotes => SetupFanoutMode::GitNotes,
        }
    }
}

#[derive(Debug, Clone, Args)]
pub struct DoctorArgs {
    /// Apply recommended setup fixes (hooks/shims/fanout defaults).
    #[arg(long)]
    pub fix: bool,
    /// Apply setup changes without interactive confirmation.
    #[arg(long)]
    pub yes: bool,
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

    setup_cmd::run(SetupArgs {
        check: !args.fix,
        yes: args.yes,
        fanout_mode: args.fanout_mode.map(DoctorFanoutMode::as_setup_mode),
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
    if args.yes && !args.fix {
        bail!("`--yes` requires `--fix`");
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
            yes: false,
            fanout_mode: Some(DoctorFanoutMode::HiddenRef),
        };
        let err = validate_args(&args).expect_err("validate");
        assert!(err.to_string().contains("requires `--fix`"));
    }

    #[test]
    fn validate_args_accepts_fix_with_fanout() {
        let args = DoctorArgs {
            fix: true,
            yes: true,
            fanout_mode: Some(DoctorFanoutMode::GitNotes),
        };
        validate_args(&args).expect("validate");
    }

    #[test]
    fn validate_args_rejects_yes_without_fix() {
        let args = DoctorArgs {
            fix: false,
            yes: true,
            fanout_mode: None,
        };
        let err = validate_args(&args).expect_err("validate");
        assert!(err.to_string().contains("requires `--fix`"));
    }
}
