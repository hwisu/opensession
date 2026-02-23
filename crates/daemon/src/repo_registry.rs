use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize, Default)]
struct RegistryPayload {
    repos: Vec<String>,
}

/// Tracks repository roots that successfully stored sessions in git-native mode.
#[derive(Debug, Default)]
pub struct RepoRegistry {
    path: Option<PathBuf>,
    repos: BTreeSet<PathBuf>,
}

impl RepoRegistry {
    pub fn load_default() -> Result<Self> {
        let path = crate::config::config_dir()?.join("repo-registry.json");
        if !path.exists() {
            return Ok(Self {
                path: Some(path),
                repos: BTreeSet::new(),
            });
        }

        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("read repo registry {}", path.display()))?;
        let payload: RegistryPayload = serde_json::from_str(&raw)
            .with_context(|| format!("parse repo registry {}", path.display()))?;
        let repos = payload
            .repos
            .iter()
            .map(PathBuf::from)
            .collect::<BTreeSet<_>>();
        Ok(Self {
            path: Some(path),
            repos,
        })
    }

    pub fn add(&mut self, repo_root: &Path) -> Result<bool> {
        let inserted = self.repos.insert(repo_root.to_path_buf());
        if inserted {
            self.persist()?;
        }
        Ok(inserted)
    }

    pub fn repo_roots(&self) -> Vec<PathBuf> {
        self.repos.iter().cloned().collect()
    }

    fn persist(&self) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
        let payload = RegistryPayload {
            repos: self
                .repos
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect(),
        };
        let bytes = serde_json::to_vec_pretty(&payload)?;
        std::fs::write(path, bytes).with_context(|| format!("write {}", path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn add_repo_deduplicates() {
        let dir = tempdir().expect("tempdir");
        let mut registry = RepoRegistry {
            path: Some(dir.path().join("repo-registry.json")),
            repos: BTreeSet::new(),
        };
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(&repo).expect("create repo dir");

        assert!(registry.add(&repo).expect("first add"));
        assert!(!registry.add(&repo).expect("duplicate add"));
        assert_eq!(registry.repo_roots().len(), 1);
    }
}
