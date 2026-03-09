use super::{
    GitCommandRunner, GitSummaryContext, GitSummaryService, parse_git_name_status,
    parse_git_numstat,
};
use crate::types::HailCompactFileChange;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Default)]
struct MockRunner {
    outputs: HashMap<String, Result<String, String>>,
}

impl MockRunner {
    fn with(args: &[&str], result: Result<&str, &str>) -> (String, Result<String, String>) {
        (
            args.join("\u{1f}"),
            result.map(ToString::to_string).map_err(ToString::to_string),
        )
    }
}

impl GitCommandRunner for MockRunner {
    fn run(&self, _repo_root: &Path, args: &[&str]) -> Result<String, String> {
        self.outputs
            .get(&args.join("\u{1f}"))
            .cloned()
            .unwrap_or_else(|| Err(format!("missing mock for {}", args.join(" "))))
    }
}

fn classify(path: &str) -> &'static str {
    if path.ends_with(".md") {
        "docs"
    } else {
        "application"
    }
}

#[test]
fn parse_git_name_status_extracts_operations_and_paths() {
    let raw = "M\tpackages/ui/src/components/SessionDetailPage.svelte\nA\tdocs/summary.md\nR100\told.rs\tnew.rs\n";
    let parsed = parse_git_name_status(raw);
    assert_eq!(
        parsed.get("packages/ui/src/components/SessionDetailPage.svelte"),
        Some(&"edit".to_string())
    );
    assert_eq!(parsed.get("docs/summary.md"), Some(&"create".to_string()));
    assert_eq!(parsed.get("new.rs"), Some(&"edit".to_string()));
}

#[test]
fn parse_git_numstat_extracts_line_counts() {
    let raw = "12\t3\tpackages/ui/src/components/SessionDetailPage.svelte\n-\t-\tassets/logo.png\n";
    let parsed = parse_git_numstat(raw);
    assert_eq!(
        parsed.get("packages/ui/src/components/SessionDetailPage.svelte"),
        Some(&(12, 3))
    );
    assert_eq!(parsed.get("assets/logo.png"), Some(&(0, 0)));
}

#[test]
fn collect_commit_context_uses_subject_and_file_changes() {
    let runner = MockRunner {
        outputs: HashMap::from([
            MockRunner::with(
                &["show", "--name-status", "--format=", "--no-color", "abc123"],
                Ok("M\tsrc/lib.rs\nA\tdocs/summary.md\n"),
            ),
            MockRunner::with(
                &["show", "--numstat", "--format=", "--no-color", "abc123"],
                Ok("5\t1\tsrc/lib.rs\n2\t0\tdocs/summary.md\n"),
            ),
            MockRunner::with(
                &["show", "--no-patch", "--format=%h %s", "abc123"],
                Ok("abc123 feat: improve auth\n"),
            ),
        ]),
    };
    let service = GitSummaryService::new(runner);

    let context = service
        .collect_commit_context(Path::new("/tmp/repo"), "abc123", 10, classify)
        .expect("commit context");

    assert_eq!(context.source, "git_commit");
    assert_eq!(context.commit.as_deref(), Some("abc123"));
    assert_eq!(
        context.timeline_signals.first().map(String::as_str),
        Some("commit: abc123 feat: improve auth")
    );
    assert_eq!(context.file_changes.len(), 2);
    assert_eq!(context.file_changes[0].path, "docs/summary.md");
    assert_eq!(context.file_changes[0].operation, "create");
    assert_eq!(context.file_changes[0].lines_added, 2);
    assert_eq!(context.file_changes[1].path, "src/lib.rs");
    assert_eq!(context.file_changes[1].operation, "edit");
    assert_eq!(context.file_changes[1].lines_added, 5);
    assert_eq!(context.file_changes[1].lines_removed, 1);
}

#[test]
fn collect_commit_context_falls_back_to_commit_when_subject_missing() {
    let runner = MockRunner {
        outputs: HashMap::from([
            MockRunner::with(
                &[
                    "show",
                    "--name-status",
                    "--format=",
                    "--no-color",
                    "deadbeef",
                ],
                Ok("M\tsrc/lib.rs\n"),
            ),
            MockRunner::with(
                &["show", "--numstat", "--format=", "--no-color", "deadbeef"],
                Ok("1\t0\tsrc/lib.rs\n"),
            ),
            MockRunner::with(
                &["show", "--no-patch", "--format=%h %s", "deadbeef"],
                Err("no subject"),
            ),
        ]),
    };
    let service = GitSummaryService::new(runner);

    let context = service
        .collect_commit_context(Path::new("/tmp/repo"), "deadbeef", 10, classify)
        .expect("commit context fallback");
    assert_eq!(
        context.timeline_signals.first().map(String::as_str),
        Some("commit: deadbeef")
    );
}

#[test]
fn collect_working_tree_context_merges_sources_and_limits_status_lines() {
    let runner = MockRunner {
        outputs: HashMap::from([
            MockRunner::with(
                &["diff", "--name-status", "--no-color"],
                Ok("M\tsrc/app.rs\n"),
            ),
            MockRunner::with(
                &["diff", "--cached", "--name-status", "--no-color"],
                Ok("A\tnew/file.rs\nD\told/file.rs\n"),
            ),
            MockRunner::with(
                &["diff", "--numstat", "--no-color"],
                Ok("3\t1\tsrc/app.rs\n"),
            ),
            MockRunner::with(
                &["diff", "--cached", "--numstat", "--no-color"],
                Ok("10\t0\tnew/file.rs\n0\t4\told/file.rs\n"),
            ),
            MockRunner::with(
                &["ls-files", "--others", "--exclude-standard"],
                Ok("scratch.txt\n"),
            ),
            MockRunner::with(
                &["status", "--short", "--untracked-files=normal"],
                Ok(
                    "M src/app.rs\nA new/file.rs\nD old/file.rs\n?? scratch.txt\nline5\nline6\nline7\nline8\n",
                ),
            ),
        ]),
    };
    let service = GitSummaryService::new(runner);

    let context = service
        .collect_working_tree_context(Path::new("/tmp/repo"), 10, classify)
        .expect("working tree context");
    assert_eq!(context.source, "git_working_tree");
    assert!(context.commit.is_none());
    assert_eq!(
        context.timeline_signals.first().map(String::as_str),
        Some("working_tree: 4 files changed")
    );
    assert_eq!(context.timeline_signals.len(), 7);
    assert_eq!(context.file_changes.len(), 4);
    assert!(
        context
            .file_changes
            .iter()
            .any(|change| change.path == "scratch.txt" && change.operation == "create")
    );
    assert!(
        context
            .file_changes
            .iter()
            .any(|change| change.path == "old/file.rs" && change.operation == "delete")
    );
}

#[test]
fn build_diff_preview_lines_returns_error_when_patch_missing() {
    let runner = MockRunner {
        outputs: HashMap::from([
            MockRunner::with(
                &["diff", "--cached", "--no-color", "--", "src/app.rs"],
                Ok(""),
            ),
            MockRunner::with(&["diff", "--no-color", "--", "src/app.rs"], Ok("")),
        ]),
    };
    let service = GitSummaryService::new(runner);
    let context = GitSummaryContext {
        source: "git_working_tree".to_string(),
        repo_root: PathBuf::from("/tmp"),
        commit: None,
        timeline_signals: Vec::new(),
        file_changes: vec![HailCompactFileChange {
            path: "src/app.rs".to_string(),
            layer: "application".to_string(),
            operation: "edit".to_string(),
            lines_added: 0,
            lines_removed: 0,
        }],
    };

    let error = service
        .build_diff_preview_lines(&context, 4)
        .expect_err("missing patch should fail");
    assert!(error.contains("Diff patch is unavailable"));
}

#[test]
fn build_diff_preview_lines_uses_synthetic_patch_for_create_operation() {
    let unique = format!(
        "ops-summary-git-synthetic-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    );
    let repo_root = std::env::temp_dir().join(unique);
    std::fs::create_dir_all(&repo_root).expect("create repo dir");
    std::fs::write(repo_root.join("added.txt"), "line-1\nline-2\n").expect("write new file");

    let runner = MockRunner {
        outputs: HashMap::from([
            MockRunner::with(
                &["diff", "--cached", "--no-color", "--", "added.txt"],
                Ok(""),
            ),
            MockRunner::with(&["diff", "--no-color", "--", "added.txt"], Ok("")),
        ]),
    };
    let service = GitSummaryService::new(runner);
    let context = GitSummaryContext {
        source: "git_working_tree".to_string(),
        repo_root: repo_root.clone(),
        commit: None,
        timeline_signals: Vec::new(),
        file_changes: vec![HailCompactFileChange {
            path: "added.txt".to_string(),
            layer: "application".to_string(),
            operation: "create".to_string(),
            lines_added: 0,
            lines_removed: 0,
        }],
    };

    let lines = service
        .build_diff_preview_lines(&context, 4)
        .expect("synthetic diff preview");
    assert_eq!(
        lines.first().map(String::as_str),
        Some("added.txt [create]")
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("new file mode 100644"))
    );
    assert!(lines.iter().any(|line| line.contains("+line-1")));

    std::fs::remove_dir_all(&repo_root).ok();
}
