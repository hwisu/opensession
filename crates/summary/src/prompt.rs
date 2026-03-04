use crate::text::compact_summary_snippet;
use crate::types::HailCompactFileChange;
use opensession_core::trace::{Event, EventType, Session};
use opensession_runtime_config::{SummaryOutputShape, SummaryResponseStyle, SummarySourceMode};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

const MAX_PROMPT_CHARS: usize = 10_000;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HailCompactLayerRollup {
    layer: String,
    file_count: usize,
    files: Vec<String>,
}

pub struct SummaryPromptConfig<'a> {
    pub response_style: SummaryResponseStyle,
    pub output_shape: SummaryOutputShape,
    pub source_mode: SummarySourceMode,
    pub output_instruction: &'a str,
}

pub fn collect_timeline_snippets(
    session: &Session,
    max_entries: usize,
    event_snippet: fn(&Event, usize) -> Option<String>,
) -> Vec<String> {
    let mut snippets = Vec::new();
    for event in session.events.iter().rev() {
        if snippets.len() >= max_entries {
            break;
        }

        let label = match &event.event_type {
            EventType::UserMessage => "user",
            EventType::AgentMessage => "assistant",
            EventType::Thinking => "thinking",
            EventType::TaskStart { .. } => "task_start",
            EventType::TaskEnd { .. } => "task_end",
            EventType::ToolCall { .. } | EventType::ToolResult { .. } => "tool",
            _ => continue,
        };

        let snippet = match &event.event_type {
            EventType::TaskEnd {
                summary: Some(summary),
            } => Some(compact_summary_snippet(summary, 220)),
            _ => event_snippet(event, 220),
        };
        let Some(text) = snippet else {
            continue;
        };
        if text.is_empty() {
            continue;
        }
        snippets.push(format!("{label}: {text}"));
    }
    snippets.reverse();
    snippets
}

pub fn count_diff_stats(diff: &str) -> (u64, u64) {
    let mut added = 0u64;
    let mut removed = 0u64;

    for line in diff.lines() {
        if line.starts_with("+++") || line.starts_with("---") {
            continue;
        }
        if line.starts_with('+') {
            added = added.saturating_add(1);
        } else if line.starts_with('-') {
            removed = removed.saturating_add(1);
        }
    }

    (added, removed)
}

pub fn classify_arch_layer(path: &str) -> &'static str {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    let file_name = normalized.rsplit('/').next().unwrap_or(normalized.as_str());

    if normalized.starts_with("docs/")
        || normalized.contains("/docs/")
        || file_name.ends_with(".md")
        || file_name.ends_with(".mdx")
    {
        return "docs";
    }

    if normalized.starts_with("tests/")
        || normalized.contains("/tests/")
        || normalized.contains("/test/")
        || file_name.ends_with("_test.rs")
        || file_name.ends_with(".spec.ts")
        || file_name.ends_with(".test.ts")
        || file_name.ends_with(".spec.tsx")
        || file_name.ends_with(".test.tsx")
        || file_name.ends_with(".spec.js")
        || file_name.ends_with(".test.js")
    {
        return "tests";
    }

    if normalized.ends_with("cargo.toml")
        || normalized.ends_with("cargo.lock")
        || normalized.ends_with("package.json")
        || normalized.ends_with("package-lock.json")
        || normalized.ends_with("pnpm-lock.yaml")
        || normalized.ends_with("yarn.lock")
        || normalized.ends_with("wrangler.toml")
        || normalized.ends_with(".toml")
        || normalized.ends_with(".yaml")
        || normalized.ends_with(".yml")
        || normalized.ends_with(".json")
        || normalized.ends_with(".ini")
        || normalized.ends_with(".conf")
        || normalized.starts_with("config/")
        || normalized.contains("/config/")
        || normalized.contains("runtime-config")
        || normalized.starts_with(".github/")
        || normalized.contains("/.github/")
    {
        return "config";
    }

    if normalized.contains("/ui/")
        || normalized.contains("/views/")
        || normalized.contains("/components/")
        || normalized.contains("/pages/")
        || normalized.contains("/widgets/")
        || normalized.contains("/frontend/")
        || normalized.contains("/presentation/")
        || normalized.contains("packages/ui/src/")
        || normalized.contains("web/src/routes/")
        || file_name == "ui.rs"
    {
        return "presentation";
    }

    if normalized.contains("/domain/")
        || normalized.contains("/entity/")
        || normalized.contains("/entities/")
        || normalized.contains("/model/")
        || normalized.contains("/models/")
        || normalized.contains("/value_object/")
        || normalized.contains("/aggregate/")
        || normalized.contains("crates/core/")
    {
        return "domain";
    }

    if normalized.contains("/infra/")
        || normalized.contains("/infrastructure/")
        || normalized.contains("/adapter/")
        || normalized.contains("/adapters/")
        || normalized.contains("/storage/")
        || normalized.contains("/repository/")
        || normalized.contains("/repositories/")
        || normalized.contains("/db/")
        || normalized.contains("/database/")
        || normalized.contains("/runtime/")
        || normalized.contains("/daemon/")
        || normalized.contains("/network/")
        || normalized.contains("/api/")
        || normalized.contains("/git/")
        || normalized.contains("/migrations/")
        || normalized.starts_with("scripts/")
    {
        return "infrastructure";
    }

    "application"
}

pub fn contains_auth_security_keyword(text: &str) -> bool {
    let normalized = text.to_ascii_lowercase();
    [
        "auth",
        "oauth",
        "oidc",
        "saml",
        "token",
        "jwt",
        "bearer",
        "apikey",
        "api_key",
        "api-key",
        "secret",
        "password",
        "credential",
        "login",
        "logout",
        "sign-in",
        "signin",
        "mfa",
        "2fa",
        "permission",
        "rbac",
        "acl",
        "encrypt",
        "decrypt",
        "security",
        "csrf",
        "xss",
        "csp",
        "cookie",
        "set-cookie",
        "hmac",
        "signature",
        "nonce",
        "tls",
        "ssl",
    ]
    .iter()
    .any(|keyword| normalized.contains(keyword))
}

pub fn collect_file_changes(session: &Session, max_entries: usize) -> Vec<HailCompactFileChange> {
    let mut by_path: HashMap<String, HailCompactFileChange> = HashMap::new();
    for event in &session.events {
        match &event.event_type {
            EventType::FileEdit { path, diff } => {
                let (added, removed) = count_diff_stats(diff.as_deref().unwrap_or_default());
                let entry = by_path
                    .entry(path.clone())
                    .or_insert_with(|| HailCompactFileChange {
                        path: path.clone(),
                        layer: classify_arch_layer(path).to_string(),
                        operation: "edit".to_string(),
                        lines_added: 0,
                        lines_removed: 0,
                    });
                entry.operation = "edit".to_string();
                entry.lines_added = entry.lines_added.saturating_add(added);
                entry.lines_removed = entry.lines_removed.saturating_add(removed);
            }
            EventType::FileCreate { path } => {
                by_path
                    .entry(path.clone())
                    .and_modify(|entry| {
                        entry.operation = "create".to_string();
                        entry.layer = classify_arch_layer(path).to_string();
                    })
                    .or_insert_with(|| HailCompactFileChange {
                        path: path.clone(),
                        layer: classify_arch_layer(path).to_string(),
                        operation: "create".to_string(),
                        lines_added: 0,
                        lines_removed: 0,
                    });
            }
            EventType::FileDelete { path } => {
                by_path
                    .entry(path.clone())
                    .and_modify(|entry| {
                        entry.operation = "delete".to_string();
                        entry.layer = classify_arch_layer(path).to_string();
                    })
                    .or_insert_with(|| HailCompactFileChange {
                        path: path.clone(),
                        layer: classify_arch_layer(path).to_string(),
                        operation: "delete".to_string(),
                        lines_added: 0,
                        lines_removed: 0,
                    });
            }
            _ => {}
        }
    }

    let mut changes = by_path.into_values().collect::<Vec<_>>();
    changes.sort_by(|lhs, rhs| lhs.path.cmp(&rhs.path));
    changes.truncate(max_entries);
    changes
}

pub fn build_summary_prompt(
    session: &Session,
    source_kind: String,
    timeline_snippets: Vec<String>,
    file_changes: Vec<HailCompactFileChange>,
    git_context: Value,
    config: SummaryPromptConfig<'_>,
) -> String {
    if timeline_snippets.is_empty() && file_changes.is_empty() {
        return String::new();
    }

    let layer_rollup = summarize_layer_rollup(&file_changes);
    let auth_security_signals = collect_auth_security_signals(&file_changes, &timeline_snippets);

    let title = session
        .context
        .title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
        .unwrap_or(session.session_id.as_str());

    let hail_compact = serde_json::json!({
        "session": {
            "id": session.session_id,
            "title": title,
            "tool": session.agent.tool,
            "provider": session.agent.provider,
            "model": session.agent.model,
            "event_count": session.stats.event_count,
            "message_count": session.stats.message_count,
            "task_count": session.stats.task_count,
            "files_changed": session.stats.files_changed,
            "lines_added": session.stats.lines_added,
            "lines_removed": session.stats.lines_removed
        },
        "summary_source": source_kind,
        "timeline_signals": timeline_snippets,
        "file_changes": file_changes,
        "layer_rollup": layer_rollup,
        "auth_security_signals": auth_security_signals,
        "git_context": git_context
    });
    let compact_json = serde_json::to_string(&hail_compact).unwrap_or_default();
    if compact_json.trim().is_empty() {
        return String::new();
    }

    let style_rule = match config.response_style {
        SummaryResponseStyle::Compact => {
            "- Response style: compact. Keep each summary field concise (single short sentence when possible)."
        }
        SummaryResponseStyle::Standard => {
            "- Response style: standard. Keep each field short but informative (1-2 sentences)."
        }
        SummaryResponseStyle::Detailed => {
            "- Response style: detailed. Include concrete context and impact while staying factual."
        }
    };
    let shape_rule = match config.output_shape {
        SummaryOutputShape::Layered => {
            "- Output shape: layered. Group file changes by architecture layer in layer_file_changes."
        }
        SummaryOutputShape::FileList => {
            "- Output shape: file_list. Prefer file-centric entries (fine-grained grouping) in layer_file_changes."
        }
        SummaryOutputShape::SecurityFirst => {
            "- Output shape: security_first. Prioritize auth/security-related changes first when present."
        }
    };
    let source_rule = match config.source_mode {
        SummarySourceMode::SessionOnly => {
            "- Input source mode: session_only. Summarize only from session event signals."
        }
        SummarySourceMode::SessionOrGitChanges => {
            "- Input source mode: session_or_git_changes. If session signals are empty, use git change signals from HAIL_COMPACT."
        }
    };
    let custom_instruction = compact_summary_snippet(config.output_instruction.trim(), 500);
    let custom_rule = if custom_instruction.is_empty() {
        String::new()
    } else {
        format!("- User output instruction: {custom_instruction}\n")
    };

    let mut prompt = format!(
        "Convert a real coding session into semantic compression.\n\
Pipeline: session -> HAIL compact -> semantic summary.\n\
Return JSON only (no markdown, no prose outside JSON):\n\
{{\n\
  \"changes\": \"overall code change summary\",\n\
  \"auth_security\": \"auth/security change summary or 'none detected'\",\n\
  \"layer_file_changes\": [\n\
    {{\"layer\":\"presentation|application|domain|infrastructure|tests|docs|config\", \"summary\":\"layer change summary\", \"files\":[\"path\"]}}\n\
  ]\n\
}}\n\
Rules:\n\
- Use only facts from HAIL_COMPACT.\n\
- Mention what was modified.\n\
- If no auth/security-related change exists, set auth_security to \"none detected\".\n\
- In layer_file_changes, include changed files grouped by architectural layer.\n\
- Keep output compact and factual.\n\
- Use the same language as the session signals when obvious.\n\
{source_rule}\n\
{style_rule}\n\
{shape_rule}\n\
{custom_rule}\n\
HAIL_COMPACT={compact_json}"
    );

    if prompt.chars().count() > MAX_PROMPT_CHARS {
        prompt = prompt.chars().take(MAX_PROMPT_CHARS).collect();
    }
    prompt
}

fn summarize_layer_rollup(changes: &[HailCompactFileChange]) -> Vec<HailCompactLayerRollup> {
    let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for change in changes {
        grouped
            .entry(change.layer.clone())
            .or_default()
            .push(change.path.clone());
    }
    grouped
        .into_iter()
        .map(|(layer, mut files)| {
            files.sort();
            files.dedup();
            HailCompactLayerRollup {
                layer,
                file_count: files.len(),
                files,
            }
        })
        .collect()
}

fn collect_auth_security_signals(
    changes: &[HailCompactFileChange],
    timeline_snippets: &[String],
) -> Vec<String> {
    let mut signals = Vec::new();

    for change in changes {
        if contains_auth_security_keyword(&change.path) {
            signals.push(format!("file:{}", change.path));
        }
    }

    for snippet in timeline_snippets {
        if contains_auth_security_keyword(snippet) {
            signals.push(format!(
                "timeline:{}",
                compact_summary_snippet(snippet, 120)
            ));
        }
        if signals.len() >= 12 {
            break;
        }
    }

    signals.sort();
    signals.dedup();
    signals
}

#[cfg(test)]
mod tests {
    use super::{
        build_summary_prompt, classify_arch_layer, collect_file_changes, collect_timeline_snippets,
        contains_auth_security_keyword, count_diff_stats, SummaryPromptConfig,
    };
    use crate::types::HailCompactFileChange;
    use chrono::Utc;
    use opensession_core::trace::{Agent, Content, Event, EventType, Session};
    use opensession_runtime_config::{SummaryOutputShape, SummaryResponseStyle, SummarySourceMode};
    use serde_json::json;
    use std::collections::HashMap;

    fn make_session(session_id: &str) -> Session {
        Session::new(
            session_id.to_string(),
            Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        )
    }

    fn make_event(event_id: &str, event_type: EventType, text: &str) -> Event {
        Event {
            event_id: event_id.to_string(),
            timestamp: Utc::now(),
            event_type,
            task_id: None,
            content: Content::text(text),
            duration_ms: None,
            attributes: HashMap::new(),
        }
    }

    fn event_snippet(event: &Event, _max_chars: usize) -> Option<String> {
        if event.event_id.contains("skip") {
            None
        } else {
            Some(format!("snippet-{}", event.event_id))
        }
    }

    #[test]
    fn count_diff_stats_counts_added_and_removed_lines() {
        let diff = "\
diff --git a/src/a.rs b/src/a.rs\n\
--- a/src/a.rs\n\
+++ b/src/a.rs\n\
@@ -1,2 +1,3 @@\n\
 line\n\
-old\n\
+new\n\
+extra\n";

        let (added, removed) = count_diff_stats(diff);
        assert_eq!((added, removed), (2, 1));
    }

    #[test]
    fn classify_arch_layer_prefers_expected_buckets() {
        assert_eq!(
            classify_arch_layer("packages/ui/src/components/SessionDetailPage.svelte"),
            "presentation"
        );
        assert_eq!(
            classify_arch_layer("crates/runtime-config/src/lib.rs"),
            "config"
        );
        assert_eq!(
            classify_arch_layer("tests/session_summary_test.rs"),
            "tests"
        );
        assert_eq!(classify_arch_layer("docs/summary.md"), "docs");
    }

    #[test]
    fn contains_auth_security_keyword_detects_common_security_terms() {
        assert!(contains_auth_security_keyword(
            "updated oauth token validation"
        ));
        assert!(contains_auth_security_keyword(
            "set-cookie hardened for csrf"
        ));
        assert!(!contains_auth_security_keyword(
            "refactored timeline renderer"
        ));
    }

    #[test]
    fn collect_timeline_snippets_prefers_task_end_summary_and_preserves_order() {
        let mut session = make_session("timeline-summary");
        session
            .events
            .push(make_event("e-user", EventType::UserMessage, "hello"));
        session.events.push(make_event(
            "skip-custom",
            EventType::Custom {
                kind: "x".to_string(),
            },
            "ignored",
        ));
        session.events.push(make_event(
            "e-task-end",
            EventType::TaskEnd {
                summary: Some("  done   with   auth   ".to_string()),
            },
            "",
        ));
        session.events.push(make_event(
            "e-tool",
            EventType::ToolCall {
                name: "apply_patch".to_string(),
            },
            "",
        ));

        let snippets = collect_timeline_snippets(&session, 10, event_snippet);
        assert_eq!(snippets.len(), 3);
        assert_eq!(snippets[0], "user: snippet-e-user");
        assert_eq!(snippets[1], "task_end: done with auth");
        assert_eq!(snippets[2], "tool: snippet-e-tool");
    }

    #[test]
    fn collect_file_changes_merges_and_truncates_sorted_paths() {
        let mut session = make_session("file-change-merge");
        session.events.push(make_event(
            "e1",
            EventType::FileEdit {
                path: "b.rs".to_string(),
                diff: Some("+a\n-b\n+x\n".to_string()),
            },
            "",
        ));
        session.events.push(make_event(
            "e2",
            EventType::FileCreate {
                path: "a.rs".to_string(),
            },
            "",
        ));
        session.events.push(make_event(
            "e3",
            EventType::FileEdit {
                path: "b.rs".to_string(),
                diff: Some("+k\n".to_string()),
            },
            "",
        ));
        session.events.push(make_event(
            "e4",
            EventType::FileDelete {
                path: "c.rs".to_string(),
            },
            "",
        ));

        let changes = collect_file_changes(&session, 2);
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].path, "a.rs");
        assert_eq!(changes[0].operation, "create");
        assert_eq!(changes[1].path, "b.rs");
        assert_eq!(changes[1].operation, "edit");
        assert_eq!(changes[1].lines_added, 3);
        assert_eq!(changes[1].lines_removed, 1);
    }

    #[test]
    fn build_summary_prompt_returns_empty_without_signals() {
        let session = make_session("prompt-empty");
        let prompt = build_summary_prompt(
            &session,
            "session_events".to_string(),
            Vec::new(),
            Vec::new(),
            serde_json::Value::Null,
            SummaryPromptConfig {
                response_style: SummaryResponseStyle::Standard,
                output_shape: SummaryOutputShape::Layered,
                source_mode: SummarySourceMode::SessionOnly,
                output_instruction: "",
            },
        );

        assert!(prompt.is_empty());
    }

    #[test]
    fn build_summary_prompt_reflects_style_shape_source_and_security_signals() {
        let mut session = make_session("prompt-rules");
        session
            .events
            .push(make_event("e-user", EventType::UserMessage, "summarize"));
        session.recompute_stats();

        let prompt = build_summary_prompt(
            &session,
            "git_working_tree".to_string(),
            vec![
                "assistant: fixed oauth token validation".to_string(),
                "tool: refactor done".to_string(),
            ],
            vec![HailCompactFileChange {
                path: "auth/login.rs".to_string(),
                layer: "application".to_string(),
                operation: "edit".to_string(),
                lines_added: 8,
                lines_removed: 2,
            }],
            json!({"repo_root":"/tmp/repo","commit":null}),
            SummaryPromptConfig {
                response_style: SummaryResponseStyle::Detailed,
                output_shape: SummaryOutputShape::SecurityFirst,
                source_mode: SummarySourceMode::SessionOrGitChanges,
                output_instruction: "  focus on  risk and impact  ",
            },
        );

        assert!(prompt.contains("- Response style: detailed."));
        assert!(prompt.contains("- Output shape: security_first."));
        assert!(prompt.contains("- Input source mode: session_or_git_changes."));
        assert!(prompt.contains("User output instruction: focus on risk and impact"));
        assert!(prompt.contains("\"summary_source\":\"git_working_tree\""));
        assert!(prompt.contains("file:auth/login.rs"));
        assert!(prompt.contains("timeline:assistant: fixed oauth token validation"));
    }

    #[test]
    fn build_summary_prompt_truncates_to_max_chars() {
        let mut session = make_session("prompt-truncate");
        session
            .events
            .push(make_event("e-user", EventType::UserMessage, "hello"));
        session.recompute_stats();

        let oversized_timeline = format!("assistant: {}", "x".repeat(14_000));
        let prompt = build_summary_prompt(
            &session,
            "session_events".to_string(),
            vec![oversized_timeline],
            vec![HailCompactFileChange {
                path: "src/main.rs".to_string(),
                layer: "application".to_string(),
                operation: "edit".to_string(),
                lines_added: 1,
                lines_removed: 0,
            }],
            serde_json::Value::Null,
            SummaryPromptConfig {
                response_style: SummaryResponseStyle::Standard,
                output_shape: SummaryOutputShape::Layered,
                source_mode: SummarySourceMode::SessionOnly,
                output_instruction: "",
            },
        );

        assert_eq!(prompt.chars().count(), 10_000);
    }
}
