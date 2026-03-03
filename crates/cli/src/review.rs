use crate::open_target::{read_repo_open_target, OpenTarget};
use anyhow::{anyhow, bail, Context, Result};
use clap::{Args, ValueEnum};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use opensession_api::{
    LocalReviewBundle, LocalReviewCommit, LocalReviewPrMeta, LocalReviewSession,
};
use opensession_core::{object_store::global_store_root, Session};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};
use reqwest::Url;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{stdout, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) const LOCAL_REVIEW_ROOT_DIR: &str = ".opensession/review";
pub(crate) const LOCAL_REVIEW_SERVER_BASE_URL: &str = "http://127.0.0.1:8788";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpenMode {
    Auto,
    App,
    Web,
}

#[derive(Debug, Clone, Args)]
pub struct ReviewArgs {
    /// GitHub PR URL (`https://github.com/<owner>/<repo>/pull/<number>`).
    pub pr_link: String,
    /// Review view mode (`auto` picks TUI when attached to a terminal).
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
    Tui,
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

pub async fn run(args: ReviewArgs) -> Result<()> {
    let repo_root = resolve_repo_root(args.repo.as_deref())?;
    let pr = parse_github_pr_url(&args.pr_link)?;
    let remote = resolve_matching_remote(&repo_root, &pr)?;

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
            cached
        } else {
            let built = build_review_bundle(
                &repo_root,
                &args.pr_link,
                &pr,
                &remote.name,
                &base_sha,
                &pr_head_sha,
                commits,
            )?;
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
        ReviewView::Tui => run_review_tui(&bundle, &repo_root)?,
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

fn build_review_bundle(
    repo_root: &Path,
    pr_url: &str,
    pr: &GithubPrSpec,
    remote: &str,
    base_sha: &str,
    head_sha: &str,
    commits: Vec<CommitInfo>,
) -> Result<LocalReviewBundle> {
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

        commit_rows.push(LocalReviewCommit {
            sha: commit.sha,
            title: commit.title,
            author_name: commit.author_name,
            author_email: commit.author_email,
            authored_at: commit.authored_at,
            session_ids: session_ids_for_commit,
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
    if requested != ReviewView::Auto {
        return requested;
    }

    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        ReviewView::Tui
    } else {
        ReviewView::Web
    }
}

fn run_review_tui(bundle: &LocalReviewBundle, repo_root: &Path) -> Result<()> {
    enable_raw_mode().context("enable raw terminal mode")?;
    let mut out = stdout();
    out.execute(EnterAlternateScreen)
        .context("enter alternate screen")?;
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend).context("initialize terminal backend")?;

    let run_result = run_review_tui_loop(&mut terminal, bundle, repo_root);

    disable_raw_mode().ok();
    terminal.backend_mut().execute(LeaveAlternateScreen).ok();
    terminal.show_cursor().ok();

    run_result
}

fn run_review_tui_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    bundle: &LocalReviewBundle,
    repo_root: &Path,
) -> Result<()> {
    let mut state = ReviewTuiState::default();

    loop {
        terminal
            .draw(|frame| render_review_tui(frame, bundle, &state))
            .context("draw review TUI frame")?;

        if !event::poll(Duration::from_millis(120)).context("poll TUI events")? {
            continue;
        }
        let Event::Key(key) = event::read().context("read TUI event")? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => return Ok(()),
            KeyCode::Enter => {
                let Some(commit) = bundle.commits.get(state.selected_commit) else {
                    continue;
                };
                let Some(session) =
                    resolve_selected_session(bundle, commit, state.selected_session)
                else {
                    continue;
                };
                with_suspended_tui(terminal, || open_session_in_tui(repo_root, session))?;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                state.selected_commit = state.selected_commit.saturating_sub(1);
                state.selected_session = 0;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !bundle.commits.is_empty() {
                    state.selected_commit =
                        (state.selected_commit + 1).min(bundle.commits.len().saturating_sub(1));
                    state.selected_session = 0;
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                state.selected_session = state.selected_session.saturating_sub(1);
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if let Some(commit) = bundle.commits.get(state.selected_commit) {
                    if !commit.session_ids.is_empty() {
                        state.selected_session = (state.selected_session + 1)
                            .min(commit.session_ids.len().saturating_sub(1));
                    }
                }
            }
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Default)]
struct ReviewTuiState {
    selected_commit: usize,
    selected_session: usize,
}

fn render_review_tui(frame: &mut Frame<'_>, bundle: &LocalReviewBundle, state: &ReviewTuiState) {
    let outer = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).split(frame.area());
    let title = format!(
        " PR #{} {}/{}  commits:{}  sessions:{}  (q: quit, j/k: commits, h/l: sessions, Enter: open) ",
        bundle.pr.number,
        bundle.pr.owner,
        bundle.pr.repo,
        bundle.commits.len(),
        bundle.sessions.len()
    );
    let header = Paragraph::new(title).block(Block::default().borders(Borders::ALL));
    frame.render_widget(header, outer[0]);

    let content = Layout::horizontal([Constraint::Percentage(36), Constraint::Percentage(64)])
        .split(outer[1]);
    render_commit_panel(frame, content[0], bundle, state.selected_commit);
    render_session_panel(
        frame,
        content[1],
        bundle,
        state.selected_commit,
        state.selected_session,
    );
}

fn with_suspended_tui<F>(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    op: F,
) -> Result<()>
where
    F: FnOnce() -> Result<()>,
{
    disable_raw_mode().ok();
    terminal.backend_mut().execute(LeaveAlternateScreen).ok();
    terminal.show_cursor().ok();

    let op_result = op();

    terminal.backend_mut().execute(EnterAlternateScreen).ok();
    enable_raw_mode().ok();
    terminal.clear().ok();

    op_result
}

fn open_session_in_tui(repo_root: &Path, session: &LocalReviewSession) -> Result<()> {
    let hail = session
        .session
        .to_jsonl()
        .context("serialize selected review session as canonical HAIL JSONL")?;
    let temp_path = write_temp_review_session(&session.session_id, &hail)?;

    let status = if let Some(bin) = resolve_review_tui_binary(repo_root) {
        Command::new(bin)
            .arg(&temp_path)
            .status()
            .context("launch opensession-tui for selected review session")?
    } else if repo_root.join("Cargo.toml").exists() {
        Command::new("cargo")
            .current_dir(repo_root)
            .arg("run")
            .arg("-p")
            .arg("opensession-tui")
            .arg("--")
            .arg(&temp_path)
            .status()
            .context("launch opensession-tui via cargo fallback")?
    } else {
        let _ = std::fs::remove_file(&temp_path);
        bail!(
            "could not find `opensession-tui`; install it or run from a workspace build directory"
        );
    };

    let _ = std::fs::remove_file(&temp_path);
    if !status.success() {
        bail!("opensession-tui exited with non-zero status: {}", status);
    }
    Ok(())
}

fn resolve_review_tui_binary(repo_root: &Path) -> Option<PathBuf> {
    if let Some(bin) = find_executable_in_path_or_sibling("opensession-tui") {
        return Some(bin);
    }

    let local_debug = repo_root
        .join("target")
        .join("debug")
        .join("opensession-tui");
    if local_debug.exists() {
        return Some(local_debug);
    }

    #[cfg(windows)]
    {
        let local_debug_exe = repo_root
            .join("target")
            .join("debug")
            .join("opensession-tui.exe");
        if local_debug_exe.exists() {
            return Some(local_debug_exe);
        }
    }

    None
}

fn write_temp_review_session(session_id: &str, body: &str) -> Result<PathBuf> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let file_name = format!(
        "opensession-review-{}-{}-{}.hail.jsonl",
        sanitize_path_component(session_id),
        std::process::id(),
        nonce
    );
    let path = std::env::temp_dir().join(file_name);
    std::fs::write(&path, body)
        .with_context(|| format!("write temporary review session file {}", path.display()))?;
    Ok(path)
}

fn render_commit_panel(
    frame: &mut Frame<'_>,
    area: ratatui::layout::Rect,
    bundle: &LocalReviewBundle,
    selected_commit: usize,
) {
    let items = if bundle.commits.is_empty() {
        vec![ListItem::new(Line::from("No commits in PR range"))]
    } else {
        bundle
            .commits
            .iter()
            .map(|commit| {
                let short = commit.sha.chars().take(7).collect::<String>();
                let label = format!(
                    "{short}  {}  [{} sessions]",
                    truncate_for_tui(&commit.title, 42),
                    commit.session_ids.len()
                );
                ListItem::new(Line::from(label))
            })
            .collect::<Vec<_>>()
    };

    let mut state = ListState::default();
    if !bundle.commits.is_empty() {
        state.select(Some(selected_commit.min(bundle.commits.len() - 1)));
    }

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Commits "))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_session_panel(
    frame: &mut Frame<'_>,
    area: ratatui::layout::Rect,
    bundle: &LocalReviewBundle,
    selected_commit: usize,
    selected_session: usize,
) {
    let columns = Layout::vertical([Constraint::Length(9), Constraint::Min(1)]).split(area);

    let Some(commit) = bundle.commits.get(selected_commit) else {
        let empty = Paragraph::new("No commit selected")
            .block(Block::default().borders(Borders::ALL).title(" Sessions "));
        frame.render_widget(empty, columns[0]);
        return;
    };

    let session_items = if commit.session_ids.is_empty() {
        vec![ListItem::new(Line::from(
            "No mapped sessions for this commit",
        ))]
    } else {
        commit
            .session_ids
            .iter()
            .map(|id| ListItem::new(Line::from(id.clone())))
            .collect::<Vec<_>>()
    };
    let mut list_state = ListState::default();
    if !commit.session_ids.is_empty() {
        list_state.select(Some(selected_session.min(commit.session_ids.len() - 1)));
    }

    let list = List::new(session_items)
        .block(Block::default().borders(Borders::ALL).title(" Sessions "))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));
    frame.render_stateful_widget(list, columns[0], &mut list_state);

    let details = resolve_selected_session(bundle, commit, selected_session)
        .map(render_session_detail_lines)
        .unwrap_or_else(|| {
            vec![
                "No session payload for this commit.".to_string(),
                "".to_string(),
                format!("commit: {}", commit.sha),
                format!("title: {}", commit.title),
            ]
        });
    let paragraph = Paragraph::new(details.join("\n"))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Session Detail "),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, columns[1]);
}

fn resolve_selected_session<'a>(
    bundle: &'a LocalReviewBundle,
    commit: &'a LocalReviewCommit,
    selected_session: usize,
) -> Option<&'a LocalReviewSession> {
    let session_id = commit
        .session_ids
        .get(selected_session.min(commit.session_ids.len().saturating_sub(1)))?;
    bundle.sessions.iter().find(|row| {
        row.session_id == *session_id && row.commit_shas.iter().any(|sha| sha == &commit.sha)
    })
}

fn render_session_detail_lines(session: &LocalReviewSession) -> Vec<String> {
    let summary = first_session_summary_line(&session.session);
    vec![
        format!("session_id: {}", session.session_id),
        format!(
            "tool/model: {} / {}",
            session.session.agent.tool, session.session.agent.model
        ),
        format!(
            "stats: events={} messages={} tasks={}",
            session.session.stats.event_count,
            session.session.stats.message_count,
            session.session.stats.task_count
        ),
        format!(
            "tokens: in={} out={}",
            session.session.stats.total_input_tokens, session.session.stats.total_output_tokens
        ),
        format!("ledger_ref: {}", session.ledger_ref),
        format!("hail_path: {}", session.hail_path),
        format!(
            "mapped_commits: {}",
            if session.commit_shas.is_empty() {
                "(none)".to_string()
            } else {
                session.commit_shas.join(", ")
            }
        ),
        "".to_string(),
        format!(
            "summary: {}",
            summary.unwrap_or_else(|| "(none)".to_string())
        ),
    ]
}

fn first_session_summary_line(session: &Session) -> Option<String> {
    for event in &session.events {
        for block in &event.content.blocks {
            if let opensession_core::ContentBlock::Text { text } = block {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    return Some(truncate_for_tui(trimmed, 140));
                }
            }
        }
    }
    None
}

fn truncate_for_tui(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_string();
    }
    let mut out = String::new();
    for ch in value.chars().take(limit.saturating_sub(1)) {
        out.push(ch);
    }
    out.push('…');
    out
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

pub(crate) fn open_url_for_repo(repo_root: &Path, url: &str) -> Result<()> {
    let mode = match read_repo_open_target(repo_root) {
        Ok(Some(OpenTarget::App)) => OpenMode::App,
        Ok(Some(OpenTarget::Web)) => OpenMode::Web,
        Ok(None) => OpenMode::Auto,
        Err(err) => {
            eprintln!(
                "[opensession] failed to read repo open target ({err}); using auto open mode"
            );
            OpenMode::Auto
        }
    };
    open_url_with_mode(url, mode)
}

pub(crate) fn open_url_in_browser(url: &str) -> Result<()> {
    open_url_with_mode(url, OpenMode::Auto)
}

fn open_url_with_mode(url: &str, mode: OpenMode) -> Result<()> {
    if matches!(mode, OpenMode::App | OpenMode::Auto) {
        match try_open_in_desktop_app_for_url(url) {
            Ok(true) => return Ok(()),
            Ok(false) => {
                if matches!(mode, OpenMode::App) {
                    bail!(
                        "open target is set to `app`, but OpenSession Desktop is unavailable. next: install the desktop app or run `git config --local opensession.open-target web`"
                    );
                }
            }
            Err(err) => {
                if matches!(mode, OpenMode::App) {
                    return Err(err.context("failed to open OpenSession Desktop"));
                }
                eprintln!(
                    "[opensession] desktop app launch failed ({err}); falling back to browser open"
                );
            }
        }
    }
    if matches!(mode, OpenMode::App) {
        bail!(
            "open target is set to `app`, but this URL is not routable in the desktop app. next: run `git config --local opensession.open-target web`"
        );
    }

    #[cfg(target_os = "macos")]
    {
        let status = Command::new("open")
            .arg(url)
            .status()
            .context("launch browser via `open`")?;
        if status.success() {
            return Ok(());
        }
    }

    #[cfg(target_os = "linux")]
    {
        let status = Command::new("xdg-open")
            .arg(url)
            .status()
            .context("launch browser via `xdg-open`")?;
        if status.success() {
            return Ok(());
        }
    }

    #[cfg(target_os = "windows")]
    {
        let status = Command::new("cmd")
            .arg("/C")
            .arg("start")
            .arg("")
            .arg(url)
            .status()
            .context("launch browser via `start`")?;
        if status.success() {
            return Ok(());
        }
    }

    bail!("failed to open browser automatically")
}

fn desktop_launch_route_from_url(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return None;
    }

    let path = parsed.path();
    let supported = path == "/sessions"
        || path.starts_with("/session/")
        || path.starts_with("/review/local/")
        || path.starts_with("/src/");
    if !supported {
        return None;
    }

    let mut route = path.to_string();
    if let Some(query) = parsed.query() {
        route.push('?');
        route.push_str(query);
    }
    if let Some(fragment) = parsed.fragment() {
        route.push('#');
        route.push_str(fragment);
    }
    Some(route)
}

fn desktop_launch_route_path() -> Result<PathBuf> {
    let store_root = global_store_root().context("resolve global store root")?;
    let opensession_root = store_root.parent().ok_or_else(|| {
        anyhow!(
            "invalid global store root path: {}",
            store_root.to_string_lossy()
        )
    })?;
    Ok(opensession_root.join("desktop").join("launch-route"))
}

fn write_desktop_launch_route(route: &str) -> Result<PathBuf> {
    let path = desktop_launch_route_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create desktop launch dir {}", parent.display()))?;
    }
    fs::write(&path, route)
        .with_context(|| format!("write desktop launch route {}", path.display()))?;
    Ok(path)
}

pub(crate) fn try_open_in_desktop_app_for_url(url: &str) -> Result<bool> {
    let Some(route) = desktop_launch_route_from_url(url) else {
        return Ok(false);
    };

    let route_path = write_desktop_launch_route(&route)?;
    let launched = try_launch_desktop_app()?;
    if !launched {
        let _ = fs::remove_file(route_path);
    }
    Ok(launched)
}

fn try_launch_desktop_app() -> Result<bool> {
    #[cfg(target_os = "macos")]
    {
        let attempts: [&[&str]; 2] = [
            &["-b", "io.opensession.desktop"],
            &["-a", "OpenSession Desktop"],
        ];
        for args in attempts {
            let status = Command::new("open")
                .args(args)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            if let Ok(status) = status {
                if status.success() {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    #[cfg(not(target_os = "macos"))]
    {
        Ok(false)
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
        build_review_id, desktop_launch_route_from_url, parse_github_pr_url,
        parse_remote_repo_triplet, refresh_remote_head_fetch_args, sanitize_path_component,
        sanitize_review_id_component, GithubPrSpec,
    };

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
    fn write_temp_review_session_creates_hail_jsonl_file() {
        let body = r#"{"type":"header","version":"hail-1.0.0","session_id":"s"}"#;
        let path = super::write_temp_review_session("session/id", body).expect("write temp file");
        let file_name = path
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or_default();
        assert!(file_name.ends_with(".hail.jsonl"));
        let loaded = std::fs::read_to_string(&path).expect("read temp file");
        assert_eq!(loaded, body);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn desktop_launch_route_from_sessions_url_preserves_query() {
        let route = desktop_launch_route_from_url(
            "http://127.0.0.1:8788/sessions?git_repo_name=acme%2Frepo",
        )
        .expect("route from sessions url");
        assert_eq!(route, "/sessions?git_repo_name=acme%2Frepo");
    }

    #[test]
    fn desktop_launch_route_rejects_unhandled_urls() {
        assert_eq!(
            desktop_launch_route_from_url("https://example.com/docs"),
            None
        );
        assert_eq!(
            desktop_launch_route_from_url("opensession://sessions"),
            None
        );
    }
}
