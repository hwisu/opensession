use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use opensession_core::handoff::{
    generate_handoff_markdown_v2, generate_merged_handoff_markdown_v2, merge_summaries,
    validate_handoff_summaries, HandoffSummary, HandoffValidationReport,
};
use opensession_core::handoff_artifact::{
    sort_sessions_time_asc, source_from_session, HandoffArtifact, HandoffPayloadFormat,
    HANDOFF_ARTIFACT_VERSION, HANDOFF_MERGE_POLICY_TIME_ASC,
};
use opensession_core::Session;
use opensession_git_native::{
    artifact_ref_name, list_handoff_artifact_refs, load_handoff_artifact, ops,
    store_handoff_artifact,
};
use opensession_parsers::discover::discover_sessions;
use opensession_parsers::{all_parsers, SessionParser};
use std::io::{IsTerminal, Write};

#[derive(Debug, Clone)]
struct ResolvedSession {
    session: Session,
    source_path: Option<PathBuf>,
}

/// Run the handoff command: parse session file(s) and output a structured summary.
#[allow(clippy::too_many_arguments)]
pub async fn run_handoff(
    files: &[PathBuf],
    last: Option<&str>,
    output: Option<&Path>,
    format: crate::output::OutputFormat,
    claude: Option<&str>,
    gemini: Option<&str>,
    tool_refs: &[String],
    validate: bool,
    strict: bool,
    populate: Option<&str>,
) -> Result<()> {
    let sessions = resolve_sessions(files, last, claude, gemini, tool_refs)?;

    if sessions.is_empty() {
        bail!("No sessions to process.");
    }

    // Use the unified output module
    let output_format = format;

    let mut result = Vec::new();
    let validation_enabled = validate || strict;
    if validation_enabled {
        let summaries: Vec<HandoffSummary> =
            sessions.iter().map(HandoffSummary::from_session).collect();
        let reports = validate_handoff_summaries(&summaries);
        print_validation_reports(&reports)?;
        let has_errors = has_error_findings(&reports);
        if strict && has_errors {
            bail!("Handoff validation failed in strict mode (error-level findings).");
        }
        crate::output::render_output_with_options(
            &sessions,
            &output_format,
            &mut result,
            &crate::output::RenderOptions {
                validation_reports: Some(&reports),
            },
        )?;
    } else {
        crate::output::render_output(&sessions, &output_format, &mut result)?;
    }
    let result_str = String::from_utf8(result)?;

    let final_result = result_str;

    if let Some(out) = output {
        std::fs::write(out, &final_result)
            .with_context(|| format!("Failed to write {}", out.display()))?;
        eprintln!("Handoff written to {}", out.display());
    }

    if let Some(spec) = populate {
        run_populate(spec, &final_result)?;
        return Ok(());
    }

    if output.is_none() {
        let mut stdout = std::io::stdout();
        write_handoff_to_writer(&mut stdout, &final_result)?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn run_handoff_save(
    files: &[PathBuf],
    last: Option<&str>,
    claude: Option<&str>,
    gemini: Option<&str>,
    tool_refs: &[String],
    artifact_id: Option<&str>,
    payload_format: HandoffPayloadFormat,
) -> Result<()> {
    let resolved = resolve_sessions_with_sources(files, last, claude, gemini, tool_refs)?;
    if resolved.is_empty() {
        bail!("No sessions to process.");
    }

    let artifact =
        build_artifact_from_resolved(artifact_id, payload_format, resolved, chrono::Utc::now())?;

    let repo_root = resolve_repo_root()?;
    let artifact_json = serde_json::to_vec_pretty(&artifact)?;
    let ref_name = store_handoff_artifact(&repo_root, &artifact.artifact_id, &artifact_json)?;

    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "artifact_id": artifact.artifact_id,
            "ref": ref_name,
            "source_count": artifact.sources.len(),
            "stale": artifact.is_stale(),
        }))?
    );
    Ok(())
}

pub async fn run_handoff_artifact_list() -> Result<()> {
    let repo_root = resolve_repo_root()?;
    let refs = list_handoff_artifact_refs(&repo_root)?;
    let mut rows = Vec::new();
    for ref_name in refs {
        let artifact = load_artifact(&repo_root, &ref_name)?;
        rows.push(serde_json::json!({
            "artifact_id": artifact.artifact_id,
            "ref": ref_name,
            "created_at": artifact.created_at,
            "payload_format": artifact.payload_format,
            "source_count": artifact.sources.len(),
            "stale": artifact.is_stale(),
        }));
    }
    println!("{}", serde_json::to_string_pretty(&rows)?);
    Ok(())
}

pub async fn run_handoff_artifact_show(id_or_ref: &str) -> Result<()> {
    let repo_root = resolve_repo_root()?;
    let artifact = load_artifact(&repo_root, id_or_ref)?;
    let stale_reasons = artifact.stale_reasons();
    let output = serde_json::json!({
        "artifact": artifact,
        "stale": !stale_reasons.is_empty(),
        "stale_reasons": stale_reasons,
    });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

pub async fn run_handoff_artifact_refresh(id_or_ref: &str) -> Result<()> {
    let repo_root = resolve_repo_root()?;
    let mut artifact = load_artifact(&repo_root, id_or_ref)?;
    if artifact.sources.is_empty() {
        bail!("Artifact has no source files to refresh.");
    }

    let stale_reasons = artifact.stale_reasons();
    if stale_reasons.is_empty() {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "artifact_id": artifact.artifact_id,
                "ref": artifact_ref_name(&artifact.artifact_id),
                "refreshed": false,
                "stale": false,
            }))?
        );
        return Ok(());
    }

    let mut resolved = Vec::new();
    let parsers = all_parsers();
    for source in &artifact.sources {
        let path = PathBuf::from(&source.source_path);
        if !path.exists() {
            continue;
        }
        let session = parse_file(&parsers, &path)?;
        resolved.push(ResolvedSession {
            session,
            source_path: Some(path),
        });
    }
    if resolved.is_empty() {
        bail!("Unable to refresh artifact: all source files are missing.");
    }

    let existing_id = artifact.artifact_id.clone();
    let existing_created_at = artifact.created_at;
    let existing_payload_format = artifact.payload_format;
    artifact = build_artifact_from_resolved(
        Some(&existing_id),
        existing_payload_format,
        resolved,
        existing_created_at,
    )?;

    let artifact_json = serde_json::to_vec_pretty(&artifact)?;
    let ref_name = store_handoff_artifact(&repo_root, &artifact.artifact_id, &artifact_json)?;

    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "artifact_id": artifact.artifact_id,
            "ref": ref_name,
            "refreshed": true,
            "stale_before": stale_reasons.len(),
            "stale_after": artifact.is_stale(),
        }))?
    );
    Ok(())
}

pub async fn run_handoff_artifact_render_md(id_or_ref: &str, output: Option<&Path>) -> Result<()> {
    let repo_root = resolve_repo_root()?;
    let artifact = load_artifact(&repo_root, id_or_ref)?;
    let markdown = artifact
        .derived_markdown
        .as_deref()
        .map(ToOwned::to_owned)
        .with_context(|| {
            format!(
                "Artifact `{}` has no derived_markdown",
                artifact.artifact_id
            )
        })?;

    let path = output
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("HANDOFF.md"));
    std::fs::write(&path, markdown)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    eprintln!("Rendered {}", path.display());
    Ok(())
}

fn resolve_repo_root() -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("Failed to read current directory")?;
    let root = ops::find_repo_root(&cwd).with_context(|| {
        format!(
            "Current path {} is not inside a git repository",
            cwd.display()
        )
    })?;
    Ok(root)
}

fn load_artifact(repo_root: &Path, id_or_ref: &str) -> Result<HandoffArtifact> {
    let bytes = load_handoff_artifact(repo_root, id_or_ref)?;
    let artifact: HandoffArtifact = serde_json::from_slice(&bytes)?;
    Ok(artifact)
}

fn build_artifact_from_resolved(
    artifact_id: Option<&str>,
    payload_format: HandoffPayloadFormat,
    mut resolved: Vec<ResolvedSession>,
    created_at: chrono::DateTime<chrono::Utc>,
) -> Result<HandoffArtifact> {
    resolved.sort_by(|left, right| {
        opensession_core::handoff_artifact::merge_time_order(
            left.session.context.created_at,
            &left.session.session_id,
            right.session.context.created_at,
            &right.session.session_id,
        )
    });

    let mut sessions = resolved
        .iter()
        .map(|item| item.session.clone())
        .collect::<Vec<_>>();
    sort_sessions_time_asc(&mut sessions);

    let summaries: Vec<HandoffSummary> =
        sessions.iter().map(HandoffSummary::from_session).collect();
    let payload_values = summaries
        .iter()
        .map(|summary| crate::output::summary_to_json_v2(summary, None))
        .collect::<Vec<_>>();

    let merged = merge_summaries(&summaries);
    let derived_markdown = match summaries.len() {
        0 => None,
        1 => Some(generate_handoff_markdown_v2(&summaries[0])),
        _ => Some(generate_merged_handoff_markdown_v2(&merged)),
    };

    let mut sources = Vec::new();
    for item in &resolved {
        let path = item
            .source_path
            .clone()
            .or_else(|| source_path_from_session(&item.session));
        let Some(path) = path else {
            continue;
        };
        if !path.exists() {
            continue;
        }
        if let Ok(source) = source_from_session(&item.session, &path) {
            sources.push(source);
        }
    }

    let artifact_id = artifact_id
        .map(normalize_artifact_id)
        .filter(|id| !id.is_empty())
        .unwrap_or_else(default_artifact_id);

    Ok(HandoffArtifact {
        version: HANDOFF_ARTIFACT_VERSION.to_string(),
        artifact_id,
        created_at,
        merge_policy: HANDOFF_MERGE_POLICY_TIME_ASC.to_string(),
        sources,
        payload_format,
        payload: serde_json::Value::Array(payload_values),
        derived_markdown,
    })
}

fn default_artifact_id() -> String {
    format!("artifact-{}", chrono::Utc::now().format("%Y%m%dT%H%M%S%3f"))
}

fn normalize_artifact_id(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.trim().chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

fn source_path_from_session(session: &Session) -> Option<PathBuf> {
    for key in ["source_path", "source_file", "session_path", "path"] {
        let path = session
            .context
            .attributes
            .get(key)
            .and_then(|value| value.as_str())
            .map(PathBuf::from);
        if let Some(path) = path {
            if path.exists() {
                return Some(path);
            }
        }
    }
    None
}

fn write_handoff_to_writer(writer: &mut dyn Write, payload: &str) -> Result<()> {
    if let Err(err) = writer.write_all(payload.as_bytes()) {
        if err.kind() == std::io::ErrorKind::BrokenPipe {
            return Ok(());
        }
        return Err(err).context("Failed to write handoff output to stdout");
    }
    Ok(())
}

fn has_error_findings(reports: &[HandoffValidationReport]) -> bool {
    reports.iter().any(|report| {
        report
            .findings
            .iter()
            .any(|finding| finding.severity.eq_ignore_ascii_case("error"))
    })
}

fn print_validation_reports(reports: &[HandoffValidationReport]) -> Result<()> {
    let passed = reports.iter().filter(|report| report.passed).count();
    eprintln!("Handoff validation: {passed}/{} passed", reports.len());

    for report in reports {
        if report.findings.is_empty() {
            continue;
        }
        eprintln!(
            "- [{}] {} finding(s)",
            report.session_id,
            report.findings.len()
        );
        for finding in &report.findings {
            eprintln!(
                "  - [{}] {}: {}",
                finding.severity, finding.code, finding.message
            );
        }
    }

    let machine = serde_json::json!({
        "version": "0.1",
        "type": "handoff_validation",
        "reports": reports,
    });
    eprintln!("{}", serde_json::to_string(&machine)?);
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PopulateProvider {
    Claude,
    Codex,
    OpenCode,
    Gemini,
    Amp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PopulateSpec {
    provider: PopulateProvider,
    model: Option<String>,
}

impl PopulateSpec {
    fn parse(raw: &str) -> Result<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            bail!("--populate requires a provider name, e.g. --populate claude");
        }
        let mut parts = trimmed.splitn(2, ':');
        let provider_raw = parts.next().unwrap_or_default().trim().to_ascii_lowercase();
        let model = parts
            .next()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);

        let provider = match provider_raw.as_str() {
            "claude" => PopulateProvider::Claude,
            "codex" => PopulateProvider::Codex,
            "opencode" => PopulateProvider::OpenCode,
            "gemini" => PopulateProvider::Gemini,
            "amp" => PopulateProvider::Amp,
            _ => bail!(
                "Unsupported --populate provider: {provider_raw}. Supported: claude, codex, opencode, gemini, amp"
            ),
        };

        Ok(Self { provider, model })
    }
}

fn run_populate(raw_spec: &str, handoff_payload: &str) -> Result<()> {
    let spec = PopulateSpec::parse(raw_spec)?;
    let prompt = populate_prompt(&spec);

    let (bin, mut args): (&str, Vec<String>) = match spec.provider {
        PopulateProvider::Claude => ("claude", vec!["-c".to_string()]),
        PopulateProvider::Codex => ("codex", vec!["exec".to_string()]),
        PopulateProvider::OpenCode => ("opencode", vec!["run".to_string()]),
        PopulateProvider::Gemini => ("gemini", vec!["-p".to_string()]),
        PopulateProvider::Amp => ("amp", vec!["-x".to_string()]),
    };
    args.push(prompt);

    let mut child = Command::new(bin)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("Failed to start populate command `{bin}`"))?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(handoff_payload.as_bytes())
            .context("Failed to pipe handoff payload into populate command stdin")?;
    }

    let status = child.wait()?;
    if !status.success() {
        bail!("Populate command `{bin}` exited with status {status}");
    }
    Ok(())
}

fn populate_prompt(spec: &PopulateSpec) -> String {
    let model_hint = spec
        .model
        .as_deref()
        .map(|model| format!("Model preference: `{model}` if supported by this CLI. "))
        .unwrap_or_default();
    format!(
        "{model_hint}Please populate `HANDOFF.md` from the JSON payload on stdin. Use any `*_undefined_reason` or `*_missing_reason` hints to fill missing sections. Keep unresolved items explicit."
    )
}

/// Resolve session files: explicit paths, --last, or tool refs.
fn resolve_sessions(
    files: &[PathBuf],
    last: Option<&str>,
    claude: Option<&str>,
    gemini: Option<&str>,
    tool_refs: &[String],
) -> Result<Vec<Session>> {
    Ok(
        resolve_sessions_with_sources(files, last, claude, gemini, tool_refs)?
            .into_iter()
            .map(|resolved| resolved.session)
            .collect(),
    )
}

fn resolve_sessions_with_sources(
    files: &[PathBuf],
    last: Option<&str>,
    claude: Option<&str>,
    gemini: Option<&str>,
    tool_refs: &[String],
) -> Result<Vec<ResolvedSession>> {
    use crate::session_ref::{tool_flag_to_name, SessionRef};

    let parsers = all_parsers();

    // If explicit files are given, parse them all
    if !files.is_empty() {
        let mut sessions = Vec::new();
        for file in files {
            if !file.exists() {
                bail!("File not found: {}", file.display());
            }
            let session = parse_file(&parsers, file)?;
            sessions.push(ResolvedSession {
                session,
                source_path: Some(file.clone()),
            });
        }
        return Ok(sessions);
    }

    // Collect tool-specific session refs
    let mut ref_pairs: Vec<(Option<&str>, SessionRef)> = Vec::new();

    if let Some(r) = claude {
        ref_pairs.push((Some("claude-code"), SessionRef::parse(r)));
    }
    if let Some(r) = gemini {
        ref_pairs.push((Some("gemini"), SessionRef::parse(r)));
    }
    for tool_ref_str in tool_refs {
        // Format: "tool_name ref" e.g. "amp HEAD~2"
        let parts: Vec<&str> = tool_ref_str.splitn(2, ' ').collect();
        if parts.len() == 2 {
            let tool_name = tool_flag_to_name(parts[0]);
            ref_pairs.push((Some(tool_name), SessionRef::parse(parts[1])));
        } else {
            ref_pairs.push((None, SessionRef::parse(parts[0])));
        }
    }

    // If we have session refs, resolve them via the index DB
    if !ref_pairs.is_empty() {
        let db = opensession_local_db::LocalDb::open()?;
        let mut sessions = Vec::new();
        for (tool, sref) in &ref_pairs {
            match sref {
                SessionRef::File(path) => {
                    sessions.push(ResolvedSession {
                        session: parse_file(&parsers, path)?,
                        source_path: Some(path.clone()),
                    });
                }
                _ => {
                    let rows = sref.resolve(&db, *tool)?;
                    for row in &rows {
                        let source = row
                            .source_path
                            .as_deref()
                            .with_context(|| format!("Session {} has no source_path", row.id))?;
                        let path = PathBuf::from(source);
                        sessions.push(ResolvedSession {
                            session: parse_file(&parsers, &path)?,
                            source_path: Some(path),
                        });
                    }
                }
            }
        }
        return Ok(sessions);
    }

    // Otherwise resolve file(s) via --last or interactive
    let last_count = parse_last_count(last)?;
    let resolved = resolve_session_files(last_count)?;
    let mut sessions = Vec::new();
    for path in resolved {
        sessions.push(ResolvedSession {
            session: parse_file(&parsers, &path)?,
            source_path: Some(path),
        });
    }
    Ok(sessions)
}

fn parse_file(parsers: &[Box<dyn SessionParser>], file: &Path) -> Result<Session> {
    let parser: Option<&dyn SessionParser> = parsers
        .iter()
        .find(|p| p.can_parse(file))
        .map(|p| p.as_ref());

    let parser = match parser {
        Some(p) => p,
        None => bail!(
            "No parser found for file: {}\nSupported formats: Claude Code (.jsonl), Codex (.jsonl), OpenCode (.json), Cline, Amp, Cursor, Gemini",
            file.display()
        ),
    };

    parser
        .parse(file)
        .with_context(|| format!("Failed to parse {}", file.display()))
}

fn parse_last_count(last: Option<&str>) -> Result<Option<u32>> {
    let Some(raw) = last else {
        return Ok(None);
    };

    let value = raw.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("HEAD") {
        return Ok(Some(1));
    }

    if let Ok(count) = value.parse::<u32>() {
        if count == 0 {
            bail!("--last count must be >= 1");
        }
        return Ok(Some(count));
    }

    if let Some(rest) = value
        .strip_prefix("HEAD~")
        .or_else(|| value.strip_prefix("head~"))
    {
        let count = rest
            .parse::<u32>()
            .with_context(|| format!("Invalid --last value `{value}`"))?;
        if count == 0 {
            bail!("--last count must be >= 1");
        }
        return Ok(Some(count));
    }

    bail!("Invalid --last value `{value}`. Use `--last`, `--last 6`, or `--last HEAD~6`.")
}

/// Resolve which session files to use: --last count or latest-only fallback.
fn resolve_session_files(last_count: Option<u32>) -> Result<Vec<PathBuf>> {
    let count = last_count.unwrap_or(1);
    if let Some(paths) = resolve_last_paths_from_local_index(count) {
        if std::io::stdout().is_terminal() {
            eprintln!(
                "Using {}/{} most recent sessions (local index)",
                paths.len(),
                count
            );
        }
        return Ok(paths);
    }

    let all = collect_all_session_paths()?;
    if all.is_empty() {
        bail!("No AI sessions found on this machine. Nothing to hand off.");
    }

    let mut selected = Vec::new();
    for (path, _tool) in all.iter().take(count as usize) {
        selected.push(path.clone());
    }
    if selected.is_empty() {
        bail!("No sessions found for --last {count}");
    }
    if std::io::stdout().is_terminal() {
        if count == 1 {
            if let Some((_, tool)) = all.first() {
                eprintln!("Using most recent session [{tool}]");
            }
        } else {
            eprintln!("Using {}/{} most recent sessions", selected.len(), count);
        }
    }
    Ok(selected)
}

fn resolve_last_paths_from_local_index(count: u32) -> Option<Vec<PathBuf>> {
    let db = opensession_local_db::LocalDb::open().ok()?;
    let fetch_count = count.saturating_mul(8).max(count).min(512);
    let rows = db.get_sessions_latest(fetch_count).ok()?;
    if rows.is_empty() {
        return None;
    }

    let selected =
        select_existing_session_paths(rows.into_iter().map(|row| row.source_path), count);
    if selected.len() == count as usize {
        Some(selected)
    } else {
        None
    }
}

fn select_existing_session_paths(
    source_paths: impl IntoIterator<Item = Option<String>>,
    count: u32,
) -> Vec<PathBuf> {
    let mut selected = Vec::new();
    let mut seen = HashSet::new();
    for source_path in source_paths.into_iter().flatten() {
        if source_path.trim().is_empty() {
            continue;
        }
        let path = PathBuf::from(source_path);
        if !path.exists() {
            continue;
        }
        if !seen.insert(path.clone()) {
            continue;
        }
        selected.push(path);
        if selected.len() >= count as usize {
            break;
        }
    }
    selected
}

/// Collect all discovered session paths, sorted by modification time (newest first).
fn collect_all_session_paths() -> Result<Vec<(PathBuf, String)>> {
    let locations = discover_sessions();
    let mut all: Vec<(PathBuf, String)> = Vec::new();

    for loc in locations {
        for path in loc.paths {
            all.push((path, loc.tool.clone()));
        }
    }

    // Sort by modification time, newest first
    all.sort_by(|(a, _), (b, _)| {
        let ma = std::fs::metadata(a)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        let mb = std::fs::metadata(b)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        mb.cmp(&ma)
    });

    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::{
        has_error_findings, parse_last_count, populate_prompt, select_existing_session_paths,
        write_handoff_to_writer, PopulateProvider, PopulateSpec,
    };
    use opensession_core::handoff::{format_duration, generate_handoff_markdown, HandoffSummary};
    use opensession_core::handoff::{HandoffValidationReport, ValidationFinding};
    use opensession_core::testing;
    use opensession_core::{Agent, Event, EventType, Session, Stats};

    fn make_agent() -> Agent {
        testing::agent()
    }

    fn make_event(event_type: EventType, text: &str) -> Event {
        testing::event(event_type, text)
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(45), "45s");
        assert_eq!(format_duration(90), "1m 30s");
        assert_eq!(format_duration(750), "12m 30s");
        assert_eq!(format_duration(3661), "1h 1m 1s");
    }

    #[test]
    fn test_generate_handoff_basic() {
        let mut session = Session::new("test-id".to_string(), make_agent());
        session.stats = Stats {
            event_count: 10,
            message_count: 3,
            tool_call_count: 5,
            task_count: 0,
            duration_seconds: 750,
            ..Default::default()
        };
        session
            .events
            .push(make_event(EventType::UserMessage, "Fix the build error"));
        session.events.push(make_event(
            EventType::FileEdit {
                path: "src/main.rs".to_string(),
                diff: None,
            },
            "",
        ));
        session.events.push(make_event(
            EventType::FileRead {
                path: "Cargo.toml".to_string(),
            },
            "",
        ));
        session.events.push(make_event(
            EventType::ShellCommand {
                command: "cargo build".to_string(),
                exit_code: Some(0),
            },
            "",
        ));

        let summary = HandoffSummary::from_session(&session);
        let md = generate_handoff_markdown(&summary);

        assert!(md.contains("# Session Handoff"));
        assert!(md.contains("Fix the build error"));
        assert!(md.contains("claude-code (claude-opus-4-6)"));
        assert!(md.contains("12m 30s"));
        assert!(md.contains("`src/main.rs` (edited)"));
        assert!(md.contains("`Cargo.toml`"));
        assert!(md.contains("`cargo build` → 0"));
    }

    #[test]
    fn test_files_read_excludes_modified() {
        let mut session = Session::new("test-id".to_string(), make_agent());
        session
            .events
            .push(make_event(EventType::UserMessage, "test"));
        session.events.push(make_event(
            EventType::FileRead {
                path: "src/main.rs".to_string(),
            },
            "",
        ));
        session.events.push(make_event(
            EventType::FileEdit {
                path: "src/main.rs".to_string(),
                diff: None,
            },
            "",
        ));
        session.events.push(make_event(
            EventType::FileRead {
                path: "README.md".to_string(),
            },
            "",
        ));

        let summary = HandoffSummary::from_session(&session);
        let md = generate_handoff_markdown(&summary);

        assert!(md.contains("## Files Read\n- `README.md`"));
    }

    #[test]
    fn test_file_create_not_overwritten_by_edit() {
        let mut session = Session::new("test-id".to_string(), make_agent());
        session
            .events
            .push(make_event(EventType::UserMessage, "test"));
        session.events.push(make_event(
            EventType::FileCreate {
                path: "new_file.rs".to_string(),
            },
            "",
        ));
        session.events.push(make_event(
            EventType::FileEdit {
                path: "new_file.rs".to_string(),
                diff: None,
            },
            "",
        ));

        let summary = HandoffSummary::from_session(&session);
        let md = generate_handoff_markdown(&summary);
        assert!(md.contains("`new_file.rs` (created)"));
    }

    #[test]
    fn test_shell_error_in_errors_section() {
        let mut session = Session::new("test-id".to_string(), make_agent());
        session
            .events
            .push(make_event(EventType::UserMessage, "test"));
        session.events.push(make_event(
            EventType::ShellCommand {
                command: "cargo test".to_string(),
                exit_code: Some(1),
            },
            "",
        ));

        let summary = HandoffSummary::from_session(&session);
        let md = generate_handoff_markdown(&summary);
        assert!(md.contains("## Errors"));
        assert!(md.contains("Shell: `cargo test` → exit 1"));
    }

    #[test]
    fn test_parse_last_count_variants() {
        assert_eq!(parse_last_count(None).unwrap(), None);
        assert_eq!(parse_last_count(Some("")).unwrap(), Some(1));
        assert_eq!(parse_last_count(Some("HEAD")).unwrap(), Some(1));
        assert_eq!(parse_last_count(Some("6")).unwrap(), Some(6));
        assert_eq!(parse_last_count(Some("HEAD~4")).unwrap(), Some(4));
        assert!(parse_last_count(Some("0")).is_err());
        assert!(parse_last_count(Some("HEAD~0")).is_err());
    }

    #[test]
    fn test_strict_only_fails_on_error_findings() {
        let warning_report = HandoffValidationReport {
            session_id: "s1".to_string(),
            passed: false,
            findings: vec![ValidationFinding {
                code: "objective_missing".to_string(),
                severity: "warning".to_string(),
                message: "Objective missing".to_string(),
            }],
        };
        assert!(!has_error_findings(&[warning_report]));

        let error_report = HandoffValidationReport {
            session_id: "s2".to_string(),
            passed: false,
            findings: vec![ValidationFinding {
                code: "work_package_cycle".to_string(),
                severity: "error".to_string(),
                message: "Cycle detected".to_string(),
            }],
        };
        assert!(has_error_findings(&[error_report]));
    }

    #[test]
    fn test_populate_spec_parse_and_prompt() {
        let spec = PopulateSpec::parse("claude:opus-4.6").unwrap();
        assert_eq!(spec.provider, PopulateProvider::Claude);
        assert_eq!(spec.model.as_deref(), Some("opus-4.6"));

        let prompt = populate_prompt(&spec);
        assert!(prompt.contains("Model preference: `opus-4.6`"));
        assert!(prompt.contains("HANDOFF.md"));
    }

    #[test]
    fn test_write_handoff_to_writer_ignores_broken_pipe() {
        struct BrokenPipeWriter;
        impl std::io::Write for BrokenPipeWriter {
            fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "pipe closed",
                ))
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let mut writer = BrokenPipeWriter;
        assert!(write_handoff_to_writer(&mut writer, "payload").is_ok());
    }

    #[test]
    fn test_select_existing_session_paths_filters_invalid_and_dedupes() {
        let tmp = tempfile::tempdir().unwrap();
        let path_a = tmp.path().join("a.jsonl");
        let path_b = tmp.path().join("b.jsonl");
        std::fs::write(&path_a, "{}").unwrap();
        std::fs::write(&path_b, "{}").unwrap();

        let selected = select_existing_session_paths(
            vec![
                Some(path_a.to_string_lossy().into_owned()),
                Some("".to_string()),
                Some(path_a.to_string_lossy().into_owned()),
                Some("/nonexistent/path.jsonl".to_string()),
                Some(path_b.to_string_lossy().into_owned()),
            ],
            3,
        );

        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0], path_a);
        assert_eq!(selected[1], path_b);
    }
}
