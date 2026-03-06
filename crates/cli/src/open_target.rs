use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use std::path::Path;
use std::process::Command;

pub const OPEN_TARGET_GIT_CONFIG_KEY: &str = "opensession.open-target";

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OpenTarget {
    #[value(name = "app", alias = "desktop")]
    App,
    #[value(name = "web", alias = "browser")]
    Web,
}

impl OpenTarget {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Web => "web",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "1" | "app" | "desktop" => Some(Self::App),
            "2" | "web" | "browser" => Some(Self::Web),
            _ => None,
        }
    }
}

pub fn read_repo_open_target(repo_root: &Path) -> Result<Option<OpenTarget>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("config")
        .arg("--local")
        .arg("--get")
        .arg(OPEN_TARGET_GIT_CONFIG_KEY)
        .output()
        .context("read git open target")?;
    if !output.status.success() {
        return Ok(None);
    }
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if raw.is_empty() {
        return Ok(None);
    }
    Ok(Some(OpenTarget::parse(&raw).unwrap_or(OpenTarget::App)))
}

pub fn write_repo_open_target(repo_root: &Path, target: OpenTarget) -> Result<()> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("config")
        .arg("--local")
        .arg(OPEN_TARGET_GIT_CONFIG_KEY)
        .arg(target.as_str())
        .output()
        .context("write git open target")?;
    if !output.status.success() {
        bail!(
            "failed to store open target in git config: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{OpenTarget, read_repo_open_target, write_repo_open_target};
    use std::process::Command;

    #[test]
    fn parse_open_target_choice_accepts_aliases() {
        assert_eq!(OpenTarget::parse("1"), Some(OpenTarget::App));
        assert_eq!(OpenTarget::parse("app"), Some(OpenTarget::App));
        assert_eq!(OpenTarget::parse("desktop"), Some(OpenTarget::App));
        assert_eq!(OpenTarget::parse("2"), Some(OpenTarget::Web));
        assert_eq!(OpenTarget::parse("web"), Some(OpenTarget::Web));
        assert_eq!(OpenTarget::parse("browser"), Some(OpenTarget::Web));
        assert_eq!(OpenTarget::parse("unknown"), None);
    }

    #[test]
    fn write_and_read_open_target_roundtrip() {
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

        assert_eq!(read_repo_open_target(&repo).expect("read"), None);
        write_repo_open_target(&repo, OpenTarget::Web).expect("write");
        assert_eq!(
            read_repo_open_target(&repo).expect("read"),
            Some(OpenTarget::Web)
        );
        write_repo_open_target(&repo, OpenTarget::App).expect("write");
        assert_eq!(
            read_repo_open_target(&repo).expect("read"),
            Some(OpenTarget::App)
        );
    }
}
