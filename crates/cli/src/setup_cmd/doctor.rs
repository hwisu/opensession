use crate::open_target::OpenTarget;
use std::io::{self, IsTerminal};

use super::{FanoutMode, SetupProfile};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DoctorLevel {
    Ok,
    Info,
    Warn,
    Fail,
}

#[derive(Debug, Default, Clone, Copy)]
pub(super) struct DoctorSummary {
    ok: usize,
    info: usize,
    warn: usize,
    fail: usize,
}

impl DoctorSummary {
    pub(super) fn record(&mut self, level: DoctorLevel) {
        match level {
            DoctorLevel::Ok => self.ok += 1,
            DoctorLevel::Info => self.info += 1,
            DoctorLevel::Warn => self.warn += 1,
            DoctorLevel::Fail => self.fail += 1,
        }
    }

    pub(super) fn issue_categories(&self) -> usize {
        self.warn + self.fail
    }
}

pub(super) fn suggested_doctor_command(
    mode: FanoutMode,
    target: OpenTarget,
    profile: SetupProfile,
) -> String {
    format!(
        "opensession doctor --fix --yes --profile {} --fanout-mode {} --open-target {}",
        profile.as_str(),
        mode.as_str(),
        target.as_str(),
    )
}

pub(super) fn doctor_colors_enabled() -> bool {
    io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

pub(super) fn doctor_tag(level: DoctorLevel, colors: bool) -> String {
    let plain = match level {
        DoctorLevel::Ok => "[ OK ]",
        DoctorLevel::Info => "[INFO]",
        DoctorLevel::Warn => "[WARN]",
        DoctorLevel::Fail => "[FAIL]",
    };
    if !colors {
        return plain.to_string();
    }
    let code = match level {
        DoctorLevel::Ok => "32",
        DoctorLevel::Info => "36",
        DoctorLevel::Warn => "33",
        DoctorLevel::Fail => "31",
    };
    format!("\x1b[1;{code}m{plain}\x1b[0m")
}

pub(super) fn print_doctor_item(colors: bool, level: DoctorLevel, label: &str, detail: &str) {
    println!(
        "{} {:<18} {}",
        doctor_tag(level, colors),
        format!("{label}:"),
        detail
    );
}

pub(super) fn print_doctor_hint(detail: &str) {
    println!("       hint: {detail}");
}
