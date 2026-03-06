use crate::text::compact_summary_snippet;
use crate::types::HailCompactFileChange;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct GitSummaryContext {
    pub source: String,
    pub repo_root: PathBuf,
    pub commit: Option<String>,
    pub timeline_signals: Vec<String>,
    pub file_changes: Vec<HailCompactFileChange>,
}

pub trait GitCommandRunner {
    fn run(&self, repo_root: &Path, args: &[&str]) -> Result<String, String>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ShellGitCommandRunner;

impl GitCommandRunner for ShellGitCommandRunner {
    fn run(&self, repo_root: &Path, args: &[&str]) -> Result<String, String> {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(repo_root)
            .args(args)
            .output()
            .map_err(|err| format!("failed to run git {}: {err}", args.join(" ")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if stderr.is_empty() {
                return Err(format!(
                    "git {} failed with status {}",
                    args.join(" "),
                    output.status
                ));
            }
            return Err(stderr);
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

pub struct GitSummaryService<R> {
    runner: R,
}

impl<R: GitCommandRunner> GitSummaryService<R> {
    pub fn new(runner: R) -> Self {
        Self { runner }
    }

    pub fn collect_commit_context(
        &self,
        repo_root: &Path,
        commit: &str,
        max_entries: usize,
        classify_arch_layer: fn(&str) -> &'static str,
    ) -> Option<GitSummaryContext> {
        let name_status = self
            .runner
            .run(
                repo_root,
                &["show", "--name-status", "--format=", "--no-color", commit],
            )
            .ok()?;
        let numstat = self
            .runner
            .run(
                repo_root,
                &["show", "--numstat", "--format=", "--no-color", commit],
            )
            .unwrap_or_default();

        let file_changes = build_git_file_changes(
            parse_git_name_status(&name_status),
            parse_git_numstat(&numstat),
            Vec::new(),
            max_entries,
            classify_arch_layer,
        );
        if file_changes.is_empty() {
            return None;
        }

        let mut timeline_signals = Vec::new();
        if let Ok(subject) = self
            .runner
            .run(repo_root, &["show", "--no-patch", "--format=%h %s", commit])
        {
            let compact = compact_summary_snippet(&subject, 180);
            if !compact.is_empty() {
                timeline_signals.push(format!("commit: {compact}"));
            }
        }
        if timeline_signals.is_empty() {
            timeline_signals.push(format!("commit: {}", compact_summary_snippet(commit, 32)));
        }

        Some(GitSummaryContext {
            source: "git_commit".to_string(),
            repo_root: repo_root.to_path_buf(),
            commit: Some(commit.to_string()),
            timeline_signals,
            file_changes,
        })
    }

    pub fn collect_working_tree_context(
        &self,
        repo_root: &Path,
        max_entries: usize,
        classify_arch_layer: fn(&str) -> &'static str,
    ) -> Option<GitSummaryContext> {
        let mut operation_by_path = HashMap::new();
        for args in [
            ["diff", "--name-status", "--no-color"].as_slice(),
            ["diff", "--cached", "--name-status", "--no-color"].as_slice(),
        ] {
            let Ok(raw) = self.runner.run(repo_root, args) else {
                continue;
            };
            for (path, operation) in parse_git_name_status(&raw) {
                operation_by_path.insert(path, operation);
            }
        }

        let mut numstat_by_path: HashMap<String, (u64, u64)> = HashMap::new();
        for args in [
            ["diff", "--numstat", "--no-color"].as_slice(),
            ["diff", "--cached", "--numstat", "--no-color"].as_slice(),
        ] {
            let Ok(raw) = self.runner.run(repo_root, args) else {
                continue;
            };
            for (path, (added, removed)) in parse_git_numstat(&raw) {
                let entry = numstat_by_path.entry(path).or_insert((0, 0));
                entry.0 = entry.0.saturating_add(added);
                entry.1 = entry.1.saturating_add(removed);
            }
        }

        let untracked_paths = self
            .runner
            .run(repo_root, &["ls-files", "--others", "--exclude-standard"])
            .map(|raw| parse_git_untracked_paths(&raw))
            .unwrap_or_default();

        let file_changes = build_git_file_changes(
            operation_by_path,
            numstat_by_path,
            untracked_paths,
            max_entries,
            classify_arch_layer,
        );
        if file_changes.is_empty() {
            return None;
        }

        let mut timeline_signals = vec![format!(
            "working_tree: {} files changed",
            file_changes.len()
        )];
        if let Ok(status) = self.runner.run(
            repo_root,
            &["status", "--short", "--untracked-files=normal"],
        ) {
            for line in status.lines().take(6) {
                let compact = compact_summary_snippet(line, 140);
                if compact.is_empty() {
                    continue;
                }
                timeline_signals.push(format!("status: {compact}"));
            }
        }

        Some(GitSummaryContext {
            source: "git_working_tree".to_string(),
            repo_root: repo_root.to_path_buf(),
            commit: None,
            timeline_signals,
            file_changes,
        })
    }

    pub fn build_diff_preview_lines(
        &self,
        context: &GitSummaryContext,
        max_files: usize,
    ) -> Result<Vec<String>, String> {
        let mut lines = Vec::new();
        let commit = context.commit.as_deref();
        for change in context.file_changes.iter().take(max_files) {
            let patch = self.git_patch_for_file(
                &context.repo_root,
                commit,
                &change.path,
                &change.operation,
            );
            if patch.trim().is_empty() {
                continue;
            }

            lines.push(format!("{} [{}]", change.path, change.operation));
            for line in diff_preview_lines(&patch, 14, 180) {
                lines.push(format!("  {line}"));
            }
            lines.push(String::new());
        }

        while lines.last().is_some_and(|line| line.is_empty()) {
            lines.pop();
        }

        if lines.is_empty() {
            return Err("Diff patch is unavailable for detected changes".to_string());
        }
        Ok(lines)
    }

    fn git_patch_for_file(
        &self,
        repo_root: &Path,
        commit: Option<&str>,
        path: &str,
        operation: &str,
    ) -> String {
        if let Some(commit) = commit {
            return self
                .runner
                .run(
                    repo_root,
                    &["show", "--no-color", "--format=", commit, "--", path],
                )
                .unwrap_or_default();
        }

        let staged = self
            .runner
            .run(repo_root, &["diff", "--cached", "--no-color", "--", path])
            .unwrap_or_default();
        let unstaged = self
            .runner
            .run(repo_root, &["diff", "--no-color", "--", path])
            .unwrap_or_default();

        let mut patch = String::new();
        if !staged.trim().is_empty() {
            patch.push_str(&staged);
            if !staged.ends_with('\n') {
                patch.push('\n');
            }
        }
        if !unstaged.trim().is_empty() {
            patch.push_str(&unstaged);
        }
        if !patch.trim().is_empty() {
            return patch;
        }
        if operation == "create" {
            return synthetic_new_file_patch(repo_root, path, 20).unwrap_or_default();
        }
        patch
    }
}

pub fn parse_git_name_status(raw: &str) -> HashMap<String, String> {
    let mut operations = HashMap::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parts = trimmed.split('\t').collect::<Vec<_>>();
        if parts.is_empty() {
            continue;
        }

        let status = parts[0].trim();
        let path = if status.starts_with('R') || status.starts_with('C') {
            parts.get(2).or_else(|| parts.get(1))
        } else {
            parts.get(1)
        };
        let Some(path) = path
            .copied()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };

        let operation = match status.chars().next().unwrap_or('M') {
            'A' => "create",
            'D' => "delete",
            _ => "edit",
        };
        operations.insert(path.to_string(), operation.to_string());
    }
    operations
}

pub fn parse_git_numstat(raw: &str) -> HashMap<String, (u64, u64)> {
    let mut stats = HashMap::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parts = trimmed.split('\t').collect::<Vec<_>>();
        if parts.len() < 3 {
            continue;
        }
        let path = parts[2].trim();
        if path.is_empty() {
            continue;
        }
        let added = parts[0].trim().parse::<u64>().unwrap_or(0);
        let removed = parts[1].trim().parse::<u64>().unwrap_or(0);
        stats.insert(path.to_string(), (added, removed));
    }
    stats
}

pub fn parse_git_untracked_paths(raw: &str) -> Vec<String> {
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn build_git_file_changes(
    operation_by_path: HashMap<String, String>,
    numstat_by_path: HashMap<String, (u64, u64)>,
    untracked_paths: Vec<String>,
    max_entries: usize,
    classify_arch_layer: fn(&str) -> &'static str,
) -> Vec<HailCompactFileChange> {
    let mut by_path: HashMap<String, HailCompactFileChange> = HashMap::new();

    for (path, operation) in operation_by_path {
        by_path
            .entry(path.clone())
            .and_modify(|entry| {
                entry.operation = operation.clone();
                entry.layer = classify_arch_layer(&path).to_string();
            })
            .or_insert_with(|| HailCompactFileChange {
                path: path.clone(),
                layer: classify_arch_layer(&path).to_string(),
                operation,
                lines_added: 0,
                lines_removed: 0,
            });
    }

    for (path, (added, removed)) in numstat_by_path {
        let entry = by_path
            .entry(path.clone())
            .or_insert_with(|| HailCompactFileChange {
                path: path.clone(),
                layer: classify_arch_layer(&path).to_string(),
                operation: "edit".to_string(),
                lines_added: 0,
                lines_removed: 0,
            });
        entry.lines_added = entry.lines_added.saturating_add(added);
        entry.lines_removed = entry.lines_removed.saturating_add(removed);
    }

    for path in untracked_paths {
        by_path
            .entry(path.clone())
            .and_modify(|entry| {
                entry.operation = "create".to_string();
                entry.layer = classify_arch_layer(&path).to_string();
            })
            .or_insert_with(|| HailCompactFileChange {
                path: path.clone(),
                layer: classify_arch_layer(&path).to_string(),
                operation: "create".to_string(),
                lines_added: 0,
                lines_removed: 0,
            });
    }

    let mut changes = by_path.into_values().collect::<Vec<_>>();
    changes.sort_by(|lhs, rhs| lhs.path.cmp(&rhs.path));
    changes.truncate(max_entries);
    changes
}

fn synthetic_new_file_patch(repo_root: &Path, path: &str, max_lines: usize) -> Option<String> {
    let full_path = repo_root.join(path);
    if !full_path.exists() {
        return None;
    }
    let bytes = std::fs::read(&full_path).ok()?;
    let content = String::from_utf8_lossy(&bytes);
    let mut out = String::new();
    out.push_str(&format!("diff --git a/{path} b/{path}\n"));
    out.push_str("new file mode 100644\n");
    out.push_str("--- /dev/null\n");
    out.push_str(&format!("+++ b/{path}\n"));
    out.push_str("@@ new file @@\n");

    let mut wrote_any = false;
    for line in content.lines().take(max_lines) {
        out.push('+');
        out.push_str(line);
        out.push('\n');
        wrote_any = true;
    }
    if !wrote_any {
        out.push_str("+(empty file)\n");
    } else if content.lines().count() > max_lines {
        out.push_str("+…\n");
    }
    Some(out)
}

fn truncate_preview_line(raw: &str, max_chars: usize) -> String {
    if raw.chars().count() <= max_chars {
        return raw.to_string();
    }
    let mut out = String::new();
    for ch in raw.chars().take(max_chars.saturating_sub(1)) {
        out.push(ch);
    }
    out.push('…');
    out
}

fn diff_preview_lines(raw: &str, max_lines: usize, max_chars: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut iter = raw.lines();
    for _ in 0..max_lines {
        let Some(line) = iter.next() else {
            break;
        };
        lines.push(truncate_preview_line(line, max_chars));
    }
    if iter.next().is_some() {
        lines.push("…".to_string());
    }
    lines
}

#[cfg(test)]
mod tests {
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
        let raw =
            "12\t3\tpackages/ui/src/components/SessionDetailPage.svelte\n-\t-\tassets/logo.png\n";
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
}
