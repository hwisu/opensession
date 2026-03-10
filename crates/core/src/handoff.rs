//! Handoff context generation — extract structured summaries from sessions.
//!
//! This module provides programmatic extraction of session summaries for handoff
//! between agent sessions. It supports both single-session and multi-session merge.

#[path = "handoff/execution.rs"]
mod execution;
#[path = "handoff/hail_export.rs"]
mod hail_export;
#[path = "handoff/markdown.rs"]
mod markdown;
#[path = "handoff/merge.rs"]
mod merge;
#[cfg(test)]
#[path = "handoff/tests.rs"]
mod tests;
#[path = "handoff/validation.rs"]
mod validation;

use std::collections::{HashMap, HashSet};

use crate::extract::truncate_str;
use crate::{ContentBlock, Event, EventType, Session, Stats};

use execution::{
    build_execution_contract, build_work_packages, collect_evidence, collect_open_questions,
    collect_undefined_fields, dedupe_keep_order,
};

pub use hail_export::generate_handoff_hail;
pub use markdown::{
    generate_handoff_markdown, generate_handoff_markdown_v2, generate_merged_handoff_markdown,
    generate_merged_handoff_markdown_v2,
};
pub use merge::merge_summaries;
pub use validation::{
    HandoffValidationReport, ValidationFinding, validate_handoff_summaries,
    validate_handoff_summary,
};

/// A file change observed during a session.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FileChange {
    pub path: String,
    /// "created" | "edited" | "deleted"
    pub action: &'static str,
}

/// A shell command executed during a session.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ShellCmd {
    pub command: String,
    pub exit_code: Option<i32>,
}

/// A user→agent conversation pair.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Conversation {
    pub user: String,
    pub agent: String,
}

#[derive(Debug, Clone, serde::Serialize, Default)]
pub struct ExecutionContract {
    pub done_definition: Vec<String>,
    pub next_actions: Vec<String>,
    pub parallel_actions: Vec<String>,
    pub ordered_steps: Vec<OrderedStep>,
    pub ordered_commands: Vec<String>,
    pub rollback_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rollback_hint_missing_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rollback_hint_undefined_reason: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct OrderedStep {
    pub sequence: u32,
    pub work_package_id: String,
    pub title: String,
    pub status: String,
    pub depends_on: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, Default)]
pub struct Uncertainty {
    pub assumptions: Vec<String>,
    pub open_questions: Vec<String>,
    pub decision_required: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CheckRun {
    pub command: String,
    pub status: String,
    pub exit_code: Option<i32>,
    pub event_id: String,
}

#[derive(Debug, Clone, serde::Serialize, Default)]
pub struct Verification {
    pub checks_run: Vec<CheckRun>,
    pub checks_passed: Vec<String>,
    pub checks_failed: Vec<String>,
    pub required_checks_missing: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EvidenceRef {
    pub id: String,
    pub claim: String,
    pub event_id: String,
    pub timestamp: String,
    pub source_type: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct WorkPackage {
    pub id: String,
    pub title: String,
    pub status: String,
    pub sequence: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    pub depends_on: Vec<String>,
    pub files: Vec<String>,
    pub commands: Vec<String>,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct UndefinedField {
    pub path: String,
    pub undefined_reason: String,
}

/// Summary extracted from a single session.
#[derive(Debug, Clone, serde::Serialize)]
pub struct HandoffSummary {
    pub source_session_id: String,
    pub objective: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub objective_undefined_reason: Option<String>,
    pub tool: String,
    pub model: String,
    pub duration_seconds: u64,
    pub stats: Stats,
    pub files_modified: Vec<FileChange>,
    pub files_read: Vec<String>,
    pub shell_commands: Vec<ShellCmd>,
    pub errors: Vec<String>,
    pub task_summaries: Vec<String>,
    pub key_conversations: Vec<Conversation>,
    pub user_messages: Vec<String>,
    pub execution_contract: ExecutionContract,
    pub uncertainty: Uncertainty,
    pub verification: Verification,
    pub evidence: Vec<EvidenceRef>,
    pub work_packages: Vec<WorkPackage>,
    pub undefined_fields: Vec<UndefinedField>,
}

/// Merged handoff from multiple sessions.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MergedHandoff {
    pub source_session_ids: Vec<String>,
    pub summaries: Vec<HandoffSummary>,
    /// Deduplicated union of all modified files
    pub all_files_modified: Vec<FileChange>,
    /// Deduplicated union of all files read (minus modified)
    pub all_files_read: Vec<String>,
    pub total_duration_seconds: u64,
    pub total_errors: Vec<String>,
}

const MAX_KEY_CONVERSATIONS: usize = 12;
const MAX_USER_MESSAGES: usize = 18;
const HEAD_KEEP_MESSAGES: usize = 3;
const HEAD_KEEP_CONVERSATIONS: usize = 2;

impl HandoffSummary {
    /// Extract a structured summary from a parsed session.
    pub fn from_session(session: &Session) -> Self {
        let objective = extract_objective(session);
        let objective_undefined_reason = objective_unavailable_reason(&objective);

        let files_modified = collect_file_changes(&session.events);
        let modified_paths: HashSet<&str> =
            files_modified.iter().map(|f| f.path.as_str()).collect();
        let files_read = collect_files_read(&session.events, &modified_paths);
        let shell_commands = collect_shell_commands(&session.events);
        let errors = collect_errors(&session.events);
        let task_summaries = collect_task_summaries(&session.events);
        let user_messages = collect_user_messages(&session.events);
        let key_conversations = collect_conversation_pairs(&session.events);
        let verification = collect_verification(&session.events);
        let uncertainty = collect_uncertainty(session, &verification);
        let evidence = collect_evidence(session, &objective, &task_summaries, &uncertainty);
        let work_packages = build_work_packages(&session.events, &evidence);
        let execution_contract = build_execution_contract(
            &task_summaries,
            &verification,
            &uncertainty,
            &shell_commands,
            &files_modified,
            &work_packages,
        );
        let undefined_fields = collect_undefined_fields(
            objective_undefined_reason.as_deref(),
            &execution_contract,
            &evidence,
        );

        HandoffSummary {
            source_session_id: session.session_id.clone(),
            objective,
            objective_undefined_reason,
            tool: session.agent.tool.clone(),
            model: session.agent.model.clone(),
            duration_seconds: session.stats.duration_seconds,
            stats: session.stats.clone(),
            files_modified,
            files_read,
            shell_commands,
            errors,
            task_summaries,
            key_conversations,
            user_messages,
            execution_contract,
            uncertainty,
            verification,
            evidence,
            work_packages,
            undefined_fields,
        }
    }
}

/// Collect file changes, preserving create/delete precedence over edits.
fn collect_file_changes(events: &[Event]) -> Vec<FileChange> {
    let map = events.iter().fold(HashMap::new(), |mut map, event| {
        match &event.event_type {
            EventType::FileCreate { path } => {
                map.insert(path.clone(), "created");
            }
            EventType::FileEdit { path, .. } => {
                map.entry(path.clone()).or_insert("edited");
            }
            EventType::FileDelete { path } => {
                map.insert(path.clone(), "deleted");
            }
            _ => {}
        }
        map
    });

    let mut changes: Vec<FileChange> = map
        .into_iter()
        .map(|(path, action)| FileChange { path, action })
        .collect();
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    changes
}

fn collect_files_read(events: &[Event], modified_paths: &HashSet<&str>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut files = Vec::new();
    for event in events {
        if let EventType::FileRead { path } = &event.event_type
            && !modified_paths.contains(path.as_str())
            && seen.insert(path.clone())
        {
            files.push(path.clone());
        }
    }
    files
}

fn collect_shell_commands(events: &[Event]) -> Vec<ShellCmd> {
    events
        .iter()
        .filter_map(|event| match &event.event_type {
            EventType::ShellCommand { command, exit_code } => Some(ShellCmd {
                command: command.clone(),
                exit_code: *exit_code,
            }),
            _ => None,
        })
        .collect()
}

fn collect_errors(events: &[Event]) -> Vec<String> {
    let mut errors = Vec::new();
    for event in events {
        match &event.event_type {
            EventType::ShellCommand {
                command,
                exit_code: Some(code),
            } if *code != 0 => {
                errors.push(format!(
                    "Command failed ({code}): {}",
                    collapse_whitespace(command)
                ));
            }
            EventType::ToolResult { name, is_error, .. } if *is_error => {
                if let Some(text) = extract_text_from_event(event) {
                    errors.push(format!("Tool {name} error: {}", truncate_str(&text, 200)));
                } else {
                    errors.push(format!("Tool {name} reported an error."));
                }
            }
            EventType::Custom { kind } if kind == "turn_aborted" => {
                errors.push("Turn aborted.".to_string());
            }
            _ => {}
        }
    }
    errors
}

fn collect_verification(events: &[Event]) -> Verification {
    let checks_run = events
        .iter()
        .filter_map(|event| match &event.event_type {
            EventType::ShellCommand { command, exit_code } => Some(CheckRun {
                command: collapse_whitespace(command),
                status: match exit_code {
                    Some(0) => "passed".to_string(),
                    Some(_) => "failed".to_string(),
                    None => "unknown".to_string(),
                },
                exit_code: *exit_code,
                event_id: event.event_id.clone(),
            }),
            _ => None,
        })
        .collect::<Vec<_>>();

    let mut checks_passed = checks_run
        .iter()
        .filter(|check| check.status == "passed")
        .map(|check| check.command.clone())
        .collect::<Vec<_>>();
    let mut checks_failed = checks_run
        .iter()
        .filter(|check| check.status == "failed")
        .map(|check| check.command.clone())
        .collect::<Vec<_>>();

    dedupe_keep_order(&mut checks_passed);
    dedupe_keep_order(&mut checks_failed);

    let mut required_checks_missing = Vec::new();
    let has_files_modified = events.iter().any(|event| {
        matches!(
            &event.event_type,
            EventType::FileEdit { .. }
                | EventType::FileCreate { .. }
                | EventType::FileDelete { .. }
        )
    });

    if has_files_modified && checks_run.is_empty() {
        required_checks_missing
            .push("No verification command was run after file modifications.".to_string());
    }

    Verification {
        checks_run,
        checks_passed,
        checks_failed,
        required_checks_missing,
    }
}

fn collect_uncertainty(session: &Session, verification: &Verification) -> Uncertainty {
    let mut assumptions = Vec::new();
    if verification.checks_run.is_empty() {
        assumptions.push("Verification status is unknown because no checks were recorded.".into());
    }
    if session.context.title.as_deref().is_none_or(str::is_empty) {
        assumptions.push("Session title was unavailable.".into());
    }

    let mut open_questions = collect_open_questions(&session.events);
    let mut decision_required = Vec::new();
    if session.events.iter().any(
        |event| matches!(&event.event_type, EventType::Custom { kind } if kind == "turn_aborted"),
    ) {
        decision_required.push("Turn aborted before completion; decide whether to retry.".into());
    }
    if !verification.checks_failed.is_empty() {
        decision_required
            .push("Fix failing verification commands before handoff is complete.".into());
    }

    dedupe_keep_order(&mut assumptions);
    dedupe_keep_order(&mut open_questions);
    dedupe_keep_order(&mut decision_required);

    Uncertainty {
        assumptions,
        open_questions,
        decision_required,
    }
}

fn collect_task_summaries(events: &[Event]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut summaries = Vec::new();

    for event in events {
        let EventType::TaskEnd {
            summary: Some(summary),
        } = &event.event_type
        else {
            continue;
        };

        let summary = summary.trim();
        if summary.is_empty() {
            continue;
        }

        let normalized = collapse_whitespace(summary);
        if normalized.eq_ignore_ascii_case("synthetic end (missing task_complete)") {
            continue;
        }
        if seen.insert(normalized.clone()) {
            summaries.push(truncate_str(&normalized, 180));
        }
    }

    summaries
}

fn collect_user_messages(events: &[Event]) -> Vec<String> {
    let messages = events
        .iter()
        .filter(|e| matches!(&e.event_type, EventType::UserMessage))
        .filter_map(extract_text_from_event)
        .map(|msg| truncate_str(&collapse_whitespace(&msg), 240))
        .collect::<Vec<_>>();
    condense_head_tail(messages, HEAD_KEEP_MESSAGES, MAX_USER_MESSAGES)
}

/// Pair adjacent User→Agent messages into conversations.
///
/// Filters to message events only, then uses `windows(2)` to find
/// UserMessage→AgentMessage pairs.
fn collect_conversation_pairs(events: &[Event]) -> Vec<Conversation> {
    let messages: Vec<&Event> = events
        .iter()
        .filter(|e| {
            matches!(
                &e.event_type,
                EventType::UserMessage | EventType::AgentMessage
            )
        })
        .collect();

    let conversations = messages
        .windows(2)
        .filter_map(|pair| match (&pair[0].event_type, &pair[1].event_type) {
            (EventType::UserMessage, EventType::AgentMessage) => {
                let user_text = extract_text_from_event(pair[0])?;
                let agent_text = extract_text_from_event(pair[1])?;
                Some(Conversation {
                    user: truncate_str(&user_text, 300),
                    agent: truncate_str(&agent_text, 300),
                })
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    condense_head_tail(
        conversations,
        HEAD_KEEP_CONVERSATIONS,
        MAX_KEY_CONVERSATIONS,
    )
}

fn condense_head_tail<T: Clone>(items: Vec<T>, head_keep: usize, max_total: usize) -> Vec<T> {
    if items.len() <= max_total {
        return items;
    }

    let max_total = max_total.max(head_keep);
    let tail_keep = max_total.saturating_sub(head_keep);
    let mut condensed = Vec::with_capacity(max_total);

    condensed.extend(items.iter().take(head_keep).cloned());
    condensed.extend(
        items
            .iter()
            .skip(items.len().saturating_sub(tail_keep))
            .cloned(),
    );
    condensed
}

fn extract_first_user_text(session: &Session) -> Option<String> {
    crate::extract::extract_first_user_text(session)
}

fn extract_objective(session: &Session) -> String {
    if let Some(user_text) = extract_first_user_text(session).filter(|t| !t.trim().is_empty()) {
        return truncate_str(&collapse_whitespace(&user_text), 200);
    }

    if let Some(task_title) = session
        .events
        .iter()
        .find_map(|event| match &event.event_type {
            EventType::TaskStart { title: Some(title) } => {
                let title = title.trim();
                if title.is_empty() {
                    None
                } else {
                    Some(title.to_string())
                }
            }
            _ => None,
        })
    {
        return truncate_str(&collapse_whitespace(&task_title), 200);
    }

    if let Some(task_summary) = session
        .events
        .iter()
        .find_map(|event| match &event.event_type {
            EventType::TaskEnd {
                summary: Some(summary),
            } => {
                let summary = summary.trim();
                if summary.is_empty() {
                    None
                } else {
                    Some(summary.to_string())
                }
            }
            _ => None,
        })
    {
        return truncate_str(&collapse_whitespace(&task_summary), 200);
    }

    if let Some(title) = session.context.title.as_deref().map(str::trim)
        && !title.is_empty()
    {
        return truncate_str(&collapse_whitespace(title), 200);
    }

    "(objective unavailable)".to_string()
}

fn extract_text_from_event(event: &Event) -> Option<String> {
    for block in &event.content.blocks {
        if let ContentBlock::Text { text } = block {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn collapse_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn objective_unavailable_reason(objective: &str) -> Option<String> {
    if objective.trim().is_empty() || objective == "(objective unavailable)" {
        Some(
            "No user prompt, task title/summary, or session title could be used to infer objective."
                .to_string(),
        )
    } else {
        None
    }
}

/// Format seconds into a human-readable duration string.
pub fn format_duration(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        let m = seconds / 60;
        let s = seconds % 60;
        format!("{m}m {s}s")
    } else {
        let h = seconds / 3600;
        let m = (seconds % 3600) / 60;
        let s = seconds % 60;
        format!("{h}h {m}m {s}s")
    }
}
