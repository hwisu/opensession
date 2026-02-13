use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Top-level daemon configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DaemonConfig {
    #[serde(default)]
    pub daemon: DaemonSettings,
    #[serde(default)]
    pub server: ServerSettings,
    #[serde(default)]
    pub identity: IdentitySettings,
    #[serde(default)]
    pub privacy: PrivacySettings,
    #[serde(default)]
    pub watchers: WatcherSettings,
    #[serde(default)]
    pub git_storage: GitStorageSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonSettings {
    #[serde(default = "default_false")]
    pub auto_publish: bool,
    #[serde(default = "default_debounce")]
    pub debounce_secs: u64,
    #[serde(default = "default_publish_on")]
    pub publish_on: PublishMode,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_health_check_interval")]
    pub health_check_interval_secs: u64,
    #[serde(default = "default_realtime_debounce_ms")]
    pub realtime_debounce_ms: u64,
}

impl Default for DaemonSettings {
    fn default() -> Self {
        Self {
            auto_publish: false,
            debounce_secs: 5,
            publish_on: PublishMode::Manual,
            max_retries: 3,
            health_check_interval_secs: 300,
            realtime_debounce_ms: 500,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PublishMode {
    SessionEnd,
    Realtime,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerSettings {
    #[serde(default = "default_server_url")]
    pub url: String,
    #[serde(default)]
    pub api_key: String,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            url: default_server_url(),
            api_key: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentitySettings {
    #[serde(default = "default_nickname")]
    pub nickname: String,
    /// Team ID to upload sessions to
    #[serde(default)]
    pub team_id: String,
}

impl Default for IdentitySettings {
    fn default() -> Self {
        Self {
            nickname: default_nickname(),
            team_id: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacySettings {
    #[serde(default = "default_true")]
    pub strip_paths: bool,
    #[serde(default = "default_true")]
    pub strip_env_vars: bool,
    #[serde(default = "default_exclude_patterns")]
    pub exclude_patterns: Vec<String>,
    #[serde(default)]
    pub exclude_tools: Vec<String>,
}

impl Default for PrivacySettings {
    fn default() -> Self {
        Self {
            strip_paths: true,
            strip_env_vars: true,
            exclude_patterns: default_exclude_patterns(),
            exclude_tools: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatcherSettings {
    #[serde(default = "default_true")]
    pub claude_code: bool,
    #[serde(default = "default_true")]
    pub opencode: bool,
    #[serde(default = "default_true")]
    pub goose: bool,
    #[serde(default = "default_true")]
    pub aider: bool,
    #[serde(default)]
    pub cursor: bool,
    #[serde(default)]
    pub custom_paths: Vec<String>,
}

impl Default for WatcherSettings {
    fn default() -> Self {
        Self {
            claude_code: true,
            opencode: true,
            goose: true,
            aider: true,
            cursor: false,
            custom_paths: Vec::new(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

fn default_debounce() -> u64 {
    5
}

fn default_max_retries() -> u32 {
    3
}

fn default_health_check_interval() -> u64 {
    300
}

fn default_realtime_debounce_ms() -> u64 {
    500
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitStorageSettings {
    #[serde(default)]
    pub method: GitStorageMethod,
    #[serde(default)]
    pub token: String,
    /// Enable shadow branch checkpointing (opt-in).
    #[serde(default)]
    pub shadow: bool,
    /// Idle timeout (seconds) before condensing a shadow to the archive branch.
    #[serde(default = "default_shadow_condense_timeout")]
    pub shadow_condense_timeout_secs: u64,
}

impl Default for GitStorageSettings {
    fn default() -> Self {
        Self {
            method: GitStorageMethod::None,
            token: String::new(),
            shadow: false,
            shadow_condense_timeout_secs: 300,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum GitStorageMethod {
    PlatformApi,
    /// Store sessions as git objects on an orphan branch (no external API needed).
    Native,
    #[default]
    #[serde(other)]
    None,
}

fn default_shadow_condense_timeout() -> u64 {
    300
}

fn default_publish_on() -> PublishMode {
    PublishMode::Manual
}

fn default_server_url() -> String {
    "https://opensession.io".to_string()
}

fn default_nickname() -> String {
    "user".to_string()
}

fn default_exclude_patterns() -> Vec<String> {
    vec![
        "*.env".to_string(),
        "*secret*".to_string(),
        "*credential*".to_string(),
    ]
}

/// Get the config directory path
pub fn config_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .context("Could not determine home directory")?;
    Ok(PathBuf::from(home).join(".config").join("opensession"))
}

/// Get the daemon config file path
pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("daemon.toml"))
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
    let config: DaemonConfig = toml::from_str(&content)
        .with_context(|| format!("Failed to parse daemon config at {}", path.display()))?;
    Ok(config)
}

/// Resolve watch paths based on watcher config
pub fn resolve_watch_paths(config: &DaemonConfig) -> Vec<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));

    let mut paths = Vec::new();

    let builtins: &[(bool, &[&str])] = &[
        (config.watchers.claude_code, &[".claude", "projects"]),
        (config.watchers.opencode, &[".local", "share", "opencode"]),
        (config.watchers.goose, &[".local", "share", "goose"]),
        (config.watchers.aider, &[".aider"]),
        (config.watchers.cursor, &[".cursor"]),
    ];

    for &(enabled, segments) in builtins {
        if enabled {
            let p = segments.iter().fold(home.clone(), |acc, s| acc.join(s));
            if p.exists() {
                paths.push(p);
            }
        }
    }

    for custom in &config.watchers.custom_paths {
        let p = PathBuf::from(shellexpand(custom, &home));
        if p.exists() {
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
}

/// Project-level privacy overrides.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectPrivacy {
    pub strip_paths: Option<bool>,
    pub strip_env_vars: Option<bool>,
    pub exclude_patterns: Option<Vec<String>>,
    pub exclude_tools: Option<Vec<String>>,
}

/// Project-level identity overrides (e.g., different team for different repos).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectIdentity {
    pub team_id: Option<String>,
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
        if let Some(ref team_id) = identity.team_id {
            merged.identity.team_id = team_id.clone();
        }
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
        let base = merged.identity.get_or_insert(ProjectIdentity {
            team_id: None,
            nickname: None,
        });
        if local_identity.team_id.is_some() {
            base.team_id = local_identity.team_id.clone();
        }
        if local_identity.nickname.is_some() {
            base.nickname = local_identity.nickname.clone();
        }
    }

    if local.hooks.is_some() {
        merged.hooks = local.hooks.clone();
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
# This file is committed to version control and shared with the team.
# Create `.opensession/config.local.toml` for personal overrides (add to .gitignore).

# [privacy]
# strip_paths = true
# strip_env_vars = true
# exclude_patterns = ["*.env", "*secret*"]
# exclude_tools = []

# [identity]
# team_id = ""           # Override the default team for this repo

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
        assert!(parsed.watchers.claude_code);
        assert!(!parsed.watchers.cursor);
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
team_id = "team-abc"
"#,
        )
        .unwrap();

        let loaded = load_project_config(tmp.path()).unwrap();
        let privacy = loaded.privacy.unwrap();
        assert_eq!(privacy.strip_paths, Some(false));
        assert_eq!(privacy.exclude_patterns, Some(vec!["*.log".to_string()]));
        let identity = loaded.identity.unwrap();
        assert_eq!(identity.team_id, Some("team-abc".to_string()));
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
                team_id: Some("project-team".to_string()),
                nickname: None,
            }),
            ..Default::default()
        };

        let merged = merge_project_config(&global, &project);
        assert_eq!(merged.identity.team_id, "project-team");
        // nickname unchanged
        assert_eq!(merged.identity.nickname, "user");
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
                exclude_tools: Some(vec!["aider".to_string(), "cursor".to_string()]),
            }),
            ..Default::default()
        };

        let merged = merge_project_config(&global, &project);
        // Union, no duplicates
        assert_eq!(
            merged.privacy.exclude_patterns,
            vec!["*.env", "*secret*", "*.log"]
        );
        assert_eq!(merged.privacy.exclude_tools, vec!["cursor", "aider"]);
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
                team_id: Some("shared-team".to_string()),
                nickname: None,
            }),
            hooks: Some(ProjectHooks {
                prepare_commit_msg: true,
                post_commit: true,
                pre_push: false,
            }),
        };

        let local = ProjectConfig {
            privacy: Some(ProjectPrivacy {
                strip_paths: Some(false), // override
                strip_env_vars: None,
                exclude_patterns: Some(vec!["*.log".to_string()]), // union
                exclude_tools: None,
            }),
            identity: Some(ProjectIdentity {
                team_id: None,
                nickname: Some("me".to_string()), // add
            }),
            hooks: Some(ProjectHooks {
                prepare_commit_msg: false, // override entire hooks
                post_commit: true,
                pre_push: true,
            }),
        };

        let merged = merge_project_configs(&shared, &local);

        let privacy = merged.privacy.unwrap();
        assert_eq!(privacy.strip_paths, Some(false)); // local wins
        assert_eq!(
            privacy.exclude_patterns,
            Some(vec!["*.env".to_string(), "*.log".to_string()])
        );

        let identity = merged.identity.unwrap();
        assert_eq!(identity.team_id, Some("shared-team".to_string())); // shared preserved
        assert_eq!(identity.nickname, Some("me".to_string())); // local added

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
