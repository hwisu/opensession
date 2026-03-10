use opensession_core::source_uri::{SourceSpec, SourceUri, SourceUriError};
use opensession_paths::local_store_root;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct StoredObject {
    pub uri: SourceUri,
    pub sha256: String,
    pub path: PathBuf,
    pub bytes: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum LocalStoreError {
    #[error("could not determine home directory")]
    HomeUnavailable,
    #[error("invalid hash: {0}")]
    InvalidHash(String),
    #[error("object not found: {0}")]
    NotFound(String),
    #[error("uri error: {0}")]
    Uri(#[from] SourceUriError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

pub fn store_local_object(bytes: &[u8], cwd: &Path) -> Result<StoredObject, LocalStoreError> {
    let sha256 = sha256_hex(bytes);
    validate_hash(&sha256)?;
    let root = default_store_root(cwd)?;
    let path = object_path(&root, &sha256)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if !path.exists() {
        std::fs::write(&path, bytes)?;
    }
    Ok(StoredObject {
        uri: SourceUri::Src(SourceSpec::Local {
            sha256: sha256.clone(),
        }),
        sha256,
        path,
        bytes: bytes.len(),
    })
}

pub fn read_local_object(
    hash: &str,
    cwd: &Path,
) -> Result<(SourceUri, PathBuf, Vec<u8>), LocalStoreError> {
    validate_hash(hash)?;
    for root in candidate_roots(cwd)? {
        let path = object_path(&root, hash)?;
        if path.exists() {
            let bytes = std::fs::read(&path)?;
            return Ok((
                SourceUri::Src(SourceSpec::Local {
                    sha256: hash.to_string(),
                }),
                path,
                bytes,
            ));
        }
    }
    Err(LocalStoreError::NotFound(hash.to_string()))
}

pub fn read_local_object_from_uri(
    uri: &SourceUri,
    cwd: &Path,
) -> Result<(PathBuf, Vec<u8>), LocalStoreError> {
    let hash = uri
        .as_local_hash()
        .ok_or_else(|| LocalStoreError::NotFound("uri is not a local source object".to_string()))?;
    let (_uri, path, bytes) = read_local_object(hash, cwd)?;
    Ok((path, bytes))
}

fn default_store_root(cwd: &Path) -> Result<PathBuf, LocalStoreError> {
    if let Some(repo_root) = find_repo_root(cwd) {
        return Ok(repo_root.join(".opensession").join("objects"));
    }
    global_store_root()
}

pub fn global_store_root() -> Result<PathBuf, LocalStoreError> {
    local_store_root().map_err(|_| LocalStoreError::HomeUnavailable)
}

fn object_path(root: &Path, hash: &str) -> Result<PathBuf, LocalStoreError> {
    validate_hash(hash)?;
    Ok(root
        .join("sha256")
        .join(&hash[0..2])
        .join(&hash[2..4])
        .join(format!("{hash}.jsonl")))
}

pub fn candidate_roots(cwd: &Path) -> Result<Vec<PathBuf>, LocalStoreError> {
    let mut roots = Vec::new();
    if let Some(repo_root) = find_repo_root(cwd) {
        roots.push(repo_root.join(".opensession").join("objects"));
    }
    roots.push(global_store_root()?);
    roots.dedup();
    Ok(roots)
}

pub fn find_repo_root(from: &Path) -> Option<PathBuf> {
    let mut current = from.to_path_buf();
    if current.is_file() {
        current.pop();
    }
    loop {
        if current.join(".git").exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

fn validate_hash(hash: &str) -> Result<(), LocalStoreError> {
    let is_valid = hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit());
    if is_valid {
        Ok(())
    } else {
        Err(LocalStoreError::InvalidHash(hash.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LocalStoreError, find_repo_root, global_store_root, read_local_object, sha256_hex,
        store_local_object,
    };
    use opensession_paths::local_store_root;
    use tempfile::tempdir;

    #[test]
    fn sha256_is_stable() {
        assert_eq!(
            sha256_hex(b"opensession"),
            "f9a2fe35d5e0700b552c63f8dfeb0b0853c5ab051d980b102f15254486c3c2ee".to_string()
        );
    }

    #[test]
    fn global_store_root_uses_standard_home_fallback() {
        let root = global_store_root().expect("global store root");
        assert_eq!(root, local_store_root().expect("centralized store root"));
    }

    #[test]
    fn store_and_read_repo_scoped_object() {
        let tmp = tempdir().expect("tempdir");
        std::fs::create_dir_all(tmp.path().join(".git")).expect("create .git");
        let nested = tmp.path().join("a/b/c");
        std::fs::create_dir_all(&nested).expect("create nested");

        let stored =
            store_local_object(b"{\"type\":\"header\"}\n", &nested).expect("store local object");
        let (uri, path, bytes) =
            read_local_object(&stored.sha256, &nested).expect("read local object");
        assert_eq!(uri.to_string(), stored.uri.to_string());
        assert_eq!(path, stored.path);
        assert_eq!(bytes, b"{\"type\":\"header\"}\n");
        assert!(
            stored
                .path
                .to_string_lossy()
                .contains("/.opensession/objects/")
        );
    }

    #[test]
    fn read_local_object_returns_not_found_for_missing_hash() {
        let tmp = tempdir().expect("tempdir");
        let error = read_local_object(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            tmp.path(),
        )
        .expect_err("missing object");
        assert!(matches!(error, LocalStoreError::NotFound(_)));
    }

    #[test]
    fn finds_repo_root_from_nested_path() {
        let tmp = tempdir().expect("tempdir");
        std::fs::create_dir_all(tmp.path().join(".git")).expect("create .git");
        let nested = tmp.path().join("x/y/z");
        std::fs::create_dir_all(&nested).expect("create nested");
        assert_eq!(find_repo_root(&nested), Some(tmp.path().to_path_buf()));
    }
}
