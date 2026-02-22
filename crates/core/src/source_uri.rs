use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use regex::Regex;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceUri {
    Src(SourceSpec),
    Artifact { sha256: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceSpec {
    Local {
        sha256: String,
    },
    Gh {
        owner: String,
        repo: String,
        r#ref: String,
        path: String,
    },
    Gl {
        project: String,
        r#ref: String,
        path: String,
    },
    Git {
        remote: String,
        r#ref: String,
        path: String,
    },
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SourceUriError {
    #[error("uri must start with os://")]
    InvalidScheme,
    #[error("unsupported uri kind: {0}")]
    UnsupportedKind(String),
    #[error("invalid uri structure: {0}")]
    InvalidStructure(String),
    #[error("invalid sha256: {0}")]
    InvalidHash(String),
    #[error("invalid ref encoding: {0}")]
    InvalidRefEncoding(String),
    #[error("invalid path encoding: {0}")]
    InvalidPathEncoding(String),
    #[error("invalid base64url segment: {0}")]
    InvalidBase64(String),
}

impl SourceUri {
    pub fn parse(input: &str) -> Result<Self, SourceUriError> {
        let body = input
            .strip_prefix("os://")
            .ok_or(SourceUriError::InvalidScheme)?;

        if let Some(hash) = body.strip_prefix("artifact/") {
            validate_sha256(hash)?;
            return Ok(Self::Artifact {
                sha256: hash.to_string(),
            });
        }

        let segments = split_non_empty(body);
        if segments.len() < 2 {
            return Err(SourceUriError::InvalidStructure(
                "expected os://src/<provider>/...".to_string(),
            ));
        }

        if segments[0] != "src" {
            return Err(SourceUriError::UnsupportedKind(segments[0].to_string()));
        }

        let provider = segments[1];
        let rest = &segments[2..];
        match provider {
            "local" => parse_local(rest),
            "gh" => parse_gh(rest),
            "gl" => parse_gl(rest),
            "git" => parse_git(rest),
            other => Err(SourceUriError::UnsupportedKind(other.to_string())),
        }
    }

    pub fn is_remote_source(&self) -> bool {
        matches!(
            self,
            Self::Src(SourceSpec::Gh { .. })
                | Self::Src(SourceSpec::Gl { .. })
                | Self::Src(SourceSpec::Git { .. })
        )
    }

    pub fn as_local_hash(&self) -> Option<&str> {
        match self {
            Self::Src(SourceSpec::Local { sha256 }) => Some(sha256),
            _ => None,
        }
    }

    pub fn as_artifact_hash(&self) -> Option<&str> {
        match self {
            Self::Artifact { sha256 } => Some(sha256),
            _ => None,
        }
    }

    pub fn to_web_path(&self) -> Option<String> {
        match self {
            Self::Src(SourceSpec::Gh {
                owner,
                repo,
                r#ref,
                path,
            }) => Some(format!(
                "/src/gh/{owner}/{repo}/ref/{}/path/{}",
                encode_ref(r#ref),
                encode_path(path)
            )),
            Self::Src(SourceSpec::Gl {
                project,
                r#ref,
                path,
            }) => Some(format!(
                "/src/gl/{}/ref/{}/path/{}",
                encode_b64(project),
                encode_ref(r#ref),
                encode_path(path)
            )),
            Self::Src(SourceSpec::Git {
                remote,
                r#ref,
                path,
            }) => Some(format!(
                "/src/git/{}/ref/{}/path/{}",
                encode_b64(remote),
                encode_ref(r#ref),
                encode_path(path)
            )),
            _ => None,
        }
    }
}

impl fmt::Display for SourceUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Artifact { sha256 } => write!(f, "os://artifact/{sha256}"),
            Self::Src(SourceSpec::Local { sha256 }) => write!(f, "os://src/local/{sha256}"),
            Self::Src(SourceSpec::Gh {
                owner,
                repo,
                r#ref,
                path,
            }) => write!(
                f,
                "os://src/gh/{owner}/{repo}/ref/{}/path/{}",
                encode_ref(r#ref),
                encode_path(path)
            ),
            Self::Src(SourceSpec::Gl {
                project,
                r#ref,
                path,
            }) => write!(
                f,
                "os://src/gl/{}/ref/{}/path/{}",
                encode_b64(project),
                encode_ref(r#ref),
                encode_path(path)
            ),
            Self::Src(SourceSpec::Git {
                remote,
                r#ref,
                path,
            }) => write!(
                f,
                "os://src/git/{}/ref/{}/path/{}",
                encode_b64(remote),
                encode_ref(r#ref),
                encode_path(path)
            ),
        }
    }
}

fn parse_local(rest: &[&str]) -> Result<SourceUri, SourceUriError> {
    if rest.len() != 1 {
        return Err(SourceUriError::InvalidStructure(
            "local uri must be os://src/local/<sha256>".to_string(),
        ));
    }
    let hash = rest[0];
    validate_sha256(hash)?;
    Ok(SourceUri::Src(SourceSpec::Local {
        sha256: hash.to_string(),
    }))
}

fn parse_gh(rest: &[&str]) -> Result<SourceUri, SourceUriError> {
    if rest.len() < 6 {
        return Err(SourceUriError::InvalidStructure(
            "gh uri must be os://src/gh/<owner>/<repo>/ref/<ref>/path/<path...>".to_string(),
        ));
    }
    if rest[2] != "ref" || rest[4] != "path" {
        return Err(SourceUriError::InvalidStructure(
            "gh uri must contain /ref/<ref>/path/<path...>".to_string(),
        ));
    }
    validate_owner_repo(rest[0])?;
    validate_owner_repo(rest[1])?;
    let decoded_ref = decode_ref(rest[3])?;
    let path = decode_path(&rest[5..])?;
    Ok(SourceUri::Src(SourceSpec::Gh {
        owner: rest[0].to_string(),
        repo: rest[1].to_string(),
        r#ref: decoded_ref,
        path,
    }))
}

fn parse_gl(rest: &[&str]) -> Result<SourceUri, SourceUriError> {
    if rest.len() < 5 {
        return Err(SourceUriError::InvalidStructure(
            "gl uri must be os://src/gl/<project_b64>/ref/<ref>/path/<path...>".to_string(),
        ));
    }
    if rest[1] != "ref" || rest[3] != "path" {
        return Err(SourceUriError::InvalidStructure(
            "gl uri must contain /ref/<ref>/path/<path...>".to_string(),
        ));
    }
    let project = decode_b64(rest[0])?;
    let decoded_ref = decode_ref(rest[2])?;
    let path = decode_path(&rest[4..])?;
    Ok(SourceUri::Src(SourceSpec::Gl {
        project,
        r#ref: decoded_ref,
        path,
    }))
}

fn parse_git(rest: &[&str]) -> Result<SourceUri, SourceUriError> {
    if rest.len() < 5 {
        return Err(SourceUriError::InvalidStructure(
            "git uri must be os://src/git/<remote_b64>/ref/<ref>/path/<path...>".to_string(),
        ));
    }
    if rest[1] != "ref" || rest[3] != "path" {
        return Err(SourceUriError::InvalidStructure(
            "git uri must contain /ref/<ref>/path/<path...>".to_string(),
        ));
    }
    let remote = decode_b64(rest[0])?;
    let decoded_ref = decode_ref(rest[2])?;
    let path = decode_path(&rest[4..])?;
    Ok(SourceUri::Src(SourceSpec::Git {
        remote,
        r#ref: decoded_ref,
        path,
    }))
}

fn validate_sha256(hash: &str) -> Result<(), SourceUriError> {
    let is_hex = hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit());
    if !is_hex {
        return Err(SourceUriError::InvalidHash(hash.to_string()));
    }
    Ok(())
}

fn validate_owner_repo(value: &str) -> Result<(), SourceUriError> {
    static OWNER_REPO_RE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
        Regex::new(r"^[A-Za-z0-9._-]{1,200}$").expect("owner/repo regex should compile")
    });
    if OWNER_REPO_RE.is_match(value) {
        Ok(())
    } else {
        Err(SourceUriError::InvalidStructure(format!(
            "invalid owner/repo segment: {value}"
        )))
    }
}

fn encode_ref(value: &str) -> String {
    urlencoding::encode(value).into_owned()
}

fn decode_ref(encoded: &str) -> Result<String, SourceUriError> {
    let decoded = urlencoding::decode(encoded)
        .map_err(|_| SourceUriError::InvalidRefEncoding(encoded.to_string()))?;
    let trimmed = decoded.trim();
    if trimmed.is_empty() {
        return Err(SourceUriError::InvalidRefEncoding(encoded.to_string()));
    }
    Ok(trimmed.to_string())
}

fn encode_path(path: &str) -> String {
    path.split('/')
        .map(|segment| urlencoding::encode(segment).into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn decode_path(segments: &[&str]) -> Result<String, SourceUriError> {
    if segments.is_empty() {
        return Err(SourceUriError::InvalidStructure(
            "path segment is required".to_string(),
        ));
    }

    let mut out = Vec::with_capacity(segments.len());
    for encoded in segments {
        let decoded = urlencoding::decode(encoded)
            .map_err(|_| SourceUriError::InvalidPathEncoding((*encoded).to_string()))?;
        let segment = decoded.trim();
        if segment.is_empty() || segment == "." || segment == ".." || segment.contains('\\') {
            return Err(SourceUriError::InvalidPathEncoding((*encoded).to_string()));
        }
        out.push(segment.to_string());
    }
    Ok(out.join("/"))
}

fn encode_b64(value: &str) -> String {
    URL_SAFE_NO_PAD.encode(value.as_bytes())
}

fn decode_b64(value: &str) -> Result<String, SourceUriError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(value.as_bytes())
        .map_err(|_| SourceUriError::InvalidBase64(value.to_string()))?;
    String::from_utf8(bytes).map_err(|_| SourceUriError::InvalidBase64(value.to_string()))
}

fn split_non_empty(value: &str) -> Vec<&str> {
    value
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{SourceSpec, SourceUri};

    #[test]
    fn parses_local_uri() {
        let hash = "a".repeat(64);
        let parsed = SourceUri::parse(&format!("os://src/local/{hash}")).expect("parse local");
        assert_eq!(
            parsed,
            SourceUri::Src(SourceSpec::Local {
                sha256: hash.clone()
            })
        );
        assert_eq!(parsed.to_string(), format!("os://src/local/{hash}"));
    }

    #[test]
    fn parses_gh_roundtrip() {
        let uri = SourceUri::Src(SourceSpec::Gh {
            owner: "hwisu".to_string(),
            repo: "opensession".to_string(),
            r#ref: "refs/heads/feature/x".to_string(),
            path: "sessions/abc.jsonl".to_string(),
        });
        let rendered = uri.to_string();
        let parsed = SourceUri::parse(&rendered).expect("parse gh");
        assert_eq!(parsed, uri);
        assert_eq!(
            parsed.to_web_path().as_deref(),
            Some(
                "/src/gh/hwisu/opensession/ref/refs%2Fheads%2Ffeature%2Fx/path/sessions/abc.jsonl"
            )
        );
    }

    #[test]
    fn parses_gl_roundtrip() {
        let uri = SourceUri::Src(SourceSpec::Gl {
            project: "group/sub/repo".to_string(),
            r#ref: "main".to_string(),
            path: "dir/session.hail.jsonl".to_string(),
        });
        let rendered = uri.to_string();
        let parsed = SourceUri::parse(&rendered).expect("parse gl");
        assert_eq!(parsed, uri);
    }

    #[test]
    fn parses_git_roundtrip() {
        let uri = SourceUri::Src(SourceSpec::Git {
            remote: "https://example.com/a/b.git".to_string(),
            r#ref: "refs/heads/opensession/sessions".to_string(),
            path: "sessions/hash.jsonl".to_string(),
        });
        let rendered = uri.to_string();
        let parsed = SourceUri::parse(&rendered).expect("parse git");
        assert_eq!(parsed, uri);
    }

    #[test]
    fn parses_artifact_uri() {
        let hash = "f".repeat(64);
        let parsed = SourceUri::parse(&format!("os://artifact/{hash}")).expect("parse artifact");
        assert_eq!(parsed.to_string(), format!("os://artifact/{hash}"));
    }

    #[test]
    fn rejects_invalid_hash() {
        let err = SourceUri::parse("os://src/local/not-a-hash").expect_err("invalid hash");
        assert!(
            err.to_string().contains("invalid sha256"),
            "unexpected error: {err}"
        );
    }
}
