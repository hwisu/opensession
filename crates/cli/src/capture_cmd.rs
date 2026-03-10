use crate::review::LOCAL_REVIEW_SERVER_BASE_URL;
use crate::user_guidance::guided_error;
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use opensession_api::{JobManifest, apply_job_manifest};
use opensession_core::session::{ATTR_SOURCE_PATH, working_directory};
use opensession_core::trace::Session;
use opensession_core::validate::validate_session;
use opensession_local_db::{LocalDb, git::extract_git_context};
use opensession_local_store::store_local_object;
use opensession_parsers::{ParseError, ParserRegistry};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Args)]
pub struct CaptureArgs {
    #[command(subcommand)]
    pub action: CaptureAction,
}

#[derive(Debug, Clone, Subcommand)]
pub enum CaptureAction {
    /// Import a native agent log plus job manifest into local OpenSession storage.
    Import(CaptureImportArgs),
}

#[derive(Debug, Clone, Args)]
#[command(after_long_help = r"Examples:
  opensession capture import --profile codex --log ./.codex/sessions/rollout.jsonl --manifest ./job_manifest.json
  opensession capture import --profile codex --log ./rollout.jsonl --manifest ./job_manifest.json --out ./session.hail.jsonl --json")]
pub struct CaptureImportArgs {
    /// Parser profile id (`codex`, `claude-code`, `gemini`, ...).
    #[arg(long)]
    pub profile: String,
    /// Native input log path.
    #[arg(long)]
    pub log: PathBuf,
    /// Job manifest path.
    #[arg(long)]
    pub manifest: PathBuf,
    /// Optional canonical HAIL output path.
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// Skip local object-store registration and local DB upsert.
    #[arg(long)]
    pub no_register: bool,
    /// Print machine-readable JSON output.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Serialize)]
struct CaptureImportOutput {
    session_id: String,
    job_id: String,
    run_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    local_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hail_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    review_url: Option<String>,
}

pub fn run(args: CaptureArgs) -> Result<()> {
    match args.action {
        CaptureAction::Import(args) => run_import(args),
    }
}

fn run_import(args: CaptureImportArgs) -> Result<()> {
    let bytes = std::fs::read(&args.log).map_err(|err| {
        guided_error(
            format!("failed to read native log `{}`: {err}", args.log.display()),
            [
                format!("check file path and permissions: {}", args.log.display()),
                "run `opensession capture import --help`".to_string(),
            ],
        )
    })?;
    let manifest_bytes = std::fs::read(&args.manifest).map_err(|err| {
        guided_error(
            format!(
                "failed to read manifest `{}`: {err}",
                args.manifest.display()
            ),
            [
                format!(
                    "check file path and permissions: {}",
                    args.manifest.display()
                ),
                "run `opensession capture import --help`".to_string(),
            ],
        )
    })?;
    let manifest: JobManifest = serde_json::from_slice(&manifest_bytes).map_err(|err| {
        guided_error(
            format!(
                "failed to parse manifest `{}`: {err}",
                args.manifest.display()
            ),
            ["ensure manifest is valid JSON and retry".to_string()],
        )
    })?;
    manifest
        .validate()
        .map_err(|message| guided_error(message, ["fix manifest fields and retry".to_string()]))?;

    let filename = args
        .log
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("session");
    let preview = ParserRegistry::default()
        .preview_bytes(filename, &bytes, Some(args.profile.as_str()))
        .map_err(|err| match err {
            ParseError::InvalidParserHint { .. }
            | ParseError::ParserSelectionRequired { .. }
            | ParseError::ParseFailed { .. } => guided_error(
                format!("{err}"),
                [
                    "run `opensession capture import --help`".to_string(),
                    format!(
                        "retry with an explicit parser profile, e.g. `opensession capture import --profile codex --log {} --manifest {}`",
                        args.log.display(),
                        args.manifest.display()
                    ),
                ],
            ),
        })?;

    let mut session = preview.session;
    merge_capture_metadata(&mut session, &args.log, &manifest);
    session.recompute_stats();
    ensure_valid_session(&session)?;
    let canonical = session
        .to_jsonl()
        .context("serialize canonical HAIL JSONL for capture import")?;

    if let Some(path) = args.out.as_ref() {
        std::fs::write(path, &canonical).map_err(|err| {
            guided_error(
                format!("failed to write canonical HAIL `{}`: {err}", path.display()),
                [format!("check output path permissions: {}", path.display())],
            )
        })?;
    }

    let output = if args.no_register {
        build_output(&session, &manifest, None, args.out.as_ref())
    } else {
        let cwd = std::env::current_dir().context("read current directory")?;
        let stored = store_local_object(canonical.as_bytes(), &cwd)
            .map_err(|err| anyhow::anyhow!("store local object: {err}"))?;
        let git_cwd = working_directory(&session)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| cwd.display().to_string());
        let git = extract_git_context(&git_cwd);
        let db = LocalDb::open().context("open local db")?;
        db.upsert_local_session_with_storage_key(
            &session,
            &args.log.display().to_string(),
            &git,
            Some(&stored.uri.to_string()),
        )
        .context("upsert captured local session")?;
        build_output(
            &session,
            &manifest,
            Some(stored.uri.to_string()),
            args.out.as_ref(),
        )
    };

    print_output(&output, args.json)?;
    Ok(())
}

fn merge_capture_metadata(
    session: &mut Session,
    log_path: &std::path::Path,
    manifest: &JobManifest,
) {
    session.context.attributes.insert(
        ATTR_SOURCE_PATH.to_string(),
        serde_json::Value::String(log_path.display().to_string()),
    );
    apply_job_manifest(session, manifest);
}

fn ensure_valid_session(session: &Session) -> Result<()> {
    if let Err(errors) = validate_session(session) {
        let details = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(guided_error(
            format!("validation failed: {details}"),
            ["inspect parser output and manifest, then retry".to_string()],
        ));
    }
    Ok(())
}

fn build_output(
    session: &Session,
    manifest: &JobManifest,
    local_uri: Option<String>,
    out_path: Option<&PathBuf>,
) -> CaptureImportOutput {
    let review_url = manifest.review_kind.map(|kind| {
        format!(
            "{LOCAL_REVIEW_SERVER_BASE_URL}/review/job/{}?kind={}&run_id={}",
            urlencoding::encode(&manifest.job_id),
            kind,
            urlencoding::encode(&manifest.run_id)
        )
    });
    CaptureImportOutput {
        session_id: session.session_id.clone(),
        job_id: manifest.job_id.clone(),
        run_id: manifest.run_id.clone(),
        local_uri,
        hail_path: out_path.map(|path| path.display().to_string()),
        review_url,
    }
}

fn print_output(output: &CaptureImportOutput, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(output)?);
        return Ok(());
    }

    println!("session_id: {}", output.session_id);
    println!("job_id: {}", output.job_id);
    println!("run_id: {}", output.run_id);
    if let Some(local_uri) = output.local_uri.as_deref() {
        println!("local_uri: {local_uri}");
    }
    if let Some(hail_path) = output.hail_path.as_deref() {
        println!("hail_path: {hail_path}");
    }
    if let Some(review_url) = output.review_url.as_deref() {
        println!("review_url: {review_url}");
    }
    Ok(())
}
