use crate::config_cmd::load_repo_config;
use anyhow::{bail, Context, Result};
use clap::Args;
use opensession_core::object_store::{find_repo_root, read_local_object_from_uri};
use opensession_core::source_uri::{SourceSpec, SourceUri};
use std::path::Path;
use std::process::Command;

const DEFAULT_GIT_REF: &str = "refs/heads/opensession/sessions";

#[derive(Debug, Clone, Args)]
pub struct ShareArgs {
    /// Source URI (`os://src/...`).
    pub uri: String,
    /// Web share mode (default).
    #[arg(long)]
    pub web: bool,
    /// Git share mode.
    #[arg(long)]
    pub git: bool,
    /// Machine-readable JSON output.
    #[arg(long)]
    pub json: bool,
    /// Copy primary output to clipboard.
    #[arg(long)]
    pub copy: bool,
    /// Git remote name or URL (required for `--git`).
    #[arg(long)]
    pub remote: Option<String>,
    /// Git ref to write into (`refs/heads/...`).
    #[arg(long = "ref")]
    pub git_ref: Option<String>,
    /// Repo-relative target path in the ref.
    #[arg(long)]
    pub path: Option<String>,
    /// Perform network push immediately.
    #[arg(long)]
    pub push: bool,
}

pub fn run(args: ShareArgs) -> Result<()> {
    let uri = SourceUri::parse(&args.uri)?;
    let mode = resolve_mode(args.web, args.git)?;
    match mode {
        ShareMode::Web => run_web(uri, &args),
        ShareMode::Git => run_git(uri, &args),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShareMode {
    Web,
    Git,
}

fn resolve_mode(web: bool, git: bool) -> Result<ShareMode> {
    if web && git {
        bail!("choose one mode: --web or --git");
    }
    if git {
        Ok(ShareMode::Git)
    } else {
        Ok(ShareMode::Web)
    }
}

fn run_web(uri: SourceUri, args: &ShareArgs) -> Result<()> {
    if !uri.is_remote_source() {
        bail!("`share --web` supports only remote sources. For local objects, use `opensession share <uri> --git --remote <name|url>`.");
    }

    let cwd = std::env::current_dir().context("read current directory")?;
    let (config_path, config) = load_repo_config(&cwd)?;
    let path = uri
        .to_web_path()
        .ok_or_else(|| anyhow::anyhow!("uri cannot be resolved to web path"))?;
    let base_url = config.share.base_url.trim_end_matches('/');
    let web_url = format!("{base_url}{path}");

    if args.copy {
        let _ = try_copy_to_clipboard(&web_url);
    }

    if args.json {
        let payload = serde_json::json!({
            "uri": uri.to_string(),
            "web_url": web_url,
            "base_url": base_url,
            "config": config_path,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    println!("{web_url}");
    println!("base_url: {base_url}");
    println!("config: {}", config_path.display());
    Ok(())
}

fn run_git(uri: SourceUri, args: &ShareArgs) -> Result<()> {
    let local_hash = uri.as_local_hash().ok_or_else(|| {
        anyhow::anyhow!("`share --git` requires a local source uri (os://src/local/<sha256>)")
    })?;

    let remote_arg = args
        .remote
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("`--remote <name|url>` is required for `share --git`"))?;

    let cwd = std::env::current_dir().context("read current directory")?;
    let (_path, bytes) = read_local_object_from_uri(&uri, &cwd)?;
    let repo_root = find_repo_root(&cwd)
        .ok_or_else(|| anyhow::anyhow!("current directory is not inside a git repository"))?;

    let target_ref = args
        .git_ref
        .clone()
        .unwrap_or_else(|| DEFAULT_GIT_REF.to_string());
    let target_path = args
        .path
        .clone()
        .unwrap_or_else(|| format!("sessions/{local_hash}.jsonl"));
    validate_rel_path(&target_path)?;

    let remote = resolve_remote(remote_arg, &repo_root)?;
    let commit_message = format!("opensession share {local_hash}");
    opensession_git_native::store_blob_at_ref(
        &repo_root,
        &target_ref,
        &target_path,
        &bytes,
        &commit_message,
    )
    .map_err(|err| anyhow::anyhow!("failed to store git object: {err}"))?;

    let shared_uri = uri_for_remote(&remote.url, &target_ref, &target_path);
    let push_cmd = format!("git push {} {target_ref}:{target_ref}", remote.push_target);
    if args.push {
        run_push(&repo_root, &remote.push_target, &target_ref)?;
    }

    if args.copy {
        let _ = try_copy_to_clipboard(&shared_uri.to_string());
    }

    if args.json {
        let payload = serde_json::json!({
            "uri": shared_uri.to_string(),
            "source_uri": uri.to_string(),
            "remote": remote.url,
            "remote_target": remote.push_target,
            "ref": target_ref,
            "path": target_path,
            "pushed": args.push,
            "push_cmd": push_cmd,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    println!("{}", shared_uri);
    println!("remote: {}", remote.url);
    println!("ref: {target_ref}");
    println!("path: {target_path}");
    println!("pushed: {}", args.push);
    if args.push {
        println!("push_cmd: (executed) {push_cmd}");
    } else {
        println!("push_cmd: {push_cmd}");
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct RemoteSpec {
    url: String,
    push_target: String,
}

fn resolve_remote(remote: &str, repo_root: &Path) -> Result<RemoteSpec> {
    if looks_like_remote_url(remote) {
        return Ok(RemoteSpec {
            url: remote.trim().to_string(),
            push_target: remote.trim().to_string(),
        });
    }

    let output = Command::new("git")
        .arg("remote")
        .arg("get-url")
        .arg(remote)
        .current_dir(repo_root)
        .output()
        .with_context(|| format!("resolve remote `{remote}`"))?;
    if !output.status.success() {
        bail!(
            "failed to resolve git remote `{remote}`: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let resolved = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if resolved.is_empty() {
        bail!("git remote `{remote}` resolved to empty url");
    }
    Ok(RemoteSpec {
        url: resolved,
        push_target: remote.to_string(),
    })
}

fn looks_like_remote_url(value: &str) -> bool {
    value.contains("://") || value.starts_with("git@")
}

fn validate_rel_path(path: &str) -> Result<()> {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed.starts_with('/') {
        bail!("path must be repository-relative");
    }
    for segment in trimmed.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." || segment.contains('\\') {
            bail!("path contains invalid segment: `{segment}`");
        }
    }
    Ok(())
}

fn run_push(repo_root: &Path, remote: &str, target_ref: &str) -> Result<()> {
    let output = Command::new("git")
        .arg("push")
        .arg(remote)
        .arg(format!("{target_ref}:{target_ref}"))
        .current_dir(repo_root)
        .output()
        .context("run git push")?;
    if !output.status.success() {
        bail!(
            "git push failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn uri_for_remote(remote_url: &str, r#ref: &str, path: &str) -> SourceUri {
    if let Some((host, repo_path)) = parse_remote_host_and_path(remote_url) {
        let cleaned = repo_path.trim_start_matches('/').trim_end_matches(".git");
        if host.eq_ignore_ascii_case("github.com") {
            let mut parts = cleaned.split('/').filter(|segment| !segment.is_empty());
            if let (Some(owner), Some(repo)) = (parts.next(), parts.next()) {
                return SourceUri::Src(SourceSpec::Gh {
                    owner: owner.to_string(),
                    repo: repo.to_string(),
                    r#ref: r#ref.to_string(),
                    path: path.to_string(),
                });
            }
        }
        if host.eq_ignore_ascii_case("gitlab.com") {
            return SourceUri::Src(SourceSpec::Gl {
                project: cleaned.to_string(),
                r#ref: r#ref.to_string(),
                path: path.to_string(),
            });
        }
    }

    SourceUri::Src(SourceSpec::Git {
        remote: remote_url.to_string(),
        r#ref: r#ref.to_string(),
        path: path.to_string(),
    })
}

fn parse_remote_host_and_path(remote_url: &str) -> Option<(String, String)> {
    let remote = remote_url.trim();
    if remote.is_empty() {
        return None;
    }

    if let Some(rest) = remote.strip_prefix("git@") {
        let mut parts = rest.splitn(2, ':');
        let host = parts.next()?.trim().to_string();
        let path = parts.next()?.trim().to_string();
        if host.is_empty() || path.is_empty() {
            return None;
        }
        return Some((host, path));
    }

    let scheme_idx = remote.find("://")?;
    let after_scheme = &remote[scheme_idx + 3..];
    let without_user = after_scheme.rsplit('@').next().unwrap_or(after_scheme);
    let mut host_and_path = without_user.splitn(2, '/');
    let host_part = host_and_path.next()?.trim();
    let path = host_and_path.next()?.trim().to_string();
    if host_part.is_empty() || path.is_empty() {
        return None;
    }
    let host = host_part.split(':').next().unwrap_or(host_part).to_string();
    Some((host, path))
}

fn try_copy_to_clipboard(value: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let mut child = Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .context("launch pbcopy")?;
        if let Some(stdin) = child.stdin.as_mut() {
            use std::io::Write;
            stdin.write_all(value.as_bytes()).context("write pbcopy")?;
        }
        let status = child.wait().context("wait pbcopy")?;
        if status.success() {
            return Ok(());
        }
    }

    #[cfg(target_os = "linux")]
    {
        let mut child = Command::new("xclip")
            .arg("-selection")
            .arg("clipboard")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .context("launch xclip")?;
        if let Some(stdin) = child.stdin.as_mut() {
            use std::io::Write;
            stdin.write_all(value.as_bytes()).context("write xclip")?;
        }
        let status = child.wait().context("wait xclip")?;
        if status.success() {
            return Ok(());
        }
    }

    #[cfg(target_os = "windows")]
    {
        let mut child = Command::new("clip")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .context("launch clip")?;
        if let Some(stdin) = child.stdin.as_mut() {
            use std::io::Write;
            stdin.write_all(value.as_bytes()).context("write clip")?;
        }
        let status = child.wait().context("wait clip")?;
        if status.success() {
            return Ok(());
        }
    }

    bail!("clipboard copy is unavailable on this platform")
}

#[cfg(test)]
mod tests {
    use super::{parse_remote_host_and_path, uri_for_remote, validate_rel_path};
    use opensession_core::source_uri::SourceSpec;
    use opensession_core::source_uri::SourceUri;

    #[test]
    fn parse_remote_supports_ssh_and_https() {
        assert_eq!(
            parse_remote_host_and_path("git@github.com:hwisu/opensession.git"),
            Some((
                "github.com".to_string(),
                "hwisu/opensession.git".to_string()
            ))
        );
        assert_eq!(
            parse_remote_host_and_path("https://gitlab.com/group/sub/repo.git"),
            Some(("gitlab.com".to_string(), "group/sub/repo.git".to_string()))
        );
    }

    #[test]
    fn uri_for_remote_detects_gh() {
        let uri = uri_for_remote(
            "https://github.com/hwisu/opensession.git",
            "refs/heads/main",
            "sessions/x.jsonl",
        );
        assert_eq!(
            uri,
            SourceUri::Src(SourceSpec::Gh {
                owner: "hwisu".to_string(),
                repo: "opensession".to_string(),
                r#ref: "refs/heads/main".to_string(),
                path: "sessions/x.jsonl".to_string(),
            })
        );
    }

    #[test]
    fn path_validation_rejects_traversal() {
        assert!(validate_rel_path("sessions/ok.jsonl").is_ok());
        assert!(validate_rel_path("../bad").is_err());
    }
}
