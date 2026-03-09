use crate::hooks::{HookType, install_hooks_with_report, plan_hook_install};
use crate::open_target::{OpenTarget, read_repo_open_target};
use anyhow::{Context, Result, bail};
use clap::{Args, ValueEnum};
use opensession_git_native::branch_ledger_ref;
use std::path::Path;

mod branch_sync;
mod doctor;
mod planning;
mod shims;
mod status;
mod validation;

use branch_sync::sync_branch_session_to_hidden_ledger;
use planning::{
    ensure_fanout_mode, ensure_open_target, print_applied_setup, print_setup_plan, read_fanout_mode,
};
use shims::{install_cli_shims, plan_cli_shims};
use status::{
    current_branch, ledger_branch_name, print_daemon_status, print_review_readiness, run_check,
};
use validation::{
    enforce_apply_mode_requirements, is_interactive_terminal, prompt_apply_confirmation,
    validate_setup_args,
};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SetupFanoutMode {
    #[value(name = "hidden_ref", alias = "hidden-ref", alias = "hidden")]
    HiddenRef,
    #[value(name = "git_notes", alias = "git-notes", alias = "notes")]
    GitNotes,
}

impl SetupFanoutMode {
    fn as_fanout_mode(self) -> FanoutMode {
        match self {
            Self::HiddenRef => FanoutMode::HiddenRef,
            Self::GitNotes => FanoutMode::GitNotes,
        }
    }
}

#[derive(Debug, Clone, Args)]
pub struct SetupArgs {
    /// Show setup status only.
    #[arg(long)]
    pub check: bool,
    /// Apply setup changes without interactive confirmation.
    #[arg(long)]
    pub yes: bool,
    /// Set fanout mode before applying setup changes.
    #[arg(long, value_enum)]
    pub fanout_mode: Option<SetupFanoutMode>,
    /// Set default review opener (`app` or `web`) for this repository.
    #[arg(long, value_enum)]
    pub open_target: Option<OpenTarget>,
    /// Choose setup profile (`local` = CLI-local-first, `app` = desktop-linked defaults).
    #[arg(long, value_enum)]
    pub profile: Option<SetupProfile>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SetupProfile {
    #[value(name = "local")]
    Local,
    #[value(name = "app")]
    App,
}

impl SetupProfile {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::App => "app",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FanoutInstallPlan {
    existing: Option<FanoutMode>,
    requested: Option<FanoutMode>,
}

impl FanoutInstallPlan {
    fn summary(self) -> String {
        match (self.existing, self.requested) {
            (Some(existing), Some(requested)) if existing == requested => {
                format!(
                    "keep {existing} (explicit via --fanout-mode)",
                    existing = existing.as_str()
                )
            }
            (Some(existing), Some(requested)) => format!(
                "change {existing} -> {requested} (via --fanout-mode)",
                existing = existing.as_str(),
                requested = requested.as_str()
            ),
            (None, Some(requested)) => {
                format!(
                    "set {requested} (via --fanout-mode)",
                    requested = requested.as_str()
                )
            }
            (Some(existing), None) => format!("keep {existing}", existing = existing.as_str()),
            (None, None) => "choose interactively (hidden_ref or git_notes)".to_string(),
        }
    }

    fn suggested_mode(self) -> FanoutMode {
        self.requested
            .or(self.existing)
            .unwrap_or(FanoutMode::HiddenRef)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OpenTargetInstallPlan {
    existing: Option<OpenTarget>,
    requested: Option<OpenTarget>,
}

impl OpenTargetInstallPlan {
    fn summary(self, profile: SetupProfile) -> String {
        match (self.existing, self.requested) {
            (Some(existing), Some(requested)) if existing == requested => {
                format!(
                    "keep {existing} (explicit via --open-target)",
                    existing = existing.as_str()
                )
            }
            (Some(existing), Some(requested)) => format!(
                "change {existing} -> {requested} (via --open-target)",
                existing = existing.as_str(),
                requested = requested.as_str()
            ),
            (None, Some(requested)) => {
                format!(
                    "set {requested} (via --open-target)",
                    requested = requested.as_str()
                )
            }
            (Some(existing), None) => format!("keep {existing}", existing = existing.as_str()),
            (None, None) => format!(
                "choose interactively (default: {})",
                planning::default_open_target_for_profile(profile).as_str()
            ),
        }
    }

    fn suggested_target(self, profile: SetupProfile) -> OpenTarget {
        self.requested
            .or(self.existing)
            .unwrap_or(planning::default_open_target_for_profile(profile))
    }
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

    validate_setup_args(&args)?;
    if args.check {
        return run_check(&repo_root);
    }
    run_install(
        &repo_root,
        args.yes,
        args.fanout_mode.map(SetupFanoutMode::as_fanout_mode),
        args.open_target,
        args.profile,
    )
}

fn run_install(
    repo_root: &Path,
    yes: bool,
    requested_fanout: Option<FanoutMode>,
    requested_open_target: Option<OpenTarget>,
    requested_profile: Option<SetupProfile>,
) -> Result<()> {
    let profile = requested_profile.unwrap_or(SetupProfile::Local);
    let interactive = is_interactive_terminal();
    let existing_fanout = read_fanout_mode(repo_root)?;
    let fanout_plan = FanoutInstallPlan {
        existing: existing_fanout,
        requested: requested_fanout,
    };
    let existing_open_target = read_repo_open_target(repo_root)?;
    let open_target_plan = OpenTargetInstallPlan {
        existing: existing_open_target,
        requested: requested_open_target,
    };
    enforce_apply_mode_requirements(interactive, yes, fanout_plan)?;

    let hook_plans = plan_hook_install(repo_root, HookType::all())?;
    let shim_plans = plan_cli_shims()?;
    print_setup_plan(
        repo_root,
        fanout_plan,
        open_target_plan,
        profile,
        &hook_plans,
        &shim_plans,
        yes,
    );

    if !yes {
        prompt_apply_confirmation(
            fanout_plan.suggested_mode(),
            open_target_plan.suggested_target(profile),
            profile,
        )?;
    }

    let fanout_mode = ensure_fanout_mode(repo_root, requested_fanout, interactive)?;
    let open_target = ensure_open_target(repo_root, requested_open_target, interactive, profile)?;
    let shim_paths = install_cli_shims()?;
    let hook_reports = install_hooks_with_report(repo_root, HookType::all())?;
    print_applied_setup(
        repo_root,
        fanout_mode,
        open_target,
        &hook_reports,
        &shim_plans,
        &shim_paths,
    );

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    #[test]
    fn print_ledger_ref_matches_helper() {
        let got = branch_ledger_ref("feature/abc");
        assert_eq!(got, "refs/opensession/branches/ZmVhdHVyZS9hYmM");
    }

    #[test]
    fn daemon_status_reports_not_running_when_pid_file_missing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let status = status::daemon_status(&tmp.path().join("daemon.pid"));
        assert_eq!(status, status::DaemonStatus::NotRunning);
    }

    #[test]
    fn daemon_status_reports_unreadable_for_invalid_pid_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let pid_path = tmp.path().join("daemon.pid");
        fs::write(&pid_path, "not-a-pid").expect("write pid");
        let status = status::daemon_status(&pid_path);
        assert!(matches!(status, status::DaemonStatus::Unreadable(_)));
    }

    #[test]
    fn daemon_status_summary_includes_hint_when_not_running() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let pid_path = tmp.path().join("daemon.pid");
        let (level, summary, hint) =
            status::daemon_status_summary(&status::DaemonStatus::NotRunning, &pid_path);
        assert_eq!(level, doctor::DoctorLevel::Info);
        assert!(summary.contains("not running"));
        assert!(hint.expect("hint should exist").contains("optional:"));
    }

    #[test]
    fn review_readiness_summary_warns_when_hidden_fanout_missing() {
        let (level, summary, hint) = status::review_readiness_summary(false, false);
        assert_eq!(level, doctor::DoctorLevel::Warn);
        assert!(summary.contains("hidden-fanout=missing"));
        assert!(
            hint.expect("hint should exist")
                .contains("opensession doctor --fix")
        );
    }

    #[test]
    fn review_readiness_summary_marks_missing_refs_as_optional_info() {
        let (level, summary, hint) = status::review_readiness_summary(true, false);
        assert_eq!(level, doctor::DoctorLevel::Info);
        assert!(summary.contains("hidden-refs=none-fetched"));
        assert!(hint.expect("hint should exist").contains("optional:"));
    }

    #[test]
    fn doctor_tag_plain_is_ascii_stable() {
        assert_eq!(doctor::doctor_tag(doctor::DoctorLevel::Ok, false), "[ OK ]");
        assert_eq!(
            doctor::doctor_tag(doctor::DoctorLevel::Info, false),
            "[INFO]"
        );
        assert_eq!(
            doctor::doctor_tag(doctor::DoctorLevel::Warn, false),
            "[WARN]"
        );
        assert_eq!(
            doctor::doctor_tag(doctor::DoctorLevel::Fail, false),
            "[FAIL]"
        );
    }

    #[test]
    fn validate_setup_args_rejects_yes_with_check() {
        let args = SetupArgs {
            check: true,
            yes: true,
            fanout_mode: None,
            open_target: None,
            print_ledger_ref: None,
            print_fanout_mode: false,
            sync_branch_session: None,
            sync_branch_commit: None,
            profile: None,
        };
        let err = validation::validate_setup_args(&args).expect_err("validate");
        assert!(err.to_string().contains("cannot be used"));
    }

    #[test]
    fn validate_setup_args_rejects_fanout_with_check() {
        let args = SetupArgs {
            check: true,
            yes: false,
            fanout_mode: Some(SetupFanoutMode::HiddenRef),
            open_target: None,
            print_ledger_ref: None,
            print_fanout_mode: false,
            sync_branch_session: None,
            sync_branch_commit: None,
            profile: None,
        };
        let err = validation::validate_setup_args(&args).expect_err("validate");
        assert!(err.to_string().contains("requires apply mode"));
    }

    #[test]
    fn validate_setup_args_rejects_open_target_with_check() {
        let args = SetupArgs {
            check: true,
            yes: false,
            fanout_mode: None,
            open_target: Some(OpenTarget::Web),
            print_ledger_ref: None,
            print_fanout_mode: false,
            sync_branch_session: None,
            sync_branch_commit: None,
            profile: None,
        };
        let err = validation::validate_setup_args(&args).expect_err("validate");
        assert!(
            err.to_string()
                .contains("`--open-target` requires apply mode")
        );
    }

    #[test]
    fn parse_apply_confirmation_accepts_yes_aliases() {
        assert!(validation::parse_apply_confirmation("y"));
        assert!(validation::parse_apply_confirmation("Y"));
        assert!(validation::parse_apply_confirmation("yes"));
        assert!(validation::parse_apply_confirmation(" YES "));
    }

    #[test]
    fn parse_apply_confirmation_rejects_non_yes_values() {
        assert!(!validation::parse_apply_confirmation(""));
        assert!(!validation::parse_apply_confirmation("n"));
        assert!(!validation::parse_apply_confirmation("no"));
        assert!(!validation::parse_apply_confirmation("anything"));
    }

    #[test]
    fn enforce_apply_mode_requirements_requires_yes_for_non_interactive() {
        let plan = FanoutInstallPlan {
            existing: Some(FanoutMode::HiddenRef),
            requested: None,
        };
        let err =
            validation::enforce_apply_mode_requirements(false, false, plan).expect_err("validate");
        assert!(err.to_string().contains("requires explicit approval"));
    }

    #[test]
    fn enforce_apply_mode_requirements_requires_explicit_fanout_for_non_interactive() {
        let plan = FanoutInstallPlan {
            existing: None,
            requested: None,
        };
        let err =
            validation::enforce_apply_mode_requirements(false, true, plan).expect_err("validate");
        assert!(err.to_string().contains("fanout mode is not configured"));
    }

    #[test]
    fn parse_fanout_choice_accepts_hidden_ref_aliases() {
        assert_eq!(
            planning::parse_fanout_choice("1"),
            Some(FanoutMode::HiddenRef)
        );
        assert_eq!(
            planning::parse_fanout_choice("hidden_ref"),
            Some(FanoutMode::HiddenRef)
        );
        assert_eq!(
            planning::parse_fanout_choice("hidden"),
            Some(FanoutMode::HiddenRef)
        );
    }

    #[test]
    fn parse_fanout_choice_accepts_git_notes_aliases() {
        assert_eq!(
            planning::parse_fanout_choice("2"),
            Some(FanoutMode::GitNotes)
        );
        assert_eq!(
            planning::parse_fanout_choice("git_notes"),
            Some(FanoutMode::GitNotes)
        );
        assert_eq!(
            planning::parse_fanout_choice("notes"),
            Some(FanoutMode::GitNotes)
        );
    }

    #[test]
    fn parse_fanout_choice_rejects_unknown_values() {
        assert_eq!(planning::parse_fanout_choice(""), None);
        assert_eq!(planning::parse_fanout_choice("unknown"), None);
    }

    #[test]
    fn parse_open_target_choice_accepts_aliases() {
        assert_eq!(
            planning::parse_open_target_choice("1"),
            Some(OpenTarget::App)
        );
        assert_eq!(
            planning::parse_open_target_choice("app"),
            Some(OpenTarget::App)
        );
        assert_eq!(
            planning::parse_open_target_choice("desktop"),
            Some(OpenTarget::App)
        );
        assert_eq!(
            planning::parse_open_target_choice("2"),
            Some(OpenTarget::Web)
        );
        assert_eq!(
            planning::parse_open_target_choice("web"),
            Some(OpenTarget::Web)
        );
        assert_eq!(
            planning::parse_open_target_choice("browser"),
            Some(OpenTarget::Web)
        );
    }

    #[test]
    fn default_open_target_depends_on_setup_profile() {
        assert_eq!(
            planning::default_open_target_for_profile(SetupProfile::Local),
            OpenTarget::Web
        );
        assert_eq!(
            planning::default_open_target_for_profile(SetupProfile::App),
            OpenTarget::App
        );
    }

    #[test]
    fn open_target_plan_uses_profile_default_when_unset() {
        let plan = OpenTargetInstallPlan {
            existing: None,
            requested: None,
        };
        assert_eq!(plan.suggested_target(SetupProfile::Local), OpenTarget::Web);
        assert_eq!(plan.suggested_target(SetupProfile::App), OpenTarget::App);
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

        assert_eq!(planning::read_fanout_mode(&repo).expect("read"), None);

        planning::write_fanout_mode(&repo, FanoutMode::GitNotes).expect("write");
        assert_eq!(
            planning::read_fanout_mode(&repo).expect("read"),
            Some(FanoutMode::GitNotes)
        );

        planning::write_fanout_mode(&repo, FanoutMode::HiddenRef).expect("write");
        assert_eq!(
            planning::read_fanout_mode(&repo).expect("read"),
            Some(FanoutMode::HiddenRef)
        );
    }
}
