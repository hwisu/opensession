use crate::url_opener::open_url_for_repo;
use anyhow::{anyhow, bail, Context, Result};
use clap::{Args, ValueEnum};
use opensession_api::{
    LocalReviewBundle, LocalReviewCommit, LocalReviewLayerFileChange, LocalReviewPrMeta,
    LocalReviewReviewerDigest, LocalReviewReviewerQa, LocalReviewSemanticSummary,
    LocalReviewSession,
};
use opensession_core::{ContentBlock, EventType, Session};
use opensession_runtime_config::SummarySettings;
use opensession_summary::{summarize_git_commit, SemanticSummaryArtifact};
use reqwest::Url;
use serde::Deserialize;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

pub(crate) const LOCAL_REVIEW_ROOT_DIR: &str = ".opensession/review";
pub(crate) const LOCAL_REVIEW_SERVER_BASE_URL: &str = "http://127.0.0.1:8788";

#[derive(Debug, Clone, Args)]
pub struct ReviewArgs {
    /// GitHub PR URL (`https://github.com/<owner>/<repo>/pull/<number>`).
    pub pr_link: String,
    /// Review view mode (`auto` resolves to web).
    #[arg(long, value_enum, default_value_t = ReviewView::Auto)]
    pub view: ReviewView,
    /// Repository root path (defaults to current repository).
    #[arg(long)]
    pub repo: Option<PathBuf>,
    /// Skip network fetch and use already-fetched refs.
    #[arg(long)]
    pub no_fetch: bool,
    /// Print review bundle stats as JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ReviewView {
    Auto,
    Web,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GithubPrSpec {
    owner: String,
    repo: String,
    number: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GitRemote {
    name: String,
    url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommitInfo {
    sha: String,
    title: String,
    author_name: String,
    author_email: String,
    authored_at: String,
}

#[derive(Debug, Clone, Deserialize)]
struct CommitIndexEntry {
    session_id: String,
    hail_path: String,
}

#[derive(Debug, Clone)]
struct BundleBuildContext {
    review_root: PathBuf,
    bundle_path: PathBuf,
}

struct BuildReviewBundleInput<'a> {
    repo_root: &'a Path,
    pr_url: &'a str,
    pr: &'a GithubPrSpec,
    remote: &'a str,
    base_sha: &'a str,
    head_sha: &'a str,
    commits: Vec<CommitInfo>,
    summary_settings: &'a SummarySettings,
}

pub async fn run(args: ReviewArgs) -> Result<()> {
    let repo_root = resolve_repo_root(args.repo.as_deref())?;
    let pr = parse_github_pr_url(&args.pr_link)?;
    let remote = resolve_matching_remote(&repo_root, &pr)?;
    let summary_settings = load_review_summary_settings();

    let pr_head_ref = format!("refs/opensession/review/pr/{}/head", pr.number);
    if !args.no_fetch {
        fetch_pr_and_hidden_refs(&repo_root, &remote.name, pr.number, &pr_head_ref)?;
    }

    let pr_head_sha = resolve_commit_oid(&repo_root, &pr_head_ref).with_context(|| {
        format!(
            "PR head ref `{pr_head_ref}` is not available. Retry without --no-fetch or check permissions."
        )
    })?;
    let default_remote_ref = resolve_default_remote_ref(&repo_root, &remote.name)?;
    let base_sha = merge_base(&repo_root, &pr_head_sha, &default_remote_ref)?;
    let commit_shas = rev_list_range(&repo_root, &base_sha, &pr_head_sha)?;
    let commits = load_commit_infos(&repo_root, &commit_shas)?;

    let ctx = prepare_bundle_paths(&repo_root, &pr, &pr_head_sha)?;
    let bundle =
        if let Some(cached) = load_cached_bundle_if_head_matches(&ctx.bundle_path, &pr_head_sha)? {
            if bundle_has_commit_semantic_summaries(&cached) {
                cached
            } else {
                let built = build_review_bundle(BuildReviewBundleInput {
                    repo_root: &repo_root,
                    pr_url: &args.pr_link,
                    pr: &pr,
                    remote: &remote.name,
                    base_sha: &base_sha,
                    head_sha: &pr_head_sha,
                    commits,
                    summary_settings: &summary_settings,
                })
                .await?;
                write_review_bundle(&ctx.bundle_path, &built)?;
                built
            }
        } else {
            let built = build_review_bundle(BuildReviewBundleInput {
                repo_root: &repo_root,
                pr_url: &args.pr_link,
                pr: &pr,
                remote: &remote.name,
                base_sha: &base_sha,
                head_sha: &pr_head_sha,
                commits,
                summary_settings: &summary_settings,
            })
            .await?;
            write_review_bundle(&ctx.bundle_path, &built)?;
            built
        };

    if args.json {
        let payload = serde_json::json!({
            "review_id": bundle.review_id,
            "bundle_path": ctx.bundle_path,
            "remote": bundle.pr.remote,
            "base_sha": bundle.pr.base_sha,
            "head_sha": bundle.pr.head_sha,
            "commit_count": bundle.commits.len(),
            "session_count": bundle.sessions.len(),
            "mapped_commit_count": bundle.commits.iter().filter(|row| !row.session_ids.is_empty()).count(),
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    let effective_view = resolve_view_mode(args.view);
    match effective_view {
        ReviewView::Web => {
            let url = ensure_web_review_server(
                &repo_root,
                &ctx.review_root,
                &bundle.review_id,
                ctx.bundle_path.parent().unwrap_or(repo_root.as_path()),
            )
            .await?;
            if let Err(err) = open_url_for_repo(&repo_root, &url) {
                println!("review url: {url}");
                return Err(err);
            }
            println!("review url: {url}");
        }
        ReviewView::Auto => unreachable!("auto view is resolved before dispatch"),
    }

    Ok(())
}

fn resolve_repo_root(repo_override: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = repo_override {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .context("read current directory")?
                .join(path)
        };
        return opensession_git_native::ops::find_repo_root(&absolute).ok_or_else(|| {
            anyhow!(
                "`{}` is not inside a git repository",
                absolute.to_string_lossy()
            )
        });
    }

    let cwd = std::env::current_dir().context("read current directory")?;
    opensession_git_native::ops::find_repo_root(&cwd)
        .ok_or_else(|| anyhow!("current directory is not inside a git repository"))
}

fn parse_github_pr_url(raw: &str) -> Result<GithubPrSpec> {
    let parsed = Url::parse(raw).with_context(|| format!("invalid PR URL: `{raw}`"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow!("PR URL must include a host"))?;
    if !host.eq_ignore_ascii_case("github.com") {
        bail!("unsupported PR host `{host}`; only github.com PR links are supported");
    }

    let segments: Vec<_> = parsed
        .path_segments()
        .ok_or_else(|| anyhow!("invalid PR URL path"))?
        .collect();
    if segments.len() < 4 || segments[2] != "pull" {
        bail!("invalid PR URL format: expected https://github.com/<owner>/<repo>/pull/<number>");
    }
    let owner = segments[0].trim();
    let repo = segments[1].trim();
    let number = segments[3]
        .parse::<u64>()
        .with_context(|| format!("invalid PR number `{}`", segments[3]))?;
    if owner.is_empty() || repo.is_empty() {
        bail!("invalid PR URL format: owner/repo must be non-empty");
    }

    Ok(GithubPrSpec {
        owner: owner.to_string(),
        repo: repo.to_string(),
        number,
    })
}

fn resolve_matching_remote(repo_root: &Path, pr: &GithubPrSpec) -> Result<GitRemote> {
    let remotes_raw = git_stdout(repo_root, &["remote"]).context("list git remotes")?;
    let mut remotes = Vec::new();
    for remote_name in remotes_raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let remote_url = git_stdout(repo_root, &["remote", "get-url", remote_name])
            .with_context(|| format!("read remote URL for `{remote_name}`"))?;
        remotes.push(GitRemote {
            name: remote_name.to_string(),
            url: remote_url.trim().to_string(),
        });
    }

    let mut matches = remotes
        .into_iter()
        .filter(|remote| remote_matches_github_pr(&remote.url, pr))
        .collect::<Vec<_>>();
    if matches.is_empty() {
        bail!(
            "no git remote matches `{}/{}; expected github.com/{}/{}",
            pr.owner,
            pr.repo,
            pr.owner,
            pr.repo
        );
    }
    matches.sort_by_key(|remote| if remote.name == "origin" { 0 } else { 1 });
    Ok(matches.remove(0))
}

fn remote_matches_github_pr(remote_url: &str, pr: &GithubPrSpec) -> bool {
    let Some((host, owner, repo)) = parse_remote_repo_triplet(remote_url) else {
        return false;
    };
    host.eq_ignore_ascii_case("github.com")
        && owner.eq_ignore_ascii_case(&pr.owner)
        && repo.eq_ignore_ascii_case(&pr.repo)
}

fn parse_remote_repo_triplet(remote_url: &str) -> Option<(String, String, String)> {
    let trimmed = remote_url.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(rest) = trimmed.strip_prefix("git@") {
        let (host, path) = rest.split_once(':')?;
        return parse_repo_owner_and_name(host, path);
    }

    if trimmed.starts_with("ssh://")
        || trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
    {
        let url = Url::parse(trimmed).ok()?;
        let host = url.host_str()?;
        let path = url.path().trim_start_matches('/');
        return parse_repo_owner_and_name(host, path);
    }

    // Local path or unknown remote syntax.
    None
}

fn parse_repo_owner_and_name(host: &str, path: &str) -> Option<(String, String, String)> {
    let cleaned = path.trim().trim_start_matches('/').trim_end_matches(".git");
    let mut parts = cleaned.split('/').filter(|segment| !segment.is_empty());
    let owner = parts.next()?.to_string();
    let repo = parts.next()?.to_string();
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some((host.to_string(), owner, repo))
}

fn fetch_pr_and_hidden_refs(
    repo_root: &Path,
    remote: &str,
    pr_number: u64,
    pr_head_ref: &str,
) -> Result<()> {
    run_git(
        repo_root,
        &[
            "fetch".into(),
            remote.into(),
            format!("+refs/pull/{pr_number}/head:{pr_head_ref}"),
        ],
    )
    .with_context(|| {
        format!("failed to fetch PR ref `refs/pull/{pr_number}/head` from remote `{remote}`")
    })?;

    run_git(
        repo_root,
        &[
            "fetch".into(),
            remote.into(),
            format!("+refs/opensession/*:refs/remotes/{remote}/opensession/*"),
        ],
    )
    .with_context(|| format!("failed to fetch hidden refs from remote `{remote}`"))?;

    // Refresh remote HEAD metadata used to resolve default branch.
    let _ = run_git(repo_root, &refresh_remote_head_fetch_args(remote));
    Ok(())
}

fn refresh_remote_head_fetch_args(remote: &str) -> Vec<String> {
    // Keep hidden refs fetched in this run even when fetch.prune=true globally.
    vec!["fetch".into(), "--no-prune".into(), remote.into()]
}

fn resolve_default_remote_ref(repo_root: &Path, remote: &str) -> Result<String> {
    let symbolic = git_stdout(
        repo_root,
        &[
            "symbolic-ref",
            "--quiet",
            &format!("refs/remotes/{remote}/HEAD"),
        ],
    );
    if let Ok(value) = symbolic {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    for candidate in [
        format!("refs/remotes/{remote}/main"),
        format!("refs/remotes/{remote}/master"),
    ] {
        if resolve_commit_oid(repo_root, &candidate).is_ok() {
            return Ok(candidate);
        }
    }

    bail!(
        "unable to resolve default branch for remote `{remote}` (missing refs/remotes/{remote}/HEAD)"
    );
}

fn resolve_commit_oid(repo_root: &Path, reference: &str) -> Result<String> {
    let value = git_stdout(
        repo_root,
        &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
    )
    .with_context(|| format!("resolve commit for `{reference}`"))?;
    let sha = value.trim().to_string();
    if sha.is_empty() {
        bail!("empty commit SHA for `{reference}`");
    }
    Ok(sha)
}

fn merge_base(repo_root: &Path, left: &str, right: &str) -> Result<String> {
    let value = git_stdout(repo_root, &["merge-base", left, right])
        .with_context(|| format!("compute merge-base({left}, {right})"))?;
    let sha = value.trim().to_string();
    if sha.is_empty() {
        bail!("merge-base result is empty for `{left}` and `{right}`");
    }
    Ok(sha)
}

fn rev_list_range(repo_root: &Path, base: &str, head: &str) -> Result<Vec<String>> {
    let range = format!("{base}..{head}");
    let value = git_stdout(repo_root, &["rev-list", "--reverse", &range])
        .with_context(|| format!("list commits in range `{range}`"))?;
    Ok(value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn load_commit_infos(repo_root: &Path, commit_shas: &[String]) -> Result<Vec<CommitInfo>> {
    let mut infos = Vec::with_capacity(commit_shas.len());
    for sha in commit_shas {
        let value = git_stdout(
            repo_root,
            &[
                "show",
                "-s",
                "--format=%s%x1f%an%x1f%ae%x1f%aI",
                sha.as_str(),
            ],
        )
        .with_context(|| format!("read commit metadata for `{sha}`"))?;
        let mut parts = value.trim_end_matches('\n').split('\x1f');
        let title = parts.next().unwrap_or_default().trim().to_string();
        let author_name = parts.next().unwrap_or_default().trim().to_string();
        let author_email = parts.next().unwrap_or_default().trim().to_string();
        let authored_at = parts.next().unwrap_or_default().trim().to_string();
        infos.push(CommitInfo {
            sha: sha.clone(),
            title,
            author_name,
            author_email,
            authored_at,
        });
    }
    Ok(infos)
}

fn prepare_bundle_paths(
    repo_root: &Path,
    pr: &GithubPrSpec,
    head_sha: &str,
) -> Result<BundleBuildContext> {
    let review_root = repo_root.join(LOCAL_REVIEW_ROOT_DIR);
    let review_id = build_review_id(pr, head_sha);
    let bundle_path = review_root.join(&review_id).join("bundle.json");
    if let Some(parent) = bundle_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create review bundle directory {}", parent.display()))?;
    }
    Ok(BundleBuildContext {
        review_root,
        bundle_path,
    })
}

fn build_review_id(pr: &GithubPrSpec, head_sha: &str) -> String {
    let owner = sanitize_review_id_component(&pr.owner);
    let repo = sanitize_review_id_component(&pr.repo);
    let head7 = head_sha.chars().take(7).collect::<String>();
    format!("gh-{owner}-{repo}-pr{}-{head7}", pr.number)
}

fn sanitize_review_id_component(value: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in value.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            out.push(lower);
            prev_dash = false;
            continue;
        }
        if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

fn load_cached_bundle_if_head_matches(
    bundle_path: &Path,
    expected_head_sha: &str,
) -> Result<Option<LocalReviewBundle>> {
    if !bundle_path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read(bundle_path)
        .with_context(|| format!("read cached review bundle {}", bundle_path.display()))?;
    let parsed: LocalReviewBundle =
        serde_json::from_slice(&raw).context("parse cached review bundle JSON")?;
    if parsed.pr.head_sha == expected_head_sha {
        return Ok(Some(parsed));
    }
    Ok(None)
}

fn write_review_bundle(path: &Path, bundle: &LocalReviewBundle) -> Result<()> {
    let body = serde_json::to_vec_pretty(bundle).context("serialize local review bundle")?;
    std::fs::write(path, body).with_context(|| format!("write review bundle {}", path.display()))
}

fn bundle_has_commit_semantic_summaries(bundle: &LocalReviewBundle) -> bool {
    bundle
        .commits
        .iter()
        .all(|commit| commit.semantic_summary.is_some())
}

pub(crate) fn load_review_summary_settings() -> SummarySettings {
    match crate::runtime_settings::load_runtime_config() {
        Ok(config) => config.summary,
        Err(error) => {
            eprintln!(
                "[opensession] failed to load runtime summary settings ({error}); using defaults"
            );
            SummarySettings::default()
        }
    }
}

pub(crate) async fn summarize_commit_for_review(
    repo_root: &Path,
    commit_sha: &str,
    settings: &SummarySettings,
) -> Option<LocalReviewSemanticSummary> {
    match summarize_git_commit(repo_root, commit_sha, settings).await {
        Ok(artifact) => Some(local_review_semantic_summary_from_artifact(artifact)),
        Err(error) => Some(LocalReviewSemanticSummary {
            changes: "commit semantic summary unavailable".to_string(),
            auth_security: "none detected".to_string(),
            layer_file_changes: Vec::new(),
            source_kind: "git_commit".to_string(),
            generation_kind: "heuristic_fallback".to_string(),
            provider: "disabled".to_string(),
            model: None,
            error: Some(error),
            diff_tree: Vec::new(),
        }),
    }
}

pub(crate) fn local_review_semantic_summary_from_artifact(
    artifact: SemanticSummaryArtifact,
) -> LocalReviewSemanticSummary {
    LocalReviewSemanticSummary {
        changes: artifact.summary.changes,
        auth_security: artifact.summary.auth_security,
        layer_file_changes: artifact
            .summary
            .layer_file_changes
            .into_iter()
            .map(|layer| LocalReviewLayerFileChange {
                layer: layer.layer,
                summary: layer.summary,
                files: layer.files,
            })
            .collect(),
        source_kind: enum_label(&artifact.source_kind),
        generation_kind: enum_label(&artifact.generation_kind),
        provider: enum_label(&artifact.provider),
        model: (!artifact.model.trim().is_empty()).then_some(artifact.model),
        error: artifact.error,
        diff_tree: artifact
            .diff_tree
            .into_iter()
            .filter_map(|layer| serde_json::to_value(layer).ok())
            .collect(),
    }
}

fn enum_label<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value)
        .ok()
        .map(|raw| raw.trim_matches('"').to_string())
        .filter(|raw| !raw.trim().is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

pub(crate) fn build_reviewer_digest_for_commit(
    sessions: &[LocalReviewSession],
    commit_sha: &str,
) -> LocalReviewReviewerDigest {
    let mut pending_questions = VecDeque::<String>::new();
    let mut qa = Vec::<LocalReviewReviewerQa>::new();
    let mut modified_files = BTreeSet::<String>::new();

    for row in sessions
        .iter()
        .filter(|row| row.commit_shas.iter().any(|sha| sha == commit_sha))
    {
        for event in &row.session.events {
            let source = event
                .attributes
                .get("source")
                .and_then(|value| value.as_str())
                .map(|value| value.trim().to_ascii_lowercase())
                .unwrap_or_default();

            match &event.event_type {
                EventType::SystemMessage if source == "interactive_question" => {
                    if let Some(text) = first_text_for_reviewer_digest(&event.content.blocks) {
                        pending_questions.push_back(text);
                    }
                }
                EventType::UserMessage if source == "interactive" => {
                    let Some(answer) = first_text_for_reviewer_digest(&event.content.blocks) else {
                        continue;
                    };
                    let question = pending_questions
                        .pop_front()
                        .unwrap_or_else(|| "(interactive question missing)".to_string());
                    qa.push(LocalReviewReviewerQa {
                        question,
                        answer: Some(answer),
                    });
                }
                EventType::FileEdit { path, .. }
                | EventType::FileCreate { path }
                | EventType::FileDelete { path } => {
                    let trimmed = path.trim();
                    if !trimmed.is_empty() {
                        modified_files.insert(trimmed.to_string());
                    }
                }
                _ => {}
            }
        }
    }

    qa.extend(
        pending_questions
            .into_iter()
            .map(|question| LocalReviewReviewerQa {
                question,
                answer: None,
            }),
    );
    if qa.len() > 12 {
        qa.truncate(12);
    }

    let modified_files = modified_files.into_iter().collect::<Vec<_>>();
    let test_files = modified_files
        .iter()
        .filter(|path| is_test_file_path(path))
        .cloned()
        .collect::<Vec<_>>();

    LocalReviewReviewerDigest {
        qa,
        modified_files,
        test_files,
    }
}

fn first_text_for_reviewer_digest(blocks: &[ContentBlock]) -> Option<String> {
    for block in blocks {
        if let ContentBlock::Text { text } = block {
            let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
            let trimmed = compact.trim();
            if trimmed.is_empty() {
                continue;
            }
            return Some(truncate_for_reviewer_digest(trimmed, 220));
        }
    }
    None
}

fn truncate_for_reviewer_digest(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (idx, ch) in text.chars().enumerate() {
        if idx >= max_chars {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

fn is_test_file_path(path: &str) -> bool {
    let normalized = path.trim().replace('\\', "/").to_ascii_lowercase();
    if normalized.is_empty() {
        return false;
    }
    normalized.contains("/tests/")
        || normalized.contains("/test/")
        || normalized.contains("/__tests__/")
        || normalized.ends_with(".test.ts")
        || normalized.ends_with(".test.tsx")
        || normalized.ends_with(".test.js")
        || normalized.ends_with(".test.jsx")
        || normalized.ends_with(".spec.ts")
        || normalized.ends_with(".spec.tsx")
        || normalized.ends_with(".spec.js")
        || normalized.ends_with(".spec.jsx")
        || normalized.ends_with("_test.rs")
        || normalized.ends_with("_spec.rs")
        || normalized.ends_with("_test.py")
}

async fn build_review_bundle(input: BuildReviewBundleInput<'_>) -> Result<LocalReviewBundle> {
    let BuildReviewBundleInput {
        repo_root,
        pr_url,
        pr,
        remote,
        base_sha,
        head_sha,
        commits,
        summary_settings,
    } = input;

    let ledger_refs = list_remote_ledger_refs(repo_root, remote)?;
    let mut session_rows: Vec<LocalReviewSession> = Vec::new();
    let mut session_key_to_index: HashMap<String, usize> = HashMap::new();
    let mut commit_rows = Vec::with_capacity(commits.len());

    for commit in commits {
        let mut session_ids_for_commit = Vec::new();
        let mut seen_session_ids = HashSet::new();
        let index_prefix = format!("v1/index/commits/{}/", sanitize_path_component(&commit.sha));

        for ledger_ref in &ledger_refs {
            let index_paths = list_tree_paths(repo_root, ledger_ref, &index_prefix)?;
            for index_path in index_paths {
                if !index_path.ends_with(".json") {
                    continue;
                }
                let index_raw =
                    git_show_file(repo_root, ledger_ref, &index_path).with_context(|| {
                        format!("read commit index `{index_path}` from ledger ref `{ledger_ref}`")
                    })?;
                let index_entry: CommitIndexEntry =
                    serde_json::from_str(&index_raw).with_context(|| {
                        format!("parse commit index payload `{index_path}` as JSON")
                    })?;
                let key = format!(
                    "{}\n{}\n{}",
                    index_entry.session_id, ledger_ref, index_entry.hail_path
                );
                let session_index = if let Some(existing) = session_key_to_index.get(&key).copied()
                {
                    existing
                } else {
                    let hail_raw = git_show_file(repo_root, ledger_ref, &index_entry.hail_path)
                        .with_context(|| {
                            format!(
                                "read HAIL payload `{}` from `{ledger_ref}`",
                                index_entry.hail_path
                            )
                        })?;
                    let session = Session::from_jsonl(&hail_raw).with_context(|| {
                        format!(
                            "parse HAIL payload `{}` from `{ledger_ref}`",
                            index_entry.hail_path
                        )
                    })?;
                    session_rows.push(LocalReviewSession {
                        session_id: index_entry.session_id.clone(),
                        ledger_ref: ledger_ref.clone(),
                        hail_path: index_entry.hail_path.clone(),
                        commit_shas: vec![commit.sha.clone()],
                        session,
                    });
                    let created = session_rows.len() - 1;
                    session_key_to_index.insert(key, created);
                    created
                };

                let session_row = session_rows
                    .get_mut(session_index)
                    .ok_or_else(|| anyhow!("invalid session index during bundle build"))?;
                if !session_row.commit_shas.iter().any(|sha| sha == &commit.sha) {
                    session_row.commit_shas.push(commit.sha.clone());
                }

                if seen_session_ids.insert(index_entry.session_id.clone()) {
                    session_ids_for_commit.push(index_entry.session_id);
                }
            }
        }

        let semantic_summary =
            summarize_commit_for_review(repo_root, &commit.sha, summary_settings).await;
        let reviewer_digest = build_reviewer_digest_for_commit(&session_rows, &commit.sha);

        commit_rows.push(LocalReviewCommit {
            sha: commit.sha,
            title: commit.title,
            author_name: commit.author_name,
            author_email: commit.author_email,
            authored_at: commit.authored_at,
            session_ids: session_ids_for_commit,
            reviewer_digest,
            semantic_summary,
        });
    }

    Ok(LocalReviewBundle {
        review_id: build_review_id(pr, head_sha),
        generated_at: chrono::Utc::now().to_rfc3339(),
        pr: LocalReviewPrMeta {
            url: pr_url.to_string(),
            owner: pr.owner.clone(),
            repo: pr.repo.clone(),
            number: pr.number,
            remote: remote.to_string(),
            base_sha: base_sha.to_string(),
            head_sha: head_sha.to_string(),
        },
        commits: commit_rows,
        sessions: session_rows,
    })
}

fn list_remote_ledger_refs(repo_root: &Path, remote: &str) -> Result<Vec<String>> {
    let prefix = format!("refs/remotes/{remote}/opensession/branches");
    let output = git_stdout(
        repo_root,
        &["for-each-ref", "--format=%(refname)", prefix.as_str()],
    )
    .with_context(|| format!("list remote ledger refs under `{prefix}`"))?;
    Ok(output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn list_tree_paths(repo_root: &Path, reference: &str, prefix: &str) -> Result<Vec<String>> {
    let output = git_stdout(
        repo_root,
        &["ls-tree", "-r", "--name-only", reference, prefix],
    )
    .with_context(|| format!("list tree paths from `{reference}` under `{prefix}`"))?;
    Ok(output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn git_show_file(repo_root: &Path, reference: &str, path: &str) -> Result<String> {
    let spec = format!("{reference}:{path}");
    git_stdout(repo_root, &["show", spec.as_str()])
}

fn sanitize_path_component(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn resolve_view_mode(requested: ReviewView) -> ReviewView {
    match requested {
        ReviewView::Auto => ReviewView::Web,
        ReviewView::Web => ReviewView::Web,
    }
}

pub(crate) async fn ensure_web_review_server(
    repo_root: &Path,
    review_root: &Path,
    review_id: &str,
    bundle_dir: &Path,
) -> Result<String> {
    let review_api_url = format!("{LOCAL_REVIEW_SERVER_BASE_URL}/api/review/local/{review_id}");
    let health_url = format!("{LOCAL_REVIEW_SERVER_BASE_URL}/api/health");
    let static_version_url = format!("{LOCAL_REVIEW_SERVER_BASE_URL}/_app/version.json");

    let mut review_ok = endpoint_ok(&review_api_url).await;
    let mut static_ok = endpoint_ok(&static_version_url).await;
    let health_ok = endpoint_ok(&health_url).await;

    if !(health_ok || (review_ok && static_ok)) {
        spawn_local_review_server(repo_root, review_root, bundle_dir)?;
        for _ in 0..40 {
            review_ok = endpoint_ok(&review_api_url).await;
            static_ok = endpoint_ok(&static_version_url).await;
            if review_ok && static_ok {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    review_ok = endpoint_ok(&review_api_url).await;
    static_ok = endpoint_ok(&static_version_url).await;
    if !(review_ok && static_ok) {
        bail!(
            "local review server is unavailable or incomplete at {LOCAL_REVIEW_SERVER_BASE_URL} (api_ok={review_ok}, static_ok={static_ok}); see .opensession/server-data/logs/review-server.log and ensure opensession-server serves /api/review/local/{{id}} plus static assets"
        );
    }

    Ok(format!(
        "{LOCAL_REVIEW_SERVER_BASE_URL}/review/local/{review_id}"
    ))
}

fn spawn_local_review_server(
    repo_root: &Path,
    review_root: &Path,
    _bundle_dir: &Path,
) -> Result<()> {
    let web_dir = repo_root.join("web").join("build");
    let data_dir = repo_root.join(".opensession").join("server-data");
    std::fs::create_dir_all(&data_dir)
        .with_context(|| format!("create server data directory {}", data_dir.display()))?;
    let logs_dir = data_dir.join("logs");
    std::fs::create_dir_all(&logs_dir)
        .with_context(|| format!("create server logs directory {}", logs_dir.display()))?;
    let log_path = logs_dir.join("review-server.log");
    let stdout_log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("open server log file {}", log_path.display()))?;
    let stderr_log = stdout_log
        .try_clone()
        .with_context(|| format!("clone server log file {}", log_path.display()))?;

    let mut cmd = if let Some(server_bin) = resolve_review_server_binary(repo_root) {
        Command::new(server_bin)
    } else if repo_root.join("Cargo.toml").exists() {
        let mut cargo = Command::new("cargo");
        cargo
            .arg("run")
            .arg("-p")
            .arg("opensession-server")
            .arg("--");
        cargo
    } else {
        bail!("could not find `opensession-server` binary (or cargo workspace fallback)");
    };

    cmd.current_dir(repo_root)
        .env("PORT", "8788")
        .env("BASE_URL", LOCAL_REVIEW_SERVER_BASE_URL)
        .env("OPENSESSION_DATA_DIR", &data_dir)
        .env("OPENSESSION_WEB_DIR", &web_dir)
        .env("OPENSESSION_LOCAL_REVIEW_ROOT", review_root)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_log))
        .stderr(Stdio::from(stderr_log))
        .spawn()
        .context("spawn local review server")?;

    Ok(())
}

fn resolve_review_server_binary(repo_root: &Path) -> Option<PathBuf> {
    if let Some(bin) = find_executable_in_path_or_sibling("opensession-server") {
        return Some(bin);
    }

    let local_debug = repo_root
        .join("target")
        .join("debug")
        .join("opensession-server");
    if local_debug.exists() {
        return Some(local_debug);
    }

    #[cfg(windows)]
    {
        let local_debug_exe = repo_root
            .join("target")
            .join("debug")
            .join("opensession-server.exe");
        if local_debug_exe.exists() {
            return Some(local_debug_exe);
        }
    }

    None
}

async fn endpoint_ok(url: &str) -> bool {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(1))
        .build()
    {
        Ok(client) => client,
        Err(_) => return false,
    };

    match client.get(url).send().await {
        Ok(response) => response.status().is_success(),
        Err(_) => false,
    }
}

fn find_executable_in_path_or_sibling(name: &str) -> Option<PathBuf> {
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(dir) = current_exe.parent() {
            let candidate = dir.join(name);
            if candidate.exists() {
                return Some(candidate);
            }
            #[cfg(windows)]
            {
                let candidate = dir.join(format!("{name}.exe"));
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }

    let which_out = Command::new("which").arg(name).output().ok()?;
    if !which_out.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&which_out.stdout)
        .trim()
        .to_string();
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

fn git_stdout(repo_root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .output()
        .with_context(|| format!("run git {}", args.join(" ")))?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn run_git(repo_root: &Path, args: &[String]) -> Result<()> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(args.iter().map(OsString::from))
        .output()
        .with_context(|| format!("run git {}", args.join(" ")))?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        build_review_id, build_reviewer_digest_for_commit, parse_github_pr_url,
        parse_remote_repo_triplet, refresh_remote_head_fetch_args, resolve_view_mode,
        sanitize_path_component, sanitize_review_id_component, GithubPrSpec, ReviewView,
    };
    use opensession_api::LocalReviewSession;
    use opensession_core::{Agent, Content, Event, EventType, Session};
    use std::collections::HashMap;

    #[test]
    fn parse_github_pr_url_accepts_standard_link() {
        let parsed = parse_github_pr_url("https://github.com/hwisu/opensession/pull/42")
            .expect("parse PR URL");
        assert_eq!(parsed.owner, "hwisu");
        assert_eq!(parsed.repo, "opensession");
        assert_eq!(parsed.number, 42);
    }

    #[test]
    fn parse_github_pr_url_rejects_non_github_host() {
        let err = parse_github_pr_url("https://gitlab.com/group/repo/-/merge_requests/1")
            .expect_err("non-github host should fail");
        assert!(err.to_string().contains("unsupported PR host"));
    }

    #[test]
    fn parse_remote_repo_triplet_handles_ssh_and_https() {
        let ssh =
            parse_remote_repo_triplet("git@github.com:Org/Repo.git").expect("parse ssh remote");
        assert_eq!(ssh.0, "github.com");
        assert_eq!(ssh.1, "Org");
        assert_eq!(ssh.2, "Repo");

        let https = parse_remote_repo_triplet("https://github.com/org/repo.git")
            .expect("parse https remote");
        assert_eq!(https.0, "github.com");
        assert_eq!(https.1, "org");
        assert_eq!(https.2, "repo");
    }

    #[test]
    fn sanitize_review_id_component_normalizes_symbols() {
        assert_eq!(
            sanitize_review_id_component("Org.Name/Repo_Name"),
            "org-name-repo-name"
        );
    }

    #[test]
    fn build_review_id_uses_head_prefix() {
        let pr = GithubPrSpec {
            owner: "org".to_string(),
            repo: "repo".to_string(),
            number: 7,
        };
        let id = build_review_id(&pr, "1234567890abcdef");
        assert_eq!(id, "gh-org-repo-pr7-1234567");
    }

    #[test]
    fn sanitize_path_component_replaces_unsafe_chars() {
        assert_eq!(sanitize_path_component("ab/cd:ef"), "ab_cd_ef");
    }

    #[test]
    fn refresh_remote_fetch_disables_prune() {
        let args = refresh_remote_head_fetch_args("origin");
        assert_eq!(
            args,
            vec![
                "fetch".to_string(),
                "--no-prune".to_string(),
                "origin".to_string()
            ]
        );
    }

    #[test]
    fn review_auto_view_resolves_to_web() {
        assert_eq!(resolve_view_mode(ReviewView::Auto), ReviewView::Web);
        assert_eq!(resolve_view_mode(ReviewView::Web), ReviewView::Web);
    }

    #[test]
    fn reviewer_digest_captures_qa_content_and_modified_test_files() {
        let mut session = Session::new(
            "session-1".to_string(),
            Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );
        let ts = chrono::Utc::now();

        let mut question_attrs = HashMap::new();
        question_attrs.insert(
            "source".to_string(),
            serde_json::Value::String("interactive_question".to_string()),
        );
        session.events.push(Event {
            event_id: "q-1".to_string(),
            timestamp: ts,
            event_type: EventType::SystemMessage,
            task_id: None,
            content: Content::text("What should we test?"),
            duration_ms: None,
            attributes: question_attrs,
        });

        let mut answer_attrs = HashMap::new();
        answer_attrs.insert(
            "source".to_string(),
            serde_json::Value::String("interactive".to_string()),
        );
        session.events.push(Event {
            event_id: "a-1".to_string(),
            timestamp: ts + chrono::Duration::seconds(1),
            event_type: EventType::UserMessage,
            task_id: None,
            content: Content::text("Please add e2e coverage."),
            duration_ms: None,
            attributes: answer_attrs,
        });

        session.events.push(Event {
            event_id: "f-1".to_string(),
            timestamp: ts + chrono::Duration::seconds(2),
            event_type: EventType::FileEdit {
                path: "crates/cli/src/review.rs".to_string(),
                diff: None,
            },
            task_id: None,
            content: Content::empty(),
            duration_ms: None,
            attributes: HashMap::new(),
        });
        session.events.push(Event {
            event_id: "f-2".to_string(),
            timestamp: ts + chrono::Duration::seconds(3),
            event_type: EventType::FileCreate {
                path: "web/e2e-live/live-review-local.spec.ts".to_string(),
            },
            task_id: None,
            content: Content::empty(),
            duration_ms: None,
            attributes: HashMap::new(),
        });

        let digest = build_reviewer_digest_for_commit(
            &[LocalReviewSession {
                session_id: "session-1".to_string(),
                ledger_ref: "refs/remotes/origin/opensession/branches/main".to_string(),
                hail_path: "v1/sr/session-1.hail.jsonl".to_string(),
                commit_shas: vec!["a".repeat(40)],
                session,
            }],
            &"a".repeat(40),
        );

        assert_eq!(digest.qa.len(), 1);
        assert_eq!(digest.qa[0].question, "What should we test?");
        assert_eq!(
            digest.qa[0].answer.as_deref(),
            Some("Please add e2e coverage.")
        );
        assert_eq!(digest.modified_files.len(), 2);
        assert_eq!(
            digest.test_files,
            vec!["web/e2e-live/live-review-local.spec.ts".to_string()]
        );
    }
}
