use anyhow::{bail, Context, Result};
use opensession_core::Session;
use opensession_local_db::LocalDb;
use opensession_parsers::discover::discover_sessions;
use opensession_parsers::{all_parsers, SessionParser};
use opensession_tui::{export_session_timeline, CliTimelineExportOptions, CliTimelineView};
use std::path::{Path, PathBuf};

use crate::session_ref::{tool_flag_to_name, SessionRef};

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum TuiOutputFormatArg {
    Text,
    Json,
    Jsonl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum TimelineViewArg {
    Linear,
    Turn,
}

impl From<TimelineViewArg> for CliTimelineView {
    fn from(value: TimelineViewArg) -> Self {
        match value {
            TimelineViewArg::Linear => CliTimelineView::Linear,
            TimelineViewArg::Turn => CliTimelineView::Turn,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run_tui_timeline(
    session_ref: &str,
    tool: Option<&str>,
    format: TuiOutputFormatArg,
    view: TimelineViewArg,
    no_collapse: bool,
    summaries: bool,
    no_summary: bool,
    summary_provider: Option<&str>,
    max_rows: Option<usize>,
) -> Result<()> {
    if summaries && no_summary {
        bail!("--summaries and --no-summary cannot be used together");
    }

    let session = resolve_session(session_ref, tool)?;

    let export = export_session_timeline(
        session,
        CliTimelineExportOptions {
            view: view.into(),
            collapse_consecutive: !no_collapse,
            include_summaries: !no_summary,
            generate_summaries: summaries && !no_summary,
            summary_provider_override: summary_provider.map(str::to_string),
            max_rows,
        },
    )?;

    match format {
        TuiOutputFormatArg::Text => print_text(export),
        TuiOutputFormatArg::Json => {
            println!("{}", serde_json::to_string_pretty(&export)?);
        }
        TuiOutputFormatArg::Jsonl => {
            println!(
                "{}",
                serde_json::to_string(&serde_json::json!({
                    "type": "meta",
                    "session_id": export.session_id,
                    "tool": export.tool,
                    "model": export.model,
                    "total_events": export.total_events,
                    "rendered_rows": export.rendered_rows,
                    "max_active_agents": export.max_active_agents,
                    "max_lane_index": export.max_lane_index,
                    "generated_summaries": export.generated_summaries
                }))?
            );
            for (index, line) in export.lines.iter().enumerate() {
                println!(
                    "{}",
                    serde_json::to_string(&serde_json::json!({
                        "type": "line",
                        "index": index,
                        "text": line
                    }))?
                );
            }
        }
    }

    Ok(())
}

fn print_text(export: opensession_tui::CliTimelineExport) {
    println!("session_id: {}", export.session_id);
    println!("tool/model: {} / {}", export.tool, export.model);
    println!(
        "events: {} | rows: {} | max_active_agents: {} | max_lane_index: {} | generated_summaries: {}",
        export.total_events,
        export.rendered_rows,
        export.max_active_agents,
        export.max_lane_index,
        export.generated_summaries
    );
    println!();
    for line in export.lines {
        println!("{line}");
    }
}

fn resolve_session(session_ref: &str, tool: Option<&str>) -> Result<Session> {
    let tool = tool.map(tool_flag_to_name);
    let parsed = SessionRef::parse(session_ref);
    let parsers = all_parsers();

    match parsed {
        SessionRef::File(path) => parse_file(&parsers, &path),
        SessionRef::Id(id) => {
            let db = LocalDb::open()?;
            let row = SessionRef::Id(id).resolve_one(&db, tool)?;
            let path = resolve_row_source_path(&db, &row.id, row.source_path.as_deref())
                .with_context(|| format!("could not resolve source path for session {}", row.id))?;
            parse_file(&parsers, &path)
        }
        SessionRef::Latest { count } => {
            let db = LocalDb::open()?;
            let fetch_count = count.max(20);
            let rows = if let Some(tool) = tool {
                db.get_sessions_by_tool_latest(tool, fetch_count)?
            } else {
                db.get_sessions_latest(fetch_count)?
            };
            parse_first_resolvable_row(&db, &parsers, rows.into_iter().collect())
        }
        SessionRef::Single { offset } => {
            let db = LocalDb::open()?;
            let mut rows = Vec::new();
            for delta in 0..50u32 {
                let row = if let Some(tool) = tool {
                    db.get_session_by_tool_offset(tool, offset + delta)?
                } else {
                    db.get_session_by_offset(offset + delta)?
                };
                if let Some(row) = row {
                    rows.push(row);
                } else {
                    break;
                }
            }
            parse_first_resolvable_row(&db, &parsers, rows)
        }
    }
}

fn parse_first_resolvable_row(
    db: &LocalDb,
    parsers: &[Box<dyn SessionParser>],
    rows: Vec<opensession_local_db::LocalSessionRow>,
) -> Result<Session> {
    let mut last_err = None;
    for row in rows {
        let path = match resolve_row_source_path(db, &row.id, row.source_path.as_deref()) {
            Ok(path) => path,
            Err(err) => {
                last_err = Some(format!("{}: {}", row.id, err));
                continue;
            }
        };
        match parse_file(parsers, &path) {
            Ok(session) => return Ok(session),
            Err(err) => {
                last_err = Some(format!("{}: {}", row.id, err));
            }
        }
    }

    if let Some(err) = last_err {
        bail!("no parseable local session file found: {err}");
    }
    bail!("no sessions found. run `opensession index` first")
}

fn resolve_row_source_path(
    db: &LocalDb,
    session_id: &str,
    row_source: Option<&str>,
) -> Result<PathBuf> {
    if let Some(path) = row_source {
        let p = PathBuf::from(path);
        if p.exists() {
            return Ok(p);
        }
    }

    if let Some(path) = db.get_session_source_path(session_id)? {
        let p = PathBuf::from(path);
        if p.exists() {
            return Ok(p);
        }
    }

    if let Some(path) = discover_path_by_session_id(session_id) {
        return Ok(path);
    }

    bail!("session file not found on disk")
}

fn discover_path_by_session_id(session_id: &str) -> Option<PathBuf> {
    for location in discover_sessions() {
        for path in location.paths {
            if is_session_match(&path, session_id) {
                return Some(path);
            }
        }
    }
    None
}

fn is_session_match(path: &Path, session_id: &str) -> bool {
    if path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| stem == session_id)
    {
        return true;
    }

    path.to_string_lossy()
        .to_ascii_lowercase()
        .contains(&session_id.to_ascii_lowercase())
}

fn parse_file(parsers: &[Box<dyn SessionParser>], path: &Path) -> Result<Session> {
    let parser = parsers
        .iter()
        .find(|p| p.can_parse(path))
        .with_context(|| format!("No parser found for {}", path.display()))?;
    parser
        .parse(path)
        .with_context(|| format!("Failed to parse {}", path.display()))
}
