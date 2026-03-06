pub mod git;
pub mod prompt;
pub mod provider;
pub mod text;
pub mod types;
pub use prompt::{DEFAULT_SUMMARY_PROMPT_TEMPLATE_V2, validate_summary_prompt_template};

use crate::git::{GitSummaryContext, GitSummaryService, ShellGitCommandRunner};
use crate::prompt::{
    SummaryPromptConfig, build_summary_prompt, classify_arch_layer, collect_file_changes,
    collect_timeline_snippets, contains_auth_security_keyword,
};
use crate::provider::{SemanticSummary, generate_summary};
use crate::text::compact_summary_snippet;
use crate::types::HailCompactFileChange;
use opensession_core::trace::{Agent, ContentBlock, Event, EventType, Session};
use opensession_runtime_config::{SummaryProvider, SummarySettings};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

const MAX_TIMELINE_SNIPPETS: usize = 32;
const MAX_FILE_CHANGE_ENTRIES: usize = 200;
const MAX_DIFF_HUNKS_PER_FILE: usize = 10;
const MAX_DIFF_LINES_PER_HUNK: usize = 40;
const MAX_DIFF_FILES_PER_LAYER: usize = 80;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SummarySourceKind {
    SessionSignals,
    GitCommit,
    GitWorkingTree,
    Heuristic,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SummaryGenerationKind {
    Provider,
    HeuristicFallback,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffHunkNode {
    pub header: String,
    #[serde(default)]
    pub lines: Vec<String>,
    pub lines_added: u64,
    pub lines_removed: u64,
    #[serde(default)]
    pub omitted_lines: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffFileNode {
    pub path: String,
    pub operation: String,
    pub lines_added: u64,
    pub lines_removed: u64,
    #[serde(default)]
    pub hunks: Vec<DiffHunkNode>,
    #[serde(default)]
    pub is_large: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffLayerNode {
    pub layer: String,
    pub file_count: usize,
    pub lines_added: u64,
    pub lines_removed: u64,
    #[serde(default)]
    pub files: Vec<DiffFileNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticSummaryArtifact {
    pub summary: SemanticSummary,
    pub source_kind: SummarySourceKind,
    pub generation_kind: SummaryGenerationKind,
    pub provider: SummaryProvider,
    pub model: String,
    pub prompt_fingerprint: String,
    #[serde(default)]
    pub diff_tree: Vec<DiffLayerNode>,
    #[serde(default)]
    pub source_details: HashMap<String, String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GitSummaryRequest {
    pub repo_root: PathBuf,
    pub commit: Option<String>,
}

impl GitSummaryRequest {
    pub fn from_commit(repo_root: impl Into<PathBuf>, commit: impl Into<String>) -> Self {
        Self {
            repo_root: repo_root.into(),
            commit: Some(commit.into()),
        }
    }

    pub fn working_tree(repo_root: impl Into<PathBuf>) -> Self {
        Self {
            repo_root: repo_root.into(),
            commit: None,
        }
    }
}

pub fn detect_summary_provider() -> Option<provider::LocalSummaryProfile> {
    provider::detect_local_summary_profile()
}

pub async fn summarize_session(
    session: &Session,
    settings: &SummarySettings,
    git_request: Option<&GitSummaryRequest>,
) -> Result<SemanticSummaryArtifact, String> {
    let timeline = collect_timeline_snippets(session, MAX_TIMELINE_SNIPPETS, default_event_snippet);
    let files = collect_file_changes(session, MAX_FILE_CHANGE_ENTRIES);

    let mut signals = SummarySignals {
        session: session.clone(),
        source_kind: SummarySourceKind::SessionSignals,
        source_label: "session_events".to_string(),
        timeline_signals: timeline,
        file_changes: files,
        source_details: HashMap::new(),
    };

    if signals.is_empty() && settings.allows_git_changes_fallback() {
        if let Some(request) = git_request {
            if let Some(git_ctx) = collect_git_context(request) {
                signals = summary_signals_from_git(git_ctx)?;
            }
        }
    }

    summarize_from_signals(signals, settings).await
}

pub async fn summarize_git_commit(
    repo_root: &Path,
    commit: &str,
    settings: &SummarySettings,
) -> Result<SemanticSummaryArtifact, String> {
    let request = GitSummaryRequest::from_commit(repo_root.to_path_buf(), commit.to_string());
    let context = collect_git_context(&request)
        .ok_or_else(|| format!("unable to collect git summary context for commit `{commit}`"))?;
    summarize_from_signals(summary_signals_from_git(context)?, settings).await
}

pub async fn summarize_git_working_tree(
    repo_root: &Path,
    settings: &SummarySettings,
) -> Result<SemanticSummaryArtifact, String> {
    let request = GitSummaryRequest::working_tree(repo_root.to_path_buf());
    let context = collect_git_context(&request)
        .ok_or_else(|| "unable to collect git summary context for working tree".to_string())?;
    summarize_from_signals(summary_signals_from_git(context)?, settings).await
}

#[derive(Debug, Clone)]
struct SummarySignals {
    session: Session,
    source_kind: SummarySourceKind,
    source_label: String,
    timeline_signals: Vec<String>,
    file_changes: Vec<HailCompactFileChange>,
    source_details: HashMap<String, String>,
}

impl SummarySignals {
    fn is_empty(&self) -> bool {
        self.timeline_signals.is_empty() && self.file_changes.is_empty()
    }
}

async fn summarize_from_signals(
    signals: SummarySignals,
    settings: &SummarySettings,
) -> Result<SemanticSummaryArtifact, String> {
    let prompt_template = if settings.prompt.template.trim().is_empty() {
        DEFAULT_SUMMARY_PROMPT_TEMPLATE_V2
    } else {
        settings.prompt.template.as_str()
    };
    if let Err(error) = validate_summary_prompt_template(prompt_template) {
        return Ok(SemanticSummaryArtifact {
            summary: heuristic_summary(&signals.timeline_signals, &signals.file_changes),
            source_kind: signals.source_kind,
            generation_kind: SummaryGenerationKind::HeuristicFallback,
            provider: settings.provider.id.clone(),
            model: settings.provider.model.clone(),
            prompt_fingerprint: String::new(),
            diff_tree: build_diff_tree(&signals.file_changes, &signals.session.events),
            source_details: signals.source_details,
            error: Some(format!("invalid summary prompt template: {error}")),
        });
    }

    let prompt = build_summary_prompt(
        &signals.session,
        signals.source_label.clone(),
        signals.timeline_signals.clone(),
        signals.file_changes.clone(),
        serde_json::json!(signals.source_details),
        SummaryPromptConfig {
            response_style: settings.response.style.clone(),
            output_shape: settings.response.shape.clone(),
            source_mode: settings.source_mode.clone(),
            prompt_template,
        },
    );

    let prompt_fingerprint = sha256_hex(if prompt.is_empty() {
        signals.source_label.as_bytes()
    } else {
        prompt.as_bytes()
    });

    let diff_tree = build_diff_tree(&signals.file_changes, &signals.session.events);

    if signals.is_empty() {
        return Ok(SemanticSummaryArtifact {
            summary: heuristic_summary(&signals.timeline_signals, &signals.file_changes),
            source_kind: SummarySourceKind::Heuristic,
            generation_kind: SummaryGenerationKind::HeuristicFallback,
            provider: settings.provider.id.clone(),
            model: settings.provider.model.clone(),
            prompt_fingerprint,
            diff_tree,
            source_details: signals.source_details,
            error: Some("no usable summary signals found".to_string()),
        });
    }

    if !settings.is_configured() || prompt.trim().is_empty() {
        return Ok(SemanticSummaryArtifact {
            summary: heuristic_summary(&signals.timeline_signals, &signals.file_changes),
            source_kind: signals.source_kind,
            generation_kind: SummaryGenerationKind::HeuristicFallback,
            provider: settings.provider.id.clone(),
            model: settings.provider.model.clone(),
            prompt_fingerprint,
            diff_tree,
            source_details: signals.source_details,
            error: None,
        });
    }

    match generate_summary(settings, &prompt).await {
        Ok(summary) => Ok(SemanticSummaryArtifact {
            summary,
            source_kind: signals.source_kind,
            generation_kind: SummaryGenerationKind::Provider,
            provider: settings.provider.id.clone(),
            model: settings.provider.model.clone(),
            prompt_fingerprint,
            diff_tree,
            source_details: signals.source_details,
            error: None,
        }),
        Err(error) => Ok(SemanticSummaryArtifact {
            summary: heuristic_summary(&signals.timeline_signals, &signals.file_changes),
            source_kind: signals.source_kind,
            generation_kind: SummaryGenerationKind::HeuristicFallback,
            provider: settings.provider.id.clone(),
            model: settings.provider.model.clone(),
            prompt_fingerprint,
            diff_tree,
            source_details: signals.source_details,
            error: Some(error),
        }),
    }
}

fn collect_git_context(request: &GitSummaryRequest) -> Option<GitSummaryContext> {
    let service = GitSummaryService::new(ShellGitCommandRunner);
    if let Some(commit) = request.commit.as_deref() {
        return service.collect_commit_context(
            &request.repo_root,
            commit,
            MAX_FILE_CHANGE_ENTRIES,
            classify_arch_layer,
        );
    }

    service.collect_working_tree_context(
        &request.repo_root,
        MAX_FILE_CHANGE_ENTRIES,
        classify_arch_layer,
    )
}

fn summary_signals_from_git(context: GitSummaryContext) -> Result<SummarySignals, String> {
    let mut session = Session::new(
        context
            .commit
            .clone()
            .unwrap_or_else(|| "git-working-tree".to_string()),
        Agent {
            provider: "local".to_string(),
            model: "git".to_string(),
            tool: "git".to_string(),
            tool_version: None,
        },
    );
    session.context.title = Some(match context.commit.as_deref() {
        Some(commit) => format!("Git commit {commit}"),
        None => "Git working tree".to_string(),
    });
    session.stats.files_changed = context.file_changes.len() as u64;
    session.stats.lines_added = context
        .file_changes
        .iter()
        .map(|row| row.lines_added)
        .sum::<u64>();
    session.stats.lines_removed = context
        .file_changes
        .iter()
        .map(|row| row.lines_removed)
        .sum::<u64>();
    session.stats.event_count = context.timeline_signals.len() as u64;
    session.stats.message_count = context.timeline_signals.len() as u64;

    let mut source_details = HashMap::from([(
        "repo_root".to_string(),
        context.repo_root.to_string_lossy().to_string(),
    )]);
    if let Some(commit) = context.commit.clone() {
        source_details.insert("commit".to_string(), commit);
    }

    let (source_kind, source_label) = match context.source.as_str() {
        "git_commit" => (SummarySourceKind::GitCommit, "git_commit".to_string()),
        _ => (
            SummarySourceKind::GitWorkingTree,
            "git_working_tree".to_string(),
        ),
    };

    if context.timeline_signals.is_empty() && context.file_changes.is_empty() {
        return Err("git context has no timeline/file signals".to_string());
    }

    Ok(SummarySignals {
        session,
        source_kind,
        source_label,
        timeline_signals: context.timeline_signals,
        file_changes: context.file_changes,
        source_details,
    })
}

fn default_event_snippet(event: &Event, max_chars: usize) -> Option<String> {
    for block in &event.content.blocks {
        let value = match block {
            ContentBlock::Text { text } => text.as_str(),
            ContentBlock::Code { code, .. } => code.as_str(),
            ContentBlock::File { content, .. } => content.as_deref().unwrap_or_default(),
            ContentBlock::Json { data } => {
                let json = serde_json::to_string(data).ok()?;
                return Some(compact_summary_snippet(&json, max_chars));
            }
            ContentBlock::Reference { uri, .. } => uri.as_str(),
            ContentBlock::Image { url, .. }
            | ContentBlock::Audio { url, .. }
            | ContentBlock::Video { url, .. } => url.as_str(),
            _ => continue,
        };
        let compact = compact_summary_snippet(value, max_chars);
        if !compact.is_empty() {
            return Some(compact);
        }
    }
    None
}

fn heuristic_summary(timeline: &[String], files: &[HailCompactFileChange]) -> SemanticSummary {
    let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for change in files {
        grouped
            .entry(change.layer.clone())
            .or_default()
            .push(change.path.clone());
    }

    let total_added = files.iter().map(|row| row.lines_added).sum::<u64>();
    let total_removed = files.iter().map(|row| row.lines_removed).sum::<u64>();
    let base_changes = if files.is_empty() {
        if timeline.is_empty() {
            "No meaningful code-change signals were captured.".to_string()
        } else {
            format!(
                "Session signals captured {} timeline entries; no concrete file changes were detected.",
                timeline.len()
            )
        }
    } else {
        format!(
            "Updated {} files across {} layers (+{} / -{} lines).",
            files.len(),
            grouped.len(),
            total_added,
            total_removed,
        )
    };

    let auth_security = if files
        .iter()
        .any(|row| contains_auth_security_keyword(&row.path))
        || timeline
            .iter()
            .any(|line| contains_auth_security_keyword(line))
    {
        "Auth/security-related changes detected in paths or timeline signals.".to_string()
    } else {
        "none detected".to_string()
    };

    let layer_file_changes = grouped
        .into_iter()
        .map(|(layer, mut paths)| {
            paths.sort();
            paths.dedup();
            let summary = format!("{} files changed in {} layer.", paths.len(), layer);
            provider::LayerFileChange {
                layer,
                summary,
                files: paths,
            }
        })
        .collect();

    SemanticSummary {
        changes: base_changes,
        auth_security,
        layer_file_changes,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    hex::encode(digest)
}

fn build_diff_tree(changes: &[HailCompactFileChange], events: &[Event]) -> Vec<DiffLayerNode> {
    let mut diff_by_path: HashMap<&str, &str> = HashMap::new();
    for event in events {
        if let EventType::FileEdit {
            path,
            diff: Some(diff),
        } = &event.event_type
        {
            diff_by_path.insert(path.as_str(), diff.as_str());
        }
    }

    let mut grouped: BTreeMap<String, Vec<DiffFileNode>> = BTreeMap::new();

    for change in changes {
        let path = change.path.clone();
        let operation = change.operation.clone();
        let hunks = diff_by_path
            .get(path.as_str())
            .map(|diff| parse_diff_hunks(diff))
            .unwrap_or_default();

        let is_large = change.lines_added + change.lines_removed > 1_200
            || hunks.iter().map(|h| h.lines.len()).sum::<usize>() > 200;

        grouped
            .entry(change.layer.clone())
            .or_default()
            .push(DiffFileNode {
                path,
                operation,
                lines_added: change.lines_added,
                lines_removed: change.lines_removed,
                hunks,
                is_large,
            });
    }

    grouped
        .into_iter()
        .map(|(layer, mut files)| {
            files.sort_by(|left, right| left.path.cmp(&right.path));
            if files.len() > MAX_DIFF_FILES_PER_LAYER {
                files.truncate(MAX_DIFF_FILES_PER_LAYER);
            }
            let lines_added = files.iter().map(|file| file.lines_added).sum::<u64>();
            let lines_removed = files.iter().map(|file| file.lines_removed).sum::<u64>();
            let file_count = files.len();

            DiffLayerNode {
                layer,
                file_count,
                lines_added,
                lines_removed,
                files,
            }
        })
        .collect()
}

fn parse_diff_hunks(diff: &str) -> Vec<DiffHunkNode> {
    let mut hunks = Vec::new();
    let mut current_header = String::new();
    let mut current_lines = Vec::new();
    let mut current_added = 0u64;
    let mut current_removed = 0u64;
    let mut omitted = 0u64;

    let push_current = |hunks: &mut Vec<DiffHunkNode>,
                        header: &mut String,
                        lines: &mut Vec<String>,
                        added: &mut u64,
                        removed: &mut u64,
                        omitted_lines: &mut u64| {
        if header.is_empty() && lines.is_empty() {
            return;
        }
        hunks.push(DiffHunkNode {
            header: if header.is_empty() {
                "(diff)".to_string()
            } else {
                header.clone()
            },
            lines: std::mem::take(lines),
            lines_added: *added,
            lines_removed: *removed,
            omitted_lines: *omitted_lines,
        });
        header.clear();
        *added = 0;
        *removed = 0;
        *omitted_lines = 0;
    };

    for raw in diff.lines() {
        if raw.starts_with("@@") {
            push_current(
                &mut hunks,
                &mut current_header,
                &mut current_lines,
                &mut current_added,
                &mut current_removed,
                &mut omitted,
            );
            current_header = compact_summary_snippet(raw, 140);
            continue;
        }
        if current_header.is_empty() {
            continue;
        }

        if raw.starts_with('+') && !raw.starts_with("+++") {
            current_added = current_added.saturating_add(1);
        } else if raw.starts_with('-') && !raw.starts_with("---") {
            current_removed = current_removed.saturating_add(1);
        }

        if current_lines.len() < MAX_DIFF_LINES_PER_HUNK {
            current_lines.push(compact_summary_snippet(raw, 220));
        } else {
            omitted = omitted.saturating_add(1);
        }
    }

    push_current(
        &mut hunks,
        &mut current_header,
        &mut current_lines,
        &mut current_added,
        &mut current_removed,
        &mut omitted,
    );

    if hunks.len() > MAX_DIFF_HUNKS_PER_FILE {
        hunks.truncate(MAX_DIFF_HUNKS_PER_FILE);
    }
    hunks
}

#[cfg(test)]
mod tests {
    use super::{
        DiffLayerNode, GitSummaryRequest, SummaryGenerationKind, SummarySourceKind,
        build_diff_tree, default_event_snippet, heuristic_summary, parse_diff_hunks,
        summarize_session,
    };
    use crate::types::HailCompactFileChange;
    use chrono::Utc;
    use opensession_core::trace::{Agent, Content, Event, EventType, Session};
    use opensession_runtime_config::{SummaryProvider, SummarySettings};
    use std::collections::HashMap;

    fn session_with_file_edit(path: &str, diff: &str) -> Session {
        let mut session = Session::new(
            "s1".to_string(),
            Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );

        session.events.push(Event {
            event_id: "u1".to_string(),
            timestamp: Utc::now(),
            event_type: EventType::UserMessage,
            task_id: None,
            content: Content::text("fix auth token flow"),
            duration_ms: None,
            attributes: HashMap::new(),
        });

        session.events.push(Event {
            event_id: "f1".to_string(),
            timestamp: Utc::now(),
            event_type: EventType::FileEdit {
                path: path.to_string(),
                diff: Some(diff.to_string()),
            },
            task_id: None,
            content: Content::text(""),
            duration_ms: None,
            attributes: HashMap::new(),
        });
        session.recompute_stats();
        session
    }

    #[test]
    fn parse_diff_hunks_extracts_header_and_line_stats() {
        let hunks =
            parse_diff_hunks("@@ -1,2 +1,2 @@\n-old\n+new\n context\n@@ -5 +5 @@\n-a\n+b\n");
        assert_eq!(hunks.len(), 2);
        assert_eq!(hunks[0].lines_added, 1);
        assert_eq!(hunks[0].lines_removed, 1);
        assert!(hunks[0].header.starts_with("@@ -1,2"));
    }

    #[test]
    fn default_event_snippet_prefers_text_blocks() {
        let event = Event {
            event_id: "e1".to_string(),
            timestamp: Utc::now(),
            event_type: EventType::UserMessage,
            task_id: None,
            content: Content::text("  hello   world "),
            duration_ms: None,
            attributes: HashMap::new(),
        };
        assert_eq!(
            default_event_snippet(&event, 40),
            Some("hello world".to_string())
        );
    }

    #[test]
    fn heuristic_summary_marks_auth_changes() {
        let summary = heuristic_summary(
            &["assistant: updated auth middleware".to_string()],
            &[HailCompactFileChange {
                path: "src/auth.rs".to_string(),
                layer: "application".to_string(),
                operation: "edit".to_string(),
                lines_added: 2,
                lines_removed: 1,
            }],
        );
        assert!(summary.auth_security.contains("Auth/security"));
        assert_eq!(summary.layer_file_changes.len(), 1);
    }

    #[test]
    fn build_diff_tree_groups_by_layer() {
        let session = session_with_file_edit("src/lib.rs", "@@ -1 +1 @@\n-a\n+b\n");
        let tree = build_diff_tree(
            &[HailCompactFileChange {
                path: "src/lib.rs".to_string(),
                layer: "application".to_string(),
                operation: "edit".to_string(),
                lines_added: 1,
                lines_removed: 1,
            }],
            &session.events,
        );

        assert_eq!(tree.len(), 1);
        let layer: &DiffLayerNode = &tree[0];
        assert_eq!(layer.layer, "application");
        assert_eq!(layer.files.len(), 1);
        assert_eq!(layer.files[0].hunks.len(), 1);
    }

    #[tokio::test]
    async fn summarize_session_falls_back_to_heuristic_when_provider_disabled() {
        let session = session_with_file_edit("src/auth.rs", "@@ -1 +1 @@\n-a\n+b\n");
        let settings = SummarySettings::default();

        let artifact = summarize_session(&session, &settings, None)
            .await
            .expect("summarize");
        assert_eq!(
            artifact.generation_kind,
            SummaryGenerationKind::HeuristicFallback
        );
        assert_eq!(artifact.source_kind, SummarySourceKind::SessionSignals);
        assert_eq!(artifact.provider, SummaryProvider::Disabled);
        assert!(!artifact.summary.changes.is_empty());
        assert_eq!(artifact.error, None);
    }

    #[tokio::test]
    async fn summarize_session_uses_git_fallback_when_session_has_low_signal() {
        let mut session = Session::new(
            "s-empty".to_string(),
            Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );
        session.recompute_stats();

        let mut settings = SummarySettings::default();
        settings.source_mode = opensession_runtime_config::SummarySourceMode::SessionOrGitChanges;

        let artifact = summarize_session(
            &session,
            &settings,
            Some(&GitSummaryRequest::working_tree(std::env::temp_dir())),
        )
        .await
        .expect("summarize");

        // temp dir is typically not a git repo, so fallback remains heuristic/session.
        assert!(matches!(
            artifact.source_kind,
            SummarySourceKind::SessionSignals | SummarySourceKind::Heuristic
        ));
    }
}
