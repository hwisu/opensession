use crate::git::parse::{
    build_git_file_changes, diff_preview_lines, parse_git_name_status, parse_git_numstat,
    parse_git_untracked_paths,
};
use crate::git::types::GitSummaryContext;
use crate::text::compact_summary_snippet;
use std::collections::HashMap;
use std::path::Path;

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
