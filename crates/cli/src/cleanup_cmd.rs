use crate::user_guidance::guided_error;
use anyhow::{anyhow, bail, Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use opensession_git_native::ops::find_repo_root;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

const MANAGED_MARKER: &str = "opensession-managed-cleanup";
const GITLAB_MARKER_START: &str = "# >>> opensession-managed-cleanup";
const GITLAB_MARKER_END: &str = "# <<< opensession-managed-cleanup";
const PROMPTED_GIT_KEY: &str = "opensession.cleanup.prompted";
const PROMPTED_AT_GIT_KEY: &str = "opensession.cleanup.prompted-at";
const DEFAULT_TTL_DAYS: u16 = 30;
const CLEANUP_TEMPLATE_FILE: &str = "opensession-cleanup.yml";
const SESSION_REVIEW_TEMPLATE_FILE: &str = "opensession-session-review.yml";
const GITLAB_CLEANUP_TEMPLATE_INCLUDE: &str = ".gitlab/opensession-cleanup.yml";
const GITLAB_SESSION_REVIEW_TEMPLATE_INCLUDE: &str = ".gitlab/opensession-session-review.yml";

#[derive(Debug, Clone, Args)]
pub struct CleanupArgs {
    #[command(subcommand)]
    command: CleanupSubcommand,
}

#[derive(Debug, Clone, Subcommand)]
enum CleanupSubcommand {
    /// Initialize cleanup automation files for this repository.
    Init(CleanupInitArgs),
    /// Show cleanup setup status and janitor preview.
    Status(CleanupStatusArgs),
    /// Run janitor (dry-run by default).
    Run(CleanupRunArgs),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CleanupInitProvider {
    Auto,
    Github,
    Gitlab,
    Generic,
}

#[derive(Debug, Clone, Args)]
struct CleanupInitArgs {
    /// Cleanup provider mode.
    #[arg(long, value_enum, default_value_t = CleanupInitProvider::Auto)]
    provider: CleanupInitProvider,
    /// Git remote name or URL.
    #[arg(long, default_value = "origin")]
    remote: String,
    /// Hidden ref TTL in days.
    #[arg(long, default_value_t = DEFAULT_TTL_DAYS)]
    hidden_ttl_days: u16,
    /// Artifact branch TTL in days.
    #[arg(long, default_value_t = DEFAULT_TTL_DAYS)]
    artifact_ttl_days: u16,
    /// Skip confirmation prompt.
    #[arg(long)]
    yes: bool,
    /// JSON output.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Args)]
struct CleanupStatusArgs {
    /// JSON output.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Args)]
struct CleanupRunArgs {
    /// Apply deletion operations.
    #[arg(long)]
    apply: bool,
    /// JSON output.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CleanupProvider {
    Github,
    Gitlab,
    Generic,
}

impl CleanupProvider {
    fn as_str(self) -> &'static str {
        match self {
            Self::Github => "github",
            Self::Gitlab => "gitlab",
            Self::Generic => "generic",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CleanupConfig {
    version: u8,
    provider: CleanupProvider,
    remote: String,
    hidden_ttl_days: u16,
    artifact_ttl_days: u16,
    managed_at: String,
    managed_by: String,
}

#[derive(Debug, Clone)]
struct CleanupPaths {
    repo_root: PathBuf,
    cleanup_dir: PathBuf,
    config: PathBuf,
    janitor: PathBuf,
    cron_example: PathBuf,
    github_workflow: PathBuf,
    github_review_workflow: PathBuf,
    gitlab_template: PathBuf,
    gitlab_review_template: PathBuf,
    gitlab_ci: PathBuf,
}

#[derive(Debug, Clone)]
struct InitExecutionReport {
    provider: CleanupProvider,
    resolved_remote: String,
    applied_paths: Vec<String>,
    manual_steps: Vec<String>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct JanitorSummary {
    #[serde(default)]
    hidden_candidates: Vec<String>,
    #[serde(default)]
    artifact_candidates: Vec<String>,
    #[serde(default)]
    deleted: Vec<String>,
    #[serde(default)]
    failed: Vec<String>,
    #[serde(default)]
    kept_due_to_ttl: u64,
}

#[derive(Debug, Clone, Serialize)]
struct CleanupStatusJson {
    configured: bool,
    provider: Option<String>,
    janitor_present: bool,
    provider_template_ready: bool,
    next_action: Option<String>,
    janitor_preview: Option<JanitorSummary>,
    warning: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupDoctorLevel {
    Ok,
    Warn,
}

#[derive(Debug, Clone)]
pub struct CleanupDoctorStatus {
    pub level: CleanupDoctorLevel,
    pub detail: String,
    pub hint: Option<String>,
}

pub fn run(args: CleanupArgs) -> Result<()> {
    match args.command {
        CleanupSubcommand::Init(init_args) => run_init(init_args),
        CleanupSubcommand::Status(status_args) => run_status(status_args),
        CleanupSubcommand::Run(run_args) => run_execute(run_args),
    }
}

pub fn doctor_status(repo_root: &Path) -> CleanupDoctorStatus {
    let paths = cleanup_paths(repo_root.to_path_buf());
    match load_config_if_exists(&paths) {
        Ok(Some(config)) => {
            let janitor_present = paths.janitor.exists();
            let provider_ready = provider_template_ready(&paths, config.provider);
            if janitor_present && provider_ready {
                return CleanupDoctorStatus {
                    level: CleanupDoctorLevel::Ok,
                    detail: format!(
                        "configured provider={} hidden_ttl={}d artifact_ttl={}d",
                        config.provider.as_str(),
                        config.hidden_ttl_days,
                        config.artifact_ttl_days
                    ),
                    hint: None,
                };
            }

            CleanupDoctorStatus {
                level: CleanupDoctorLevel::Warn,
                detail: format!(
                    "partially configured provider={} janitor={} provider_template={}",
                    config.provider.as_str(),
                    if janitor_present {
                        "present"
                    } else {
                        "missing"
                    },
                    if provider_ready { "ready" } else { "missing" }
                ),
                hint: Some(
                    "run `opensession cleanup init --provider auto` to repair cleanup automation"
                        .to_string(),
                ),
            }
        }
        Ok(None) => CleanupDoctorStatus {
            level: CleanupDoctorLevel::Warn,
            detail: "not configured".to_string(),
            hint: Some(
                "run `opensession cleanup init --provider auto` to enable hidden ref cleanup"
                    .to_string(),
            ),
        },
        Err(err) => CleanupDoctorStatus {
            level: CleanupDoctorLevel::Warn,
            detail: format!("unavailable ({err})"),
            hint: Some(
                "run `opensession cleanup init --provider auto` to regenerate cleanup config"
                    .to_string(),
            ),
        },
    }
}

pub fn maybe_prompt_cleanup_init_after_push(repo_root: &Path, remote: &str) -> Result<()> {
    if !is_interactive_terminal() {
        return Ok(());
    }

    let paths = cleanup_paths(repo_root.to_path_buf());
    if paths.config.exists() {
        return Ok(());
    }

    if prompt_already_seen(repo_root)? {
        return Ok(());
    }

    eprintln!("[opensession] cleanup automation is not configured for this repository.");
    eprint!("[opensession] install cleanup automation now? [y/N]: ");
    io::stderr().flush().ok();

    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .context("read cleanup init prompt")?;

    let accepted = parse_confirmation(&line);
    mark_prompt_seen(repo_root)?;
    if !accepted {
        eprintln!("[opensession] cleanup automation was skipped.");
        return Ok(());
    }

    let _ = init_cleanup(
        repo_root,
        InitRequest {
            provider: CleanupInitProvider::Auto,
            remote: remote.to_string(),
            hidden_ttl_days: DEFAULT_TTL_DAYS,
            artifact_ttl_days: DEFAULT_TTL_DAYS,
            yes: true,
            json: false,
            silent: true,
        },
    )?;

    eprintln!("[opensession] cleanup automation configured.");
    Ok(())
}

fn run_init(args: CleanupInitArgs) -> Result<()> {
    let repo_root = resolve_repo_root()?;
    let report = init_cleanup(
        &repo_root,
        InitRequest {
            provider: args.provider,
            remote: args.remote,
            hidden_ttl_days: args.hidden_ttl_days,
            artifact_ttl_days: args.artifact_ttl_days,
            yes: args.yes,
            json: args.json,
            silent: false,
        },
    )?;

    if args.json {
        let payload = serde_json::json!({
            "configured": true,
            "provider": report.provider.as_str(),
            "resolved_remote": report.resolved_remote,
            "applied_paths": report.applied_paths,
            "manual_steps": report.manual_steps,
            "warnings": report.warnings,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    println!("cleanup configured: provider={}", report.provider.as_str());
    println!("remote: {}", report.resolved_remote);
    if !report.applied_paths.is_empty() {
        println!("applied:");
        for path in &report.applied_paths {
            println!("  - {path}");
        }
    }
    if !report.manual_steps.is_empty() {
        println!("manual steps:");
        for step in &report.manual_steps {
            println!("  - {step}");
        }
    }
    if !report.warnings.is_empty() {
        println!("warnings:");
        for warning in &report.warnings {
            println!("  - {warning}");
        }
    }

    Ok(())
}

fn run_status(args: CleanupStatusArgs) -> Result<()> {
    let repo_root = resolve_repo_root()?;
    let paths = cleanup_paths(repo_root);

    let Some(config) = load_config_if_exists(&paths)? else {
        let payload = CleanupStatusJson {
            configured: false,
            provider: None,
            janitor_present: paths.janitor.exists(),
            provider_template_ready: false,
            next_action: Some("opensession cleanup init --provider auto".to_string()),
            janitor_preview: None,
            warning: None,
        };

        if args.json {
            println!("{}", serde_json::to_string_pretty(&payload)?);
        } else {
            println!("cleanup: not configured");
            println!("next: opensession cleanup init --provider auto");
        }
        return Ok(());
    };

    let janitor_present = paths.janitor.exists();
    let provider_ready = provider_template_ready(&paths, config.provider);

    let mut warning = None;
    let mut janitor_preview = None;
    if janitor_present {
        match run_janitor(&paths, false, true) {
            Ok(output) => {
                let summary: JanitorSummary =
                    serde_json::from_slice(&output.stdout).context("parse janitor summary")?;
                janitor_preview = Some(summary);
            }
            Err(err) => {
                warning = Some(format!("janitor dry-run failed: {err}"));
            }
        }
    } else {
        warning = Some("janitor script is missing".to_string());
    }

    let next_action = if !provider_ready || !janitor_present {
        Some("opensession cleanup init --provider auto".to_string())
    } else {
        None
    };

    let payload = CleanupStatusJson {
        configured: true,
        provider: Some(config.provider.as_str().to_string()),
        janitor_present,
        provider_template_ready: provider_ready,
        next_action,
        janitor_preview,
        warning,
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    println!(
        "cleanup: configured (provider={})",
        config.provider.as_str()
    );
    println!(
        "janitor: {}",
        if payload.janitor_present {
            "present"
        } else {
            "missing"
        }
    );
    println!(
        "provider template: {}",
        if payload.provider_template_ready {
            "ready"
        } else {
            "missing"
        }
    );

    if let Some(preview) = payload.janitor_preview {
        println!(
            "preview: hidden_candidates={} artifact_candidates={} kept_due_to_ttl={}",
            preview.hidden_candidates.len(),
            preview.artifact_candidates.len(),
            preview.kept_due_to_ttl,
        );
    }

    if let Some(warning) = payload.warning {
        println!("warning: {warning}");
    }
    if let Some(next) = payload.next_action {
        println!("next: {next}");
    }

    Ok(())
}

fn run_execute(args: CleanupRunArgs) -> Result<()> {
    let repo_root = resolve_repo_root()?;
    let paths = cleanup_paths(repo_root);

    if !paths.janitor.exists() {
        return Err(guided_error(
            "cleanup janitor is not configured",
            [
                "initialize cleanup first: `opensession cleanup init --provider auto`",
                "then preview with `opensession cleanup status` or `opensession cleanup run`",
            ],
        ));
    }

    let output = run_janitor(&paths, args.apply, args.json)?;
    print!("{}", String::from_utf8_lossy(&output.stdout));

    if !output.status.success() {
        bail!(
            "cleanup run failed (exit={}){}",
            output.status.code().unwrap_or(1),
            stderr_suffix(&output.stderr),
        );
    }

    Ok(())
}

#[derive(Debug, Clone)]
struct InitRequest {
    provider: CleanupInitProvider,
    remote: String,
    hidden_ttl_days: u16,
    artifact_ttl_days: u16,
    yes: bool,
    json: bool,
    silent: bool,
}

fn init_cleanup(repo_root: &Path, req: InitRequest) -> Result<InitExecutionReport> {
    let paths = cleanup_paths(repo_root.to_path_buf());
    let remote = resolve_remote(&req.remote, repo_root)?;

    let mut warnings = Vec::new();
    let provider = match req.provider {
        CleanupInitProvider::Auto => {
            let detected = detect_provider_for_remote(&remote.url);
            if detected == CleanupProvider::Generic {
                warnings.push(format!(
                    "provider auto detection fell back to generic for remote `{}`",
                    remote.url
                ));
            }
            detected
        }
        CleanupInitProvider::Github => CleanupProvider::Github,
        CleanupInitProvider::Gitlab => CleanupProvider::Gitlab,
        CleanupInitProvider::Generic => CleanupProvider::Generic,
    };

    let mut manual_steps = Vec::<String>::new();
    let mut applied_paths = Vec::<String>::new();

    let mut plan_lines = vec![
        format!("provider: {}", provider.as_str()),
        format!("remote: {}", remote.push_target),
        format!("hidden_ttl_days: {}", req.hidden_ttl_days),
        format!("artifact_ttl_days: {}", req.artifact_ttl_days),
        format!("write: {}", paths.config.display()),
        format!("write: {}", paths.janitor.display()),
        format!("write: {}", paths.cron_example.display()),
    ];
    match provider {
        CleanupProvider::Github => {
            plan_lines.push(format!("write: {}", paths.github_workflow.display()));
            plan_lines.push(format!("write: {}", paths.github_review_workflow.display()));
        }
        CleanupProvider::Gitlab => {
            plan_lines.push(format!("write: {}", paths.gitlab_template.display()));
            plan_lines.push(format!("write: {}", paths.gitlab_review_template.display()));
            plan_lines.push(format!("update: {}", paths.gitlab_ci.display()));
        }
        CleanupProvider::Generic => {}
    }

    if !req.yes {
        if !is_interactive_terminal() {
            bail!("cleanup init requires --yes in non-interactive mode");
        }
        if !req.silent {
            println!("cleanup init plan:");
            for line in &plan_lines {
                println!("  - {line}");
            }
        }
        prompt_confirmation("Apply cleanup setup? [y/N]: ")?;
    } else if !req.silent && !req.json {
        println!("cleanup init plan:");
        for line in &plan_lines {
            println!("  - {line}");
        }
        println!("  - confirmation: skipped (--yes)");
    }

    fs::create_dir_all(&paths.cleanup_dir)
        .with_context(|| format!("create {}", paths.cleanup_dir.display()))?;

    let config = CleanupConfig {
        version: 1,
        provider,
        remote: remote.push_target.clone(),
        hidden_ttl_days: req.hidden_ttl_days,
        artifact_ttl_days: req.artifact_ttl_days,
        managed_at: chrono::Utc::now().to_rfc3339(),
        managed_by: MANAGED_MARKER.to_string(),
    };

    let config_body = toml::to_string_pretty(&config).context("serialize cleanup config")?;
    fs::write(&paths.config, config_body)
        .with_context(|| format!("write {}", paths.config.display()))?;
    applied_paths.push(path_display(repo_root, &paths.config));

    let janitor = render_janitor_template(&config);
    write_managed_file(
        &paths.janitor,
        &janitor,
        repo_root,
        &mut applied_paths,
        &mut manual_steps,
    )?;
    set_executable_if_unix(&paths.janitor)?;

    let cron_example = render_template(include_str!("templates/cleanup/cron.example.tmpl"), &[]);
    write_managed_file(
        &paths.cron_example,
        &cron_example,
        repo_root,
        &mut applied_paths,
        &mut manual_steps,
    )?;

    match provider {
        CleanupProvider::Github => {
            write_embedded_template(
                &paths.github_workflow,
                include_str!("templates/cleanup/github-workflow.yml.tmpl"),
                repo_root,
                &mut applied_paths,
                &mut manual_steps,
            )?;
            write_embedded_template(
                &paths.github_review_workflow,
                include_str!("templates/cleanup/github-session-review.yml.tmpl"),
                repo_root,
                &mut applied_paths,
                &mut manual_steps,
            )?;
        }
        CleanupProvider::Gitlab => {
            write_embedded_template(
                &paths.gitlab_template,
                include_str!("templates/cleanup/gitlab-cleanup.yml.tmpl"),
                repo_root,
                &mut applied_paths,
                &mut manual_steps,
            )?;
            write_embedded_template(
                &paths.gitlab_review_template,
                include_str!("templates/cleanup/gitlab-session-review.yml.tmpl"),
                repo_root,
                &mut applied_paths,
                &mut manual_steps,
            )?;
            update_gitlab_ci(
                &paths.gitlab_ci,
                repo_root,
                &mut applied_paths,
                &mut manual_steps,
            )?;
        }
        CleanupProvider::Generic => {}
    }

    Ok(InitExecutionReport {
        provider,
        resolved_remote: remote.push_target,
        applied_paths,
        manual_steps,
        warnings,
    })
}

fn run_janitor(paths: &CleanupPaths, apply: bool, json: bool) -> Result<std::process::Output> {
    let mut cmd = Command::new("sh");
    cmd.arg(&paths.janitor).current_dir(&paths.repo_root);
    if apply {
        cmd.arg("--apply");
    }
    if json {
        cmd.arg("--json");
    }

    cmd.output().with_context(|| {
        format!(
            "run cleanup janitor {}",
            path_display(&paths.repo_root, &paths.janitor)
        )
    })
}

fn cleanup_paths(repo_root: PathBuf) -> CleanupPaths {
    let cleanup_dir = repo_root.join(".opensession").join("cleanup");
    CleanupPaths {
        repo_root: repo_root.clone(),
        config: cleanup_dir.join("config.toml"),
        janitor: cleanup_dir.join("janitor.sh"),
        cron_example: cleanup_dir.join("cron.example"),
        cleanup_dir,
        github_workflow: repo_root
            .join(".github")
            .join("workflows")
            .join(CLEANUP_TEMPLATE_FILE),
        github_review_workflow: repo_root
            .join(".github")
            .join("workflows")
            .join(SESSION_REVIEW_TEMPLATE_FILE),
        gitlab_template: repo_root.join(".gitlab").join(CLEANUP_TEMPLATE_FILE),
        gitlab_review_template: repo_root.join(".gitlab").join(SESSION_REVIEW_TEMPLATE_FILE),
        gitlab_ci: repo_root.join(".gitlab-ci.yml"),
    }
}

fn resolve_repo_root() -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("read current directory")?;
    find_repo_root(&cwd).ok_or_else(|| anyhow!("current directory is not inside a git repository"))
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
        bail!("git remote `{remote}` resolved to empty URL");
    }

    Ok(RemoteSpec {
        url: resolved,
        push_target: remote.to_string(),
    })
}

fn detect_provider_for_remote(remote_url: &str) -> CleanupProvider {
    if let Some((host, _path)) = parse_remote_host_and_path(remote_url) {
        let host = host.to_ascii_lowercase();
        if host == "github.com" {
            return CleanupProvider::Github;
        }
        if host == "gitlab.com" || host.contains("gitlab") {
            return CleanupProvider::Gitlab;
        }
    }
    CleanupProvider::Generic
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

fn looks_like_remote_url(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.contains("://") || trimmed.starts_with("git@")
}

fn prompt_confirmation(prompt: &str) -> Result<()> {
    eprint!("{prompt}");
    io::stderr().flush().ok();

    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .context("read cleanup confirmation")?;
    if parse_confirmation(&line) {
        return Ok(());
    }

    bail!("cleanup init cancelled by user")
}

fn parse_confirmation(raw: &str) -> bool {
    let trimmed = raw.trim();
    trimmed.eq_ignore_ascii_case("y") || trimmed.eq_ignore_ascii_case("yes")
}

fn render_janitor_template(config: &CleanupConfig) -> String {
    render_template(
        include_str!("templates/cleanup/janitor.sh.tmpl"),
        &[
            ("{{REMOTE}}", &config.remote),
            ("{{HIDDEN_TTL_DAYS}}", &config.hidden_ttl_days.to_string()),
            (
                "{{ARTIFACT_TTL_DAYS}}",
                &config.artifact_ttl_days.to_string(),
            ),
        ],
    )
}

fn render_template(input: &str, vars: &[(&str, &str)]) -> String {
    let mut out = input.to_string();
    for (key, value) in vars {
        out = out.replace(key, value);
    }
    out
}

fn write_embedded_template(
    path: &Path,
    template: &str,
    repo_root: &Path,
    applied_paths: &mut Vec<String>,
    manual_steps: &mut Vec<String>,
) -> Result<()> {
    write_managed_file(path, template, repo_root, applied_paths, manual_steps)
}

fn write_managed_file(
    path: &Path,
    body: &str,
    repo_root: &Path,
    applied_paths: &mut Vec<String>,
    manual_steps: &mut Vec<String>,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }

    if path.exists() {
        let existing = fs::read_to_string(path)
            .with_context(|| format!("read existing {}", path.display()))?;
        if !existing.contains(MANAGED_MARKER) {
            manual_steps.push(format!(
                "file `{}` exists without `{MANAGED_MARKER}` marker; update it manually",
                path_display(repo_root, path)
            ));
            return Ok(());
        }
    }

    fs::write(path, body).with_context(|| format!("write {}", path.display()))?;
    applied_paths.push(path_display(repo_root, path));
    Ok(())
}

fn update_gitlab_ci(
    path: &Path,
    repo_root: &Path,
    applied_paths: &mut Vec<String>,
    manual_steps: &mut Vec<String>,
) -> Result<()> {
    let include_block = format!(
        "{GITLAB_MARKER_START}\ninclude:\n  - local: '{GITLAB_CLEANUP_TEMPLATE_INCLUDE}'\n  - local: '{GITLAB_SESSION_REVIEW_TEMPLATE_INCLUDE}'\n{GITLAB_MARKER_END}\n"
    );

    if !path.exists() {
        fs::write(path, include_block).with_context(|| format!("write {}", path.display()))?;
        applied_paths.push(path_display(repo_root, path));
        return Ok(());
    }

    let existing = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    if let Some(replaced) = replace_gitlab_marker_block(&existing, &include_block) {
        fs::write(path, replaced).with_context(|| format!("write {}", path.display()))?;
        applied_paths.push(path_display(repo_root, path));
        return Ok(());
    }

    manual_steps.push(format!(
        "`{}` has no `{MANAGED_MARKER}` block; add this include manually:\n{}",
        path_display(repo_root, path),
        include_block.trim_end()
    ));
    Ok(())
}

fn replace_gitlab_marker_block(existing: &str, replacement: &str) -> Option<String> {
    let start = existing.find(GITLAB_MARKER_START)?;
    let end_rel = existing[start..].find(GITLAB_MARKER_END)?;
    let end = start + end_rel + GITLAB_MARKER_END.len();

    let mut merged = String::new();
    merged.push_str(&existing[..start]);
    merged.push_str(replacement);
    if end < existing.len() {
        let remainder = existing[end..].trim_start_matches('\n');
        if !remainder.is_empty() {
            merged.push_str(remainder);
            if !merged.ends_with('\n') {
                merged.push('\n');
            }
        }
    }
    Some(merged)
}

fn set_executable_if_unix(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)
            .with_context(|| format!("read metadata {}", path.display()))?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms)
            .with_context(|| format!("set executable permissions on {}", path.display()))?;
    }

    Ok(())
}

fn path_display(repo_root: &Path, path: &Path) -> String {
    path.strip_prefix(repo_root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn load_config_if_exists(paths: &CleanupPaths) -> Result<Option<CleanupConfig>> {
    if !paths.config.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(&paths.config)
        .with_context(|| format!("read {}", paths.config.display()))?;
    let parsed: CleanupConfig = toml::from_str(&raw).context("parse cleanup config")?;
    validate_config(&parsed)?;
    Ok(Some(parsed))
}

fn validate_config(config: &CleanupConfig) -> Result<()> {
    if config.version != 1 {
        bail!(
            "unsupported cleanup config version {}; expected 1",
            config.version
        );
    }
    if config.remote.trim().is_empty() {
        bail!("cleanup config remote is empty");
    }
    Ok(())
}

fn provider_template_ready(paths: &CleanupPaths, provider: CleanupProvider) -> bool {
    match provider {
        CleanupProvider::Github => {
            paths.github_workflow.exists() && paths.github_review_workflow.exists()
        }
        CleanupProvider::Gitlab => {
            if !paths.gitlab_template.exists()
                || !paths.gitlab_review_template.exists()
                || !paths.gitlab_ci.exists()
            {
                return false;
            }
            fs::read_to_string(&paths.gitlab_ci)
                .map(|body| {
                    body.contains(GITLAB_MARKER_START)
                        && body.contains(GITLAB_MARKER_END)
                        && body.contains(GITLAB_CLEANUP_TEMPLATE_INCLUDE)
                        && body.contains(GITLAB_SESSION_REVIEW_TEMPLATE_INCLUDE)
                })
                .unwrap_or(false)
        }
        CleanupProvider::Generic => paths.cron_example.exists(),
    }
}

fn prompt_already_seen(repo_root: &Path) -> Result<bool> {
    let Some(raw) = git_config_get(repo_root, PROMPTED_GIT_KEY)? else {
        return Ok(false);
    };
    Ok(parse_truthy_value(&raw))
}

fn parse_truthy_value(raw: &str) -> bool {
    let trimmed = raw.trim();
    trimmed == "1"
        || trimmed.eq_ignore_ascii_case("true")
        || trimmed.eq_ignore_ascii_case("yes")
        || trimmed.eq_ignore_ascii_case("on")
}

fn mark_prompt_seen(repo_root: &Path) -> Result<()> {
    git_config_set(repo_root, PROMPTED_GIT_KEY, "true")?;
    git_config_set(
        repo_root,
        PROMPTED_AT_GIT_KEY,
        &chrono::Utc::now().to_rfc3339(),
    )?;
    Ok(())
}

fn git_config_get(repo_root: &Path, key: &str) -> Result<Option<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("config")
        .arg("--local")
        .arg("--get")
        .arg(key)
        .output()
        .with_context(|| format!("read git config `{key}`"))?;

    if output.status.success() {
        return Ok(Some(
            String::from_utf8_lossy(&output.stdout).trim().to_string(),
        ));
    }

    if output.status.code() == Some(1) {
        return Ok(None);
    }

    bail!(
        "failed to read git config `{key}`: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    )
}

fn git_config_set(repo_root: &Path, key: &str, value: &str) -> Result<()> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("config")
        .arg("--local")
        .arg(key)
        .arg(value)
        .output()
        .with_context(|| format!("write git config `{key}`"))?;

    if !output.status.success() {
        bail!(
            "failed to write git config `{key}`: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(())
}

fn is_interactive_terminal() -> bool {
    io::stdin().is_terminal() && io::stderr().is_terminal()
}

fn stderr_suffix(stderr: &[u8]) -> String {
    let trimmed = String::from_utf8_lossy(stderr).trim().to_string();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!(": {trimmed}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_provider_matches_expected_hosts() {
        assert_eq!(
            detect_provider_for_remote("https://github.com/acme/repo.git"),
            CleanupProvider::Github
        );
        assert_eq!(
            detect_provider_for_remote("git@gitlab.com:group/repo.git"),
            CleanupProvider::Gitlab
        );
        assert_eq!(
            detect_provider_for_remote("https://gitlab.internal.example.com/group/repo.git"),
            CleanupProvider::Gitlab
        );
        assert_eq!(
            detect_provider_for_remote("https://code.example.com/group/repo.git"),
            CleanupProvider::Generic
        );
    }

    #[test]
    fn cleanup_config_roundtrip_is_stable() {
        let config = CleanupConfig {
            version: 1,
            provider: CleanupProvider::Generic,
            remote: "origin".to_string(),
            hidden_ttl_days: 30,
            artifact_ttl_days: 30,
            managed_at: "2026-02-27T00:00:00Z".to_string(),
            managed_by: MANAGED_MARKER.to_string(),
        };

        let body = toml::to_string_pretty(&config).expect("serialize");
        let parsed: CleanupConfig = toml::from_str(&body).expect("parse");
        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.provider, CleanupProvider::Generic);
        assert_eq!(parsed.hidden_ttl_days, 30);
        assert_eq!(parsed.artifact_ttl_days, 30);
    }

    #[test]
    fn replace_gitlab_marker_block_updates_existing_content() {
        let existing =
            format!("header\n{GITLAB_MARKER_START}\nold\n{GITLAB_MARKER_END}\ntrailer\n");
        let replacement = format!(
            "{GITLAB_MARKER_START}\ninclude:\n  - local: '.gitlab/opensession-cleanup.yml'\n{GITLAB_MARKER_END}\n"
        );

        let updated = replace_gitlab_marker_block(&existing, &replacement).expect("replace");
        assert!(updated.contains(".gitlab/opensession-cleanup.yml"));
        assert!(updated.contains("header"));
        assert!(updated.contains("trailer"));
    }

    #[test]
    fn replace_gitlab_marker_block_requires_markers() {
        assert!(replace_gitlab_marker_block("include: []", "replacement").is_none());
    }

    #[test]
    fn parse_confirmation_accepts_yes_only() {
        assert!(parse_confirmation("y"));
        assert!(parse_confirmation("Yes"));
        assert!(!parse_confirmation("n"));
        assert!(!parse_confirmation(""));
    }

    #[test]
    fn parse_truthy_value_accepts_expected_tokens() {
        assert!(parse_truthy_value("1"));
        assert!(parse_truthy_value("TRUE"));
        assert!(parse_truthy_value(" yes "));
        assert!(parse_truthy_value("On"));
        assert!(!parse_truthy_value("0"));
        assert!(!parse_truthy_value("no"));
    }
}
