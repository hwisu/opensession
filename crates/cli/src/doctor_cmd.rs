use crate::setup_cmd::{self, SetupArgs, SetupFanoutMode};
use crate::user_guidance::guided_error;
use anyhow::Result;
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
#[command(after_long_help = r"Recovery examples:
  opensession doctor
  opensession doctor --fix
  opensession doctor --fix --yes --fanout-mode hidden_ref
  opensession docs quickstart")]
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
        println!("hint: run `opensession docs quickstart` for a 5-minute first-user flow.");
    }

    Ok(())
}

fn validate_args(args: &DoctorArgs) -> Result<()> {
    if args.fanout_mode.is_some() && !args.fix {
        return Err(guided_error(
            "`--fanout-mode` requires `--fix`",
            [
                "run `opensession doctor --fix --fanout-mode hidden_ref`",
                "or run `opensession doctor --fix --fanout-mode git_notes`",
            ],
        ));
    }
    if args.yes && !args.fix {
        return Err(guided_error(
            "`--yes` requires `--fix`",
            [
                "run `opensession doctor --fix --yes --fanout-mode hidden_ref`",
                "or run `opensession doctor --fix` in interactive mode",
            ],
        ));
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
        let msg = err.to_string();
        assert!(msg.contains("requires `--fix`"));
        assert!(msg.contains("next:"));
        assert!(msg.contains("doctor --fix --fanout-mode hidden_ref"));
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
        let msg = err.to_string();
        assert!(msg.contains("requires `--fix`"));
        assert!(msg.contains("next:"));
        assert!(msg.contains("doctor --fix --yes --fanout-mode hidden_ref"));
    }
}
