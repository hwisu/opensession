use crate::user_guidance::{guided_error, guided_error_with_doc};
use anyhow::{Context, Result, bail};
use clap::Args;
use opensession_core::Session;
use opensession_core::object_store::store_local_object;
use std::path::PathBuf;

#[derive(Debug, Clone, Args)]
pub struct RegisterArgs {
    /// Canonical HAIL JSONL file path.
    pub file: PathBuf,
    /// Print only URI.
    #[arg(long)]
    pub quiet: bool,
    /// Print machine-readable JSON output.
    #[arg(long)]
    pub json: bool,
}

pub fn run(args: RegisterArgs) -> Result<()> {
    let raw = std::fs::read_to_string(&args.file).map_err(|err| {
        guided_error(
            format!("failed to read input file `{}`: {err}", args.file.display()),
            [
                format!("check file path and permissions: {}", args.file.display()),
                "run `opensession register --help`".to_string(),
            ],
        )
    })?;

    let mut session = Session::from_jsonl(&raw).map_err(|err| {
        guided_error_with_doc(
            format!("register expects canonical HAIL JSONL: {err}"),
            [
                format!(
                    "convert source logs first: `opensession parse --profile codex {} --out ./session.hail.jsonl`",
                    args.file.display()
                ),
                "retry register with the canonical file: `opensession register ./session.hail.jsonl`"
                    .to_string(),
            ],
            "docs.md#Getting Started",
        )
    })?;
    session.recompute_stats();
    let canonical = session
        .to_jsonl()
        .context("serialize canonical HAIL JSONL")?;

    let cwd = std::env::current_dir().context("read current directory")?;
    let stored = store_local_object(canonical.as_bytes(), &cwd)?;
    let uri = stored.uri.to_string();

    if args.json {
        let payload = serde_json::json!({
            "uri": uri,
            "hash": stored.sha256,
            "store_path": stored.path,
            "bytes": stored.bytes,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    if args.quiet {
        println!("{uri}");
        return Ok(());
    }

    if stored.bytes == 0 {
        bail!("stored object is empty");
    }
    println!("{uri}");
    println!("stored_at: {}", stored.path.display());
    println!("bytes: {}", stored.bytes);
    Ok(())
}
