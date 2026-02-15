use anyhow::{bail, Context, Result};
use chrono::{DateTime, Local};
use dialoguer::Select;
use opensession_core::config::DaemonConfig;
use opensession_parsers::discover::discover_for_tool;
use opensession_tui::{RunOptions, SummaryLaunchOverride};
use serde::Serialize;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum SummaryStyleArg {
    Chat,
    Responses,
}

impl SummaryStyleArg {
    fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Responses => "responses",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum SummaryContentModeArg {
    Normal,
    Minimal,
}

impl SummaryContentModeArg {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Minimal => "minimal",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ToggleArg {
    On,
    Off,
}

impl ToggleArg {
    pub(crate) fn as_bool(self) -> bool {
        matches!(self, Self::On)
    }
}

#[derive(Debug, Clone)]
pub struct ViewArgs {
    pub agent: String,
    pub active_within_minutes: u32,
    pub limit: usize,
    pub non_interactive: bool,
    pub latest: bool,
    pub dry_run: bool,
    pub summary_provider: Option<String>,
    pub summary_model: Option<String>,
    pub summary_content_mode: Option<SummaryContentModeArg>,
    pub summary_disk_cache: Option<ToggleArg>,
    pub sum_endpoint: Option<String>,
    pub sum_base: Option<String>,
    pub sum_path: Option<String>,
    pub sum_style: Option<SummaryStyleArg>,
    pub sum_key: Option<String>,
    pub sum_key_header: Option<String>,
}

#[derive(Debug, Clone)]
enum SelectionMode {
    Latest,
    ActiveWindow,
    FallbackLatest,
}

impl SelectionMode {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Latest => "latest",
            Self::ActiveWindow => "active-window",
            Self::FallbackLatest => "fallback-latest",
        }
    }
}

#[derive(Debug, Clone)]
struct Candidate {
    path: PathBuf,
    modified: SystemTime,
}

#[derive(Debug, Serialize)]
struct DryRunOutput {
    agent: String,
    selection_mode: String,
    selected_path: String,
    selected_modified_unix: u64,
    candidate_count: usize,
    used_interactive_picker: bool,
    summary_provider: Option<String>,
    summary_model: Option<String>,
    summary_content_mode: Option<String>,
    summary_disk_cache_enabled: Option<bool>,
    sum_endpoint: Option<String>,
    sum_base: Option<String>,
    sum_path: Option<String>,
    sum_style: Option<String>,
    sum_key_header: Option<String>,
    summary_event_window: u32,
    summary_auto_phases: bool,
}

pub fn run_view(args: ViewArgs) -> Result<()> {
    let normalized_tool = normalize_agent_alias(&args.agent)?;
    let limit = args.limit.max(1);
    let (candidates, selection_mode) = collect_candidates(
        &normalized_tool,
        args.latest,
        args.active_within_minutes,
        limit,
    )?;
    let (selected, used_interactive_picker) =
        choose_candidate(&normalized_tool, &candidates, args.non_interactive)?;

    let summary_provider = resolve_summary_provider_override(&normalized_tool, &args);
    let summary_override = SummaryLaunchOverride {
        provider: summary_provider.clone(),
        model: normalize_opt(args.summary_model),
        content_mode: args
            .summary_content_mode
            .map(|mode| mode.as_str().to_string()),
        disk_cache_enabled: args.summary_disk_cache.map(ToggleArg::as_bool),
        openai_compat_endpoint: normalize_opt(args.sum_endpoint),
        openai_compat_base: normalize_opt(args.sum_base),
        openai_compat_path: normalize_opt(args.sum_path),
        openai_compat_style: args.sum_style.map(|s| s.as_str().to_string()),
        openai_compat_api_key: normalize_opt(args.sum_key),
        openai_compat_api_key_header: normalize_opt(args.sum_key_header.clone()),
    };
    let has_override = summary_override.has_any_override();

    if args.dry_run {
        let summary_event_window = current_summary_event_window();
        let selected_modified_unix = selected
            .modified
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let payload = DryRunOutput {
            agent: normalized_tool.clone(),
            selection_mode: selection_mode.as_str().to_string(),
            selected_path: selected.path.to_string_lossy().to_string(),
            selected_modified_unix,
            candidate_count: candidates.len(),
            used_interactive_picker,
            summary_provider,
            summary_model: summary_override.model,
            summary_content_mode: summary_override.content_mode,
            summary_disk_cache_enabled: summary_override.disk_cache_enabled,
            sum_endpoint: summary_override.openai_compat_endpoint,
            sum_base: summary_override.openai_compat_base,
            sum_path: summary_override.openai_compat_path,
            sum_style: summary_override.openai_compat_style,
            sum_key_header: summary_override.openai_compat_api_key_header,
            summary_event_window,
            summary_auto_phases: summary_event_window == 0,
        };
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    let run_options = RunOptions {
        paths: Some(vec![selected.path.to_string_lossy().to_string()]),
        auto_enter_detail: true,
        summary_override: if has_override {
            Some(summary_override)
        } else {
            None
        },
        focus_detail_view: true,
    };
    opensession_tui::run_with_options(run_options)
}

fn choose_candidate(
    tool: &str,
    candidates: &[Candidate],
    non_interactive: bool,
) -> Result<(Candidate, bool)> {
    if candidates.is_empty() {
        bail!("no candidate sessions found for tool '{tool}'");
    }
    if candidates.len() == 1 {
        return Ok((candidates[0].clone(), false));
    }

    let can_prompt =
        !non_interactive && std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    if !can_prompt {
        return Ok((candidates[0].clone(), false));
    }

    let items: Vec<String> = candidates.iter().map(candidate_display_line).collect();

    let selection = Select::new()
        .with_prompt(format!("Select active {tool} session"))
        .items(&items)
        .default(0)
        .interact()
        .context("failed to select session candidate")?;

    Ok((candidates[selection].clone(), true))
}

fn collect_candidates(
    tool: &str,
    latest: bool,
    active_within_minutes: u32,
    limit: usize,
) -> Result<(Vec<Candidate>, SelectionMode)> {
    let mut discovered: Vec<Candidate> = discover_for_tool(tool)
        .into_iter()
        .filter(|path| !is_excluded_path(path))
        .filter_map(|path| {
            let modified = std::fs::metadata(&path).ok()?.modified().ok()?;
            Some(Candidate { path, modified })
        })
        .collect();

    sort_candidates(&mut discovered);
    discovered.truncate(limit);

    if discovered.is_empty() {
        bail!("no local sessions found for tool '{tool}'");
    }

    if latest {
        return Ok((vec![discovered[0].clone()], SelectionMode::Latest));
    }

    let window = Duration::from_secs((active_within_minutes as u64).saturating_mul(60));
    let now = SystemTime::now();
    let active: Vec<Candidate> = discovered
        .iter()
        .filter(|candidate| {
            now.duration_since(candidate.modified)
                .map(|age| age <= window)
                .unwrap_or(false)
        })
        .cloned()
        .collect();

    if active.is_empty() {
        return Ok((vec![discovered[0].clone()], SelectionMode::FallbackLatest));
    }

    Ok((active, SelectionMode::ActiveWindow))
}

fn sort_candidates(candidates: &mut [Candidate]) {
    candidates.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then_with(|| a.path.cmp(&b.path))
    });
}

fn normalize_agent_alias(raw: &str) -> Result<String> {
    let normalized = raw.trim().to_ascii_lowercase();
    let mapped = match normalized.as_str() {
        "claude" | "claude-code" => "claude-code",
        "codex" => "codex",
        "cursor" => "cursor",
        "gemini" => "gemini",
        "opencode" => "opencode",
        "cline" => "cline",
        "amp" => "amp",
        _ => {
            bail!(
                "unsupported agent '{}'; expected one of: claude|codex|cursor|gemini|opencode|cline|amp",
                raw
            )
        }
    };
    Ok(mapped.to_string())
}

fn resolve_summary_provider_override(tool: &str, args: &ViewArgs) -> Option<String> {
    if let Some(provider) = normalize_opt(args.summary_provider.clone()) {
        return Some(provider);
    }
    default_cli_provider_for_tool(tool)
}

fn default_cli_provider_for_tool(tool: &str) -> Option<String> {
    match tool {
        "claude-code" if command_exists("claude") => Some("cli:claude".to_string()),
        "codex" if command_exists("codex") => Some("cli:codex".to_string()),
        "cursor" if command_exists("cursor") || command_exists("cursor-agent") => {
            Some("cli:cursor".to_string())
        }
        "gemini" if command_exists("gemini") => Some("cli:gemini".to_string()),
        _ => None,
    }
}

fn command_exists(binary: &str) -> bool {
    Command::new("sh")
        .arg("-lc")
        .arg(format!("command -v {binary} >/dev/null 2>&1"))
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn normalize_opt(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn is_excluded_path(path: &Path) -> bool {
    opensession_parsers::claude_code::is_claude_subagent_path(path)
}

fn candidate_display_line(candidate: &Candidate) -> String {
    let modified_local = DateTime::<Local>::from(candidate.modified).format("%Y-%m-%d %H:%M:%S");
    let age_minutes = SystemTime::now()
        .duration_since(candidate.modified)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs()
        / 60;
    format!(
        "[{}m ago | {}] {}",
        age_minutes,
        modified_local,
        candidate.path.display()
    )
}

fn current_summary_event_window() -> u32 {
    load_daemon_config_for_view()
        .map(|cfg| cfg.daemon.summary_event_window)
        .unwrap_or_else(|_| DaemonConfig::default().daemon.summary_event_window)
}

fn load_daemon_config_for_view() -> Result<DaemonConfig> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .context("home directory not found")?;
    let path = PathBuf::from(home)
        .join(".config")
        .join("opensession")
        .join("daemon.toml");
    if !path.exists() {
        return Ok(DaemonConfig::default());
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let config = toml::from_str::<DaemonConfig>(&content)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alias_maps_claude_to_tool_name() {
        assert_eq!(
            normalize_agent_alias("claude").expect("map alias"),
            "claude-code"
        );
        assert_eq!(
            normalize_agent_alias("claude-code").expect("map canonical"),
            "claude-code"
        );
    }

    #[test]
    fn unsupported_alias_returns_error() {
        let err = normalize_agent_alias("goose").expect_err("unsupported");
        assert!(format!("{err:#}").contains("unsupported agent"));
    }

    #[test]
    fn collect_candidates_ties_break_by_path() {
        let now = SystemTime::now();
        let mut candidates = vec![
            Candidate {
                path: PathBuf::from("/tmp/z-session.jsonl"),
                modified: now,
            },
            Candidate {
                path: PathBuf::from("/tmp/a-session.jsonl"),
                modified: now,
            },
            Candidate {
                path: PathBuf::from("/tmp/m-session.jsonl"),
                modified: now + Duration::from_secs(1),
            },
        ];

        sort_candidates(&mut candidates);

        assert_eq!(candidates[0].path, PathBuf::from("/tmp/m-session.jsonl"));
        assert_eq!(candidates[1].path, PathBuf::from("/tmp/a-session.jsonl"));
        assert_eq!(candidates[2].path, PathBuf::from("/tmp/z-session.jsonl"));
    }
}
