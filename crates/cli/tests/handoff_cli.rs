use opensession_core::testing;
use opensession_core::{Agent, Content, Event, EventType, Session};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn make_home() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

fn write_file(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    fs::write(path, body).expect("write file");
}

fn run(home: &Path, cwd: &Path, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_opensession"));
    cmd.args(args)
        .current_dir(cwd)
        .env("HOME", home)
        .env("NO_COLOR", "1");
    cmd.output().expect("run opensession")
}

fn run_git(cwd: &Path, args: &[&str]) -> Output {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {} failed\nstdout:{}\nstderr:{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    output
}

fn init_git_repo(path: &Path) {
    fs::create_dir_all(path).expect("create repo");
    run_git(path, &["init", "--initial-branch=main"]);
    run_git(path, &["config", "user.email", "test@example.com"]);
    run_git(path, &["config", "user.name", "Test"]);
    write_file(&path.join("README.md"), "repo\n");
    run_git(path, &["add", "."]);
    run_git(path, &["commit", "-m", "init"]);
}

fn make_hail_jsonl(session_id: &str) -> String {
    let mut session = Session::new(
        session_id.to_string(),
        Agent {
            provider: "openai".to_string(),
            model: "gpt-5".to_string(),
            tool: "codex".to_string(),
            tool_version: None,
        },
    );
    session.events.push(Event {
        event_id: "e1".to_string(),
        timestamp: chrono::Utc::now(),
        event_type: EventType::UserMessage,
        task_id: None,
        content: Content::text("implement the feature"),
        duration_ms: None,
        attributes: Default::default(),
    });
    session.recompute_stats();
    session.to_jsonl().expect("to jsonl")
}

fn make_hail_jsonl_with_cwd(session_id: &str, cwd: &Path) -> String {
    let mut session = Session::new(
        session_id.to_string(),
        Agent {
            provider: "openai".to_string(),
            model: "gpt-5".to_string(),
            tool: "codex".to_string(),
            tool_version: None,
        },
    );
    session
        .context
        .attributes
        .insert("cwd".to_string(), Value::String(cwd.display().to_string()));
    session.events.push(Event {
        event_id: "e1".to_string(),
        timestamp: chrono::Utc::now(),
        event_type: EventType::UserMessage,
        task_id: None,
        content: Content::text("wire session sync"),
        duration_ms: None,
        attributes: Default::default(),
    });
    session.recompute_stats();
    session.to_jsonl().expect("to jsonl")
}

fn make_auxiliary_hail_jsonl_with_cwd(session_id: &str, cwd: &Path, parent_id: &str) -> String {
    let mut session = Session::new(
        session_id.to_string(),
        Agent {
            provider: "openai".to_string(),
            model: "gpt-5".to_string(),
            tool: "codex".to_string(),
            tool_version: None,
        },
    );
    session.context.title = Some("auxiliary helper".to_string());
    session
        .context
        .related_session_ids
        .push(parent_id.to_string());
    session
        .context
        .attributes
        .insert("cwd".to_string(), Value::String(cwd.display().to_string()));
    session.context.attributes.insert(
        "session_role".to_string(),
        Value::String("auxiliary".to_string()),
    );
    session.events.push(Event {
        event_id: "e1".to_string(),
        timestamp: chrono::Utc::now(),
        event_type: EventType::AgentMessage,
        task_id: None,
        content: Content::text("helper output"),
        duration_ms: None,
        attributes: Default::default(),
    });
    session.recompute_stats();
    session.to_jsonl().expect("to jsonl")
}

fn make_hail_jsonl_with_cwd_and_window(
    session_id: &str,
    cwd: &Path,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
) -> String {
    let mut session = Session::new(
        session_id.to_string(),
        Agent {
            provider: "openai".to_string(),
            model: "gpt-5".to_string(),
            tool: "codex".to_string(),
            tool_version: None,
        },
    );
    session.context.created_at = created_at;
    session.context.updated_at = updated_at;
    session
        .context
        .attributes
        .insert("cwd".to_string(), Value::String(cwd.display().to_string()));
    session.events.push(Event {
        event_id: "e1".to_string(),
        timestamp: chrono::Utc::now(),
        event_type: EventType::UserMessage,
        task_id: None,
        content: Content::text("session spans multiple commits"),
        duration_ms: None,
        attributes: Default::default(),
    });
    session.recompute_stats();
    session.to_jsonl().expect("to jsonl")
}

fn first_non_empty_line(output: &[u8]) -> String {
    String::from_utf8_lossy(output)
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default()
        .to_string()
}

fn setup_review_fixture_with_options(
    tmp: &tempfile::TempDir,
    fetch_hidden_refs: bool,
    include_auxiliary: bool,
) -> (std::path::PathBuf, String) {
    let author = tmp.path().join("author");
    let reviewer = tmp.path().join("reviewer");
    let remote = tmp.path().join("review-remote.git");

    init_git_repo(&author);
    run_git(
        tmp.path(),
        &["init", "--bare", remote.to_str().expect("remote path")],
    );
    run_git(
        &author,
        &[
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    run_git(&author, &["push", "origin", "main:main"]);

    run_git(&author, &["checkout", "-b", "feature/review"]);
    write_file(&author.join("src").join("feature.txt"), "review data\n");
    run_git(&author, &["add", "."]);
    run_git(&author, &["commit", "-m", "feat: add review flow"]);
    let feature_sha = first_non_empty_line(&run_git(&author, &["rev-parse", "HEAD"]).stdout);

    let storage = opensession_git_native::NativeGitStorage;
    let ledger_ref = opensession_git_native::branch_ledger_ref("feature/review");
    let session_body = make_hail_jsonl("s-review");
    let meta_body = serde_json::json!({
        "schema_version": 2,
        "session_id": "s-review",
        "git": { "commits": [feature_sha.clone()] }
    })
    .to_string();
    storage
        .store_session_at_ref(
            &author,
            &ledger_ref,
            "s-review",
            session_body.as_bytes(),
            meta_body.as_bytes(),
            std::slice::from_ref(&feature_sha),
        )
        .expect("store session in hidden ledger");
    if include_auxiliary {
        let auxiliary_body =
            make_auxiliary_hail_jsonl_with_cwd("s-review-aux", &author, "s-review");
        let auxiliary_meta = serde_json::json!({
            "schema_version": 2,
            "session_id": "s-review-aux",
            "session_role": "auxiliary",
            "title": "auxiliary helper",
            "tool": "codex",
            "stats": { "files_changed": 0 },
            "git": { "commits": [feature_sha.clone()] }
        })
        .to_string();
        storage
            .store_session_at_ref(
                &author,
                &ledger_ref,
                "s-review-aux",
                auxiliary_body.as_bytes(),
                auxiliary_meta.as_bytes(),
                std::slice::from_ref(&feature_sha),
            )
            .expect("store auxiliary session in hidden ledger");
    }

    run_git(
        &author,
        &["push", "origin", &format!("{feature_sha}:refs/pull/7/head")],
    );
    run_git(
        &author,
        &["push", "origin", &format!("{ledger_ref}:{ledger_ref}")],
    );

    run_git(
        tmp.path(),
        &[
            "clone",
            remote.to_str().expect("remote path"),
            reviewer.to_str().expect("reviewer path"),
        ],
    );
    run_git(
        &reviewer,
        &[
            "fetch",
            "origin",
            "+refs/pull/7/head:refs/opensession/review/pr/7/head",
        ],
    );
    if fetch_hidden_refs {
        run_git(
            &reviewer,
            &[
                "fetch",
                "origin",
                "+refs/opensession/*:refs/remotes/origin/opensession/*",
            ],
        );
    }

    run_git(
        &reviewer,
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com/acme/private-repo.git",
        ],
    );

    (
        reviewer,
        "https://github.com/acme/private-repo/pull/7".to_string(),
    )
}

fn setup_review_fixture(
    tmp: &tempfile::TempDir,
    fetch_hidden_refs: bool,
) -> (std::path::PathBuf, String) {
    setup_review_fixture_with_options(tmp, fetch_hidden_refs, false)
}

fn setup_review_fixture_with_auxiliary(
    tmp: &tempfile::TempDir,
    fetch_hidden_refs: bool,
) -> (std::path::PathBuf, String) {
    setup_review_fixture_with_options(tmp, fetch_hidden_refs, true)
}

#[path = "handoff_cli/cleanup_cli.rs"]
mod cleanup_cli;
#[path = "handoff_cli/handoff_cases.rs"]
mod handoff_cases;
#[path = "handoff_cli/help_cli.rs"]
mod help_cli;
#[path = "handoff_cli/inspect_cli.rs"]
mod inspect_cli;
#[path = "handoff_cli/parse_cli.rs"]
mod parse_cli;
#[path = "handoff_cli/review_cli.rs"]
mod review_cli;
#[path = "handoff_cli/setup_cli.rs"]
mod setup_cli;
#[path = "handoff_cli/share_cli.rs"]
mod share_cli;
#[path = "handoff_cli/view_cli.rs"]
mod view_cli;
