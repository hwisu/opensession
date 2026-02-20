use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

// Re-export shared runtime config types
pub use opensession_runtime_config::{
    apply_compat_fallbacks, DaemonConfig, DaemonSettings, GitStorageMethod, PublishMode,
    CONFIG_FILE_NAME,
};

/// Get the config directory path
pub fn config_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .context("Could not determine home directory")?;
    Ok(PathBuf::from(home).join(".config").join("opensession"))
}

/// Get the daemon config file path
pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join(CONFIG_FILE_NAME))
}

/// Get the PID file path
pub fn pid_file_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("daemon.pid"))
}

/// Get the state file path
pub fn state_file_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("state.json"))
}

/// Load daemon config from disk
pub fn load_config() -> Result<DaemonConfig> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(DaemonConfig::default());
    }

    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read daemon config at {}", path.display()))?;
    let parsed: Option<toml::Value> = toml::from_str(&content).ok();
    let mut config: DaemonConfig = toml::from_str(&content)
        .with_context(|| format!("Failed to parse daemon config at {}", path.display()))?;
    apply_compat_fallbacks(&mut config, parsed.as_ref());
    Ok(config)
}

/// Resolve watch paths based on watcher config
pub fn resolve_watch_paths(config: &DaemonConfig) -> Vec<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));

    let raw_paths = if config.watchers.custom_paths.is_empty() {
        // Backward compatibility: older configs may not have custom_paths yet.
        DaemonConfig::default().watchers.custom_paths
    } else {
        config.watchers.custom_paths.clone()
    };

    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    for raw in raw_paths {
        let p = PathBuf::from(shellexpand(&raw, &home));
        if p.exists() && seen.insert(p.clone()) {
            paths.push(p);
        }
    }

    paths
}

/// Simple ~ expansion
fn shellexpand(path: &str, home: &Path) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        format!("{}/{}", home.display(), rest)
    } else {
        path.to_string()
    }
}

// ── Per-repo project configuration ─────────────────────────────────────

/// Per-repo configuration stored in `.opensession/config.toml`.
/// All fields are optional — only overrides what's set.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectConfig {
    #[serde(default)]
    pub privacy: Option<ProjectPrivacy>,
    #[serde(default)]
    pub identity: Option<ProjectIdentity>,
    #[serde(default)]
    pub hooks: Option<ProjectHooks>,
    /// When false, prevents uploading sessions from this repo as Personal (Public).
    #[serde(default)]
    pub allow_public: Option<bool>,
}

/// Project-level privacy overrides.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectPrivacy {
    pub strip_paths: Option<bool>,
    pub strip_env_vars: Option<bool>,
    pub exclude_patterns: Option<Vec<String>>,
    pub exclude_tools: Option<Vec<String>>,
}

/// Project-level identity overrides.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectIdentity {
    pub nickname: Option<String>,
}

/// Project-level git hook configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectHooks {
    /// Add AI-Session trailer to commit messages (default: true)
    #[serde(default = "default_true")]
    pub prepare_commit_msg: bool,
    /// Record commit-session links after commit (default: true)
    #[serde(default = "default_true")]
    pub post_commit: bool,
    /// Scan for secrets before push (default: false)
    #[serde(default)]
    pub pre_push: bool,
}

impl Default for ProjectHooks {
    fn default() -> Self {
        Self {
            prepare_commit_msg: true,
            post_commit: true,
            pre_push: false,
        }
    }
}

fn default_true() -> bool {
    true
}

/// Find the repo root by walking up from `cwd` looking for `.git`.
pub fn find_repo_root(cwd: &str) -> Option<PathBuf> {
    opensession_git_native::ops::find_repo_root(Path::new(cwd))
}

/// Load per-repo project config from `.opensession/config.toml` in the repo root.
pub fn load_project_config(repo_root: &Path) -> Option<ProjectConfig> {
    let config_path = repo_root.join(".opensession").join("config.toml");
    if !config_path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&config_path).ok()?;
    toml::from_str(&content).ok()
}

/// Load local (personal, gitignored) project config.
pub fn load_project_local_config(repo_root: &Path) -> Option<ProjectConfig> {
    let config_path = repo_root.join(".opensession").join("config.local.toml");
    if !config_path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&config_path).ok()?;
    toml::from_str(&content).ok()
}

/// Merge project-level config over global daemon config.
/// Project settings override global settings where specified.
/// For Vec fields (exclude_patterns, exclude_tools), use union (deduplicated).
pub fn merge_project_config(global: &DaemonConfig, project: &ProjectConfig) -> DaemonConfig {
    let mut merged = global.clone();

    if let Some(ref privacy) = project.privacy {
        if let Some(strip_paths) = privacy.strip_paths {
            merged.privacy.strip_paths = strip_paths;
        }
        if let Some(strip_env_vars) = privacy.strip_env_vars {
            merged.privacy.strip_env_vars = strip_env_vars;
        }
        if let Some(ref patterns) = privacy.exclude_patterns {
            for p in patterns {
                if !merged.privacy.exclude_patterns.contains(p) {
                    merged.privacy.exclude_patterns.push(p.clone());
                }
            }
        }
        if let Some(ref tools) = privacy.exclude_tools {
            for t in tools {
                if !merged.privacy.exclude_tools.contains(t) {
                    merged.privacy.exclude_tools.push(t.clone());
                }
            }
        }
    }

    if let Some(ref identity) = project.identity {
        if let Some(ref nickname) = identity.nickname {
            merged.identity.nickname = nickname.clone();
        }
    }

    merged
}

/// Merge two ProjectConfigs. `local` overrides `shared` where specified.
pub fn merge_project_configs(shared: &ProjectConfig, local: &ProjectConfig) -> ProjectConfig {
    let mut merged = shared.clone();

    // Privacy: local overrides shared field by field
    if let Some(ref local_privacy) = local.privacy {
        let base = merged.privacy.get_or_insert(ProjectPrivacy {
            strip_paths: None,
            strip_env_vars: None,
            exclude_patterns: None,
            exclude_tools: None,
        });
        if local_privacy.strip_paths.is_some() {
            base.strip_paths = local_privacy.strip_paths;
        }
        if local_privacy.strip_env_vars.is_some() {
            base.strip_env_vars = local_privacy.strip_env_vars;
        }
        if let Some(ref patterns) = local_privacy.exclude_patterns {
            let existing = base.exclude_patterns.get_or_insert_with(Vec::new);
            for p in patterns {
                if !existing.contains(p) {
                    existing.push(p.clone());
                }
            }
        }
        if let Some(ref tools) = local_privacy.exclude_tools {
            let existing = base.exclude_tools.get_or_insert_with(Vec::new);
            for t in tools {
                if !existing.contains(t) {
                    existing.push(t.clone());
                }
            }
        }
    }

    if let Some(ref local_identity) = local.identity {
        let base = merged
            .identity
            .get_or_insert(ProjectIdentity { nickname: None });
        if local_identity.nickname.is_some() {
            base.nickname = local_identity.nickname.clone();
        }
    }

    if local.hooks.is_some() {
        merged.hooks = local.hooks.clone();
    }

    if local.allow_public.is_some() {
        merged.allow_public = local.allow_public;
    }

    merged
}

/// Load the effective project config for a repo root, merging shared + local.
pub fn load_effective_project_config(repo_root: &Path) -> Option<ProjectConfig> {
    let shared = load_project_config(repo_root);
    let local = load_project_local_config(repo_root);

    match (shared, local) {
        (Some(s), Some(l)) => Some(merge_project_configs(&s, &l)),
        (Some(s), None) => Some(s),
        (None, Some(l)) => Some(l),
        (None, None) => None,
    }
}

/// Generate a default project config template with comments.
#[cfg(test)]
pub fn generate_default_project_config() -> String {
    r#"# OpenSession per-repo configuration
# This file is committed to version control and shared with collaborators.
# Create `.opensession/config.local.toml` for personal overrides (add to .gitignore).

# [privacy]
# strip_paths = true
# strip_env_vars = true
# exclude_patterns = ["*.env", "*secret*"]
# exclude_tools = []

# [identity]
# nickname = ""          # Optional per-repo display handle override

# [hooks]
# prepare_commit_msg = true  # Add AI-Session trailer to commit messages
# post_commit = true         # Record commit↔session links
# pre_push = false           # Scan for secrets before push
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use opensession_runtime_config::PrivacySettings;

    #[test]
    fn test_default_config_serializes() {
        let config = DaemonConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(toml_str.contains("auto_publish = false"));
        assert!(toml_str.contains("debounce_secs = 5"));
        assert!(toml_str.contains("publish_on = \"manual\""));
        assert!(toml_str.contains("max_retries = 3"));
        assert!(toml_str.contains("health_check_interval_secs = 300"));
        assert!(toml_str.contains("realtime_debounce_ms = 500"));
    }

    #[test]
    fn test_config_roundtrip() {
        let config = DaemonConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: DaemonConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.daemon.debounce_secs, 5);
        assert_eq!(parsed.daemon.publish_on, PublishMode::Manual);
        assert_eq!(parsed.daemon.max_retries, 3);
        assert_eq!(parsed.daemon.health_check_interval_secs, 300);
        assert_eq!(parsed.daemon.realtime_debounce_ms, 500);
        assert!(!parsed.watchers.custom_paths.is_empty());
    }

    #[test]
    fn test_find_repo_root() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("myrepo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let subdir = repo.join("src").join("deep");
        std::fs::create_dir_all(&subdir).unwrap();

        // From deep subdir, should find repo root
        let found = find_repo_root(subdir.to_str().unwrap());
        assert_eq!(found, Some(repo.clone()));

        // From repo root itself
        let found = find_repo_root(repo.to_str().unwrap());
        assert_eq!(found, Some(repo));

        // From a path with no .git
        let no_repo = tmp.path().join("norope");
        std::fs::create_dir_all(&no_repo).unwrap();
        assert_eq!(find_repo_root(no_repo.to_str().unwrap()), None);
    }

    #[test]
    fn test_load_project_config() {
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".opensession");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            r#"
[privacy]
strip_paths = false
exclude_patterns = ["*.log"]

[identity]
nickname = "repo-user"
"#,
        )
        .unwrap();

        let loaded = load_project_config(tmp.path()).unwrap();
        let privacy = loaded.privacy.unwrap();
        assert_eq!(privacy.strip_paths, Some(false));
        assert_eq!(privacy.exclude_patterns, Some(vec!["*.log".to_string()]));
        let identity = loaded.identity.unwrap();
        assert_eq!(identity.nickname, Some("repo-user".to_string()));
    }

    #[test]
    fn test_merge_project_config_privacy_override() {
        let global = DaemonConfig::default();
        let project = ProjectConfig {
            privacy: Some(ProjectPrivacy {
                strip_paths: Some(false),
                strip_env_vars: None,
                exclude_patterns: None,
                exclude_tools: None,
            }),
            ..Default::default()
        };

        let merged = merge_project_config(&global, &project);
        assert!(!merged.privacy.strip_paths);
        // Unchanged
        assert!(merged.privacy.strip_env_vars);
    }

    #[test]
    fn test_merge_project_config_identity_override() {
        let global = DaemonConfig::default();
        let project = ProjectConfig {
            identity: Some(ProjectIdentity {
                nickname: Some("project-nick".to_string()),
            }),
            ..Default::default()
        };

        let merged = merge_project_config(&global, &project);
        assert_eq!(merged.identity.nickname, "project-nick");
    }

    #[test]
    fn test_merge_project_config_patterns_union() {
        let global = DaemonConfig {
            privacy: PrivacySettings {
                exclude_patterns: vec!["*.env".to_string(), "*secret*".to_string()],
                exclude_tools: vec!["cursor".to_string()],
                ..Default::default()
            },
            ..Default::default()
        };

        let project = ProjectConfig {
            privacy: Some(ProjectPrivacy {
                strip_paths: None,
                strip_env_vars: None,
                exclude_patterns: Some(vec!["*secret*".to_string(), "*.log".to_string()]),
                exclude_tools: Some(vec!["codex".to_string(), "cursor".to_string()]),
            }),
            ..Default::default()
        };

        let merged = merge_project_config(&global, &project);
        // Union, no duplicates
        assert_eq!(
            merged.privacy.exclude_patterns,
            vec!["*.env", "*secret*", "*.log"]
        );
        assert_eq!(merged.privacy.exclude_tools, vec!["cursor", "codex"]);
    }

    #[test]
    fn test_merge_project_configs_shared_and_local() {
        let shared = ProjectConfig {
            privacy: Some(ProjectPrivacy {
                strip_paths: Some(true),
                strip_env_vars: None,
                exclude_patterns: Some(vec!["*.env".to_string()]),
                exclude_tools: None,
            }),
            identity: Some(ProjectIdentity {
                nickname: Some("shared-nick".to_string()),
            }),
            hooks: Some(ProjectHooks {
                prepare_commit_msg: true,
                post_commit: true,
                pre_push: false,
            }),
            ..Default::default()
        };

        let local = ProjectConfig {
            privacy: Some(ProjectPrivacy {
                strip_paths: Some(false), // override
                strip_env_vars: None,
                exclude_patterns: Some(vec!["*.log".to_string()]), // union
                exclude_tools: None,
            }),
            identity: Some(ProjectIdentity {
                nickname: Some("me".to_string()), // add
            }),
            hooks: Some(ProjectHooks {
                prepare_commit_msg: false, // override entire hooks
                post_commit: true,
                pre_push: true,
            }),
            ..Default::default()
        };

        let merged = merge_project_configs(&shared, &local);

        let privacy = merged.privacy.unwrap();
        assert_eq!(privacy.strip_paths, Some(false)); // local wins
        assert_eq!(
            privacy.exclude_patterns,
            Some(vec!["*.env".to_string(), "*.log".to_string()])
        );

        let identity = merged.identity.unwrap();
        assert_eq!(identity.nickname, Some("me".to_string())); // local override

        let hooks = merged.hooks.unwrap();
        assert!(!hooks.prepare_commit_msg); // local override
        assert!(hooks.pre_push); // local override
    }

    #[test]
    fn test_generate_default_project_config() {
        let template = generate_default_project_config();
        // All lines are comments, so parsing should yield an empty (default) config
        let parsed: ProjectConfig = toml::from_str(&template).unwrap();
        assert!(parsed.privacy.is_none());
        assert!(parsed.identity.is_none());
        assert!(parsed.hooks.is_none());
    }
}
