use crate::open_target::OpenTarget;
use crate::runtime_settings::{
    apply_summary_profile, detect_local_summary_profile, load_runtime_config, save_runtime_config,
};
use crate::setup_cmd::{self, SetupArgs, SetupFanoutMode, SetupProfile};
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
  opensession doctor --fix --profile local
  opensession doctor --fix --yes --profile app --fanout-mode hidden_ref --open-target app
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
    /// Set default review opener (`app` or `web`) before applying fixes.
    #[arg(long, value_enum)]
    pub open_target: Option<OpenTarget>,
    /// Choose setup profile (`local` = CLI-local-first, `app` = desktop-linked defaults).
    #[arg(long, value_enum)]
    pub profile: Option<SetupProfile>,
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
        open_target: args.open_target,
        print_ledger_ref: None,
        print_fanout_mode: false,
        sync_branch_session: None,
        sync_branch_commit: None,
        profile: args.profile,
    })?;

    if args.fix {
        let mut runtime = load_runtime_config()?;
        if !runtime.summary.is_configured() {
            if let Some(profile) = detect_local_summary_profile() {
                apply_summary_profile(&mut runtime, &profile);
                let path = save_runtime_config(&runtime)?;
                println!(
                    "summary provider detected and applied: {:?} ({})",
                    profile.provider,
                    path.display()
                );
            } else {
                println!("summary provider detect: none (keep disabled)");
            }
        }
    }

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
    if args.open_target.is_some() && !args.fix {
        return Err(guided_error(
            "`--open-target` requires `--fix`",
            [
                "run `opensession doctor --fix --profile app --open-target app`",
                "or run `opensession doctor --fix --profile local --open-target web`",
            ],
        ));
    }
    if args.yes && !args.fix {
        return Err(guided_error(
            "`--yes` requires `--fix`",
            [
                "run `opensession doctor --fix --yes --profile local --fanout-mode hidden_ref`",
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
            open_target: None,
            profile: None,
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
            open_target: Some(OpenTarget::Web),
            profile: Some(SetupProfile::Local),
        };
        validate_args(&args).expect("validate");
    }

    #[test]
    fn validate_args_rejects_yes_without_fix() {
        let args = DoctorArgs {
            fix: false,
            yes: true,
            fanout_mode: None,
            open_target: None,
            profile: None,
        };
        let err = validate_args(&args).expect_err("validate");
        let msg = err.to_string();
        assert!(msg.contains("requires `--fix`"));
        assert!(msg.contains("next:"));
        assert!(msg.contains("doctor --fix --yes --profile local --fanout-mode hidden_ref"));
    }

    #[test]
    fn validate_args_rejects_open_target_without_fix() {
        let args = DoctorArgs {
            fix: false,
            yes: false,
            fanout_mode: None,
            open_target: Some(OpenTarget::App),
            profile: None,
        };
        let err = validate_args(&args).expect_err("validate");
        let msg = err.to_string();
        assert!(msg.contains("`--open-target` requires `--fix`"));
        assert!(msg.contains("doctor --fix --profile app --open-target app"));
    }
}
