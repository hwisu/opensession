use crate::open_target::{OpenTarget, read_repo_open_target};
use anyhow::{Context, Result, anyhow, bail};
use opensession_local_store::global_store_root;
use reqwest::Url;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpenMode {
    Auto,
    App,
    Web,
}

pub(crate) fn open_url_for_repo(repo_root: &Path, url: &str) -> Result<()> {
    open_url_with_mode(url, resolve_open_mode(repo_root))
}

pub(crate) fn open_url_in_browser(url: &str) -> Result<()> {
    open_url_with_mode(url, OpenMode::Auto)
}

pub(crate) fn try_open_in_desktop_app_for_url(url: &str) -> Result<bool> {
    let Some(route) = desktop_launch_route_from_url(url) else {
        return Ok(false);
    };

    let route_path = write_desktop_launch_route(&route)?;
    let launched = MacDesktopAdapter.launch()?;
    if !launched {
        let _ = fs::remove_file(route_path);
    }
    Ok(launched)
}

pub(crate) fn resolve_open_mode(repo_root: &Path) -> OpenMode {
    match read_repo_open_target(repo_root) {
        Ok(Some(OpenTarget::App)) => OpenMode::App,
        Ok(Some(OpenTarget::Web)) => OpenMode::Web,
        Ok(None) => OpenMode::Auto,
        Err(err) => {
            eprintln!(
                "[opensession] failed to read repo open target ({err}); using auto open mode"
            );
            OpenMode::Auto
        }
    }
}

fn open_url_with_mode(url: &str, mode: OpenMode) -> Result<()> {
    if matches!(mode, OpenMode::App | OpenMode::Auto) {
        match try_open_in_desktop_app_for_url(url) {
            Ok(true) => return Ok(()),
            Ok(false) => {
                if matches!(mode, OpenMode::App) {
                    bail!(
                        "open target is set to `app`, but OpenSession Desktop is unavailable. next: install the desktop app or run `git config --local opensession.open-target web`"
                    );
                }
            }
            Err(err) => {
                if matches!(mode, OpenMode::App) {
                    return Err(err.context("failed to open OpenSession Desktop"));
                }
                eprintln!(
                    "[opensession] desktop app launch failed ({err}); falling back to browser open"
                );
            }
        }
    }
    if matches!(mode, OpenMode::App) {
        bail!(
            "open target is set to `app`, but this URL is not routable in the desktop app. next: run `git config --local opensession.open-target web`"
        );
    }

    SystemBrowserAdapter.open(url)
}

trait UrlAdapter {
    fn open(&self, url: &str) -> Result<()>;
}

trait DesktopLaunchAdapter {
    fn launch(&self) -> Result<bool>;
}

struct SystemBrowserAdapter;

impl UrlAdapter for SystemBrowserAdapter {
    fn open(&self, url: &str) -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            let status = Command::new("open")
                .arg(url)
                .status()
                .context("launch browser via `open`")?;
            if status.success() {
                return Ok(());
            }
        }

        #[cfg(target_os = "linux")]
        {
            let linux_attempts: [(&str, &[&str]); 3] = [
                ("xdg-open", &[url]),
                ("gio", &["open", url]),
                ("sensible-browser", &[url]),
            ];
            for (program, args) in linux_attempts {
                match Command::new(program).args(args).status() {
                    Ok(status) if status.success() => return Ok(()),
                    Ok(_) => continue,
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(_) => continue,
                }
            }
            bail!("{}", linux_open_help_message());
        }

        #[cfg(target_os = "windows")]
        {
            let status = Command::new("cmd")
                .arg("/C")
                .arg("start")
                .arg("")
                .arg(url)
                .status()
                .context("launch browser via `start`")?;
            if status.success() {
                return Ok(());
            }
        }

        bail!("failed to open browser automatically")
    }
}

#[cfg(any(target_os = "linux", test))]
fn linux_open_help_message() -> &'static str {
    "failed to open browser automatically on Linux (tried: xdg-open, gio open, sensible-browser). install xdg-utils or set `opensession.open-target app|web` explicitly"
}

struct MacDesktopAdapter;

impl DesktopLaunchAdapter for MacDesktopAdapter {
    fn launch(&self) -> Result<bool> {
        #[cfg(target_os = "macos")]
        {
            let attempts: [&[&str]; 2] = [
                &["-b", "io.opensession.desktop"],
                &["-a", "OpenSession Desktop"],
            ];
            for args in attempts {
                let status = Command::new("open")
                    .args(args)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
                if let Ok(status) = status {
                    if status.success() {
                        return Ok(true);
                    }
                }
            }
            Ok(false)
        }

        #[cfg(not(target_os = "macos"))]
        {
            Ok(false)
        }
    }
}

fn desktop_launch_route_from_url(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return None;
    }

    let path = parsed.path();
    let supported = path == "/sessions"
        || path.starts_with("/session/")
        || path.starts_with("/review/local/")
        || path.starts_with("/src/");
    if !supported {
        return None;
    }

    let mut route = path.to_string();
    if let Some(query) = parsed.query() {
        route.push('?');
        route.push_str(query);
    }
    if let Some(fragment) = parsed.fragment() {
        route.push('#');
        route.push_str(fragment);
    }
    Some(route)
}

fn desktop_launch_route_path() -> Result<PathBuf> {
    let store_root = global_store_root().context("resolve global store root")?;
    let opensession_root = store_root.parent().ok_or_else(|| {
        anyhow!(
            "invalid global store root path: {}",
            store_root.to_string_lossy()
        )
    })?;
    Ok(opensession_root.join("desktop").join("launch-route"))
}

fn write_desktop_launch_route(route: &str) -> Result<PathBuf> {
    let path = desktop_launch_route_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create desktop launch dir {}", parent.display()))?;
    }
    fs::write(&path, route)
        .with_context(|| format!("write desktop launch route {}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::{desktop_launch_route_from_url, linux_open_help_message};

    #[test]
    fn desktop_launch_route_from_sessions_url_preserves_query() {
        let route = desktop_launch_route_from_url(
            "http://127.0.0.1:8788/sessions?git_repo_name=acme%2Frepo",
        )
        .expect("route from sessions url");
        assert_eq!(route, "/sessions?git_repo_name=acme%2Frepo");
    }

    #[test]
    fn desktop_launch_route_rejects_unhandled_urls() {
        assert_eq!(
            desktop_launch_route_from_url("https://example.com/docs"),
            None
        );
        assert_eq!(
            desktop_launch_route_from_url("opensession://sessions"),
            None
        );
    }

    #[test]
    fn linux_open_help_mentions_install_hints() {
        let message = linux_open_help_message();
        assert!(message.contains("xdg-open"));
        assert!(message.contains("gio open"));
        assert!(message.contains("sensible-browser"));
    }
}
