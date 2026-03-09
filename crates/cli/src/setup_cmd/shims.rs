use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ShimInstallAction {
    InstallNew,
    ReplaceExisting,
    KeepExisting,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ShimInstallPlan {
    pub(super) name: &'static str,
    pub(super) path: PathBuf,
    pub(super) action: ShimInstallAction,
}

#[derive(Debug, Clone)]
pub(super) struct ShimPaths {
    pub(super) opensession: PathBuf,
    pub(super) ops: PathBuf,
}

pub(super) fn plan_cli_shims() -> Result<Vec<ShimInstallPlan>> {
    let exe = std::env::current_exe().context("resolve current opensession executable path")?;
    let mut plans = Vec::new();
    for name in ["opensession", "ops"] {
        plans.push(plan_cli_shim(name, &exe)?);
    }
    Ok(plans)
}

fn plan_cli_shim(name: &'static str, exe: &Path) -> Result<ShimInstallPlan> {
    let path = shim_path(name)?;
    let action = if !path.exists() {
        ShimInstallAction::InstallNew
    } else if std::fs::canonicalize(&path).ok() == std::fs::canonicalize(exe).ok() {
        ShimInstallAction::KeepExisting
    } else {
        ShimInstallAction::ReplaceExisting
    };
    Ok(ShimInstallPlan { name, path, action })
}

pub(super) fn shim_action_label(action: ShimInstallAction) -> &'static str {
    match action {
        ShimInstallAction::InstallNew => "install",
        ShimInstallAction::ReplaceExisting => "replace",
        ShimInstallAction::KeepExisting => "keep",
    }
}

pub(super) fn shim_path(name: &str) -> Result<PathBuf> {
    Ok(opensession_paths::data_dir()
        .context("Could not determine shim base directory")?
        .join("bin")
        .join(name))
}

pub(super) fn install_cli_shims() -> Result<ShimPaths> {
    let exe = std::env::current_exe().context("resolve current opensession executable path")?;
    let opensession = install_cli_shim("opensession", &exe)?;
    let ops = install_cli_shim("ops", &exe)?;
    Ok(ShimPaths { opensession, ops })
}

fn install_cli_shim(name: &str, exe: &Path) -> Result<PathBuf> {
    let shim = shim_path(name)?;
    if let Some(parent) = shim.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create shim directory {}", parent.display()))?;
    }

    let existing_matches = if shim.exists() {
        std::fs::canonicalize(&shim).ok() == std::fs::canonicalize(exe).ok()
    } else {
        false
    };
    if existing_matches {
        return Ok(shim);
    }

    if shim.exists() {
        std::fs::remove_file(&shim)
            .with_context(|| format!("remove existing shim {}", shim.display()))?;
    }

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(exe, &shim)
            .with_context(|| format!("create shim symlink {}", shim.display()))?;
    }

    #[cfg(not(unix))]
    {
        std::fs::copy(exe, &shim)
            .with_context(|| format!("create shim copy {}", shim.display()))?;
    }

    Ok(shim)
}
