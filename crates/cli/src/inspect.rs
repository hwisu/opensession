use crate::handoff_v1::{load_artifact_by_hash, resolve_artifact_hash};
use anyhow::{Context, Result};
use clap::Args;
use opensession_core::object_store::read_local_object_from_uri;
use opensession_core::source_uri::SourceUri;
use opensession_core::Session;

#[derive(Debug, Clone, Args)]
pub struct InspectArgs {
    /// Source or artifact URI.
    pub uri: String,
    /// Machine-readable JSON output.
    #[arg(long)]
    pub json: bool,
}

pub fn run(args: InspectArgs) -> Result<()> {
    let uri = SourceUri::parse(&args.uri)?;
    match uri {
        SourceUri::Src(_) => inspect_source(&uri, args.json),
        SourceUri::Artifact { .. } => inspect_artifact(&args.uri, args.json),
    }
}

fn inspect_source(uri: &SourceUri, json: bool) -> Result<()> {
    let cwd = std::env::current_dir().context("read current directory")?;
    let (path, bytes) = read_local_object_from_uri(uri, &cwd)?;
    let session = Session::from_jsonl(&String::from_utf8_lossy(&bytes))
        .context("parse local object as HAIL JSONL")?;
    let payload = serde_json::json!({
        "uri": uri.to_string(),
        "path": path,
        "session_id": session.session_id,
        "tool": session.agent.tool,
        "model": session.agent.model,
        "event_count": session.stats.event_count,
        "message_count": session.stats.message_count,
        "task_count": session.stats.task_count,
        "duration_seconds": session.stats.duration_seconds,
        "total_input_tokens": session.stats.total_input_tokens,
        "total_output_tokens": session.stats.total_output_tokens,
    });
    if json {
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("uri: {}", payload["uri"].as_str().unwrap_or_default());
        println!("path: {}", payload["path"].as_str().unwrap_or_default());
        println!(
            "session_id: {}",
            payload["session_id"].as_str().unwrap_or_default()
        );
        println!("tool: {}", payload["tool"].as_str().unwrap_or_default());
        println!("model: {}", payload["model"].as_str().unwrap_or_default());
        println!(
            "event_count: {}",
            payload["event_count"].as_u64().unwrap_or_default()
        );
        println!(
            "message_count: {}",
            payload["message_count"].as_u64().unwrap_or_default()
        );
        println!(
            "task_count: {}",
            payload["task_count"].as_u64().unwrap_or_default()
        );
        println!(
            "duration_seconds: {}",
            payload["duration_seconds"].as_u64().unwrap_or_default()
        );
        println!(
            "total_tokens: in={} out={}",
            payload["total_input_tokens"].as_u64().unwrap_or_default(),
            payload["total_output_tokens"].as_u64().unwrap_or_default()
        );
    }
    Ok(())
}

fn inspect_artifact(id_or_uri: &str, json: bool) -> Result<()> {
    let cwd = std::env::current_dir().context("read current directory")?;
    let hash = resolve_artifact_hash(id_or_uri, &cwd)?;
    let (path, record) = load_artifact_by_hash(&hash, &cwd)?;
    let payload = serde_json::json!({
        "uri": format!("os://artifact/{}", record.sha256),
        "path": path,
        "created_at": record.created_at,
        "source_count": record.source_uris.len(),
        "raw_session_count": record.raw_sessions.len(),
        "validation_report_count": record.validation_reports.len(),
    });
    if json {
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("uri: {}", payload["uri"].as_str().unwrap_or_default());
        println!("path: {}", payload["path"].as_str().unwrap_or_default());
        println!(
            "created_at: {}",
            payload["created_at"].as_str().unwrap_or_default()
        );
        println!(
            "source_count: {}",
            payload["source_count"].as_u64().unwrap_or_default()
        );
        println!(
            "raw_session_count: {}",
            payload["raw_session_count"].as_u64().unwrap_or_default()
        );
        println!(
            "validation_reports: {}",
            payload["validation_report_count"]
                .as_u64()
                .unwrap_or_default()
        );
    }
    Ok(())
}
