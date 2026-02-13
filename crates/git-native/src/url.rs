/// Generate a raw content URL from the git remote and file path.
pub fn generate_raw_url(remote_url: &str, rel_path: &str) -> String {
    // Normalize remote URL to extract owner/repo
    let normalized = remote_url
        .trim_end_matches(".git")
        .replace("git@github.com:", "https://github.com/")
        .replace("git@gitlab.com:", "https://gitlab.com/");

    if normalized.contains("github.com") {
        // https://raw.githubusercontent.com/{owner}/{repo}/opensession/sessions/{path}
        let path = normalized.trim_start_matches("https://github.com/");
        format!(
            "https://raw.githubusercontent.com/{}/opensession/sessions/{}",
            path, rel_path
        )
    } else if normalized.contains("gitlab.com") {
        // https://gitlab.com/{owner}/{repo}/-/raw/opensession/sessions/{path}
        let path = normalized.trim_start_matches("https://gitlab.com/");
        format!(
            "https://gitlab.com/{}/-/raw/opensession/sessions/{}",
            path, rel_path
        )
    } else {
        // Fallback: just use the remote URL as base
        format!("{}/raw/opensession/sessions/{}", normalized, rel_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_github_ssh_url() {
        let url = generate_raw_url("git@github.com:user/repo.git", "v1/ab/abc123.hail.jsonl");
        assert_eq!(
            url,
            "https://raw.githubusercontent.com/user/repo/opensession/sessions/v1/ab/abc123.hail.jsonl"
        );
    }

    #[test]
    fn test_github_https_url() {
        let url = generate_raw_url("https://github.com/user/repo", "v1/ab/abc123.hail.jsonl");
        assert_eq!(
            url,
            "https://raw.githubusercontent.com/user/repo/opensession/sessions/v1/ab/abc123.hail.jsonl"
        );
    }

    #[test]
    fn test_gitlab_url() {
        let url = generate_raw_url("git@gitlab.com:user/repo.git", "v1/ab/abc123.hail.jsonl");
        assert_eq!(
            url,
            "https://gitlab.com/user/repo/-/raw/opensession/sessions/v1/ab/abc123.hail.jsonl"
        );
    }
}
