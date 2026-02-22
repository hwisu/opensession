use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use opensession_core::handoff::{validate_handoff_summaries, HandoffSummary};
use opensession_core::object_store::{
    find_repo_root, global_store_root, read_local_object_from_uri, sha256_hex, store_local_object,
};
use opensession_core::source_uri::SourceUri;
use opensession_core::validate::validate_session;
use opensession_core::Session;
use opensession_local_db::{LocalDb, LocalSessionFilter};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Args)]
pub struct HandoffArgs {
    #[command(subcommand)]
    pub action: HandoffCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum HandoffCommand {
    /// Build a new immutable handoff artifact.
    Build(HandoffBuildArgs),
    /// Inspect and manage handoff artifacts.
    Artifacts {
        #[command(subcommand)]
        action: HandoffArtifactsCommand,
    },
}

#[derive(Debug, Clone, Args)]
pub struct HandoffBuildArgs {
    /// Input session files.
    pub inputs: Vec<PathBuf>,
    /// Use latest N sessions from local index.
    #[arg(long)]
    pub last: Option<usize>,
    /// Build from existing local source URI(s).
    #[arg(long = "from", value_name = "URI")]
    pub from_uris: Vec<String>,
    /// Validate generated summaries and fail on error-level findings.
    #[arg(long)]
    pub validate: bool,
    /// Pin alias to move after build.
    #[arg(long)]
    pub pin: Option<String>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum HandoffArtifactsCommand {
    List,
    Get {
        id_or_uri: String,
        #[arg(long, value_enum, default_value_t = ArtifactFormatArg::Canonical)]
        format: ArtifactFormatArg,
        #[arg(long, value_enum, default_value_t = ArtifactEncodeArg::Jsonl)]
        encode: ArtifactEncodeArg,
    },
    Verify {
        id_or_uri: String,
    },
    Pin {
        alias: String,
        id_or_uri: String,
    },
    Unpin {
        alias: String,
    },
    Rm {
        id_or_uri: String,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum ArtifactFormatArg {
    Canonical,
    Raw,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum ArtifactEncodeArg {
    Jsonl,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactRecord {
    pub version: String,
    pub sha256: String,
    pub created_at: String,
    pub source_uris: Vec<String>,
    pub canonical_jsonl: String,
    pub raw_sessions: Vec<Session>,
    #[serde(default)]
    pub validation_reports: Vec<serde_json::Value>,
}

pub fn run(args: HandoffArgs) -> Result<()> {
    match args.action {
        HandoffCommand::Build(build) => run_build(build),
        HandoffCommand::Artifacts { action } => run_artifacts(action),
    }
}

fn run_build(args: HandoffBuildArgs) -> Result<()> {
    let cwd = std::env::current_dir().context("read current directory")?;
    let mut sessions = Vec::<Session>::new();
    let mut source_uris = Vec::<String>::new();

    for input in &args.inputs {
        let mut session = parse_session_input(input)?;
        session.recompute_stats();
        let canonical = session.to_jsonl().context("serialize canonical jsonl")?;
        let stored = store_local_object(canonical.as_bytes(), &cwd)?;
        source_uris.push(stored.uri.to_string());
        sessions.push(session);
    }

    for uri in &args.from_uris {
        let parsed = SourceUri::parse(uri)?;
        let (_path, bytes) = read_local_object_from_uri(&parsed, &cwd)?;
        let mut session = Session::from_jsonl(&String::from_utf8_lossy(&bytes))
            .context("parse source uri as HAIL JSONL")?;
        session.recompute_stats();
        source_uris.push(parsed.to_string());
        sessions.push(session);
    }

    if let Some(last_count) = args.last {
        if last_count == 0 {
            bail!("--last must be greater than zero");
        }
        let recent = load_last_sessions(last_count)?;
        for mut session in recent {
            session.recompute_stats();
            let canonical = session.to_jsonl().context("serialize canonical jsonl")?;
            let stored = store_local_object(canonical.as_bytes(), &cwd)?;
            source_uris.push(stored.uri.to_string());
            sessions.push(session);
        }
    }

    if sessions.is_empty() {
        bail!("no sessions provided (use inputs, --from, or --last)");
    }

    let summaries = sessions
        .iter()
        .map(HandoffSummary::from_session)
        .collect::<Vec<_>>();
    let reports = validate_handoff_summaries(&summaries);
    if args.validate {
        let has_errors = reports.iter().any(|report| {
            report
                .findings
                .iter()
                .any(|finding| finding.severity == "error")
        });
        if has_errors {
            let count = reports
                .iter()
                .flat_map(|report| report.findings.iter())
                .filter(|finding| finding.severity == "error")
                .count();
            bail!("handoff validation failed: {count} error-level findings");
        }
    }

    let canonical_jsonl = canonicalize_summaries(&summaries)?;
    let sha256 = sha256_hex(canonical_jsonl.as_bytes());
    let artifact_uri = SourceUri::Artifact {
        sha256: sha256.clone(),
    };

    let mut sorted_uris = source_uris
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    sorted_uris.sort();

    let record = ArtifactRecord {
        version: "v1".to_string(),
        sha256: sha256.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
        source_uris: sorted_uris,
        canonical_jsonl,
        raw_sessions: sessions,
        validation_reports: reports
            .iter()
            .map(serde_json::to_value)
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("serialize validation reports")?,
    };
    store_artifact_record(&record, &cwd)?;

    if let Some(alias) = args.pin {
        set_pin(&alias, &sha256, &cwd)?;
    }

    println!("{artifact_uri}");
    Ok(())
}

fn run_artifacts(action: HandoffArtifactsCommand) -> Result<()> {
    match action {
        HandoffArtifactsCommand::List => run_artifacts_list(),
        HandoffArtifactsCommand::Get {
            id_or_uri,
            format,
            encode,
        } => run_artifacts_get(&id_or_uri, format, encode),
        HandoffArtifactsCommand::Verify { id_or_uri } => run_artifacts_verify(&id_or_uri),
        HandoffArtifactsCommand::Pin { alias, id_or_uri } => run_artifacts_pin(&alias, &id_or_uri),
        HandoffArtifactsCommand::Unpin { alias } => run_artifacts_unpin(&alias),
        HandoffArtifactsCommand::Rm { id_or_uri } => run_artifacts_rm(&id_or_uri),
    }
}

fn run_artifacts_list() -> Result<()> {
    let cwd = std::env::current_dir().context("read current directory")?;
    let root = default_artifact_root(&cwd)?;
    let pins = list_pins(&root)?;
    let mut rows = Vec::new();
    for hash in list_artifact_hashes(&root)? {
        if let Ok((path, record)) =
            load_artifact_by_hash_from_roots(&hash, std::slice::from_ref(&root))
        {
            let aliases = aliases_for_hash(&pins, &hash);
            rows.push(serde_json::json!({
                "uri": format!("os://artifact/{}", record.sha256),
                "hash": record.sha256,
                "created_at": record.created_at,
                "sources": record.source_uris.len(),
                "pins": aliases,
                "path": path,
            }));
        }
    }
    println!("{}", serde_json::to_string_pretty(&rows)?);
    Ok(())
}

fn run_artifacts_get(
    id_or_uri: &str,
    format: ArtifactFormatArg,
    encode: ArtifactEncodeArg,
) -> Result<()> {
    let cwd = std::env::current_dir().context("read current directory")?;
    let hash = resolve_artifact_hash(id_or_uri, &cwd)?;
    let (_path, record) = load_artifact_by_hash(&hash, &cwd)?;

    match (format, encode) {
        (ArtifactFormatArg::Canonical, ArtifactEncodeArg::Jsonl) => {
            print!("{}", record.canonical_jsonl);
        }
        (ArtifactFormatArg::Canonical, ArtifactEncodeArg::Json) => {
            let rows = record
                .canonical_jsonl
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(serde_json::from_str::<serde_json::Value>)
                .collect::<std::result::Result<Vec<_>, _>>()
                .context("parse canonical jsonl rows")?;
            println!("{}", serde_json::to_string_pretty(&rows)?);
        }
        (ArtifactFormatArg::Raw, ArtifactEncodeArg::Json) => {
            println!("{}", serde_json::to_string_pretty(&record.raw_sessions)?);
        }
        (ArtifactFormatArg::Raw, ArtifactEncodeArg::Jsonl) => {
            for session in &record.raw_sessions {
                println!("{}", serde_json::to_string(session)?);
            }
        }
    }
    Ok(())
}

fn run_artifacts_verify(id_or_uri: &str) -> Result<()> {
    let cwd = std::env::current_dir().context("read current directory")?;
    let hash = resolve_artifact_hash(id_or_uri, &cwd)?;
    let (_path, record) = load_artifact_by_hash(&hash, &cwd)?;
    let recomputed = sha256_hex(record.canonical_jsonl.as_bytes());
    if recomputed != record.sha256 || record.sha256 != hash {
        bail!("artifact hash mismatch");
    }

    for session in &record.raw_sessions {
        if let Err(errors) = validate_session(session) {
            let details = errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ");
            bail!("artifact raw session validation failed: {details}");
        }
    }

    println!("verified: os://artifact/{hash}");
    Ok(())
}

fn run_artifacts_pin(alias: &str, id_or_uri: &str) -> Result<()> {
    let cwd = std::env::current_dir().context("read current directory")?;
    let hash = resolve_artifact_hash(id_or_uri, &cwd)?;
    let (_path, _record) = load_artifact_by_hash(&hash, &cwd)?;
    set_pin(alias, &hash, &cwd)?;
    println!("{alias} -> os://artifact/{hash}");
    Ok(())
}

fn run_artifacts_unpin(alias: &str) -> Result<()> {
    validate_alias(alias)?;
    let cwd = std::env::current_dir().context("read current directory")?;
    let root = default_artifact_root(&cwd)?;
    let path = pin_path(&root, alias)?;
    if !path.exists() {
        bail!("pin not found: {alias}");
    }
    std::fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
    println!("unpinned: {alias}");
    Ok(())
}

fn run_artifacts_rm(id_or_uri: &str) -> Result<()> {
    let cwd = std::env::current_dir().context("read current directory")?;
    let hash = resolve_artifact_hash(id_or_uri, &cwd)?;
    let root = default_artifact_root(&cwd)?;
    let pins = list_pins(&root)?;
    let aliases = aliases_for_hash(&pins, &hash);
    if !aliases.is_empty() {
        bail!(
            "artifact is pinned by aliases: {} (unpin first)",
            aliases.join(", ")
        );
    }

    let (path, _record) = load_artifact_by_hash(&hash, &cwd)?;
    std::fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
    println!("removed: os://artifact/{hash}");
    Ok(())
}

fn canonicalize_summaries(summaries: &[HandoffSummary]) -> Result<String> {
    let mut sorted = summaries
        .iter()
        .map(|summary| {
            serde_json::to_value(summary)
                .context("serialize summary")
                .map(|value| (summary.source_session_id.clone(), value))
        })
        .collect::<Result<Vec<_>>>()?;
    sorted.sort_by(|left, right| left.0.cmp(&right.0));

    let mut out = String::new();
    for (_id, value) in sorted {
        out.push_str(&serde_json::to_string(&value)?);
        out.push('\n');
    }
    Ok(out)
}

fn parse_session_input(path: &Path) -> Result<Session> {
    let raw = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    if let Ok(session) = Session::from_jsonl(&raw) {
        return Ok(session);
    }

    if let Some(session) = opensession_parsers::parse_with_default_parsers(path)
        .with_context(|| format!("parse {}", path.display()))?
    {
        return Ok(session);
    }

    bail!(
        "unsupported input format for {} (use `opensession parse --profile ...` first)",
        path.display()
    );
}

fn load_last_sessions(count: usize) -> Result<Vec<Session>> {
    let db = LocalDb::open()?;
    let filter = LocalSessionFilter {
        limit: Some(count as u32),
        ..Default::default()
    };
    let rows = db.list_sessions(&filter)?;
    let mut sessions = Vec::new();
    for row in rows {
        let Some(source_path) = row.source_path else {
            continue;
        };
        let path = PathBuf::from(&source_path);
        if !path.exists() {
            continue;
        }
        sessions.push(parse_session_input(&path)?);
        if sessions.len() >= count {
            break;
        }
    }
    Ok(sessions)
}

fn store_artifact_record(record: &ArtifactRecord, cwd: &Path) -> Result<PathBuf> {
    let root = default_artifact_root(cwd)?;
    let path = artifact_path(&root, &record.sha256)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    if !path.exists() {
        std::fs::write(&path, serde_json::to_vec_pretty(record)?)
            .with_context(|| format!("write {}", path.display()))?;
    }
    Ok(path)
}

pub fn load_artifact_by_hash(hash: &str, cwd: &Path) -> Result<(PathBuf, ArtifactRecord)> {
    let roots = candidate_artifact_roots(cwd)?;
    load_artifact_by_hash_from_roots(hash, &roots)
}

fn load_artifact_by_hash_from_roots(
    hash: &str,
    roots: &[PathBuf],
) -> Result<(PathBuf, ArtifactRecord)> {
    validate_hash(hash)?;
    for root in roots {
        let path = artifact_path(root, hash)?;
        if !path.exists() {
            continue;
        }
        let bytes = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        let record: ArtifactRecord =
            serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))?;
        return Ok((path, record));
    }
    bail!("artifact not found: {hash}");
}

pub fn resolve_artifact_hash(id_or_uri: &str, cwd: &Path) -> Result<String> {
    if let Ok(uri) = SourceUri::parse(id_or_uri) {
        return uri
            .as_artifact_hash()
            .map(ToOwned::to_owned)
            .ok_or_else(|| anyhow::anyhow!("uri is not an artifact uri"));
    }

    if is_hash(id_or_uri) {
        return Ok(id_or_uri.to_string());
    }

    let root = default_artifact_root(cwd)?;
    let pins = list_pins(&root)?;
    if let Some(hash) = pins.get(id_or_uri) {
        return Ok(hash.clone());
    }

    bail!("invalid artifact identifier: {id_or_uri}")
}

fn set_pin(alias: &str, hash: &str, cwd: &Path) -> Result<()> {
    validate_alias(alias)?;
    validate_hash(hash)?;
    let root = default_artifact_root(cwd)?;
    let pin = pin_path(&root, alias)?;
    if let Some(parent) = pin.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    std::fs::write(&pin, format!("{hash}\n"))
        .with_context(|| format!("write {}", pin.display()))?;
    Ok(())
}

fn list_pins(root: &Path) -> Result<BTreeMap<String, String>> {
    let dir = root.join("pins");
    let mut out = BTreeMap::new();
    if !dir.exists() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let content =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let hash = content.trim();
        if is_hash(hash) {
            out.insert(name.to_string(), hash.to_string());
        }
    }
    Ok(out)
}

fn aliases_for_hash(pins: &BTreeMap<String, String>, hash: &str) -> Vec<String> {
    pins.iter()
        .filter_map(|(alias, pinned)| {
            if pinned == hash {
                Some(alias.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
}

fn default_artifact_root(cwd: &Path) -> Result<PathBuf> {
    if let Some(repo_root) = find_repo_root(cwd) {
        return Ok(repo_root.join(".opensession").join("artifacts"));
    }
    let global_objects = global_store_root()?;
    let parent = global_objects
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid global object path"))?;
    Ok(parent.join("artifacts"))
}

fn candidate_artifact_roots(cwd: &Path) -> Result<Vec<PathBuf>> {
    let mut roots = Vec::new();
    if let Some(repo_root) = find_repo_root(cwd) {
        roots.push(repo_root.join(".opensession").join("artifacts"));
    }
    let global_objects = global_store_root()?;
    let parent = global_objects
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid global object path"))?;
    roots.push(parent.join("artifacts"));
    roots.dedup();
    Ok(roots)
}

fn list_artifact_hashes(root: &Path) -> Result<Vec<String>> {
    let base = root.join("sha256");
    if !base.exists() {
        return Ok(Vec::new());
    }

    let mut hashes = Vec::<String>::new();
    for first in std::fs::read_dir(&base).with_context(|| format!("read {}", base.display()))? {
        let first = first?;
        if !first.path().is_dir() {
            continue;
        }
        for second in
            std::fs::read_dir(first.path()).with_context(|| "read sha256 fanout level 2")?
        {
            let second = second?;
            if !second.path().is_dir() {
                continue;
            }
            for leaf in
                std::fs::read_dir(second.path()).with_context(|| "read sha256 fanout leaf")?
            {
                let leaf = leaf?;
                let path = leaf.path();
                if !path.is_file() {
                    continue;
                }
                let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
                    continue;
                };
                if let Some(hash) = file_name.strip_suffix(".json") {
                    if is_hash(hash) {
                        hashes.push(hash.to_string());
                    }
                }
            }
        }
    }
    hashes.sort();
    hashes.dedup();
    Ok(hashes)
}

fn artifact_path(root: &Path, hash: &str) -> Result<PathBuf> {
    validate_hash(hash)?;
    Ok(root
        .join("sha256")
        .join(&hash[0..2])
        .join(&hash[2..4])
        .join(format!("{hash}.json")))
}

fn pin_path(root: &Path, alias: &str) -> Result<PathBuf> {
    validate_alias(alias)?;
    Ok(root.join("pins").join(alias))
}

fn validate_alias(alias: &str) -> Result<()> {
    let trimmed = alias.trim();
    if trimmed.is_empty() {
        bail!("alias cannot be empty");
    }
    if !trimmed
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
    {
        bail!("alias contains invalid characters");
    }
    Ok(())
}

fn validate_hash(hash: &str) -> Result<()> {
    if is_hash(hash) {
        Ok(())
    } else {
        bail!("invalid sha256 hash: {hash}")
    }
}

fn is_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::{canonicalize_summaries, is_hash, validate_alias};
    use opensession_core::testing;
    use opensession_core::Session;

    #[test]
    fn hash_validator_accepts_sha256() {
        assert!(is_hash(&"a".repeat(64)));
        assert!(!is_hash("not-hash"));
    }

    #[test]
    fn alias_validator_rejects_spaces() {
        assert!(validate_alias("latest").is_ok());
        assert!(validate_alias("bad alias").is_err());
    }

    #[test]
    fn canonicalization_is_deterministic() {
        let mut s1 = Session::new("b".to_string(), testing::agent());
        s1.recompute_stats();
        let mut s2 = Session::new("a".to_string(), testing::agent());
        s2.recompute_stats();
        let summaries = vec![
            opensession_core::handoff::HandoffSummary::from_session(&s1),
            opensession_core::handoff::HandoffSummary::from_session(&s2),
        ];
        let canonical = canonicalize_summaries(&summaries).expect("canonicalize");
        let first = canonical.lines().next().expect("line");
        assert!(first.contains("\"source_session_id\":\"a\""));
    }
}
