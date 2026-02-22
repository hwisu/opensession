use anyhow::{Context, Result};
use clap::Args;
use opensession_core::validate::validate_session;
use opensession_parsers::ingest::{preview_parse_bytes, ParseError};
use std::path::PathBuf;

#[derive(Debug, Clone, Args)]
pub struct ParseArgs {
    /// Parser profile id (`hail`, `codex`, `claude-code`, `gemini`, ...).
    #[arg(long)]
    pub profile: String,
    /// Input file path.
    pub file: PathBuf,
    /// Print parser/warning preview to stderr.
    #[arg(long)]
    pub preview: bool,
    /// Validate parsed HAIL before output.
    #[arg(long)]
    pub validate: bool,
    /// Optional output file path (default stdout).
    #[arg(long)]
    pub out: Option<PathBuf>,
}

pub fn run(args: ParseArgs) -> Result<()> {
    let bytes = std::fs::read(&args.file)
        .with_context(|| format!("failed to read {}", args.file.display()))?;
    let filename = args
        .file
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("session");

    let preview = preview_parse_bytes(filename, &bytes, Some(args.profile.as_str())).map_err(
        |err| match err {
            ParseError::InvalidParserHint { .. }
            | ParseError::ParserSelectionRequired { .. }
            | ParseError::ParseFailed { .. } => anyhow::anyhow!("{err}"),
        },
    )?;

    if args.preview {
        eprintln!("parser_used: {}", preview.parser_used);
        if !preview.warnings.is_empty() {
            eprintln!("warnings:");
            for warning in preview.warnings {
                eprintln!("- {warning}");
            }
        }
    }

    let mut session = preview.session;
    session.recompute_stats();
    if args.validate {
        if let Err(errors) = validate_session(&session) {
            let details = errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ");
            return Err(anyhow::anyhow!("validation failed: {details}"));
        }
    }

    let canonical = session
        .to_jsonl()
        .context("serialize canonical HAIL JSONL")?;
    if let Some(path) = args.out {
        std::fs::write(&path, canonical).with_context(|| format!("write {}", path.display()))?;
    } else {
        print!("{canonical}");
    }

    Ok(())
}
