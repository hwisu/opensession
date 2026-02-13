use std::process::Command;

/// Git metadata collected from the working directory at session time.
#[derive(Debug, Clone, Default)]
pub struct GitContext {
    pub remote: Option<String>,
    pub branch: Option<String>,
    pub commit: Option<String>,
    pub repo_name: Option<String>,
}

/// Extract git context from a working directory.
/// Returns a default (empty) context if the directory is not inside a git repo.
pub fn extract_git_context(cwd: &str) -> GitContext {
    // Check if inside a git repo
    let toplevel = git_cmd(cwd, &["rev-parse", "--show-toplevel"]);
    if toplevel.is_none() {
        return GitContext::default();
    }

    let remote = git_cmd(cwd, &["remote", "get-url", "origin"]);
    let branch = git_cmd(cwd, &["rev-parse", "--abbrev-ref", "HEAD"]);
    let commit = git_cmd(cwd, &["rev-parse", "HEAD"]);
    let repo_name = remote
        .as_deref()
        .and_then(normalize_repo_name)
        .map(String::from);

    GitContext {
        remote,
        branch,
        commit,
        repo_name,
    }
}

/// Normalize a git remote URL to "owner/repo" form.
///
/// Handles:
///   - `https://github.com/foo/bar.git` → `foo/bar`
///   - `git@github.com:foo/bar.git` → `foo/bar`
///   - `ssh://git@github.com/foo/bar` → `foo/bar`
pub fn normalize_repo_name(remote_url: &str) -> Option<&str> {
    let s = remote_url.trim();

    // SSH: git@host:owner/repo.git
    if let Some(rest) = s.strip_prefix("git@") {
        let path = rest.split_once(':')?.1;
        let path = path.strip_suffix(".git").unwrap_or(path);
        return if path.contains('/') { Some(path) } else { None };
    }

    // HTTPS or SSH scheme: https://host/owner/repo.git  or  ssh://git@host/owner/repo
    if s.starts_with("https://") || s.starts_with("http://") || s.starts_with("ssh://") {
        // Find the path after the host
        let without_scheme = s.split("://").nth(1)?;
        // skip "git@host/" or "host/"
        let path_start = without_scheme.find('/')? + 1;
        let path = &without_scheme[path_start..];
        let path = path.strip_suffix(".git").unwrap_or(path);
        return if path.contains('/') { Some(path) } else { None };
    }

    None
}

fn git_cmd(cwd: &str, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        None
    } else {
        Some(stdout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_https() {
        assert_eq!(
            normalize_repo_name("https://github.com/hwisu/opensession.git"),
            Some("hwisu/opensession")
        );
    }

    #[test]
    fn test_normalize_ssh() {
        assert_eq!(
            normalize_repo_name("git@github.com:hwisu/opensession.git"),
            Some("hwisu/opensession")
        );
    }

    #[test]
    fn test_normalize_no_suffix() {
        assert_eq!(
            normalize_repo_name("https://github.com/foo/bar"),
            Some("foo/bar")
        );
    }

    #[test]
    fn test_normalize_invalid() {
        assert_eq!(normalize_repo_name("not-a-url"), None);
    }
}
