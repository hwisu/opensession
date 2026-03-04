use crate::runtime_settings::{
    apply_summary_profile, detect_local_summary_profile, load_runtime_config, save_runtime_config,
};
use crate::user_guidance::{guided_error, guided_error_with_doc};
use anyhow::{Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use opensession_runtime_config::{
    SessionDefaultView, SummaryOutputShape, SummaryPersistMode, SummaryProvider,
    SummaryResponseStyle, SummarySourceMode, SummaryTriggerMode,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub action: ConfigAction,
}

#[derive(Debug, Clone, Subcommand)]
pub enum ConfigAction {
    /// Initialize `.opensession/config.toml` in the current repo (or cwd).
    Init {
        /// Override default web base URL.
        #[arg(long)]
        base_url: Option<String>,
    },
    /// Show effective `.opensession/config.toml`.
    Show,
    /// Show/update runtime `opensession.toml`.
    Runtime {
        #[command(subcommand)]
        action: RuntimeConfigAction,
    },
    /// Show/update summary settings in runtime `opensession.toml`.
    Summary {
        #[command(subcommand)]
        action: SummaryConfigAction,
    },
}

#[derive(Debug, Clone, Subcommand)]
pub enum RuntimeConfigAction {
    /// Show runtime config.
    Show,
    /// Update runtime defaults.
    Set(RuntimeSetArgs),
}

#[derive(Debug, Clone, Args)]
pub struct RuntimeSetArgs {
    /// Global default session view (`full` or `compressed`).
    #[arg(long, value_enum)]
    pub session_default_view: Option<SessionDefaultViewArg>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum SummaryConfigAction {
    /// Show runtime summary settings.
    Show,
    /// Detect available local summary provider.
    Detect {
        /// Apply detected provider settings into runtime config.
        #[arg(long)]
        apply: bool,
    },
    /// Update runtime summary settings.
    Set(SummarySetArgs),
}

#[derive(Debug, Clone, Args)]
pub struct SummarySetArgs {
    #[arg(long, value_enum)]
    pub provider: Option<SummaryProviderArg>,
    #[arg(long)]
    pub endpoint: Option<String>,
    #[arg(long)]
    pub model: Option<String>,
    #[arg(long, value_enum)]
    pub source_mode: Option<SummarySourceModeArg>,
    #[arg(long, value_enum)]
    pub response_style: Option<SummaryResponseStyleArg>,
    #[arg(long, value_enum)]
    pub output_shape: Option<SummaryOutputShapeArg>,
    #[arg(long)]
    pub output_instruction: Option<String>,
    #[arg(long, value_enum)]
    pub trigger_mode: Option<SummaryTriggerModeArg>,
    #[arg(long, value_enum)]
    pub persist_mode: Option<SummaryPersistModeArg>,
    /// Add template slot (`key=value`). Repeat for multiple entries.
    #[arg(long = "template-slot")]
    pub template_slots: Vec<String>,
    /// Clear existing template slot mappings before applying `--template-slot`.
    #[arg(long)]
    pub clear_template_slots: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SessionDefaultViewArg {
    Full,
    Compressed,
}

impl From<SessionDefaultViewArg> for SessionDefaultView {
    fn from(value: SessionDefaultViewArg) -> Self {
        match value {
            SessionDefaultViewArg::Full => SessionDefaultView::Full,
            SessionDefaultViewArg::Compressed => SessionDefaultView::Compressed,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SummaryProviderArg {
    Disabled,
    Ollama,
    #[value(name = "codex_exec", alias = "codex-exec")]
    CodexExec,
    #[value(name = "claude_cli", alias = "claude-cli")]
    ClaudeCli,
}

impl From<SummaryProviderArg> for SummaryProvider {
    fn from(value: SummaryProviderArg) -> Self {
        match value {
            SummaryProviderArg::Disabled => SummaryProvider::Disabled,
            SummaryProviderArg::Ollama => SummaryProvider::Ollama,
            SummaryProviderArg::CodexExec => SummaryProvider::CodexExec,
            SummaryProviderArg::ClaudeCli => SummaryProvider::ClaudeCli,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SummarySourceModeArg {
    SessionOnly,
    SessionOrGitChanges,
}

impl From<SummarySourceModeArg> for SummarySourceMode {
    fn from(value: SummarySourceModeArg) -> Self {
        match value {
            SummarySourceModeArg::SessionOnly => SummarySourceMode::SessionOnly,
            SummarySourceModeArg::SessionOrGitChanges => SummarySourceMode::SessionOrGitChanges,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SummaryResponseStyleArg {
    Compact,
    Standard,
    Detailed,
}

impl From<SummaryResponseStyleArg> for SummaryResponseStyle {
    fn from(value: SummaryResponseStyleArg) -> Self {
        match value {
            SummaryResponseStyleArg::Compact => SummaryResponseStyle::Compact,
            SummaryResponseStyleArg::Standard => SummaryResponseStyle::Standard,
            SummaryResponseStyleArg::Detailed => SummaryResponseStyle::Detailed,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SummaryOutputShapeArg {
    Layered,
    FileList,
    SecurityFirst,
}

impl From<SummaryOutputShapeArg> for SummaryOutputShape {
    fn from(value: SummaryOutputShapeArg) -> Self {
        match value {
            SummaryOutputShapeArg::Layered => SummaryOutputShape::Layered,
            SummaryOutputShapeArg::FileList => SummaryOutputShape::FileList,
            SummaryOutputShapeArg::SecurityFirst => SummaryOutputShape::SecurityFirst,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SummaryTriggerModeArg {
    Manual,
    OnSessionSave,
}

impl From<SummaryTriggerModeArg> for SummaryTriggerMode {
    fn from(value: SummaryTriggerModeArg) -> Self {
        match value {
            SummaryTriggerModeArg::Manual => SummaryTriggerMode::Manual,
            SummaryTriggerModeArg::OnSessionSave => SummaryTriggerMode::OnSessionSave,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SummaryPersistModeArg {
    None,
    LocalDb,
}

impl From<SummaryPersistModeArg> for SummaryPersistMode {
    fn from(value: SummaryPersistModeArg) -> Self {
        match value {
            SummaryPersistModeArg::None => SummaryPersistMode::None,
            SummaryPersistModeArg::LocalDb => SummaryPersistMode::LocalDb,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoConfig {
    pub share: ShareConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareConfig {
    pub base_url: String,
}

impl Default for RepoConfig {
    fn default() -> Self {
        Self {
            share: ShareConfig {
                base_url: "https://opensession.io".to_string(),
            },
        }
    }
}

pub fn run(args: ConfigArgs) -> Result<()> {
    match args.action {
        ConfigAction::Init { base_url } => run_init(base_url),
        ConfigAction::Show => run_show(),
        ConfigAction::Runtime { action } => run_runtime(action),
        ConfigAction::Summary { action } => run_summary(action),
    }
}

pub fn load_repo_config(cwd: &Path) -> Result<(PathBuf, RepoConfig)> {
    let path = config_path(cwd)?;
    if !path.exists() {
        return Err(guided_error_with_doc(
            format!("missing config: {}", path.display()),
            [
                "initialize config: `opensession config init --base-url https://opensession.io`"
                    .to_string(),
                "verify config: `opensession config show`".to_string(),
            ],
            "README.md#Share",
        ));
    }

    let raw = std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let parsed: RepoConfig =
        toml::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
    if parsed.share.base_url.trim().is_empty() {
        return Err(guided_error(
            format!(
                "invalid config: share.base_url is empty ({})",
                path.display()
            ),
            [
                "set a valid base URL: `opensession config init --base-url https://opensession.io`"
                    .to_string(),
                "inspect file content and retry: `opensession config show`".to_string(),
            ],
        ));
    }
    Ok((path, parsed))
}

fn run_init(base_url: Option<String>) -> Result<()> {
    let cwd = std::env::current_dir().context("read current directory")?;
    let path = config_path(&cwd)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }

    let mut cfg = if path.exists() {
        let (_, loaded) = load_repo_config(&cwd)?;
        loaded
    } else {
        RepoConfig::default()
    };

    if let Some(url) = base_url {
        cfg.share.base_url = normalize_base_url(&url)?;
    } else {
        cfg.share.base_url = normalize_base_url(&cfg.share.base_url)?;
    }

    let body = toml::to_string_pretty(&cfg).context("serialize config")?;
    std::fs::write(&path, body).with_context(|| format!("write {}", path.display()))?;

    println!("config: {}", path.display());
    println!("base_url: {}", cfg.share.base_url);
    Ok(())
}

fn run_show() -> Result<()> {
    let cwd = std::env::current_dir().context("read current directory")?;
    let (path, cfg) = load_repo_config(&cwd)?;
    println!("config: {}", path.display());
    println!("base_url: {}", cfg.share.base_url);
    Ok(())
}

fn run_runtime(action: RuntimeConfigAction) -> Result<()> {
    match action {
        RuntimeConfigAction::Show => {
            let cfg = load_runtime_config()?;
            println!("{}", toml::to_string_pretty(&cfg)?);
            Ok(())
        }
        RuntimeConfigAction::Set(args) => {
            let mut cfg = load_runtime_config()?;
            if let Some(view) = args.session_default_view {
                cfg.daemon.session_default_view = view.into();
            }
            let path = save_runtime_config(&cfg)?;
            println!("runtime_config: {}", path.display());
            println!(
                "session_default_view: {:?}",
                cfg.daemon.session_default_view
            );
            Ok(())
        }
    }
}

fn run_summary(action: SummaryConfigAction) -> Result<()> {
    match action {
        SummaryConfigAction::Show => {
            let cfg = load_runtime_config()?;
            println!("{}", toml::to_string_pretty(&cfg.summary)?);
            Ok(())
        }
        SummaryConfigAction::Detect { apply } => {
            let profile = detect_local_summary_profile();
            if let Some(profile) = &profile {
                println!(
                    "detected: provider={:?} model={} endpoint={}",
                    profile.provider, profile.model, profile.endpoint
                );
            } else {
                println!("detected: none");
            }

            if apply {
                let mut cfg = load_runtime_config()?;
                if let Some(profile) = profile {
                    apply_summary_profile(&mut cfg, &profile);
                    let path = save_runtime_config(&cfg)?;
                    println!("applied: {}", path.display());
                } else {
                    cfg.summary.provider = SummaryProvider::Disabled;
                    let path = save_runtime_config(&cfg)?;
                    println!("applied: {} (summary.provider=disabled)", path.display());
                }
            }
            Ok(())
        }
        SummaryConfigAction::Set(args) => {
            let mut cfg = load_runtime_config()?;
            if let Some(provider) = args.provider {
                cfg.summary.provider = provider.into();
            }
            if let Some(endpoint) = args.endpoint {
                cfg.summary.endpoint = endpoint;
            }
            if let Some(model) = args.model {
                cfg.summary.model = model;
            }
            if let Some(source_mode) = args.source_mode {
                cfg.summary.source_mode = source_mode.into();
            }
            if let Some(response_style) = args.response_style {
                cfg.summary.response_style = response_style.into();
            }
            if let Some(output_shape) = args.output_shape {
                cfg.summary.output_shape = output_shape.into();
            }
            if let Some(output_instruction) = args.output_instruction {
                cfg.summary.output_instruction = output_instruction;
            }
            if let Some(trigger_mode) = args.trigger_mode {
                cfg.summary.trigger_mode = trigger_mode.into();
            }
            if let Some(persist_mode) = args.persist_mode {
                cfg.summary.persist_mode = persist_mode.into();
            }

            if args.clear_template_slots {
                cfg.summary.template_slots.clear();
            }
            for raw in args.template_slots {
                let (key, value) = raw.split_once('=').ok_or_else(|| {
                    guided_error(
                        format!("invalid --template-slot `{raw}`"),
                        ["expected format: --template-slot key=value"],
                    )
                })?;
                let key = key.trim();
                let value = value.trim();
                if key.is_empty() {
                    return Err(guided_error(
                        format!("invalid --template-slot `{raw}`"),
                        ["slot key must be non-empty"],
                    ));
                }
                cfg.summary
                    .template_slots
                    .insert(key.to_string(), value.to_string());
            }

            let path = save_runtime_config(&cfg)?;
            println!("runtime_config: {}", path.display());
            println!("{}", toml::to_string_pretty(&cfg.summary)?);
            Ok(())
        }
    }
}

fn config_path(cwd: &Path) -> Result<PathBuf> {
    let root =
        opensession_core::object_store::find_repo_root(cwd).unwrap_or_else(|| cwd.to_path_buf());
    Ok(root.join(".opensession").join("config.toml"))
}

fn normalize_base_url(value: &str) -> Result<String> {
    let trimmed = value.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(guided_error(
            "base_url cannot be empty",
            [
                "use an explicit URL, e.g. `opensession config init --base-url https://opensession.io`",
            ],
        ));
    }
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        return Err(guided_error(
            "base_url must start with http:// or https://",
            ["example: `opensession config init --base-url https://opensession.io`"],
        ));
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::{normalize_base_url, RepoConfig};

    #[test]
    fn default_repo_config_has_base_url() {
        assert_eq!(
            RepoConfig::default().share.base_url,
            "https://opensession.io".to_string()
        );
    }

    #[test]
    fn normalize_base_url_strips_trailing_slash() {
        assert_eq!(
            normalize_base_url("https://opensession.io/").expect("normalize"),
            "https://opensession.io".to_string()
        );
    }
}
