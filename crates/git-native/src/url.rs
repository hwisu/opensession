/// Generate a raw content URL from the git remote and file path.
pub fn generate_raw_url(remote_url: &str, rev: &str, rel_path: &str) -> String {
    // Normalize remote URL to extract owner/repo
    let normalized = remote_url
        .trim_end_matches(".git")
        .replace("git@github.com:", "https://github.com/")
        .replace("git@gitlab.com:", "https://gitlab.com/");

    if normalized.contains("github.com") {
        // https://raw.githubusercontent.com/{owner}/{repo}/{rev}/{path}
        let path = normalized.trim_start_matches("https://github.com/");
        format!(
            "https://raw.githubusercontent.com/{}/{}/{}",
            path, rev, rel_path
        )
    } else if normalized.contains("gitlab.com") {
        // https://gitlab.com/{owner}/{repo}/-/raw/{rev}/{path}
        let path = normalized.trim_start_matches("https://gitlab.com/");
        format!("https://gitlab.com/{}/-/raw/{}/{}", path, rev, rel_path)
    } else {
        // Fallback: just use the remote URL as base
        format!("{}/raw/{}/{}", normalized, rev, rel_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_github_ssh_url() {
        let url = generate_raw_url(
            "git@github.com:user/repo.git",
            "abcd1234",
            "v1/ab/abc123.hail.jsonl",
        );
        assert_eq!(
            url,
            "https://raw.githubusercontent.com/user/repo/abcd1234/v1/ab/abc123.hail.jsonl"
        );
    }

    #[test]
    fn test_github_https_url() {
        let url = generate_raw_url(
            "https://github.com/user/repo",
            "refs/opensession/branches/bWFpbg",
            "v1/ab/abc123.hail.jsonl",
        );
        assert_eq!(
            url,
            "https://raw.githubusercontent.com/user/repo/refs/opensession/branches/bWFpbg/v1/ab/abc123.hail.jsonl"
        );
    }

    #[test]
    fn test_gitlab_url() {
        let url = generate_raw_url(
            "git@gitlab.com:user/repo.git",
            "abcd1234",
            "v1/ab/abc123.hail.jsonl",
        );
        assert_eq!(
            url,
            "https://gitlab.com/user/repo/-/raw/abcd1234/v1/ab/abc123.hail.jsonl"
        );
    }
}
