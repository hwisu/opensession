use crate::review::LOCAL_REVIEW_SERVER_BASE_URL;
use crate::user_guidance::guided_error;
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use opensession_api::{
    JobArtifactRef, JobManifest, JobProtocol, JobReviewKind, JobStage, JobStatus,
    apply_job_manifest,
};
use opensession_core::session::{ATTR_SOURCE_PATH, working_directory};
use opensession_core::trace::Session;
use opensession_core::validate::validate_session;
use opensession_local_db::{LocalDb, git::extract_git_context};
use opensession_local_store::store_local_object;
use opensession_parsers::{ParseError, ParserRegistry};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::path::PathBuf;

const ENV_CAPTURE_PROFILE: &str = "OPENSESSION_CAPTURE_PROFILE";
const ENV_CAPTURE_MANIFEST: &str = "OPENSESSION_CAPTURE_MANIFEST";
const ENV_CAPTURE_PROTOCOL: &str = "OPENSESSION_CAPTURE_PROTOCOL";
const ENV_CAPTURE_SYSTEM: &str = "OPENSESSION_CAPTURE_SYSTEM";
const ENV_CAPTURE_JOB_ID: &str = "OPENSESSION_CAPTURE_JOB_ID";
const ENV_CAPTURE_JOB_TITLE: &str = "OPENSESSION_CAPTURE_JOB_TITLE";
const ENV_CAPTURE_RUN_ID: &str = "OPENSESSION_CAPTURE_RUN_ID";
const ENV_CAPTURE_ATTEMPT: &str = "OPENSESSION_CAPTURE_ATTEMPT";
const ENV_CAPTURE_STAGE: &str = "OPENSESSION_CAPTURE_STAGE";
const ENV_CAPTURE_REVIEW_KIND: &str = "OPENSESSION_CAPTURE_REVIEW_KIND";
const ENV_CAPTURE_STATUS: &str = "OPENSESSION_CAPTURE_STATUS";
const ENV_CAPTURE_THREAD_ID: &str = "OPENSESSION_CAPTURE_THREAD_ID";
const ENV_CAPTURE_ARTIFACTS_JSON: &str = "OPENSESSION_CAPTURE_ARTIFACTS_JSON";

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
  opensession capture import --log ./.codex/sessions/rollout.jsonl
  opensession capture import --log ./rollout.jsonl --manifest ./job_manifest.json --out ./session.jsonl --json

Defaults:
  - parser profile auto-detects when --profile is omitted
  - job_manifest.json next to the log is auto-discovered when --manifest is omitted
  - if no manifest exists, job metadata is synthesized from env/defaults

Env overrides:
  OPENSESSION_CAPTURE_PROFILE
  OPENSESSION_CAPTURE_MANIFEST
  OPENSESSION_CAPTURE_PROTOCOL
  OPENSESSION_CAPTURE_SYSTEM
  OPENSESSION_CAPTURE_JOB_ID
  OPENSESSION_CAPTURE_JOB_TITLE
  OPENSESSION_CAPTURE_RUN_ID
  OPENSESSION_CAPTURE_ATTEMPT
  OPENSESSION_CAPTURE_STAGE
  OPENSESSION_CAPTURE_REVIEW_KIND
  OPENSESSION_CAPTURE_STATUS
  OPENSESSION_CAPTURE_THREAD_ID
  OPENSESSION_CAPTURE_ARTIFACTS_JSON")]
pub struct CaptureImportArgs {
    /// Parser profile id (`codex`, `claude-code`, `gemini`, ...). Auto-detected when omitted.
    #[arg(long)]
    pub profile: Option<String>,
    /// Native input log path.
    #[arg(long)]
    pub log: PathBuf,
    /// Job manifest path. Auto-discovers sidecars when omitted.
    #[arg(long)]
    pub manifest: Option<PathBuf>,
    /// Optional canonical session output path.
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
    parser_used: String,
    job_id: String,
    run_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    local_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hail_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    review_url: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct JobManifestPatch {
    #[serde(default)]
    protocol: Option<JobProtocol>,
    #[serde(default)]
    system: Option<String>,
    #[serde(default)]
    job_id: Option<String>,
    #[serde(default)]
    job_title: Option<String>,
    #[serde(default)]
    run_id: Option<String>,
    #[serde(default)]
    attempt: Option<i64>,
    #[serde(default)]
    stage: Option<JobStage>,
    #[serde(default)]
    review_kind: Option<JobReviewKind>,
    #[serde(default)]
    status: Option<JobStatus>,
    #[serde(default)]
    thread_id: Option<String>,
    #[serde(default)]
    artifacts: Option<Vec<JobArtifactRef>>,
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
    let filename = args
        .log
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("session");
    let parser_hint = args
        .profile
        .clone()
        .or_else(|| env_trimmed(ENV_CAPTURE_PROFILE));
    let preview = ParserRegistry::default()
        .preview_bytes(filename, &bytes, parser_hint.as_deref())
        .map_err(|err| match err {
            ParseError::InvalidParserHint { .. }
            | ParseError::ParserSelectionRequired { .. }
            | ParseError::ParseFailed { .. } => guided_error(
                format!("{err}"),
                [
                    "run `opensession capture import --help`".to_string(),
                    format!("retry with an explicit parser profile, e.g. `opensession capture import --profile codex --log {}`", args.log.display()),
                ],
            ),
        })?;

    let mut session = preview.session;
    let manifest = resolve_manifest(&args, &session)?;
    manifest.validate().map_err(|message| {
        guided_error(message, ["fix manifest/env values and retry".to_string()])
    })?;
    merge_capture_metadata(&mut session, &args.log, &manifest);
    session.recompute_stats();
    ensure_valid_session(&session)?;
    let canonical = session
        .to_jsonl()
        .context("serialize canonical session JSONL for capture import")?;

    if let Some(path) = args.out.as_ref() {
        std::fs::write(path, &canonical).map_err(|err| {
            guided_error(
                format!(
                    "failed to write canonical session JSONL `{}`: {err}",
                    path.display()
                ),
                [format!("check output path permissions: {}", path.display())],
            )
        })?;
    }

    let output = if args.no_register {
        build_output(
            &session,
            &preview.parser_used,
            &manifest,
            None,
            args.out.as_ref(),
        )
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
            &preview.parser_used,
            &manifest,
            Some(stored.uri.to_string()),
            args.out.as_ref(),
        )
    };

    print_output(&output, args.json)?;
    Ok(())
}

fn resolve_manifest(args: &CaptureImportArgs, session: &Session) -> Result<JobManifest> {
    let file_patch = load_manifest_patch(args)?;
    let env_patch = env_manifest_patch()?;
    let review_kind = env_patch.review_kind.or(file_patch.review_kind);
    let stage = env_patch
        .stage
        .or(file_patch.stage)
        .unwrap_or(match review_kind {
            Some(_) => JobStage::Review,
            None => JobStage::Execution,
        });
    let job_id = env_patch
        .job_id
        .or(file_patch.job_id)
        .unwrap_or_else(|| infer_job_id(&args.log, session));

    Ok(JobManifest {
        protocol: env_patch
            .protocol
            .or(file_patch.protocol)
            .unwrap_or(JobProtocol::Opensession),
        system: env_patch
            .system
            .or(file_patch.system)
            .unwrap_or_else(|| "opensession".to_string()),
        job_title: env_patch
            .job_title
            .or(file_patch.job_title)
            .unwrap_or_else(|| infer_job_title(&job_id)),
        run_id: env_patch
            .run_id
            .or(file_patch.run_id)
            .unwrap_or_else(|| session.session_id.clone()),
        attempt: env_patch.attempt.or(file_patch.attempt).unwrap_or(0),
        status: env_patch
            .status
            .or(file_patch.status)
            .unwrap_or_else(|| infer_status(stage, review_kind)),
        thread_id: env_patch.thread_id.or(file_patch.thread_id),
        artifacts: env_patch
            .artifacts
            .or(file_patch.artifacts)
            .unwrap_or_default(),
        stage,
        review_kind,
        job_id,
    })
}

fn load_manifest_patch(args: &CaptureImportArgs) -> Result<JobManifestPatch> {
    let manifest_path = args
        .manifest
        .clone()
        .or_else(|| env_trimmed(ENV_CAPTURE_MANIFEST).map(PathBuf::from))
        .or_else(|| discover_manifest_path(&args.log));

    let Some(path) = manifest_path else {
        return Ok(JobManifestPatch::default());
    };

    let manifest_bytes = std::fs::read(&path).map_err(|err| {
        guided_error(
            format!("failed to read manifest `{}`: {err}", path.display()),
            [
                format!("check file path and permissions: {}", path.display()),
                "run `opensession capture import --help`".to_string(),
            ],
        )
    })?;

    serde_json::from_slice(&manifest_bytes).map_err(|err| {
        guided_error(
            format!("failed to parse manifest `{}`: {err}", path.display()),
            ["ensure manifest is valid JSON and retry".to_string()],
        )
    })
}

fn env_manifest_patch() -> Result<JobManifestPatch> {
    Ok(JobManifestPatch {
        protocol: parse_env_enum(ENV_CAPTURE_PROTOCOL)?,
        system: env_trimmed(ENV_CAPTURE_SYSTEM),
        job_id: env_trimmed(ENV_CAPTURE_JOB_ID),
        job_title: env_trimmed(ENV_CAPTURE_JOB_TITLE),
        run_id: env_trimmed(ENV_CAPTURE_RUN_ID),
        attempt: parse_env_number(ENV_CAPTURE_ATTEMPT)?,
        stage: parse_env_enum(ENV_CAPTURE_STAGE)?,
        review_kind: parse_env_enum(ENV_CAPTURE_REVIEW_KIND)?,
        status: parse_env_enum(ENV_CAPTURE_STATUS)?,
        thread_id: env_trimmed(ENV_CAPTURE_THREAD_ID),
        artifacts: parse_env_json(ENV_CAPTURE_ARTIFACTS_JSON)?,
    })
}

fn env_trimmed(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_env_enum<T: DeserializeOwned>(name: &str) -> Result<Option<T>> {
    let Some(value) = env_trimmed(name) else {
        return Ok(None);
    };
    let raw = serde_json::Value::String(value.clone());
    serde_json::from_value(raw).map(Some).map_err(|err| {
        guided_error(
            format!("invalid {name}: {err}"),
            [format!(
                "set {name} to a supported snake_case value and retry"
            )],
        )
    })
}

fn parse_env_number(name: &str) -> Result<Option<i64>> {
    let Some(value) = env_trimmed(name) else {
        return Ok(None);
    };
    value.parse::<i64>().map(Some).map_err(|err| {
        guided_error(
            format!("invalid {name}: {err}"),
            [format!("set {name} to an integer and retry")],
        )
    })
}

fn parse_env_json<T: DeserializeOwned>(name: &str) -> Result<Option<T>> {
    let Some(value) = env_trimmed(name) else {
        return Ok(None);
    };
    serde_json::from_str(&value).map(Some).map_err(|err| {
        guided_error(
            format!("invalid {name}: {err}"),
            [format!("set {name} to valid JSON and retry")],
        )
    })
}

fn discover_manifest_path(log_path: &std::path::Path) -> Option<PathBuf> {
    let parent = log_path.parent()?;
    let stem = log_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("session");
    let file_name = log_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("session");
    [
        parent.join("job_manifest.json"),
        parent.join("opensession-job.json"),
        parent.join(format!("{stem}.job_manifest.json")),
        parent.join(format!("{stem}.manifest.json")),
        parent.join(format!("{stem}.job.json")),
        parent.join(format!("{file_name}.manifest.json")),
    ]
    .into_iter()
    .find(|path| path.exists())
}

fn infer_job_id(log_path: &std::path::Path, session: &Session) -> String {
    log_path
        .file_stem()
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| session.session_id.clone())
}

fn infer_job_title(job_id: &str) -> String {
    let humanized = job_id
        .split(['-', '_', '.'])
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if humanized.is_empty() {
        job_id.to_string()
    } else {
        humanized
    }
}

fn infer_status(stage: JobStage, review_kind: Option<JobReviewKind>) -> JobStatus {
    match (stage, review_kind) {
        (JobStage::Review, Some(JobReviewKind::Todo)) => JobStatus::Pending,
        (JobStage::Planning, _) => JobStatus::Pending,
        _ => JobStatus::Completed,
    }
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
    parser_used: &str,
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
        parser_used: parser_used.to_string(),
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
    println!("parser_used: {}", output.parser_used);
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
