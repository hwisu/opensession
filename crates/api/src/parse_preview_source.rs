use base64::Engine;
use std::collections::HashSet;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitSource {
    pub remote: String,
    pub r#ref: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubSource {
    pub owner: String,
    pub repo: String,
    pub r#ref: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsePreviewSourceError {
    message: String,
}

impl ParsePreviewSourceError {
    pub fn invalid_source(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for ParsePreviewSourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ParsePreviewSourceError {}

pub fn normalize_git_source(
    remote: &str,
    r#ref: &str,
    path: &str,
) -> Result<GitSource, ParsePreviewSourceError> {
    let remote = decode_and_trim(remote, "remote")?;
    let url = validate_remote_url(&remote)?;
    let remote = normalized_remote_origin(&url);

    let r#ref = decode_and_trim(r#ref, "ref")?;
    if !is_valid_ref(r#ref.as_str()) {
        return Err(ParsePreviewSourceError::invalid_source(
            "ref must match [A-Za-z0-9._/-]{1,255} without '..', '~', '^', ':', or '\\'",
        ));
    }

    let path = normalize_repo_path(path)?;

    Ok(GitSource {
        remote,
        r#ref,
        path,
    })
}

pub fn normalize_github_source(
    owner: &str,
    repo: &str,
    r#ref: &str,
    path: &str,
) -> Result<GithubSource, ParsePreviewSourceError> {
    let owner = decode_and_trim(owner, "owner")?;
    if !is_valid_owner_repo(owner.as_str()) {
        return Err(ParsePreviewSourceError::invalid_source(
            "owner must match [A-Za-z0-9._-]{1,100}",
        ));
    }

    let repo = decode_and_trim(repo, "repo")?;
    if !is_valid_owner_repo(repo.as_str()) {
        return Err(ParsePreviewSourceError::invalid_source(
            "repo must match [A-Za-z0-9._-]{1,100}",
        ));
    }

    let r#ref = decode_and_trim(r#ref, "ref")?;
    if !is_valid_ref(r#ref.as_str()) {
        return Err(ParsePreviewSourceError::invalid_source(
            "ref must match [A-Za-z0-9._/-]{1,255} without '..', '~', '^', ':', or '\\'",
        ));
    }

    let path = normalize_repo_path(path)?;

    Ok(GithubSource {
        owner,
        repo,
        r#ref,
        path,
    })
}

pub fn normalize_filename(filename: &str) -> Result<String, ParsePreviewSourceError> {
    let decoded = decode_and_trim(filename, "filename")?;
    let normalized = decoded
        .replace('\\', "/")
        .rsplit('/')
        .next()
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    if normalized.is_empty() || normalized == "." || normalized == ".." {
        return Err(ParsePreviewSourceError::invalid_source(
            "inline filename must be a non-empty filename",
        ));
    }
    if normalized.len() > 255 {
        return Err(ParsePreviewSourceError::invalid_source(
            "inline filename is too long (max 255 chars)",
        ));
    }
    Ok(normalized)
}

pub fn decode_inline_content(content_base64: &str) -> Result<Vec<u8>, ParsePreviewSourceError> {
    let trimmed = content_base64.trim();
    if trimmed.is_empty() {
        return Err(ParsePreviewSourceError::invalid_source(
            "inline content_base64 is required",
        ));
    }
    base64::engine::general_purpose::STANDARD
        .decode(trimmed)
        .map_err(|_| {
            ParsePreviewSourceError::invalid_source("inline content_base64 must be valid base64")
        })
}

pub fn file_name_from_path(path: &str) -> String {
    path.rsplit('/')
        .next()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("session.txt")
        .to_string()
}

pub fn repo_path_from_remote(remote: &str) -> Result<String, ParsePreviewSourceError> {
    let parsed = validate_remote_url(remote)?;
    Ok(repo_path_segments(&parsed)?.join("/"))
}

pub fn build_git_raw_url(
    source: &GitSource,
    gitlab_hosts: &HashSet<String>,
) -> Result<String, ParsePreviewSourceError> {
    let remote = validate_remote_url(&source.remote)?;
    let host = remote
        .host_str()
        .ok_or_else(|| ParsePreviewSourceError::invalid_source("remote host is required"))?
        .to_ascii_lowercase();

    if host == "github.com" {
        return build_github_raw_url(&remote, &source.r#ref, &source.path);
    }
    if is_gitlab_host(&host, gitlab_hosts) {
        return build_gitlab_raw_url(&remote, &source.r#ref, &source.path);
    }

    build_generic_raw_url(&remote, &source.r#ref, &source.path)
}

pub fn provider_for_host(host: &str, gitlab_hosts: &HashSet<String>) -> Option<&'static str> {
    if host == "github.com" {
        return Some("github");
    }
    if is_gitlab_host(host, gitlab_hosts) {
        return Some("gitlab");
    }
    None
}

pub fn is_allowed_content_type(content_type: &str) -> bool {
    let media_type = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if media_type.is_empty() {
        return true;
    }
    if media_type.starts_with("text/") {
        return true;
    }
    media_type == "application/json"
        || media_type == "application/jsonl"
        || media_type == "application/x-ndjson"
        || media_type.ends_with("+json")
}

pub fn looks_binary(bytes: &[u8]) -> bool {
    bytes.contains(&0) || std::str::from_utf8(bytes).is_err()
}

fn decode_and_trim(value: &str, field: &str) -> Result<String, ParsePreviewSourceError> {
    urlencoding::decode(value)
        .map(|decoded| decoded.trim().to_string())
        .map_err(|_| {
            ParsePreviewSourceError::invalid_source(format!(
                "{field} contains invalid percent encoding"
            ))
        })
}

fn normalize_repo_path(path: &str) -> Result<String, ParsePreviewSourceError> {
    let decoded = decode_and_trim(path, "path")?;
    if decoded.is_empty() {
        return Err(ParsePreviewSourceError::invalid_source("path is required"));
    }
    if decoded.starts_with('/') {
        return Err(ParsePreviewSourceError::invalid_source(
            "path must be repository-relative",
        ));
    }

    let mut normalized_segments = Vec::<String>::new();
    for segment in decoded.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(ParsePreviewSourceError::invalid_source(
                "path cannot contain empty, '.' or '..' segments",
            ));
        }
        if segment.contains('\\') {
            return Err(ParsePreviewSourceError::invalid_source(
                "path cannot contain backslash characters",
            ));
        }
        normalized_segments.push(segment.to_string());
    }

    Ok(normalized_segments.join("/"))
}

fn is_valid_owner_repo(value: &str) -> bool {
    let len = value.len();
    (1..=100).contains(&len)
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
}

fn is_valid_ref(value: &str) -> bool {
    let len = value.len();
    if !(1..=255).contains(&len) {
        return false;
    }
    if value.contains("..")
        || value.contains('~')
        || value.contains('^')
        || value.contains(':')
        || value.contains('\\')
    {
        return false;
    }
    if value.starts_with('/') || value.ends_with('/') || value.contains("//") {
        return false;
    }

    value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-' || b == b'/')
}

fn validate_remote_url(remote: &str) -> Result<Url, ParsePreviewSourceError> {
    let parsed = Url::parse(remote).map_err(|_| {
        ParsePreviewSourceError::invalid_source("remote must be an absolute http(s) URL")
    })?;

    let scheme = parsed.scheme().to_ascii_lowercase();
    if scheme != "https" {
        return Err(ParsePreviewSourceError::invalid_source(
            "remote must use https",
        ));
    }

    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(ParsePreviewSourceError::invalid_source(
            "remote cannot include credentials",
        ));
    }

    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(ParsePreviewSourceError::invalid_source(
            "remote cannot include query or fragment",
        ));
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| ParsePreviewSourceError::invalid_source("remote host is required"))?;
    if host.parse::<std::net::IpAddr>().is_ok() {
        return Err(ParsePreviewSourceError::invalid_source(
            "remote host must be a DNS name",
        ));
    }

    if host.eq_ignore_ascii_case("localhost") {
        return Err(ParsePreviewSourceError::invalid_source(
            "remote host must not be localhost",
        ));
    }

    Ok(parsed)
}

fn normalized_remote_origin(url: &Url) -> String {
    let mut normalized = url.clone();
    normalized.set_query(None);
    normalized.set_fragment(None);
    let _ = normalized.set_username("");
    let _ = normalized.set_password(None);

    let trimmed_path = normalized.path().trim_end_matches('/').to_string();
    if trimmed_path.is_empty() {
        normalized.set_path("/");
    } else {
        normalized.set_path(&trimmed_path);
    }

    let mut rendered = normalized.to_string();
    if rendered.ends_with('/') && normalized.path() == "/" {
        rendered.pop();
    }
    rendered
}

fn repo_path_segments(url: &Url) -> Result<Vec<String>, ParsePreviewSourceError> {
    let mut segments: Vec<String> = url
        .path_segments()
        .ok_or_else(|| {
            ParsePreviewSourceError::invalid_source("remote repository path is invalid")
        })?
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            urlencoding::decode(segment)
                .map(|decoded| decoded.trim().to_string())
                .map_err(|_| {
                    ParsePreviewSourceError::invalid_source(
                        "remote repository path contains invalid percent encoding",
                    )
                })
        })
        .collect::<Result<Vec<_>, _>>()?;

    if segments.len() < 2 {
        return Err(ParsePreviewSourceError::invalid_source(
            "remote must include owner/group and repository",
        ));
    }

    if let Some(last) = segments.last_mut() {
        *last = strip_git_suffix(last).to_string();
    }

    if segments
        .iter()
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(ParsePreviewSourceError::invalid_source(
            "remote repository path contains invalid segments",
        ));
    }

    Ok(segments)
}

fn build_github_raw_url(
    url: &Url,
    r#ref: &str,
    path: &str,
) -> Result<String, ParsePreviewSourceError> {
    let segments = repo_path_segments(url)?;
    if segments.len() != 2 {
        return Err(ParsePreviewSourceError::invalid_source(
            "github remote must look like https://github.com/{owner}/{repo}",
        ));
    }

    Ok(format!(
        "https://raw.githubusercontent.com/{}/{}/{}/{}",
        segments[0],
        segments[1],
        encode_segments(r#ref),
        encode_segments(path)
    ))
}

fn build_gitlab_raw_url(
    url: &Url,
    r#ref: &str,
    path: &str,
) -> Result<String, ParsePreviewSourceError> {
    let project_path = repo_path_segments(url)?.join("/");
    let origin = origin_from_url(url)?;
    Ok(format!(
        "{}/{}/-/raw/{}/{}",
        origin,
        encode_segments(&project_path),
        encode_segments(r#ref),
        encode_segments(path)
    ))
}

fn build_generic_raw_url(
    url: &Url,
    r#ref: &str,
    path: &str,
) -> Result<String, ParsePreviewSourceError> {
    let repo_path = repo_path_segments(url)?.join("/");
    let origin = origin_from_url(url)?;
    Ok(format!(
        "{}/{}/raw/{}/{}",
        origin,
        encode_segments(&repo_path),
        encode_segments(r#ref),
        encode_segments(path)
    ))
}

fn origin_from_url(url: &Url) -> Result<String, ParsePreviewSourceError> {
    let host = url
        .host_str()
        .ok_or_else(|| ParsePreviewSourceError::invalid_source("remote host is required"))?;
    let mut origin = format!("{}://{host}", url.scheme());
    if let Some(port) = url.port() {
        origin.push(':');
        origin.push_str(&port.to_string());
    }
    Ok(origin)
}

fn encode_segments(value: &str) -> String {
    value
        .split('/')
        .map(|segment| urlencoding::encode(segment).into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn strip_git_suffix(value: &str) -> &str {
    value.strip_suffix(".git").unwrap_or(value)
}

fn is_gitlab_host(host: &str, gitlab_hosts: &HashSet<String>) -> bool {
    host == "gitlab.com"
        || gitlab_hosts.contains(host)
        || host
            .strip_prefix("gitlab.")
            .is_some_and(|suffix| suffix.contains('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_repo_path_rejects_parent_segments() {
        let err = normalize_git_source("https://github.com/org/repo", "main", "../secret")
            .expect_err("path traversal must fail");
        assert!(
            err.message()
                .contains("path cannot contain empty, '.' or '..' segments")
        );
    }

    #[test]
    fn decode_inline_content_rejects_empty_input() {
        let err = decode_inline_content("   ").expect_err("empty inline payload must fail");
        assert_eq!(err.message(), "inline content_base64 is required");
    }

    #[test]
    fn build_git_raw_url_uses_provider_aware_patterns() {
        let no_gitlab_hosts = HashSet::new();
        let configured_gitlab_hosts = HashSet::from(["gitlab.internal.example.com".to_string()]);

        let github = build_git_raw_url(
            &GitSource {
                remote: "https://github.com/hwisu/opensession".to_string(),
                r#ref: "main".to_string(),
                path: "sessions/demo.hail.jsonl".to_string(),
            },
            &no_gitlab_hosts,
        )
        .expect("github raw url should build");
        assert_eq!(
            github,
            "https://raw.githubusercontent.com/hwisu/opensession/main/sessions/demo.hail.jsonl"
        );

        let gitlab = build_git_raw_url(
            &GitSource {
                remote: "https://gitlab.com/group/sub/repo".to_string(),
                r#ref: "feature/one".to_string(),
                path: "logs/session.hail.jsonl".to_string(),
            },
            &no_gitlab_hosts,
        )
        .expect("gitlab raw url should build");
        assert_eq!(
            gitlab,
            "https://gitlab.com/group/sub/repo/-/raw/feature/one/logs/session.hail.jsonl"
        );

        let gitlab_self_managed = build_git_raw_url(
            &GitSource {
                remote: "https://gitlab.internal.example.com/team/repo".to_string(),
                r#ref: "main".to_string(),
                path: "logs/session.hail.jsonl".to_string(),
            },
            &configured_gitlab_hosts,
        )
        .expect("self-managed gitlab raw url should build");
        assert_eq!(
            gitlab_self_managed,
            "https://gitlab.internal.example.com/team/repo/-/raw/main/logs/session.hail.jsonl"
        );

        let generic = build_git_raw_url(
            &GitSource {
                remote: "https://code.example.com/team/repo".to_string(),
                r#ref: "release/v1".to_string(),
                path: "hail/demo.hail.jsonl".to_string(),
            },
            &no_gitlab_hosts,
        )
        .expect("generic raw url should build");
        assert_eq!(
            generic,
            "https://code.example.com/team/repo/raw/release/v1/hail/demo.hail.jsonl"
        );
    }

    #[test]
    fn provider_for_host_detects_supported_hosts() {
        let no_gitlab_hosts = HashSet::new();
        let configured_gitlab_hosts = HashSet::from(["gitlab.internal.example.com".to_string()]);

        assert_eq!(
            provider_for_host("github.com", &no_gitlab_hosts),
            Some("github")
        );
        assert_eq!(
            provider_for_host("gitlab.com", &no_gitlab_hosts),
            Some("gitlab")
        );
        assert_eq!(
            provider_for_host("gitlab.internal.example.com", &no_gitlab_hosts),
            Some("gitlab")
        );
        assert_eq!(
            provider_for_host("gitlab.internal.example.com", &configured_gitlab_hosts),
            Some("gitlab")
        );
        assert_eq!(
            provider_for_host("evil-gitlab.example", &no_gitlab_hosts),
            None
        );
        assert_eq!(
            provider_for_host("code.example.com", &no_gitlab_hosts),
            None
        );
    }

    #[test]
    fn is_allowed_content_type_accepts_text_and_json() {
        assert!(is_allowed_content_type("text/plain; charset=utf-8"));
        assert!(is_allowed_content_type("application/json"));
        assert!(is_allowed_content_type("application/vnd.api+json"));
        assert!(!is_allowed_content_type("application/octet-stream"));
    }

    #[test]
    fn looks_binary_rejects_null_and_non_utf8_bytes() {
        assert!(looks_binary(b"hello\0world"));
        assert!(looks_binary(&[0xff, 0xfe, 0xfd]));
        assert!(!looks_binary("hello world".as_bytes()));
    }
}
