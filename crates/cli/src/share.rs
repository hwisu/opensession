use crate::cleanup_cmd;
use crate::config_cmd::load_repo_config;
use crate::user_guidance::guided_error;
use anyhow::{Context, Result, bail};
use clap::Args;
use opensession_core::source_uri::{SourceSpec, SourceUri};
use opensession_local_store::{find_repo_root, read_local_object_from_uri};
use std::fs::OpenOptions;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const QUICK_AUTO_PUSH_CONSENT_GIT_KEY: &str = "opensession.share.auto-push-consent";

#[derive(Debug, Clone, Args)]
#[command(after_long_help = r"Recovery examples:
  opensession share os://src/local/<sha256> --quick
  opensession share os://src/local/<sha256> --git --remote origin
  opensession config init --base-url https://opensession.io
  opensession share os://src/git/<remote_b64>/ref/<ref_enc>/path/<path...> --web")]
pub struct ShareArgs {
    /// Source URI (`os://src/...`).
    pub uri: String,
    /// Web share mode (default).
    #[arg(long)]
    pub web: bool,
    /// Git share mode.
    #[arg(long)]
    pub git: bool,
    /// Quick git share mode (auto-detect remote and push after first confirmation).
    #[arg(long)]
    pub quick: bool,
    /// Machine-readable JSON output.
    #[arg(long)]
    pub json: bool,
    /// Copy primary output to clipboard.
    #[arg(long)]
    pub copy: bool,
    /// Git remote name or URL (required for `--git`).
    #[arg(long)]
    pub remote: Option<String>,
    /// Git ref to write into (default: hidden branch ledger ref).
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
    let mode = resolve_mode(args.web, args.git, args.quick)?;
    record_share_metric("share_entry", Some(mode.as_str()), None);
    let result = match mode {
        ShareMode::Web => run_web(uri, &args),
        ShareMode::Git => run_git(uri, &args),
    };
    match &result {
        Ok(()) => record_share_metric("share_succeeded", Some(mode.as_str()), None),
        Err(err) => record_share_metric(
            "share_failed",
            Some(mode.as_str()),
            Some(classify_share_failure(err.to_string().as_str())),
        ),
    }
    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShareMode {
    Web,
    Git,
}

impl ShareMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::Git => "git",
        }
    }
}

fn resolve_mode(web: bool, git: bool, quick: bool) -> Result<ShareMode> {
    if quick && web {
        return Err(guided_error(
            "`--quick` is git-only and cannot be combined with `--web`",
            [
                "use quick git sharing: `opensession share <uri> --quick`",
                "or explicit web mode for remote URIs: `opensession share <remote_uri> --web`",
            ],
        ));
    }
    if web && git {
        return Err(guided_error(
            "choose one mode: --web or --git",
            [
                "for remote source uri to web url: `opensession share <uri> --web`",
                "for local source uri to git source uri: `opensession share <uri> --git --remote origin`",
            ],
        ));
    }
    if git || quick {
        Ok(ShareMode::Git)
    } else {
        Ok(ShareMode::Web)
    }
}

fn run_web(uri: SourceUri, args: &ShareArgs) -> Result<()> {
    if !uri.is_remote_source() {
        return Err(guided_error(
            "`share --web` supports only remote sources",
            [
                "convert local uri first: `opensession share <uri> --git --remote origin`",
                "then run web share with the returned remote source uri",
            ],
        ));
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
        guided_error(
            "`share --git` requires a local source uri (os://src/local/<sha256>)",
            [
                "first register a canonical session: `opensession register ./session.hail.jsonl`",
                "then share that local uri with `--git --remote <name|url>`",
            ],
        )
    })?;

    let cwd = std::env::current_dir().context("read current directory")?;
    let (_path, bytes) = read_local_object_from_uri(&uri, &cwd)?;
    let repo_root = find_repo_root(&cwd).ok_or_else(|| {
        guided_error(
            "current directory is not inside a git repository",
            [
                "cd into the target git repository and retry",
                "or initialize one first: `git init`",
            ],
        )
    })?;

    let target_ref = args
        .git_ref
        .clone()
        .unwrap_or(default_ledger_ref(&repo_root)?);
    let target_path = args
        .path
        .clone()
        .unwrap_or_else(|| format!("sessions/{local_hash}.jsonl"));
    validate_rel_path(&target_path)?;

    let remote_arg = match args.remote.as_deref() {
        Some(remote) => remote.to_string(),
        None if args.quick => detect_quick_remote(&repo_root)?,
        None => {
            return Err(guided_error(
                "`--remote <name|url>` is required for `share --git`",
                [
                    "example: `opensession share <local_uri> --git --remote origin`",
                    "or use quick mode: `opensession share <local_uri> --quick`",
                ],
            ));
        }
    };

    if args.quick {
        record_share_metric("remote_ready", Some("quick"), None);
    }
    let remote = resolve_remote(&remote_arg, &repo_root)?;
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
    let mut consent_confirmed_once = false;
    let mut should_push = args.push;
    let mut auto_push_consent = read_quick_auto_push_consent(&repo_root).unwrap_or(false);
    if args.quick && !should_push {
        if auto_push_consent {
            should_push = true;
        } else if let Some(confirmed) =
            prompt_quick_push_consent(&repo_root, &push_cmd, &remote.push_target)?
        {
            should_push = confirmed;
            if confirmed {
                write_quick_auto_push_consent(&repo_root, true)?;
                auto_push_consent = true;
                consent_confirmed_once = true;
            }
        }
    }

    if args.quick && args.push && !auto_push_consent {
        write_quick_auto_push_consent(&repo_root, true)?;
        auto_push_consent = true;
        consent_confirmed_once = true;
    }

    if should_push {
        run_push(&repo_root, &remote.push_target, &target_ref)?;
        if !args.json {
            if let Err(err) =
                cleanup_cmd::maybe_prompt_cleanup_init_after_push(&repo_root, &remote.push_target)
            {
                eprintln!("[opensession] Warning: cleanup setup prompt failed: {err}");
            }
        }
    }

    if args.quick && consent_confirmed_once {
        record_share_metric("push_confirmed_once", Some("quick"), None);
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
            "quick": args.quick,
            "pushed": should_push,
            "push_cmd": push_cmd,
            "auto_push_consent": auto_push_consent,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    println!("{}", shared_uri);
    println!("remote: {}", remote.url);
    println!("ref: {target_ref}");
    println!("path: {target_path}");
    println!("quick: {}", args.quick);
    println!("pushed: {}", should_push);
    println!("auto_push_consent: {}", auto_push_consent);
    if should_push {
        println!("push_cmd: (executed) {push_cmd}");
    } else {
        println!("push_cmd: {push_cmd}");
        if args.quick && !auto_push_consent {
            println!(
                "hint: rerun with `--push` once, or confirm prompt in an interactive terminal to enable auto push"
            );
        }
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

fn detect_quick_remote(repo_root: &Path) -> Result<String> {
    let output = Command::new("git")
        .arg("remote")
        .current_dir(repo_root)
        .output()
        .context("list git remotes")?;
    if !output.status.success() {
        bail!(
            "failed to list git remotes: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let remotes = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    if remotes.is_empty() {
        return Err(guided_error(
            "quick share requires a configured git remote, but no remotes were found",
            [
                "add a remote first: `git remote add origin <url>`",
                "or run explicit share with URL: `opensession share <uri> --git --remote <url>`",
            ],
        ));
    }

    if remotes.iter().any(|remote| remote == "origin") {
        return Ok("origin".to_string());
    }
    if remotes.len() == 1 {
        return Ok(remotes[0].clone());
    }

    Err(guided_error(
        "quick share could not choose a remote automatically",
        [
            "pass an explicit remote: `opensession share <uri> --quick --remote origin`",
            "or use explicit git mode: `opensession share <uri> --git --remote <name|url>`",
        ],
    ))
}

fn read_quick_auto_push_consent(repo_root: &Path) -> Result<bool> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("config")
        .arg("--local")
        .arg("--get")
        .arg(QUICK_AUTO_PUSH_CONSENT_GIT_KEY)
        .output()
        .context("read quick share auto-push consent")?;
    if !output.status.success() {
        return Ok(false);
    }
    let raw = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_ascii_lowercase();
    Ok(matches!(raw.as_str(), "1" | "true" | "yes" | "on"))
}

fn write_quick_auto_push_consent(repo_root: &Path, enabled: bool) -> Result<()> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("config")
        .arg("--local")
        .arg(QUICK_AUTO_PUSH_CONSENT_GIT_KEY)
        .arg(if enabled { "true" } else { "false" })
        .output()
        .context("write quick share auto-push consent")?;
    if !output.status.success() {
        bail!(
            "failed to store quick share auto-push consent: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn prompt_quick_push_consent(
    _repo_root: &Path,
    push_cmd: &str,
    remote: &str,
) -> Result<Option<bool>> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Ok(None);
    }

    println!("quick share remote: {remote}");
    println!("quick share will execute: {push_cmd}");
    print!("push now and enable auto push for future `--quick` in this repo? [Y/n]: ");
    io::stdout().flush().context("flush stdout")?;

    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .context("read quick share confirmation")?;
    let normalized = line.trim().to_ascii_lowercase();
    if normalized.is_empty() || normalized == "y" || normalized == "yes" {
        return Ok(Some(true));
    }
    if normalized == "n" || normalized == "no" {
        return Ok(Some(false));
    }
    Ok(Some(true))
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

fn default_ledger_ref(repo_root: &Path) -> Result<String> {
    let cwd = repo_root.to_string_lossy().to_string();
    let git_ctx = opensession_git_native::extract_git_context(&cwd);
    let branch = opensession_git_native::resolve_ledger_branch(
        git_ctx.branch.as_deref(),
        git_ctx.commit.as_deref(),
    );
    Ok(opensession_git_native::branch_ledger_ref(&branch))
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
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(guided_error(
            format!("git push failed: {stderr}"),
            push_failure_hints(classify_share_failure(&stderr), remote, target_ref),
        ));
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

fn classify_share_failure(message: &str) -> &'static str {
    let lower = message.to_ascii_lowercase();
    if lower.contains("authentication")
        || lower.contains("could not read username")
        || lower.contains("permission denied (publickey)")
    {
        return "auth";
    }
    if lower.contains("permission to")
        || lower.contains("access denied")
        || lower.contains("not allowed")
    {
        return "permission";
    }
    if lower.contains("no remotes were found")
        || lower.contains("requires a configured git remote")
        || lower.contains("could not choose a remote automatically")
        || lower.contains("`--remote <name|url>` is required")
    {
        return "remote_missing";
    }
    if lower.contains("could not resolve host")
        || lower.contains("failed to connect")
        || lower.contains("connection timed out")
        || lower.contains("network is unreachable")
    {
        return "network";
    }
    "unknown"
}

fn push_failure_hints(reason: &str, remote: &str, target_ref: &str) -> [String; 2] {
    match reason {
        "auth" => [
            format!("validate credentials for `{remote}` and retry"),
            format!("retry push manually: `git push {remote} {target_ref}:{target_ref}`"),
        ],
        "permission" => [
            format!("verify write permission for remote `{remote}`"),
            format!("retry push manually: `git push {remote} {target_ref}:{target_ref}`"),
        ],
        "network" => [
            "check network connectivity and retry".to_string(),
            format!("retry push manually: `git push {remote} {target_ref}:{target_ref}`"),
        ],
        _ => [
            format!("retry push manually: `git push {remote} {target_ref}:{target_ref}`"),
            "rerun with `OPENSESSION_DEBUG=1` for expanded error output".to_string(),
        ],
    }
}

fn share_metric_path() -> Option<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;
    Some(
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("opensession")
            .join("metrics")
            .join("share-funnel.jsonl"),
    )
}

fn record_share_metric(event: &str, mode: Option<&str>, reason: Option<&str>) {
    let Some(path) = share_metric_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0);
    let payload = serde_json::json!({
        "ts": ts,
        "event": event,
        "mode": mode,
        "reason": reason,
    });
    let mut file = match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(file) => file,
        Err(_) => return,
    };
    let _ = writeln!(file, "{payload}");
}

#[cfg(target_os = "linux")]
fn linux_clipboard_candidates() -> [(&'static str, &'static [&'static str]); 3] {
    [
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard"]),
        ("xsel", &["--clipboard", "--input"]),
    ]
}

fn try_copy_to_clipboard(value: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        if try_clipboard_command("pbcopy", &[], value)? {
            return Ok(());
        }
    }

    #[cfg(target_os = "linux")]
    {
        for (program, args) in linux_clipboard_candidates() {
            if try_clipboard_command(program, args, value)? {
                return Ok(());
            }
        }
        bail!(
            "clipboard copy is unavailable on this platform (install one of: wl-clipboard, xclip, xsel)"
        );
    }

    #[cfg(target_os = "windows")]
    {
        if try_clipboard_command("clip", &[], value)? {
            return Ok(());
        }
    }

    bail!("clipboard copy is unavailable on this platform")
}

fn try_clipboard_command(program: &str, args: &[&str], value: &str) -> Result<bool> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(anyhow::Error::new(err).context(format!("launch {program}"))),
    };

    if let Some(stdin) = child.stdin.as_mut() {
        use std::io::Write;
        if let Err(err) = stdin.write_all(value.as_bytes()) {
            return Err(anyhow::Error::new(err).context(format!("write {program}")));
        }
    }

    let status = child.wait().with_context(|| format!("wait {program}"))?;
    Ok(status.success())
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    use super::linux_clipboard_candidates;
    use super::{
        QUICK_AUTO_PUSH_CONSENT_GIT_KEY, ShareMode, classify_share_failure, detect_quick_remote,
        parse_remote_host_and_path, read_quick_auto_push_consent, resolve_mode, uri_for_remote,
        validate_rel_path, write_quick_auto_push_consent,
    };
    use opensession_core::source_uri::SourceSpec;
    use opensession_core::source_uri::SourceUri;
    use std::process::Command;
    use std::{fs, path::PathBuf};

    fn init_repo(tmp: &tempfile::TempDir, name: &str) -> PathBuf {
        let repo = tmp.path().join(name);
        fs::create_dir_all(&repo).expect("create repo dir");
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
        repo
    }

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
    fn uri_for_remote_detects_gl_for_gitlab_dot_com_https_and_ssh() {
        let https = uri_for_remote(
            "https://gitlab.com/group/sub/repo.git",
            "refs/heads/main",
            "sessions/x.jsonl",
        );
        assert_eq!(
            https,
            SourceUri::Src(SourceSpec::Gl {
                project: "group/sub/repo".to_string(),
                r#ref: "refs/heads/main".to_string(),
                path: "sessions/x.jsonl".to_string(),
            })
        );

        let ssh = uri_for_remote(
            "git@gitlab.com:group/sub/repo.git",
            "refs/heads/main",
            "sessions/x.jsonl",
        );
        assert_eq!(
            ssh,
            SourceUri::Src(SourceSpec::Gl {
                project: "group/sub/repo".to_string(),
                r#ref: "refs/heads/main".to_string(),
                path: "sessions/x.jsonl".to_string(),
            })
        );
    }

    #[test]
    fn uri_for_remote_keeps_self_managed_gitlab_as_git() {
        let uri = uri_for_remote(
            "https://gitlab.internal.example.com/group/sub/repo.git",
            "refs/heads/main",
            "sessions/x.jsonl",
        );
        assert_eq!(
            uri,
            SourceUri::Src(SourceSpec::Git {
                remote: "https://gitlab.internal.example.com/group/sub/repo.git".to_string(),
                r#ref: "refs/heads/main".to_string(),
                path: "sessions/x.jsonl".to_string(),
            })
        );
    }

    #[test]
    fn uri_for_remote_keeps_generic_remote_as_git() {
        let uri = uri_for_remote(
            "https://code.example.com/team/repo.git",
            "refs/heads/main",
            "sessions/x.jsonl",
        );
        assert_eq!(
            uri,
            SourceUri::Src(SourceSpec::Git {
                remote: "https://code.example.com/team/repo.git".to_string(),
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

    #[test]
    fn resolve_mode_supports_quick_git_and_rejects_web_mix() {
        assert!(matches!(
            resolve_mode(false, false, true).expect("resolve quick mode"),
            ShareMode::Git
        ));
        assert!(resolve_mode(true, false, true).is_err());
    }

    #[test]
    fn quick_remote_detection_prefers_origin_or_single_remote() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(&tmp, "repo");

        let add_upstream = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .arg("remote")
            .arg("add")
            .arg("upstream")
            .arg("https://github.com/example/upstream.git")
            .output()
            .expect("add upstream");
        assert!(
            add_upstream.status.success(),
            "{}",
            String::from_utf8_lossy(&add_upstream.stderr)
        );
        assert_eq!(
            detect_quick_remote(&repo).expect("detect single remote"),
            "upstream"
        );

        let add_origin = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .arg("remote")
            .arg("add")
            .arg("origin")
            .arg("https://github.com/example/origin.git")
            .output()
            .expect("add origin");
        assert!(
            add_origin.status.success(),
            "{}",
            String::from_utf8_lossy(&add_origin.stderr)
        );
        assert_eq!(detect_quick_remote(&repo).expect("detect origin"), "origin");
    }

    #[test]
    fn quick_remote_detection_fails_without_remote_or_with_ambiguous_remote() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let empty = init_repo(&tmp, "empty");
        let err = detect_quick_remote(&empty).expect_err("missing remote should fail");
        let msg = err.to_string();
        assert!(msg.contains("no remotes were found"));
        assert!(msg.contains("git remote add origin"));

        let ambiguous = init_repo(&tmp, "ambiguous");
        let add_upstream = Command::new("git")
            .arg("-C")
            .arg(&ambiguous)
            .arg("remote")
            .arg("add")
            .arg("upstream")
            .arg("https://github.com/example/upstream.git")
            .output()
            .expect("add upstream");
        assert!(
            add_upstream.status.success(),
            "{}",
            String::from_utf8_lossy(&add_upstream.stderr)
        );
        let add_mirror = Command::new("git")
            .arg("-C")
            .arg(&ambiguous)
            .arg("remote")
            .arg("add")
            .arg("mirror")
            .arg("https://github.com/example/mirror.git")
            .output()
            .expect("add mirror");
        assert!(
            add_mirror.status.success(),
            "{}",
            String::from_utf8_lossy(&add_mirror.stderr)
        );
        let err = detect_quick_remote(&ambiguous).expect_err("ambiguous remotes should fail");
        let msg = err.to_string();
        assert!(msg.contains("could not choose a remote automatically"));
        assert!(msg.contains("--quick --remote origin"));
    }

    #[test]
    fn quick_auto_push_consent_roundtrip_in_repo_config() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(&tmp, "repo");

        assert!(!read_quick_auto_push_consent(&repo).expect("read default"));
        write_quick_auto_push_consent(&repo, true).expect("write true");
        assert!(read_quick_auto_push_consent(&repo).expect("read true"));
        write_quick_auto_push_consent(&repo, false).expect("write false");
        assert!(!read_quick_auto_push_consent(&repo).expect("read false"));
    }

    #[test]
    fn quick_auto_push_consent_accepts_truthy_aliases() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(&tmp, "repo");
        for value in ["1", "true", "yes", "on", "TRUE"] {
            let set = Command::new("git")
                .arg("-C")
                .arg(&repo)
                .arg("config")
                .arg("--local")
                .arg(QUICK_AUTO_PUSH_CONSENT_GIT_KEY)
                .arg(value)
                .output()
                .expect("set consent");
            assert!(set.status.success(), "failed to set value {value}");
            assert!(
                read_quick_auto_push_consent(&repo).expect("read consent"),
                "expected truthy for {value}"
            );
        }
    }

    #[test]
    fn quick_auto_push_consent_treats_unknown_value_as_disabled() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(&tmp, "repo");
        let set = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .arg("config")
            .arg("--local")
            .arg(QUICK_AUTO_PUSH_CONSENT_GIT_KEY)
            .arg("maybe")
            .output()
            .expect("set consent");
        assert!(
            set.status.success(),
            "{}",
            String::from_utf8_lossy(&set.stderr)
        );
        assert!(!read_quick_auto_push_consent(&repo).expect("read consent"));
    }

    #[test]
    fn classify_share_failure_maps_known_taxonomy() {
        assert_eq!(
            classify_share_failure("fatal: Authentication failed for https://example.com/repo.git"),
            "auth"
        );
        assert_eq!(
            classify_share_failure("fatal: Could not resolve host: github.com"),
            "network"
        );
        assert_eq!(
            classify_share_failure("quick share requires a configured git remote"),
            "remote_missing"
        );
        assert_eq!(
            classify_share_failure("Permission to org/repo denied"),
            "permission"
        );
        assert_eq!(classify_share_failure("something unexpected"), "unknown");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_clipboard_fallback_order_is_stable() {
        let candidates = linux_clipboard_candidates();
        assert_eq!(candidates[0].0, "wl-copy");
        assert_eq!(candidates[1].0, "xclip");
        assert_eq!(candidates[2].0, "xsel");
    }
}
