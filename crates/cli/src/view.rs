use crate::{
    config_cmd::load_repo_config,
    open_target::{read_repo_open_target, OpenTarget},
    review, url_opener,
    user_guidance::guided_error,
};
use anyhow::{anyhow, bail, Context, Result};
use clap::Args;
use opensession_api::{
    LocalReviewBundle, LocalReviewCommit, LocalReviewPrMeta, LocalReviewSession,
};
use opensession_core::{
    object_store::read_local_object_from_uri,
    source_uri::{SourceSpec, SourceUri},
    Session,
};
use reqwest::Url;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Args)]
#[command(after_long_help = r"Recovery examples:
  opensession view --no-open
  opensession view os://src/local/<sha256> --no-open
  opensession view ./session.hail.jsonl --no-open
  opensession view HEAD~3..HEAD --no-open")]
pub struct ViewArgs {
    /// Review target: source URI, local *.jsonl file, PR/MR URL, or commit/ref/range.
    pub target: Option<String>,
    /// Prefer TUI mode when supported by the target.
    #[arg(long)]
    pub tui: bool,
    /// Do not open a browser window.
    #[arg(long)]
    pub no_open: bool,
    /// Print machine-readable output.
    #[arg(long)]
    pub json: bool,
    /// Repository path override.
    #[arg(long)]
    pub repo: Option<PathBuf>,
    /// Skip remote fetch when resolving URL targets.
    #[arg(long)]
    pub no_fetch: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommitInfo {
    sha: String,
    title: String,
    author_name: String,
    author_email: String,
    authored_at: String,
}

#[derive(Debug, Deserialize)]
struct CommitIndexEntry {
    session_id: String,
    hail_path: String,
}

pub async fn run(args: ViewArgs) -> Result<()> {
    let Some(target) = args.target.clone() else {
        return view_repo_sessions(&args).await;
    };

    if is_github_pr_url(&target) {
        let review_args = review::ReviewArgs {
            pr_link: target.clone(),
            view: if args.tui {
                review::ReviewView::Tui
            } else {
                review::ReviewView::Web
            },
            repo: args.repo.clone(),
            no_fetch: args.no_fetch,
            json: args.json,
        };
        return review::run(review_args).await;
    }

    if args.tui {
        return Err(guided_error(
            "`view --tui` is currently supported for GitHub PR URLs only",
            [
                "use web mode for other targets: `opensession view <target>`",
                "or provide a GitHub PR URL with `--tui`",
            ],
        ));
    }

    if let Ok(uri) = SourceUri::parse(&target) {
        return view_source_uri(uri, &args).await;
    }

    let target_path = PathBuf::from(&target);
    if target_path.exists() && target_path.is_file() {
        return view_jsonl_file(&target_path, &args).await;
    }

    if let Some(commit_sha) = parse_commit_url(&target) {
        return view_commit_target(&commit_sha, &args).await;
    }
    if let Some(mr_number) = parse_gitlab_mr_url(&target) {
        return view_gitlab_mr(mr_number, &args).await;
    }

    view_commit_target(&target, &args).await.map_err(|err| {
        guided_error(
            format!("unable to resolve view target `{}`: {err}", target),
            [
                "for source URIs: `opensession view os://src/... --no-open`".to_string(),
                "for local files: `opensession view ./session.hail.jsonl --no-open`".to_string(),
                "for commits/ranges: `opensession view HEAD` or `opensession view main..feature`"
                    .to_string(),
            ],
        )
    })
}

async fn view_repo_sessions(args: &ViewArgs) -> Result<()> {
    if args.tui {
        return Err(guided_error(
            "`view --tui` currently requires an explicit GitHub PR URL target",
            [
                "provide a PR URL: `opensession view https://github.com/<owner>/<repo>/pull/<number> --tui`",
                "or use web mode without target: `opensession view`",
            ],
        ));
    }

    let repo_root = resolve_repo_root_required(args.repo.as_deref()).map_err(|err| {
        guided_error(
            format!("`opensession view` without a target requires a git repository: {err}"),
            [
                "run the command from inside a git repository",
                "or pass an explicit repository path: `opensession view --repo /path/to/repo`",
                "or provide an explicit target: `opensession view HEAD`",
            ],
        )
    })?;

    let repo_name = detect_repo_identity(&repo_root).map(|(owner, repo)| format!("{owner}/{repo}"));
    let mut sessions_base = review::LOCAL_REVIEW_SERVER_BASE_URL.to_string();
    let configured_open_target = match read_repo_open_target(&repo_root) {
        Ok(target) => target,
        Err(err) => {
            eprintln!(
                "[opensession] failed to read repo open target ({err}); using default auto behavior"
            );
            None
        }
    };
    if !args.no_open {
        if let Err(err) = ensure_sessions_web_server(&repo_root).await {
            let local_url =
                build_sessions_url(review::LOCAL_REVIEW_SERVER_BASE_URL, repo_name.as_deref())?;
            if matches!(configured_open_target, Some(OpenTarget::App)) {
                match url_opener::try_open_in_desktop_app_for_url(&local_url) {
                    Ok(true) => {
                        let mut print_args = args.clone();
                        print_args.no_open = true;
                        print_view_result(
                            "(default)",
                            "sessions",
                            None,
                            Some(local_url),
                            &print_args,
                            Some(&repo_root),
                        )?;
                        return Ok(());
                    }
                    Ok(false) => {
                        return Err(guided_error(
                            format!(
                                "open target is set to `app`, but OpenSession Desktop is unavailable while local sessions server is down: {err}"
                            ),
                            [
                                "install OpenSession Desktop and retry",
                                "or switch opener to web: `git config --local opensession.open-target web`",
                            ],
                        ));
                    }
                    Err(desktop_err) => {
                        return Err(guided_error(
                            format!(
                                "open target is set to `app`, but desktop launch failed while local sessions server is down: {desktop_err}"
                            ),
                            [
                                "check desktop app installation and retry",
                                "or switch opener to web: `git config --local opensession.open-target web`",
                            ],
                        ));
                    }
                }
            } else if !matches!(configured_open_target, Some(OpenTarget::Web)) {
                match url_opener::try_open_in_desktop_app_for_url(&local_url) {
                    Ok(true) => {
                        let mut print_args = args.clone();
                        print_args.no_open = true;
                        print_view_result(
                            "(default)",
                            "sessions",
                            None,
                            Some(local_url),
                            &print_args,
                            Some(&repo_root),
                        )?;
                        return Ok(());
                    }
                    Ok(false) => {}
                    Err(desktop_err) => eprintln!(
                        "[opensession] desktop app launch failed ({desktop_err}); trying web fallbacks"
                    ),
                }
            }

            let fallback = match load_repo_config(&repo_root) {
                Ok((_config_path, config)) => {
                    let trimmed = config.share.base_url.trim_end_matches('/').to_string();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed)
                    }
                }
                Err(_) => None,
            };
            if let Some(fallback) = fallback {
                eprintln!(
                    "[opensession] local sessions server unavailable ({err}); falling back to configured base URL: {fallback}"
                );
                sessions_base = fallback;
            } else {
                return Err(guided_error(
                    format!(
                        "local sessions server is unavailable and no explicit web base URL is configured: {err}"
                    ),
                    [
                        "for local-only output: `opensession view --no-open`",
                        "run `opensession-server` (or in source checkout: `cargo run -p opensession-server --`) and retry",
                        "or explicitly configure web base URL: `opensession config init --base-url <url>`",
                    ],
                ));
            }
        }
    }

    let url = build_sessions_url(&sessions_base, repo_name.as_deref())?;
    print_view_result(
        "(default)",
        "sessions",
        None,
        Some(url),
        args,
        Some(&repo_root),
    )
}

fn build_sessions_url(base_url: &str, repo_name: Option<&str>) -> Result<String> {
    let mut url = Url::parse(&format!("{}/sessions", base_url.trim_end_matches('/')))
        .context("build sessions URL")?;
    if let Some(repo_name) = repo_name.map(str::trim).filter(|value| !value.is_empty()) {
        url.query_pairs_mut()
            .append_pair("git_repo_name", repo_name);
    }
    Ok(url.to_string())
}

async fn ensure_sessions_web_server(repo_root: &Path) -> Result<()> {
    let bundle = build_sessions_bootstrap_bundle(repo_root);
    let _ = persist_and_resolve_local_url(repo_root, &bundle, false).await?;
    Ok(())
}

fn build_sessions_bootstrap_bundle(repo_root: &Path) -> LocalReviewBundle {
    let repo_slug = repo_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("repo");
    let review_id = format!(
        "sessions-{}",
        sanitize_review_component(repo_slug).trim_matches('-')
    );
    let (owner, repo) = detect_repo_identity(repo_root)
        .unwrap_or_else(|| ("local".to_string(), repo_slug.to_string()));
    let now = chrono::Utc::now().to_rfc3339();
    LocalReviewBundle {
        review_id,
        generated_at: now.clone(),
        pr: LocalReviewPrMeta {
            url: "sessions".to_string(),
            owner,
            repo,
            number: 0,
            remote: "local".to_string(),
            base_sha: "local".to_string(),
            head_sha: "local".to_string(),
        },
        commits: vec![LocalReviewCommit {
            sha: "local".to_string(),
            title: "session list bootstrap".to_string(),
            author_name: "local".to_string(),
            author_email: String::new(),
            authored_at: now,
            session_ids: vec![],
        }],
        sessions: vec![],
    }
}

async fn view_source_uri(uri: SourceUri, args: &ViewArgs) -> Result<()> {
    match uri {
        SourceUri::Src(SourceSpec::Local { .. }) => {
            let cwd = resolve_working_dir(args.repo.as_deref())?;
            let (_path, bytes) = read_local_object_from_uri(&uri, &cwd)?;
            let session = decode_session_bytes(&bytes)?;
            let bundle = build_bundle_from_sessions("local source uri", vec![session]);
            let review_id = bundle.review_id.clone();
            let repo_root = resolve_runtime_root(args.repo.as_deref())?;
            let url = persist_and_resolve_local_url(&repo_root, &bundle, args.no_open).await?;
            print_view_result(
                args.target.as_deref().unwrap_or("(default)"),
                "local",
                Some(review_id.as_str()),
                Some(url),
                args,
                Some(&repo_root),
            )?;
            Ok(())
        }
        SourceUri::Src(_) => {
            let cwd = resolve_working_dir(args.repo.as_deref())?;
            let path = uri
                .to_web_path()
                .ok_or_else(|| anyhow!("uri cannot be resolved to web path"))?;
            let base_url = match load_repo_config(&cwd) {
                Ok((_path, config)) => config.share.base_url.trim_end_matches('/').to_string(),
                Err(_) => review::LOCAL_REVIEW_SERVER_BASE_URL.to_string(),
            };
            let url = format!("{base_url}{path}");
            let open_repo_root = resolve_repo_root_required(args.repo.as_deref()).ok();
            print_view_result(
                args.target.as_deref().unwrap_or("(default)"),
                "source",
                None,
                Some(url),
                args,
                open_repo_root.as_deref(),
            )?;
            Ok(())
        }
        _ => bail!("unsupported target URI"),
    }
}

async fn view_jsonl_file(path: &Path, args: &ViewArgs) -> Result<()> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let session = decode_session_bytes(&bytes)?;
    let label = format!("file:{}", path.display());
    let bundle = build_bundle_from_sessions(&label, vec![session]);
    let review_id = bundle.review_id.clone();
    let repo_root = resolve_runtime_root(args.repo.as_deref())?;
    let url = persist_and_resolve_local_url(&repo_root, &bundle, args.no_open).await?;
    print_view_result(
        args.target.as_deref().unwrap_or("(default)"),
        "jsonl",
        Some(review_id.as_str()),
        Some(url),
        args,
        Some(&repo_root),
    )?;
    Ok(())
}

async fn view_gitlab_mr(number: u64, args: &ViewArgs) -> Result<()> {
    let repo_root = resolve_repo_root_required(args.repo.as_deref())?;
    let remote = "origin";
    let head_ref = format!("refs/opensession/view/mr/{number}/head");
    if !args.no_fetch {
        git(
            &repo_root,
            &[
                "fetch",
                remote,
                &format!("+refs/merge-requests/{number}/head:{head_ref}"),
            ],
        )
        .with_context(|| format!("fetch merge request {number} head ref"))?;
    }

    let head_sha = resolve_commit_oid(&repo_root, &head_ref).with_context(|| {
        format!(
            "merge request head ref `{head_ref}` is unavailable. Retry without --no-fetch or verify remote permissions."
        )
    })?;
    let base_ref = resolve_default_remote_ref(&repo_root, remote)?;
    let base_sha = merge_base(&repo_root, &head_sha, &base_ref)?;
    let commits = rev_list_range(&repo_root, &base_sha, &head_sha)?;

    let bundle = build_bundle_from_commits(
        &repo_root,
        &format!("gitlab-mr-{number}"),
        commits,
        &format!("gitlab mr {number}"),
    )?;
    let review_id = bundle.review_id.clone();
    let url = persist_and_resolve_local_url(&repo_root, &bundle, args.no_open).await?;
    print_view_result(
        args.target.as_deref().unwrap_or("(default)"),
        "mr",
        Some(review_id.as_str()),
        Some(url),
        args,
        Some(&repo_root),
    )?;
    Ok(())
}

async fn view_commit_target(target: &str, args: &ViewArgs) -> Result<()> {
    let repo_root = resolve_repo_root_required(args.repo.as_deref())?;
    let commits = resolve_commit_target_set(&repo_root, target)?;
    if commits.is_empty() {
        bail!("no commits resolved from target `{target}`");
    }

    let bundle = build_bundle_from_commits(&repo_root, target, commits, "commit-linked review")?;
    let review_id = bundle.review_id.clone();
    let url = persist_and_resolve_local_url(&repo_root, &bundle, args.no_open).await?;
    print_view_result(
        target,
        "commit",
        Some(review_id.as_str()),
        Some(url),
        args,
        Some(&repo_root),
    )?;
    Ok(())
}

fn resolve_runtime_root(repo_override: Option<&Path>) -> Result<PathBuf> {
    if let Some(repo) = repo_override {
        return Ok(if repo.is_absolute() {
            repo.to_path_buf()
        } else {
            std::env::current_dir()?.join(repo)
        });
    }
    std::env::current_dir().context("read current directory")
}

fn resolve_working_dir(repo_override: Option<&Path>) -> Result<PathBuf> {
    resolve_runtime_root(repo_override)
}

fn resolve_repo_root_required(repo_override: Option<&Path>) -> Result<PathBuf> {
    let root = resolve_runtime_root(repo_override)?;
    opensession_git_native::ops::find_repo_root(&root)
        .ok_or_else(|| anyhow!("`{}` is not inside a git repository", root.display()))
}

fn decode_session_bytes(bytes: &[u8]) -> Result<Session> {
    let text = String::from_utf8(bytes.to_vec()).context("session payload is not UTF-8")?;
    Session::from_jsonl(&text)
        .or_else(|_| serde_json::from_str(&text).context("parse session JSON"))
}

fn build_bundle_from_sessions(label: &str, sessions: Vec<Session>) -> LocalReviewBundle {
    let review_id = format!(
        "local-{}",
        sanitize_review_component(&format!("{}-{}", label, chrono::Utc::now().timestamp()))
    );
    let commit_sha = "local".to_string();
    let session_rows = sessions
        .into_iter()
        .map(|session| LocalReviewSession {
            session_id: session.session_id.clone(),
            ledger_ref: "local".to_string(),
            hail_path: "inline".to_string(),
            commit_shas: vec![commit_sha.clone()],
            session,
        })
        .collect::<Vec<_>>();
    let session_ids = session_rows
        .iter()
        .map(|session| session.session_id.clone())
        .collect::<Vec<_>>();

    LocalReviewBundle {
        review_id,
        generated_at: chrono::Utc::now().to_rfc3339(),
        pr: LocalReviewPrMeta {
            url: label.to_string(),
            owner: "local".to_string(),
            repo: "review".to_string(),
            number: 0,
            remote: "local".to_string(),
            base_sha: commit_sha.clone(),
            head_sha: commit_sha.clone(),
        },
        commits: vec![LocalReviewCommit {
            sha: commit_sha,
            title: label.to_string(),
            author_name: "local".to_string(),
            author_email: String::new(),
            authored_at: chrono::Utc::now().to_rfc3339(),
            session_ids,
        }],
        sessions: session_rows,
    }
}

fn build_bundle_from_commits(
    repo_root: &Path,
    target: &str,
    commit_shas: Vec<String>,
    title_prefix: &str,
) -> Result<LocalReviewBundle> {
    let infos = load_commit_infos(repo_root, &commit_shas)?;
    let ledger_refs = list_ledger_refs(repo_root)?;
    let mut sessions = Vec::<LocalReviewSession>::new();
    let mut session_key_to_index = HashMap::<String, usize>::new();
    let mut commit_rows = Vec::<LocalReviewCommit>::with_capacity(infos.len());

    for info in infos {
        let mut session_ids_for_commit = Vec::<String>::new();
        let mut seen = HashSet::<String>::new();
        let index_prefix = format!("v1/index/commits/{}/", sanitize_path_component(&info.sha));

        for ledger_ref in &ledger_refs {
            let index_paths = list_tree_paths(repo_root, ledger_ref, &index_prefix)?;
            for index_path in index_paths {
                if !index_path.ends_with(".json") {
                    continue;
                }
                let index_raw = git_show_file(repo_root, ledger_ref, &index_path)?;
                let index_entry: CommitIndexEntry = serde_json::from_str(&index_raw)
                    .with_context(|| format!("parse commit index `{index_path}`"))?;
                let key = format!(
                    "{}\n{}\n{}",
                    index_entry.session_id, ledger_ref, index_entry.hail_path
                );
                let session_index = if let Some(existing) = session_key_to_index.get(&key).copied()
                {
                    existing
                } else {
                    let hail_raw = git_show_file(repo_root, ledger_ref, &index_entry.hail_path)?;
                    let session = Session::from_jsonl(&hail_raw).with_context(|| {
                        format!("parse HAIL payload `{}`", index_entry.hail_path)
                    })?;
                    sessions.push(LocalReviewSession {
                        session_id: index_entry.session_id.clone(),
                        ledger_ref: ledger_ref.clone(),
                        hail_path: index_entry.hail_path.clone(),
                        commit_shas: vec![info.sha.clone()],
                        session,
                    });
                    let idx = sessions.len() - 1;
                    session_key_to_index.insert(key, idx);
                    idx
                };
                let row = sessions
                    .get_mut(session_index)
                    .ok_or_else(|| anyhow!("invalid session index while building bundle"))?;
                if !row.commit_shas.iter().any(|sha| sha == &info.sha) {
                    row.commit_shas.push(info.sha.clone());
                }
                if seen.insert(index_entry.session_id.clone()) {
                    session_ids_for_commit.push(index_entry.session_id);
                }
            }
        }

        commit_rows.push(LocalReviewCommit {
            sha: info.sha,
            title: if info.title.trim().is_empty() {
                title_prefix.to_string()
            } else {
                info.title
            },
            author_name: info.author_name,
            author_email: info.author_email,
            authored_at: info.authored_at,
            session_ids: session_ids_for_commit,
        });
    }

    let head_sha = commit_rows
        .last()
        .map(|row| row.sha.clone())
        .unwrap_or_else(|| "unknown".to_string());
    let base_sha = commit_rows
        .first()
        .map(|row| row.sha.clone())
        .unwrap_or_else(|| head_sha.clone());
    let review_id = format!("commit-{}", head_sha.chars().take(12).collect::<String>());
    let (owner, repo) = detect_repo_identity(repo_root)
        .unwrap_or_else(|| ("local".to_string(), "repo".to_string()));

    Ok(LocalReviewBundle {
        review_id,
        generated_at: chrono::Utc::now().to_rfc3339(),
        pr: LocalReviewPrMeta {
            url: target.to_string(),
            owner,
            repo,
            number: 0,
            remote: "local".to_string(),
            base_sha,
            head_sha,
        },
        commits: commit_rows,
        sessions,
    })
}

fn detect_repo_identity(repo_root: &Path) -> Option<(String, String)> {
    let remote = git_stdout(repo_root, &["remote", "get-url", "origin"]).ok()?;
    parse_remote_repo_triplet(remote.trim())
}

fn parse_remote_repo_triplet(remote_url: &str) -> Option<(String, String)> {
    if let Some(rest) = remote_url.strip_prefix("git@") {
        let (_, path) = rest.split_once(':')?;
        return parse_owner_repo_path(path);
    }
    if remote_url.starts_with("http://")
        || remote_url.starts_with("https://")
        || remote_url.starts_with("ssh://")
    {
        let url = Url::parse(remote_url).ok()?;
        return parse_owner_repo_path(url.path());
    }
    None
}

fn parse_owner_repo_path(path: &str) -> Option<(String, String)> {
    let trimmed = path.trim_matches('/');
    let mut parts = trimmed.split('/').collect::<Vec<_>>();
    if parts.len() < 2 {
        return None;
    }
    let repo = parts.pop()?.trim_end_matches(".git").to_string();
    let owner = parts.join("/");
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some((owner, repo))
}

async fn persist_and_resolve_local_url(
    repo_root: &Path,
    bundle: &LocalReviewBundle,
    no_open: bool,
) -> Result<String> {
    let review_root = repo_root.join(review::LOCAL_REVIEW_ROOT_DIR);
    let bundle_dir = review_root.join(&bundle.review_id);
    fs::create_dir_all(&bundle_dir)
        .with_context(|| format!("create local review bundle dir {}", bundle_dir.display()))?;
    let bundle_path = bundle_dir.join("bundle.json");
    fs::write(
        &bundle_path,
        serde_json::to_vec_pretty(bundle).context("serialize local review bundle")?,
    )
    .with_context(|| format!("write {}", bundle_path.display()))?;

    if no_open {
        return Ok(format!(
            "{}/review/local/{}",
            review::LOCAL_REVIEW_SERVER_BASE_URL,
            bundle.review_id
        ));
    }

    review::ensure_web_review_server(repo_root, &review_root, &bundle.review_id, &bundle_dir).await
}

fn print_view_result(
    target: &str,
    mode: &str,
    review_id: Option<&str>,
    url: Option<String>,
    args: &ViewArgs,
    open_repo_root: Option<&Path>,
) -> Result<()> {
    let review_id = review_id.map(ToOwned::to_owned);
    if args.json {
        let payload = serde_json::json!({
            "target": target,
            "mode": mode,
            "review_id": review_id,
            "url": url,
            "opened": !args.no_open,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    if let Some(url) = url {
        if !args.no_open {
            if let Some(repo_root) = open_repo_root {
                url_opener::open_url_for_repo(repo_root, &url)?;
            } else {
                url_opener::open_url_in_browser(&url)?;
            }
        }
        println!("{url}");
    }
    if let Some(review_id) = review_id {
        println!("review_id: {review_id}");
    }
    Ok(())
}

fn resolve_commit_target_set(repo_root: &Path, target: &str) -> Result<Vec<String>> {
    if let Some((base, head)) = target.split_once("..") {
        if base.trim().is_empty() || head.trim().is_empty() {
            bail!("invalid commit range `{target}`");
        }
        let base_sha = resolve_commit_oid(repo_root, base.trim())?;
        let head_sha = resolve_commit_oid(repo_root, head.trim())?;
        return rev_list_range(repo_root, &base_sha, &head_sha);
    }

    let sha = resolve_commit_oid(repo_root, target)?;
    Ok(vec![sha])
}

fn resolve_commit_oid(repo_root: &Path, reference: &str) -> Result<String> {
    let spec = format!("{reference}^{{commit}}");
    git_stdout(repo_root, &["rev-parse", spec.as_str()])
        .map(|out| out.trim().to_string())
        .with_context(|| format!("resolve commit `{reference}`"))
}

fn resolve_default_remote_ref(repo_root: &Path, remote: &str) -> Result<String> {
    let symbolic = git_stdout(
        repo_root,
        &["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"],
    )
    .ok()
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty());
    if let Some(reference) = symbolic {
        return Ok(reference);
    }
    Ok(format!("refs/remotes/{remote}/main"))
}

fn merge_base(repo_root: &Path, lhs: &str, rhs: &str) -> Result<String> {
    git_stdout(repo_root, &["merge-base", lhs, rhs])
        .map(|out| out.trim().to_string())
        .with_context(|| format!("compute merge-base between `{lhs}` and `{rhs}`"))
}

fn rev_list_range(repo_root: &Path, base_sha: &str, head_sha: &str) -> Result<Vec<String>> {
    let range = format!("{base_sha}..{head_sha}");
    let raw = git_stdout(repo_root, &["rev-list", "--reverse", &range])
        .with_context(|| format!("list commits for range `{range}`"))?;
    Ok(raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn load_commit_infos(repo_root: &Path, shas: &[String]) -> Result<Vec<CommitInfo>> {
    let mut infos = Vec::with_capacity(shas.len());
    for sha in shas {
        let raw = git_stdout(
            repo_root,
            &[
                "show",
                "-s",
                "--format=%H%x00%s%x00%an%x00%ae%x00%aI",
                sha.as_str(),
            ],
        )
        .with_context(|| format!("load commit metadata for `{sha}`"))?;
        let trimmed = raw.trim_end_matches('\n');
        let mut parts = trimmed.split('\0');
        let sha = parts.next().unwrap_or_default().trim().to_string();
        if sha.is_empty() {
            continue;
        }
        let title = parts.next().unwrap_or_default().trim().to_string();
        let author_name = parts.next().unwrap_or_default().trim().to_string();
        let author_email = parts.next().unwrap_or_default().trim().to_string();
        let authored_at = parts.next().unwrap_or_default().trim().to_string();
        infos.push(CommitInfo {
            sha,
            title,
            author_name,
            author_email,
            authored_at,
        });
    }
    Ok(infos)
}

fn list_ledger_refs(repo_root: &Path) -> Result<Vec<String>> {
    let local = git_stdout(
        repo_root,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/opensession/branches",
        ],
    )
    .unwrap_or_default();
    let remote = git_stdout(
        repo_root,
        &["for-each-ref", "--format=%(refname)", "refs/remotes"],
    )
    .unwrap_or_default();

    let mut refs = local
        .lines()
        .chain(remote.lines())
        .map(str::trim)
        .filter(|line| line.contains("/opensession/branches/"))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    refs.sort();
    refs.dedup();
    Ok(refs)
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
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn sanitize_review_component(raw: &str) -> String {
    raw.chars()
        .map(|ch| {
            let lower = ch.to_ascii_lowercase();
            if lower.is_ascii_alphanumeric() || lower == '-' || lower == '_' {
                lower
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn git_stdout(repo_root: &Path, args: &[&str]) -> Result<String> {
    let output = git(repo_root, args)?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn git(repo_root: &Path, args: &[&str]) -> Result<std::process::Output> {
    Command::new("git")
        .args(args)
        .current_dir(repo_root)
        .output()
        .with_context(|| format!("run git {}", args.join(" ")))
}

fn is_github_pr_url(raw: &str) -> bool {
    let Ok(url) = Url::parse(raw) else {
        return false;
    };
    if !url
        .host_str()
        .is_some_and(|host| host.eq_ignore_ascii_case("github.com"))
    {
        return false;
    }
    let segments = url
        .path_segments()
        .map(|s| s.collect::<Vec<_>>())
        .unwrap_or_default();
    segments.len() >= 4 && segments[2] == "pull" && segments[3].parse::<u64>().is_ok()
}

fn parse_commit_url(raw: &str) -> Option<String> {
    let url = Url::parse(raw).ok()?;
    let segments = url.path_segments()?.collect::<Vec<_>>();
    let index = segments.iter().position(|segment| *segment == "commit")?;
    let sha = segments.get(index + 1)?.trim();
    if sha.is_empty() {
        return None;
    }
    Some(sha.to_string())
}

fn parse_gitlab_mr_url(raw: &str) -> Option<u64> {
    let url = Url::parse(raw).ok()?;
    let segments = url.path_segments()?.collect::<Vec<_>>();
    for window in segments.windows(2) {
        if window[0] == "merge_requests" {
            if let Ok(number) = window[1].parse::<u64>() {
                return Some(number);
            }
        }
    }
    None
}
