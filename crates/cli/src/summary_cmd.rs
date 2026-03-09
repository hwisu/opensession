use crate::runtime_settings::load_runtime_config;
use anyhow::{Context, Result, anyhow};
use clap::{Args, Subcommand};
use opensession_core::session::working_directory;
use opensession_git_native::extract_git_context;
use opensession_local_db::{LocalDb, SessionSemanticSummaryUpsert};
use opensession_local_store::find_repo_root;
use opensession_parsers::ParserRegistry;
use opensession_summary::{
    GitSummaryRequest, SemanticSummaryArtifact, summarize_git_commit, summarize_git_working_tree,
    summarize_session,
};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Args)]
pub struct SummaryArgs {
    #[command(subcommand)]
    pub action: SummaryAction,
}

#[derive(Debug, Clone, Subcommand)]
pub enum SummaryAction {
    /// Show previously generated local semantic summary by session id.
    Show {
        /// Session id.
        session_id: String,
    },
    /// Generate semantic summary from a session file or git target.
    Run(SummaryRunArgs),
}

#[derive(Debug, Clone, Args)]
pub struct SummaryRunArgs {
    /// Session source file (raw or HAIL JSONL).
    #[arg(long)]
    pub file: Option<PathBuf>,
    /// Repository root for git-based summary generation.
    #[arg(long)]
    pub repo: Option<PathBuf>,
    /// Commit SHA to summarize.
    #[arg(long)]
    pub commit: Option<String>,
    /// Summarize current working tree diff.
    #[arg(long)]
    pub working_tree: bool,
    /// Skip local DB persistence.
    #[arg(long)]
    pub no_store: bool,
}

#[derive(Debug, Serialize)]
struct SummaryShowPayload {
    session_id: String,
    generated_at: String,
    provider: String,
    model: Option<String>,
    source_kind: String,
    generation_kind: String,
    prompt_fingerprint: Option<String>,
    summary: serde_json::Value,
    source_details: Option<serde_json::Value>,
    diff_tree: Option<serde_json::Value>,
    error: Option<String>,
}

pub async fn run(args: SummaryArgs) -> Result<()> {
    match args.action {
        SummaryAction::Show { session_id } => run_show(&session_id),
        SummaryAction::Run(args) => run_generate(args).await,
    }
}

fn run_show(session_id: &str) -> Result<()> {
    let db = LocalDb::open().context("open local db")?;
    let Some(row) = db
        .get_session_semantic_summary(session_id)
        .context("query session summary")?
    else {
        return Err(anyhow!(
            "no semantic summary cached for session `{session_id}`"
        ));
    };

    let payload = SummaryShowPayload {
        session_id: row.session_id,
        generated_at: row.generated_at,
        provider: row.provider,
        model: row.model,
        source_kind: row.source_kind,
        generation_kind: row.generation_kind,
        prompt_fingerprint: row.prompt_fingerprint,
        summary: serde_json::from_str(&row.summary_json)
            .unwrap_or(serde_json::Value::String(row.summary_json)),
        source_details: row
            .source_details_json
            .as_deref()
            .and_then(|raw| serde_json::from_str(raw).ok()),
        diff_tree: row
            .diff_tree_json
            .as_deref()
            .and_then(|raw| serde_json::from_str(raw).ok()),
        error: row.error,
    };

    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

async fn run_generate(args: SummaryRunArgs) -> Result<()> {
    let runtime = load_runtime_config().context("load runtime config")?;
    let settings = &runtime.summary;
    let parser_registry = ParserRegistry::default();

    if let Some(file) = args.file.as_deref() {
        let artifact = run_from_file(file, settings).await?;
        println!("{}", serde_json::to_string_pretty(&artifact)?);

        if !args.no_store && settings.persists_to_local_db() {
            let session = parser_registry
                .parse_path(file)
                .with_context(|| format!("parse session file {}", file.display()))?
                .ok_or_else(|| anyhow!("unsupported session source format"))?;
            let db = LocalDb::open().context("open local db")?;
            store_artifact(&db, &session.session_id, &artifact)
                .context("persist summary artifact")?;
        }
        return Ok(());
    }

    let repo_root = resolve_repo_root(args.repo.as_deref())?;
    let artifact = if let Some(commit) = args.commit.as_deref() {
        summarize_git_commit(&repo_root, commit, settings)
            .await
            .map_err(anyhow::Error::msg)?
    } else if args.working_tree {
        summarize_git_working_tree(&repo_root, settings)
            .await
            .map_err(anyhow::Error::msg)?
    } else {
        return Err(anyhow!(
            "specify either `--file <path>` or git target (`--repo ... --commit <sha>` or `--working-tree`)"
        ));
    };

    println!("{}", serde_json::to_string_pretty(&artifact)?);
    Ok(())
}

async fn run_from_file(
    path: &Path,
    settings: &opensession_runtime_config::SummarySettings,
) -> Result<SemanticSummaryArtifact> {
    let session = ParserRegistry::default()
        .parse_path(path)
        .with_context(|| format!("parse session file {}", path.display()))?
        .ok_or_else(|| anyhow!("unsupported session source format"))?;

    let git_request = if settings.allows_git_changes_fallback() {
        working_directory(&session)
            .and_then(|cwd| find_repo_root(Path::new(cwd)))
            .map(|repo_root| GitSummaryRequest {
                repo_root,
                commit: working_directory(&session).and_then(|cwd| extract_git_context(cwd).commit),
            })
    } else {
        None
    };

    summarize_session(&session, settings, git_request.as_ref())
        .await
        .map_err(anyhow::Error::msg)
}

fn resolve_repo_root(repo: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = repo {
        let candidate = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .context("read current directory")?
                .join(path)
        };
        return opensession_git_native::ops::find_repo_root(&candidate)
            .ok_or_else(|| anyhow!("`{}` is not inside a git repository", candidate.display()));
    }

    let cwd = std::env::current_dir().context("read current directory")?;
    opensession_git_native::ops::find_repo_root(&cwd)
        .ok_or_else(|| anyhow!("current directory is not inside a git repository"))
}

fn store_artifact(
    db: &LocalDb,
    session_id: &str,
    artifact: &SemanticSummaryArtifact,
) -> Result<()> {
    let summary_json = serde_json::to_string(&artifact.summary)?;
    let source_details = if artifact.source_details.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&artifact.source_details)?)
    };
    let diff_tree = if artifact.diff_tree.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&artifact.diff_tree)?)
    };

    db.upsert_session_semantic_summary(&SessionSemanticSummaryUpsert {
        session_id,
        summary_json: &summary_json,
        generated_at: &chrono::Utc::now().to_rfc3339(),
        provider: &enum_label(&artifact.provider),
        model: (!artifact.model.trim().is_empty()).then_some(artifact.model.as_str()),
        source_kind: &enum_label(&artifact.source_kind),
        generation_kind: &enum_label(&artifact.generation_kind),
        prompt_fingerprint: (!artifact.prompt_fingerprint.trim().is_empty())
            .then_some(artifact.prompt_fingerprint.as_str()),
        source_details_json: source_details.as_deref(),
        diff_tree_json: diff_tree.as_deref(),
        error: artifact.error.as_deref(),
    })?;
    Ok(())
}

fn enum_label<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value)
        .ok()
        .map(|raw| raw.trim_matches('"').to_string())
        .filter(|raw| !raw.trim().is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}
