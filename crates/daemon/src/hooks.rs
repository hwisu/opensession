use std::path::Path;

use anyhow::{Context, Result};

/// Git hook types managed by opensession.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookType {
    PrepareCommitMsg,
    PostCommit,
    PrePush,
}

impl HookType {
    /// The filename used in `.git/hooks/`.
    pub fn filename(&self) -> &'static str {
        match self {
            Self::PrepareCommitMsg => "prepare-commit-msg",
            Self::PostCommit => "post-commit",
            Self::PrePush => "pre-push",
        }
    }

    pub fn all() -> &'static [HookType] {
        &[Self::PrepareCommitMsg, Self::PostCommit, Self::PrePush]
    }
}

// ---------------------------------------------------------------------------
// Hook templates
// ---------------------------------------------------------------------------

const PREPARE_COMMIT_MSG_HOOK: &str = r#"#!/bin/sh
# opensession-managed — do not edit
# Adds AI-Session trailer to commit messages when an AI session is active.

COMMIT_MSG_FILE="$1"
COMMIT_SOURCE="$2"

# Only modify user-initiated commits (not merge, squash, etc.)
case "$COMMIT_SOURCE" in
    merge|squash) exit 0 ;;
esac

# Check if sqlite3 is available
command -v sqlite3 >/dev/null 2>&1 || exit 0

DB="$HOME/.local/share/opensession/local.db"
[ -f "$DB" ] || exit 0

# Get repo working directory
REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null)" || exit 0

# Find the most recent session active in this repo (within last 30 minutes)
SESSION_ID=$(sqlite3 "$DB" "
    SELECT s.id FROM sessions s
    LEFT JOIN session_sync ss ON ss.session_id = s.id
    WHERE s.working_directory LIKE '${REPO_ROOT}%'
    AND s.created_at >= datetime('now', '-30 minutes')
    ORDER BY s.created_at DESC
    LIMIT 1;
" 2>/dev/null)

[ -z "$SESSION_ID" ] && exit 0

# Don't add if trailer already exists
grep -q "^AI-Session:" "$COMMIT_MSG_FILE" 2>/dev/null && exit 0

# Append trailer (with blank line separator if needed)
if [ -s "$COMMIT_MSG_FILE" ]; then
    # Check if last non-empty line is already a trailer
    LAST_LINE=$(sed '/^$/d' "$COMMIT_MSG_FILE" | tail -1)
    case "$LAST_LINE" in
        *:*) ;; # Already has a trailer-like line
        *) echo "" >> "$COMMIT_MSG_FILE" ;;
    esac
fi
echo "AI-Session: $SESSION_ID" >> "$COMMIT_MSG_FILE"
"#;

const POST_COMMIT_HOOK: &str = r#"#!/bin/sh
# opensession-managed — do not edit
# Records commit<->session links after each commit.

command -v sqlite3 >/dev/null 2>&1 || exit 0

DB="$HOME/.local/share/opensession/local.db"
[ -f "$DB" ] || exit 0

COMMIT_HASH=$(git rev-parse HEAD 2>/dev/null) || exit 0
BRANCH=$(git rev-parse --abbrev-ref HEAD 2>/dev/null) || BRANCH=""
REPO_ROOT=$(git rev-parse --show-toplevel 2>/dev/null) || exit 0

# Extract AI-Session trailer from the commit message
SESSION_ID=$(git log -1 --format='%(trailers:key=AI-Session,valueonly)' "$COMMIT_HASH" 2>/dev/null)

[ -z "$SESSION_ID" ] && exit 0

# Record the link in the local DB
sqlite3 "$DB" "
    INSERT INTO commit_session_links (commit_hash, session_id, repo_path, branch)
    VALUES ('$COMMIT_HASH', '$SESSION_ID', '$REPO_ROOT', '$BRANCH')
    ON CONFLICT(commit_hash, session_id) DO NOTHING;
" 2>/dev/null

exit 0
"#;

const PRE_PUSH_HOOK: &str = r#"#!/bin/sh
# opensession-managed — do not edit
# Scans for potential secrets in commits being pushed.

remote="$1"
url="$2"

YELLOW='\033[1;33m'
RED='\033[1;31m'
NC='\033[0m'

found_secrets=0

while read local_ref local_oid remote_ref remote_oid; do
    # Skip delete pushes
    if [ "$local_oid" = "0000000000000000000000000000000000000000" ]; then
        continue
    fi

    # Determine the range to check
    if [ "$remote_oid" = "0000000000000000000000000000000000000000" ]; then
        # New branch — check all commits
        range="$local_oid"
    else
        range="$remote_oid..$local_oid"
    fi

    # Check diff content for secret patterns
    git diff "$range" -- 2>/dev/null | grep -nE \
        '(sk-[a-zA-Z0-9]{20,}|ghp_[a-zA-Z0-9]{36}|gho_[a-zA-Z0-9]{36}|-----BEGIN (RSA |EC )?PRIVATE KEY-----|ANTHROPIC_API_KEY\s*=|OPENAI_API_KEY\s*=|AWS_SECRET_ACCESS_KEY\s*=|DATABASE_URL\s*=.*password)' \
        > /tmp/opensession-secret-scan.$$ 2>/dev/null

    if [ -s /tmp/opensession-secret-scan.$$ ]; then
        found_secrets=1
        printf "${RED}[opensession]${NC} Potential secrets detected in push to ${remote}:\n"
        echo ""
        while IFS= read -r line; do
            printf "  ${YELLOW}!${NC}  %s\n" "$line"
        done < /tmp/opensession-secret-scan.$$
        echo ""
    fi

    rm -f /tmp/opensession-secret-scan.$$
done

if [ "$found_secrets" -eq 1 ]; then
    printf "${YELLOW}[opensession]${NC} Warning: potential secrets detected. Review before pushing.\n"
    echo "  To push anyway: git push --no-verify"
    echo ""
    # Warning only — don't block the push by default
    # To make it blocking, uncomment: exit 1
    exit 0
fi

exit 0
"#;

// ---------------------------------------------------------------------------
// Hook marker
// ---------------------------------------------------------------------------

const HOOK_MARKER: &str = "# opensession-managed";

// ---------------------------------------------------------------------------
// Hook template accessor
// ---------------------------------------------------------------------------

/// Generate the hook script content for a given hook type.
pub fn hook_template(hook_type: HookType) -> &'static str {
    match hook_type {
        HookType::PrepareCommitMsg => PREPARE_COMMIT_MSG_HOOK,
        HookType::PostCommit => POST_COMMIT_HOOK,
        HookType::PrePush => PRE_PUSH_HOOK,
    }
}

// ---------------------------------------------------------------------------
// Hook installer
// ---------------------------------------------------------------------------

/// Install opensession git hooks into a repository.
pub fn install_hooks(repo_root: &Path, hooks: &[HookType]) -> Result<Vec<HookType>> {
    let hooks_dir = repo_root.join(".git").join("hooks");
    if !hooks_dir.exists() {
        anyhow::bail!(
            "Not a git repository: {} (no .git/hooks)",
            repo_root.display()
        );
    }

    let mut installed = Vec::new();
    for hook_type in hooks {
        let hook_path = hooks_dir.join(hook_type.filename());

        // Check if there's an existing non-opensession hook
        if hook_path.exists() {
            let content = std::fs::read_to_string(&hook_path)
                .with_context(|| format!("read existing hook {}", hook_path.display()))?;
            if !content.contains(HOOK_MARKER) {
                // Backup existing hook
                let backup_path =
                    hooks_dir.join(format!("{}.pre-opensession", hook_type.filename()));
                std::fs::rename(&hook_path, &backup_path).with_context(|| {
                    format!("backup existing hook to {}", backup_path.display())
                })?;
            }
            // If it contains HOOK_MARKER, we'll just overwrite with new version
        }

        let template = hook_template(*hook_type);
        std::fs::write(&hook_path, template)
            .with_context(|| format!("write hook {}", hook_path.display()))?;

        // Make executable on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o755);
            std::fs::set_permissions(&hook_path, perms)
                .with_context(|| format!("chmod hook {}", hook_path.display()))?;
        }

        installed.push(*hook_type);
    }

    Ok(installed)
}

/// Uninstall opensession git hooks from a repository.
pub fn uninstall_hooks(repo_root: &Path, hooks: &[HookType]) -> Result<Vec<HookType>> {
    let hooks_dir = repo_root.join(".git").join("hooks");
    let mut uninstalled = Vec::new();

    for hook_type in hooks {
        let hook_path = hooks_dir.join(hook_type.filename());
        if !hook_path.exists() {
            continue;
        }

        let content = std::fs::read_to_string(&hook_path).unwrap_or_default();
        if !content.contains(HOOK_MARKER) {
            continue; // Not our hook
        }

        std::fs::remove_file(&hook_path)?;

        // Restore backup if exists
        let backup_path = hooks_dir.join(format!("{}.pre-opensession", hook_type.filename()));
        if backup_path.exists() {
            std::fs::rename(&backup_path, &hook_path)?;
        }

        uninstalled.push(*hook_type);
    }

    Ok(uninstalled)
}

/// Check which opensession hooks are installed in a repository.
pub fn list_installed_hooks(repo_root: &Path) -> Vec<HookType> {
    let hooks_dir = repo_root.join(".git").join("hooks");
    let mut installed = Vec::new();

    for hook_type in HookType::all() {
        let hook_path = hooks_dir.join(hook_type.filename());
        if hook_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&hook_path) {
                if content.contains(HOOK_MARKER) {
                    installed.push(*hook_type);
                }
            }
        }
    }

    installed
}

// ---------------------------------------------------------------------------
// Secret detection
// ---------------------------------------------------------------------------

/// A potential secret found in content.
#[derive(Debug, Clone)]
pub struct SecretMatch {
    pub pattern_name: String,
    pub line_number: usize,
    pub context: String, // The line with the match (redacted)
}

/// Default secret patterns: (name, regex).
pub fn default_secret_patterns() -> Vec<(&'static str, &'static str)> {
    vec![
        ("OpenAI API Key", r"sk-[a-zA-Z0-9]{20,}"),
        ("GitHub PAT", r"ghp_[a-zA-Z0-9]{36}"),
        ("GitHub OAuth Token", r"gho_[a-zA-Z0-9]{36}"),
        ("Private Key", r"-----BEGIN (RSA |EC )?PRIVATE KEY-----"),
        ("Anthropic API Key", r"sk-ant-[a-zA-Z0-9\-]{20,}"),
        ("AWS Secret", r"(?i)aws_secret_access_key\s*=\s*\S+"),
        (
            "Generic API Key Assignment",
            r"(?i)(api[_-]?key|secret[_-]?key|auth[_-]?token)\s*=\s*['\x22][a-zA-Z0-9]{16,}['\x22]",
        ),
    ]
}

/// Scan content for potential secrets.
pub fn scan_for_secrets(content: &str) -> Vec<SecretMatch> {
    let patterns = default_secret_patterns();
    let mut matches = Vec::new();

    for (line_number, line) in content.lines().enumerate() {
        for (name, pattern) in &patterns {
            if let Ok(re) = regex::Regex::new(pattern) {
                if re.is_match(line) {
                    // Redact the matched portion
                    let redacted = re.replace_all(line, "[REDACTED]").to_string();
                    matches.push(SecretMatch {
                        pattern_name: name.to_string(),
                        line_number: line_number + 1,
                        context: redacted,
                    });
                }
            }
        }
    }

    matches
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_fake_git_repo() -> TempDir {
        let dir = TempDir::new().unwrap();
        let hooks_dir = dir.path().join(".git").join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();
        dir
    }

    #[test]
    fn test_hook_type_filename() {
        assert_eq!(HookType::PrepareCommitMsg.filename(), "prepare-commit-msg");
        assert_eq!(HookType::PostCommit.filename(), "post-commit");
        assert_eq!(HookType::PrePush.filename(), "pre-push");
    }

    #[test]
    fn test_hook_type_all() {
        let all = HookType::all();
        assert_eq!(all.len(), 3);
        assert!(all.contains(&HookType::PrepareCommitMsg));
        assert!(all.contains(&HookType::PostCommit));
        assert!(all.contains(&HookType::PrePush));
    }

    #[test]
    fn test_hook_templates_contain_marker() {
        for hook_type in HookType::all() {
            let template = hook_template(*hook_type);
            assert!(
                template.contains(HOOK_MARKER),
                "{:?} template missing marker",
                hook_type,
            );
        }
    }

    #[test]
    fn test_hook_templates_start_with_shebang() {
        for hook_type in HookType::all() {
            let template = hook_template(*hook_type);
            assert!(
                template.starts_with("#!/bin/sh\n"),
                "{:?} template missing shebang",
                hook_type,
            );
        }
    }

    #[test]
    fn test_install_hooks() {
        let repo = create_fake_git_repo();
        let hooks = HookType::all();

        let installed = install_hooks(repo.path(), hooks).unwrap();
        assert_eq!(installed.len(), 3);

        // Verify files exist
        for hook_type in hooks {
            let path = repo.path().join(".git/hooks").join(hook_type.filename());
            assert!(path.exists(), "{} should exist", path.display());

            let content = fs::read_to_string(&path).unwrap();
            assert!(content.contains(HOOK_MARKER));
            assert!(content.starts_with("#!/bin/sh"));

            // Verify executable on Unix
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = fs::metadata(&path).unwrap().permissions();
                assert_eq!(perms.mode() & 0o755, 0o755);
            }
        }
    }

    #[test]
    fn test_uninstall_hooks() {
        let repo = create_fake_git_repo();
        let hooks = HookType::all();

        // Install then uninstall
        install_hooks(repo.path(), hooks).unwrap();
        let uninstalled = uninstall_hooks(repo.path(), hooks).unwrap();
        assert_eq!(uninstalled.len(), 3);

        // Verify files are gone
        for hook_type in hooks {
            let path = repo.path().join(".git/hooks").join(hook_type.filename());
            assert!(!path.exists(), "{} should be removed", path.display());
        }
    }

    #[test]
    fn test_existing_hook_backup() {
        let repo = create_fake_git_repo();
        let hooks_dir = repo.path().join(".git/hooks");

        // Write a pre-existing hook (not opensession-managed)
        let existing_content = "#!/bin/sh\necho 'my custom hook'\n";
        let hook_path = hooks_dir.join("prepare-commit-msg");
        fs::write(&hook_path, existing_content).unwrap();

        // Install opensession hooks
        install_hooks(repo.path(), &[HookType::PrepareCommitMsg]).unwrap();

        // Original should be backed up
        let backup_path = hooks_dir.join("prepare-commit-msg.pre-opensession");
        assert!(backup_path.exists(), "backup should exist");
        let backup_content = fs::read_to_string(&backup_path).unwrap();
        assert_eq!(backup_content, existing_content);

        // New hook should be our hook
        let new_content = fs::read_to_string(&hook_path).unwrap();
        assert!(new_content.contains(HOOK_MARKER));

        // Uninstall should restore the backup
        uninstall_hooks(repo.path(), &[HookType::PrepareCommitMsg]).unwrap();
        let restored_content = fs::read_to_string(&hook_path).unwrap();
        assert_eq!(restored_content, existing_content);
        assert!(
            !backup_path.exists(),
            "backup should be removed after restore"
        );
    }

    #[test]
    fn test_list_installed_hooks() {
        let repo = create_fake_git_repo();

        // Initially empty
        assert!(list_installed_hooks(repo.path()).is_empty());

        // Install two hooks
        install_hooks(
            repo.path(),
            &[HookType::PrepareCommitMsg, HookType::PostCommit],
        )
        .unwrap();

        let installed = list_installed_hooks(repo.path());
        assert_eq!(installed.len(), 2);
        assert!(installed.contains(&HookType::PrepareCommitMsg));
        assert!(installed.contains(&HookType::PostCommit));
        assert!(!installed.contains(&HookType::PrePush));
    }

    #[test]
    fn test_list_ignores_non_opensession_hooks() {
        let repo = create_fake_git_repo();
        let hooks_dir = repo.path().join(".git/hooks");

        // Write a non-opensession hook
        fs::write(hooks_dir.join("pre-push"), "#!/bin/sh\necho 'custom'\n").unwrap();

        let installed = list_installed_hooks(repo.path());
        assert!(installed.is_empty());
    }

    #[test]
    fn test_install_not_a_git_repo() {
        let dir = TempDir::new().unwrap();
        let result = install_hooks(dir.path(), HookType::all());
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Not a git repository"));
    }

    #[test]
    fn test_reinstall_overwrites_opensession_hook() {
        let repo = create_fake_git_repo();

        // Install once
        install_hooks(repo.path(), &[HookType::PostCommit]).unwrap();

        // Install again — should succeed without backup (it's already ours)
        install_hooks(repo.path(), &[HookType::PostCommit]).unwrap();

        let hooks_dir = repo.path().join(".git/hooks");
        let backup_path = hooks_dir.join("post-commit.pre-opensession");
        assert!(
            !backup_path.exists(),
            "should not create backup for opensession-managed hooks"
        );
    }

    #[test]
    fn test_scan_for_secrets() {
        let content = r#"
OPENAI_KEY = "sk-abcdefghijklmnopqrstuvwxyz1234567890"
some normal code here
ghp_abcdefghijklmnopqrstuvwxyz1234567890
gho_abcdefghijklmnopqrstuvwxyz1234567890
-----BEGIN RSA PRIVATE KEY-----
AWS_SECRET_ACCESS_KEY = AKIAIOSFODNN7EXAMPLE
api_key = 'abcdefghijklmnop1234'
"#;

        let matches = scan_for_secrets(content);
        assert!(
            matches.len() >= 5,
            "expected at least 5 secret matches, got {}",
            matches.len(),
        );

        // Verify redaction
        for m in &matches {
            assert!(
                m.context.contains("[REDACTED]"),
                "should be redacted: {:?}",
                m
            );
            assert!(m.line_number > 0, "line numbers should be 1-indexed");
        }

        // Check specific patterns were detected
        let pattern_names: Vec<&str> = matches.iter().map(|m| m.pattern_name.as_str()).collect();
        assert!(pattern_names.contains(&"OpenAI API Key"));
        assert!(pattern_names.contains(&"GitHub PAT"));
        assert!(pattern_names.contains(&"GitHub OAuth Token"));
        assert!(pattern_names.contains(&"Private Key"));
        assert!(pattern_names.contains(&"AWS Secret"));
    }

    #[test]
    fn test_scan_no_false_positives() {
        let content = r#"
let name = "hello world";
const MAX_RETRIES = 3;
fn process_data(input: &str) -> Result<()> {
    let config = load_config("settings.toml")?;
    println!("Processing {} items", items.len());
    Ok(())
}
// Short key-like strings should not match
let sk = "short";
let key = "abc123";
"#;

        let matches = scan_for_secrets(content);
        assert!(
            matches.is_empty(),
            "expected no matches for normal code, got: {:?}",
            matches,
        );
    }

    #[test]
    fn test_scan_anthropic_key() {
        let content = "ANTHROPIC_API_KEY=sk-ant-api03-abcdefghijklmnopqrstuvwx";
        let matches = scan_for_secrets(content);
        assert!(!matches.is_empty(), "should detect Anthropic API key");
        let pattern_names: Vec<&str> = matches.iter().map(|m| m.pattern_name.as_str()).collect();
        assert!(pattern_names.contains(&"Anthropic API Key"));
    }
}
