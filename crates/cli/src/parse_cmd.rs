use crate::user_guidance::guided_error;
use anyhow::{Context, Result};
use clap::Args;
use opensession_core::validate::validate_session;
use opensession_parsers::{ParseError, ParserRegistry};
use std::path::PathBuf;

#[derive(Debug, Clone, Args)]
#[command(after_long_help = r"Recovery examples:
  opensession parse --profile codex ./raw-session.jsonl --preview
  opensession parse --profile codex ./raw-session.jsonl --out ./session.hail.jsonl")]
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
    let bytes = std::fs::read(&args.file).map_err(|err| {
        guided_error(
            format!("failed to read input file `{}`: {err}", args.file.display()),
            [
                format!("check file path and permissions: {}", args.file.display()),
                "run `opensession parse --help`".to_string(),
            ],
        )
    })?;
    let filename = args
        .file
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("session");

    let preview = ParserRegistry::default()
        .preview_bytes(filename, &bytes, Some(args.profile.as_str()))
        .map_err(|err| {
            match err {
                ParseError::InvalidParserHint { .. }
                | ParseError::ParserSelectionRequired { .. }
                | ParseError::ParseFailed { .. } => guided_error(
                    format!("{err}"),
                    [
                        "run `opensession parse --help`".to_string(),
                        format!(
                            "retry with an explicit profile, e.g. `opensession parse --profile codex {}`",
                            args.file.display()
                        ),
                    ],
                ),
            }
        })?;

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
            return Err(guided_error(
                format!("validation failed: {details}"),
                [
                    "run `opensession parse --profile <profile> <file> --preview` to inspect parser warnings".to_string(),
                    "fix source session content and retry parse".to_string(),
                ],
            ));
        }
    }

    let canonical = session
        .to_jsonl()
        .context("serialize canonical HAIL JSONL")?;
    if let Some(path) = args.out {
        std::fs::write(&path, canonical).map_err(|err| {
            guided_error(
                format!("failed to write parsed output `{}`: {err}", path.display()),
                [
                    format!("check output path permissions: {}", path.display()),
                    "retry without `--out` to print JSONL to stdout".to_string(),
                ],
            )
        })?;
    } else {
        print!("{canonical}");
    }

    Ok(())
}
