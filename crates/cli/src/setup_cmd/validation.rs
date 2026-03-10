use super::doctor;
use super::{FanoutInstallPlan, FanoutMode, SetupArgs, SetupProfile};
use crate::open_target::OpenTarget;
use anyhow::{Context, Result, bail};
use std::io::{self, IsTerminal, Write};

pub(super) fn validate_setup_args(args: &SetupArgs) -> Result<()> {
    if args.check && args.yes {
        bail!("`--yes` cannot be used with `--check`");
    }
    if args.check && args.fanout_mode.is_some() {
        bail!(
            "`--fanout-mode` requires apply mode. next: run `opensession doctor --fix --yes --profile local --fanout-mode hidden_ref`"
        );
    }
    if args.check && args.open_target.is_some() {
        bail!(
            "`--open-target` requires apply mode. next: run `opensession doctor --fix --yes --profile local --open-target web`"
        );
    }
    Ok(())
}

pub(super) fn enforce_apply_mode_requirements(
    interactive: bool,
    yes: bool,
    fanout_plan: FanoutInstallPlan,
) -> Result<()> {
    let suggested_mode = fanout_plan.suggested_mode();
    if !interactive && !yes {
        bail!(
            "setup requires explicit approval in non-interactive mode.\nnext: run `{}`",
            doctor::suggested_doctor_command(suggested_mode, OpenTarget::Web, SetupProfile::Local)
        );
    }
    if !interactive && fanout_plan.existing.is_none() && fanout_plan.requested.is_none() {
        bail!(
            "fanout mode is not configured for this repository, and setup cannot prompt in non-interactive mode.\nnext: run `{}`",
            doctor::suggested_doctor_command(
                FanoutMode::HiddenRef,
                OpenTarget::Web,
                SetupProfile::Local
            )
        );
    }
    Ok(())
}

pub(super) fn is_interactive_terminal() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

pub(super) fn prompt_apply_confirmation(
    mode_hint: FanoutMode,
    open_target_hint: OpenTarget,
    profile: SetupProfile,
) -> Result<()> {
    print!("Apply these changes? [y/N]: ");
    io::stdout().flush().context("flush stdout")?;
    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .context("read setup confirmation")?;
    if parse_apply_confirmation(&line) {
        return Ok(());
    }
    bail!(
        "setup cancelled by user.\nnext: run `{}`",
        doctor::suggested_doctor_command(mode_hint, open_target_hint, profile)
    );
}

pub(super) fn parse_apply_confirmation(input: &str) -> bool {
    matches!(input.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}
