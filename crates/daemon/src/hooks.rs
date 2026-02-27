use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

/// Git hook types managed by opensession.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookType {
    PrePush,
}

impl HookType {
    /// The filename used in `.git/hooks/`.
    pub fn filename(&self) -> &'static str {
        match self {
            Self::PrePush => "pre-push",
        }
    }

    pub fn all() -> &'static [HookType] {
        &[Self::PrePush]
    }
}

// ---------------------------------------------------------------------------
// Hook templates
// ---------------------------------------------------------------------------

const PRE_PUSH_HOOK: &str = r#"#!/bin/sh
# opensession-managed — do not edit
# Best-effort ledger fanout + secret scan.

remote="$1"
_url="$2"

YELLOW='\033[1;33m'
RED='\033[1;31m'
NC='\033[0m'

found_secrets=0
fanout_failed=0
strict_mode=0
fanout_mode="hidden_ref"
notes_ref="refs/notes/opensession"

case "${OPENSESSION_STRICT:-0}" in
    1|true|TRUE|yes|on)
        strict_mode=1
        ;;
esac

if [ "${OPENSESSION_INTERNAL_PUSH:-}" = "1" ]; then
    exit 0
fi

original_hook="$(dirname "$0")/pre-push.original.pre-opensession"
if [ -f "$original_hook" ]; then
    sh "$original_hook" "$@" || exit $?
fi

tmp_fanout="/tmp/opensession-ledger-fanout.$$"
: > "$tmp_fanout"
tmp_notes="/tmp/opensession-notes-fanout.$$"
: > "$tmp_notes"

opsession_bin=""
shim_bin="${HOME}/.local/share/opensession/bin/opensession"
if [ -x "$shim_bin" ]; then
    opsession_bin="$shim_bin"
elif command -v opensession >/dev/null 2>&1; then
    opsession_bin="$(command -v opensession)"
fi

fanout_enabled=0
if [ -n "$opsession_bin" ]; then
    fanout_enabled=1
    detected_mode=$("$opsession_bin" setup --print-fanout-mode 2>/dev/null || true)
    case "$detected_mode" in
        hidden_ref|git_notes)
            fanout_mode="$detected_mode"
            ;;
    esac
fi

while read local_ref local_oid remote_ref remote_oid; do
    # Skip delete pushes
    if [ "$local_oid" = "0000000000000000000000000000000000000000" ]; then
        continue
    fi

    case "$local_ref" in
        refs/heads/*)
            if [ "$fanout_enabled" -eq 1 ]; then
                branch="${local_ref#refs/heads/}"
                "$opsession_bin" setup --sync-branch-session "$branch" --sync-branch-commit "$local_oid" >/dev/null 2>&1 || true
                ledger_ref=$("$opsession_bin" setup --print-ledger-ref "$branch" 2>/dev/null || true)
                if [ -n "$ledger_ref" ]; then
                    printf '%s\n' "$ledger_ref" >> "$tmp_fanout"
                fi
                if [ "$fanout_mode" = "git_notes" ]; then
                    printf '%s\t%s\t%s\n' "$branch" "$local_ref" "$local_oid" >> "$tmp_notes"
                fi
            fi
            ;;
    esac

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

if [ "$fanout_enabled" -eq 0 ]; then
    printf "${YELLOW}[opensession]${NC} Warning: opensession CLI/shim not found; skipping ledger fanout.\n"
    if [ "$strict_mode" -eq 1 ]; then
        printf "${RED}[opensession]${NC} Error: OPENSESSION_STRICT=1 and fanout helper is unavailable.\n"
        exit 1
    fi
else
    tmp_sorted="${tmp_fanout}.sorted"
    sort -u "$tmp_fanout" > "$tmp_sorted" 2>/dev/null
    while IFS= read -r ledger_ref; do
        [ -z "$ledger_ref" ] && continue
        # No local ledger yet for this branch; skip silently.
        if ! git show-ref --verify --quiet "$ledger_ref"; then
            continue
        fi

        push_ok=0
        for attempt in 1 2; do
            if OPENSESSION_INTERNAL_PUSH=1 git push --no-verify "$remote" "$ledger_ref:$ledger_ref" >/dev/null 2>&1; then
                push_ok=1
                break
            fi
            # Best-effort retry for transient network/remote races.
            sleep 1
        done

        if [ "$push_ok" -ne 1 ]; then
            fanout_failed=1
            printf "${YELLOW}[opensession]${NC} Warning: failed to push ledger ref %s\n" "$ledger_ref"
        fi
    done < "$tmp_sorted"
    rm -f "$tmp_sorted"

    if [ "$fanout_mode" = "git_notes" ]; then
        tmp_notes_sorted="${tmp_notes}.sorted"
        sort -u "$tmp_notes" > "$tmp_notes_sorted" 2>/dev/null
        while IFS="$(printf '\t')" read -r branch local_ref local_oid; do
            [ -z "$local_oid" ] && continue

            # Keep existing commit notes stable across repeated pushes.
            if git notes --ref=opensession show "$local_oid" >/dev/null 2>&1; then
                continue
            fi

            copied_from=""
            parent_oids=$(git show -s --format=%P "$local_oid" 2>/dev/null || true)
            if [ -n "$parent_oids" ]; then
                first_parent=$(printf '%s\n' "$parent_oids" | awk '{print $1}')
                other_parents=$(printf '%s\n' "$parent_oids" | cut -d' ' -f2-)

                # Prefer non-first parents for merge commits (PR head side).
                for parent_oid in $other_parents; do
                    if git notes --ref=opensession show "$parent_oid" >/dev/null 2>&1; then
                        if git notes --ref=opensession copy -f "$parent_oid" "$local_oid" >/dev/null 2>&1; then
                            copied_from="$parent_oid"
                            break
                        fi
                    fi
                done

                if [ -z "$copied_from" ] && [ -n "$first_parent" ]; then
                    if git notes --ref=opensession show "$first_parent" >/dev/null 2>&1; then
                        if git notes --ref=opensession copy -f "$first_parent" "$local_oid" >/dev/null 2>&1; then
                            copied_from="$first_parent"
                        fi
                    fi
                fi
            fi

            if [ -z "$copied_from" ]; then
                note_body=$(cat <<EOF
opensession fanout mode: git_notes
branch: $branch
local_ref: $local_ref
commit: $local_oid
generated_at: $(date -u +"%Y-%m-%dT%H:%M:%SZ")
EOF
)
                if ! git notes --ref=opensession add -f -m "$note_body" "$local_oid" >/dev/null 2>&1; then
                    fanout_failed=1
                    printf "${YELLOW}[opensession]${NC} Warning: failed to write git note for %s\n" "$local_oid"
                fi
            fi
        done < "$tmp_notes_sorted"
        rm -f "$tmp_notes_sorted"

        if git show-ref --verify --quiet "$notes_ref"; then
            if ! OPENSESSION_INTERNAL_PUSH=1 git push --no-verify "$remote" "$notes_ref:$notes_ref" >/dev/null 2>&1; then
                fanout_failed=1
                printf "${YELLOW}[opensession]${NC} Warning: failed to push notes ref %s\n" "$notes_ref"
            fi
        fi
    fi
fi

rm -f "$tmp_fanout"
rm -f "$tmp_notes"

if [ "$found_secrets" -eq 1 ]; then
    printf "${YELLOW}[opensession]${NC} Warning: potential secrets detected. Review before pushing.\n"
    echo "  To push anyway: git push --no-verify"
    echo ""
    # Warning only — don't block the push by default
    # To make it blocking, uncomment: exit 1
    exit 0
fi

if [ "$fanout_failed" -eq 1 ]; then
    printf "${YELLOW}[opensession]${NC} Warning: one or more ledger fanout pushes failed.\n"
    if [ "$strict_mode" -eq 1 ]; then
        printf "${RED}[opensession]${NC} Error: OPENSESSION_STRICT=1 and fanout push failed.\n"
        exit 1
    fi
fi

exit 0
"#;

// ---------------------------------------------------------------------------
// Hook marker
// ---------------------------------------------------------------------------

const HOOK_MARKER: &str = "# opensession-managed";
const ORIGINAL_HOOK_SUFFIX: &str = ".original.pre-opensession";
const LEGACY_ORIGINAL_HOOK_SUFFIX: &str = ".pre-opensession";

// ---------------------------------------------------------------------------
// Hook template accessor
// ---------------------------------------------------------------------------

/// Generate the hook script content for a given hook type.
pub fn hook_template(hook_type: HookType) -> &'static str {
    match hook_type {
        HookType::PrePush => PRE_PUSH_HOOK,
    }
}

fn ensure_git_repo(repo_root: &Path) -> Result<()> {
    if repo_root.join(".git").exists() {
        return Ok(());
    }
    bail!(
        "Not a git repository: {} (missing .git metadata)",
        repo_root.display()
    );
}

fn configured_hooks_path(repo_root: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("config")
        .arg("--local")
        .arg("--get")
        .arg("core.hooksPath")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if raw.is_empty() {
        return None;
    }
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        Some(path)
    } else {
        Some(repo_root.join(path))
    }
}

fn hooks_dir(repo_root: &Path) -> PathBuf {
    configured_hooks_path(repo_root).unwrap_or_else(|| repo_root.join(".git").join("hooks"))
}

fn suffixed_hook_path(hook_path: &Path, suffix: &str) -> Option<PathBuf> {
    let filename = hook_path.file_name()?.to_str()?;
    Some(hook_path.with_file_name(format!("{filename}{suffix}")))
}

// ---------------------------------------------------------------------------
// Hook installer
// ---------------------------------------------------------------------------

/// Install opensession git hooks into a repository.
pub fn install_hooks(repo_root: &Path, hooks: &[HookType]) -> Result<Vec<HookType>> {
    ensure_git_repo(repo_root)?;
    let hooks_dir = hooks_dir(repo_root);
    std::fs::create_dir_all(&hooks_dir)
        .with_context(|| format!("create hooks directory {}", hooks_dir.display()))?;

    let mut installed = Vec::new();
    for hook_type in hooks {
        let hook_path = hooks_dir.join(hook_type.filename());
        let legacy_backup_path = suffixed_hook_path(&hook_path, LEGACY_ORIGINAL_HOOK_SUFFIX);
        let canonical_backup_path = suffixed_hook_path(&hook_path, ORIGINAL_HOOK_SUFFIX);

        // Check if there's an existing non-opensession hook
        if hook_path.exists() {
            let content = std::fs::read_to_string(&hook_path)
                .with_context(|| format!("read existing hook {}", hook_path.display()))?;
            if content.contains(HOOK_MARKER) {
                if let (Some(legacy_path), Some(canonical_path)) =
                    (legacy_backup_path.as_ref(), canonical_backup_path.as_ref())
                {
                    if legacy_path.exists() && !canonical_path.exists() {
                        std::fs::rename(legacy_path, canonical_path).with_context(|| {
                            format!(
                                "migrate legacy original hook {} -> {}",
                                legacy_path.display(),
                                canonical_path.display()
                            )
                        })?;
                    }
                }
            } else {
                // Backup existing hook
                let backup_path =
                    hooks_dir.join(format!("{}{}", hook_type.filename(), ORIGINAL_HOOK_SUFFIX));
                std::fs::rename(&hook_path, &backup_path).with_context(|| {
                    format!("preserve original hook at {}", backup_path.display())
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
    if !repo_root.join(".git").exists() {
        return Ok(Vec::new());
    }
    let hooks_dir = hooks_dir(repo_root);
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

        // Restore original hook copy if exists
        let backup_path =
            hooks_dir.join(format!("{}{}", hook_type.filename(), ORIGINAL_HOOK_SUFFIX));
        let legacy_backup_path = hooks_dir.join(format!(
            "{}{}",
            hook_type.filename(),
            LEGACY_ORIGINAL_HOOK_SUFFIX
        ));
        if backup_path.exists() {
            std::fs::rename(&backup_path, &hook_path)?;
        } else if legacy_backup_path.exists() {
            std::fs::rename(&legacy_backup_path, &hook_path)?;
        }

        uninstalled.push(*hook_type);
    }

    Ok(uninstalled)
}

/// Check which opensession hooks are installed in a repository.
pub fn list_installed_hooks(repo_root: &Path) -> Vec<HookType> {
    if !repo_root.join(".git").exists() {
        return Vec::new();
    }
    let hooks_dir = hooks_dir(repo_root);
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
        assert_eq!(HookType::PrePush.filename(), "pre-push");
    }

    #[test]
    fn test_hook_type_all() {
        let all = HookType::all();
        assert_eq!(all.len(), 1);
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
        assert_eq!(installed.len(), hooks.len());

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
        assert_eq!(uninstalled.len(), hooks.len());

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
        let hook_path = hooks_dir.join("pre-push");
        fs::write(&hook_path, existing_content).unwrap();

        // Install opensession hooks
        install_hooks(repo.path(), &[HookType::PrePush]).unwrap();

        // Original should be backed up
        let backup_path = hooks_dir.join("pre-push.original.pre-opensession");
        assert!(backup_path.exists(), "backup should exist");
        let backup_content = fs::read_to_string(&backup_path).unwrap();
        assert_eq!(backup_content, existing_content);

        // New hook should be our hook
        let new_content = fs::read_to_string(&hook_path).unwrap();
        assert!(new_content.contains(HOOK_MARKER));

        // Uninstall should restore the backup
        uninstall_hooks(repo.path(), &[HookType::PrePush]).unwrap();
        let restored_content = fs::read_to_string(&hook_path).unwrap();
        assert_eq!(restored_content, existing_content);
        assert!(
            !backup_path.exists(),
            "backup should be removed after restore"
        );
    }

    #[test]
    fn test_installed_hook_invokes_backed_up_pre_push() {
        let repo = create_fake_git_repo();
        let hooks_dir = repo.path().join(".git/hooks");
        let marker = repo.path().join("backup-ran.txt");
        let custom_hook = format!("#!/bin/sh\necho backup-ran > \"{}\"\n", marker.display());
        let hook_path = hooks_dir.join("pre-push");
        fs::write(&hook_path, custom_hook).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755)).unwrap();
        }

        install_hooks(repo.path(), &[HookType::PrePush]).unwrap();

        let installed_hook = hooks_dir.join("pre-push");
        let status = std::process::Command::new("sh")
            .arg(&installed_hook)
            .arg("origin")
            .arg("https://example.com/repo.git")
            .current_dir(repo.path())
            .status()
            .expect("run installed pre-push");
        assert!(status.success());
        assert!(marker.exists(), "backed up pre-push should be invoked");
    }

    #[test]
    fn test_list_installed_hooks() {
        let repo = create_fake_git_repo();

        // Initially empty
        assert!(list_installed_hooks(repo.path()).is_empty());

        install_hooks(repo.path(), HookType::all()).unwrap();

        let installed = list_installed_hooks(repo.path());
        assert_eq!(installed.len(), 1);
        assert!(installed.contains(&HookType::PrePush));
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
    fn test_install_hooks_respects_core_hooks_path() {
        let repo = TempDir::new().unwrap();
        let init = std::process::Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .arg("init")
            .output()
            .expect("git init");
        assert!(
            init.status.success(),
            "{}",
            String::from_utf8_lossy(&init.stderr)
        );
        let config = std::process::Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .arg("config")
            .arg("--local")
            .arg("core.hooksPath")
            .arg(".githooks")
            .output()
            .expect("git config core.hooksPath");
        assert!(
            config.status.success(),
            "{}",
            String::from_utf8_lossy(&config.stderr)
        );

        install_hooks(repo.path(), &[HookType::PrePush]).unwrap();

        let configured_hook = repo.path().join(".githooks").join("pre-push");
        let legacy_hook = repo.path().join(".git").join("hooks").join("pre-push");
        assert!(configured_hook.exists());
        assert!(!legacy_hook.exists());
        let installed = list_installed_hooks(repo.path());
        assert_eq!(installed, vec![HookType::PrePush]);
    }

    #[test]
    fn test_reinstall_overwrites_opensession_hook() {
        let repo = create_fake_git_repo();

        // Install once
        install_hooks(repo.path(), &[HookType::PrePush]).unwrap();

        // Install again — should succeed without backup (it's already ours)
        install_hooks(repo.path(), &[HookType::PrePush]).unwrap();

        let hooks_dir = repo.path().join(".git/hooks");
        let backup_path = hooks_dir.join("pre-push.original.pre-opensession");
        assert!(
            !backup_path.exists(),
            "should not create backup for opensession-managed hooks"
        );
    }

    #[test]
    fn test_pre_push_template_has_no_sqlite_dependency() {
        let template = hook_template(HookType::PrePush);
        assert!(!template.contains("sqlite3"));
        assert!(!template.contains("local.db"));
    }

    #[test]
    fn test_pre_push_template_contains_fanout_guard() {
        let template = hook_template(HookType::PrePush);
        assert!(template.contains("OPENSESSION_INTERNAL_PUSH"));
        assert!(template.contains("setup --sync-branch-session"));
        assert!(template.contains("--sync-branch-commit"));
        assert!(template.contains("setup --print-ledger-ref"));
        assert!(template.contains("setup --print-fanout-mode"));
        assert!(template.contains("git notes --ref=opensession"));
        assert!(template.contains("git notes --ref=opensession copy -f"));
        assert!(template.contains("git show -s --format=%P"));
        assert!(template.contains("refs/notes/opensession"));
        assert!(template.contains(".local/share/opensession/bin/opensession"));
        assert!(template.contains("OPENSESSION_STRICT"));
        assert!(template.contains("git show-ref --verify --quiet \"$ledger_ref\""));
        assert!(template.contains("for attempt in 1 2; do"));
    }

    #[test]
    fn test_pre_push_template_pushes_hidden_refs_before_notes_branch() {
        let template = hook_template(HookType::PrePush);
        let hidden_push = template
            .find("sort -u \"$tmp_fanout\"")
            .expect("hidden fanout loop must exist");
        let notes_branch = template[hidden_push..]
            .find("if [ \"$fanout_mode\" = \"git_notes\" ]; then")
            .map(|idx| idx + hidden_push)
            .expect("notes branch guard after hidden fanout must exist");
        assert!(
            hidden_push < notes_branch,
            "hidden refs must be pushed regardless of notes mode"
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
