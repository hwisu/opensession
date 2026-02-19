//! Handoff context generation — extract structured summaries from sessions.
//!
//! This module provides programmatic extraction of session summaries for handoff
//! between agent sessions. It supports both single-session and multi-session merge.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::extract::truncate_str;
use crate::{Content, ContentBlock, Event, EventType, Session, SessionContext, Stats};

// ─── Types ───────────────────────────────────────────────────────────────────

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
    pub ordered_commands: Vec<String>,
    pub rollback_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rollback_hint_missing_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rollback_hint_undefined_reason: Option<String>,
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

// ─── Extraction ──────────────────────────────────────────────────────────────

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
        let execution_contract = build_execution_contract(
            &task_summaries,
            &verification,
            &uncertainty,
            &shell_commands,
        );
        let evidence = collect_evidence(session, &objective, &task_summaries, &uncertainty);
        let work_packages = build_work_packages(&session.events, &evidence);
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

// ─── Functional extractors ──────────────────────────────────────────────────

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
    let mut result: Vec<FileChange> = map
        .into_iter()
        .map(|(path, action)| FileChange { path, action })
        .collect();
    result.sort_by(|a, b| a.path.cmp(&b.path));
    result
}

/// Collect read-only file paths (excluding those that were also modified).
fn collect_files_read(events: &[Event], modified_paths: &HashSet<&str>) -> Vec<String> {
    let mut read: Vec<String> = events
        .iter()
        .filter_map(|e| match &e.event_type {
            EventType::FileRead { path } if !modified_paths.contains(path.as_str()) => {
                Some(path.clone())
            }
            _ => None,
        })
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    read.sort();
    read
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

/// Collect errors from failed shell commands and tool results.
fn collect_errors(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match &event.event_type {
            EventType::ShellCommand { command, exit_code }
                if *exit_code != Some(0) && exit_code.is_some() =>
            {
                Some(format!(
                    "Shell: `{}` → exit {}",
                    truncate_str(command, 80),
                    exit_code.unwrap()
                ))
            }
            EventType::ToolResult {
                is_error: true,
                name,
                ..
            } => {
                let detail = extract_text_from_event(event);
                Some(match detail {
                    Some(d) => format!("Tool error: {} — {}", name, truncate_str(&d, 80)),
                    None => format!("Tool error: {name}"),
                })
            }
            _ => None,
        })
        .collect()
}

fn collect_verification(events: &[Event]) -> Verification {
    let mut tool_result_by_call: HashMap<String, (&Event, bool)> = HashMap::new();
    for event in events {
        if let EventType::ToolResult { is_error, .. } = &event.event_type {
            if let Some(call_id) = event.semantic_call_id() {
                tool_result_by_call
                    .entry(call_id.to_string())
                    .or_insert((event, *is_error));
            }
        }
    }

    let mut checks_run = Vec::new();
    for event in events {
        let EventType::ShellCommand { command, exit_code } = &event.event_type else {
            continue;
        };

        let (status, resolved_exit_code) = match exit_code {
            Some(0) => ("passed".to_string(), Some(0)),
            Some(code) => ("failed".to_string(), Some(*code)),
            None => {
                if let Some(call_id) = event.semantic_call_id() {
                    if let Some((_, is_error)) = tool_result_by_call.get(call_id) {
                        if *is_error {
                            ("failed".to_string(), None)
                        } else {
                            ("passed".to_string(), None)
                        }
                    } else {
                        ("unknown".to_string(), None)
                    }
                } else {
                    ("unknown".to_string(), None)
                }
            }
        };

        checks_run.push(CheckRun {
            command: collapse_whitespace(command),
            status,
            exit_code: resolved_exit_code,
            event_id: event.event_id.clone(),
        });
    }

    let mut checks_passed: Vec<String> = checks_run
        .iter()
        .filter(|run| run.status == "passed")
        .map(|run| run.command.clone())
        .collect();
    let mut checks_failed: Vec<String> = checks_run
        .iter()
        .filter(|run| run.status == "failed")
        .map(|run| run.command.clone())
        .collect();

    dedupe_keep_order(&mut checks_passed);
    dedupe_keep_order(&mut checks_failed);

    let unresolved_failed = unresolved_failed_commands(&checks_run);
    let mut required_checks_missing = unresolved_failed
        .iter()
        .map(|cmd| format!("Unresolved failed check: `{cmd}`"))
        .collect::<Vec<_>>();

    let has_modified_files = events.iter().any(|event| {
        matches!(
            event.event_type,
            EventType::FileEdit { .. }
                | EventType::FileCreate { .. }
                | EventType::FileDelete { .. }
        )
    });
    if has_modified_files && checks_run.is_empty() {
        required_checks_missing
            .push("No verification command found after file modifications.".to_string());
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
    if extract_objective(session) == "(objective unavailable)" {
        assumptions.push(
            "Objective inferred as unavailable; downstream agent must restate objective."
                .to_string(),
        );
    }

    let open_questions = collect_open_questions(&session.events);

    let mut decision_required = Vec::new();
    for event in &session.events {
        if let EventType::Custom { kind } = &event.event_type {
            if kind == "turn_aborted" {
                let reason = event
                    .attr_str("reason")
                    .map(String::from)
                    .unwrap_or_else(|| "turn aborted".to_string());
                decision_required.push(format!("Turn aborted: {reason}"));
            }
        }
    }
    for missing in &verification.required_checks_missing {
        decision_required.push(missing.clone());
    }
    for question in &open_questions {
        decision_required.push(format!("Resolve open question: {question}"));
    }

    dedupe_keep_order(&mut assumptions);
    let mut open_questions = open_questions;
    dedupe_keep_order(&mut open_questions);
    dedupe_keep_order(&mut decision_required);

    Uncertainty {
        assumptions,
        open_questions,
        decision_required,
    }
}

fn build_execution_contract(
    task_summaries: &[String],
    verification: &Verification,
    uncertainty: &Uncertainty,
    shell_commands: &[ShellCmd],
) -> ExecutionContract {
    let done_definition = task_summaries.to_vec();

    let mut next_actions = unresolved_failed_commands(&verification.checks_run)
        .into_iter()
        .map(|cmd| format!("Fix and re-run `{cmd}` until the check passes."))
        .collect::<Vec<_>>();
    next_actions.extend(
        uncertainty
            .open_questions
            .iter()
            .map(|q| format!("Resolve open question: {q}")),
    );

    if done_definition.is_empty() && next_actions.is_empty() {
        next_actions.push(
            "Define completion criteria and run at least one verification command.".to_string(),
        );
    }
    dedupe_keep_order(&mut next_actions);

    let unresolved = unresolved_failed_commands(&verification.checks_run);
    let mut ordered_commands = unresolved;
    for cmd in shell_commands
        .iter()
        .map(|c| collapse_whitespace(&c.command))
    {
        if !ordered_commands.iter().any(|existing| existing == &cmd) {
            ordered_commands.push(cmd);
        }
    }

    let has_git_commit = shell_commands
        .iter()
        .any(|cmd| cmd.command.to_ascii_lowercase().contains("git commit"));
    let (rollback_hint, rollback_hint_missing_reason) = if has_git_commit {
        (
            Some(
                "Use `git revert <commit>` for committed changes, then re-run verification."
                    .to_string(),
            ),
            None,
        )
    } else {
        (
            None,
            Some("No committed change signal found in events.".to_string()),
        )
    };

    ExecutionContract {
        done_definition,
        next_actions,
        ordered_commands,
        rollback_hint,
        rollback_hint_missing_reason: rollback_hint_missing_reason.clone(),
        rollback_hint_undefined_reason: rollback_hint_missing_reason,
    }
}

fn collect_evidence(
    session: &Session,
    objective: &str,
    task_summaries: &[String],
    uncertainty: &Uncertainty,
) -> Vec<EvidenceRef> {
    let mut evidence = Vec::new();
    let mut next_id = 1usize;

    if let Some(event) = find_objective_event(session) {
        evidence.push(EvidenceRef {
            id: format!("evidence-{next_id}"),
            claim: format!("objective: {objective}"),
            event_id: event.event_id.clone(),
            timestamp: event.timestamp.to_rfc3339(),
            source_type: event_source_type(event),
        });
        next_id += 1;
    }

    for summary in task_summaries {
        if let Some(event) = find_task_summary_event(&session.events, summary) {
            evidence.push(EvidenceRef {
                id: format!("evidence-{next_id}"),
                claim: format!("task_done: {summary}"),
                event_id: event.event_id.clone(),
                timestamp: event.timestamp.to_rfc3339(),
                source_type: event_source_type(event),
            });
            next_id += 1;
        }
    }

    for decision in &uncertainty.decision_required {
        if let Some(event) = find_decision_event(&session.events, decision) {
            evidence.push(EvidenceRef {
                id: format!("evidence-{next_id}"),
                claim: format!("decision_required: {decision}"),
                event_id: event.event_id.clone(),
                timestamp: event.timestamp.to_rfc3339(),
                source_type: event_source_type(event),
            });
            next_id += 1;
        }
    }

    evidence
}

fn build_work_packages(events: &[Event], evidence: &[EvidenceRef]) -> Vec<WorkPackage> {
    #[derive(Default)]
    struct WorkPackageAcc {
        title: Option<String>,
        status: String,
        first_ts: Option<chrono::DateTime<chrono::Utc>>,
        files: HashSet<String>,
        commands: Vec<String>,
        evidence_refs: Vec<String>,
    }

    let mut evidence_by_event: HashMap<&str, Vec<String>> = HashMap::new();
    for ev in evidence {
        evidence_by_event
            .entry(ev.event_id.as_str())
            .or_default()
            .push(ev.id.clone());
    }

    let mut grouped: BTreeMap<String, WorkPackageAcc> = BTreeMap::new();
    for event in events {
        let key = package_key_for_event(event);
        let acc = grouped
            .entry(key.clone())
            .or_insert_with(|| WorkPackageAcc {
                status: "pending".to_string(),
                ..Default::default()
            });

        if acc.first_ts.is_none() {
            acc.first_ts = Some(event.timestamp);
        }
        if let Some(ids) = evidence_by_event.get(event.event_id.as_str()) {
            acc.evidence_refs.extend(ids.clone());
        }

        match &event.event_type {
            EventType::TaskStart { title } => {
                if let Some(title) = title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
                    acc.title = Some(title.to_string());
                }
                if acc.status != "completed" {
                    acc.status = "in_progress".to_string();
                }
            }
            EventType::TaskEnd { .. } => {
                acc.status = "completed".to_string();
            }
            EventType::FileEdit { path, .. }
            | EventType::FileCreate { path }
            | EventType::FileDelete { path } => {
                acc.files.insert(path.clone());
            }
            EventType::ShellCommand { command, .. } => {
                acc.commands.push(collapse_whitespace(command));
            }
            _ => {}
        }
    }

    let mut packages = grouped
        .into_iter()
        .map(|(id, mut acc)| {
            dedupe_keep_order(&mut acc.commands);
            dedupe_keep_order(&mut acc.evidence_refs);
            let mut files: Vec<String> = acc.files.into_iter().collect();
            files.sort();
            WorkPackage {
                title: acc.title.unwrap_or_else(|| {
                    if id == "main" {
                        "Main flow".to_string()
                    } else {
                        format!("Task {id}")
                    }
                }),
                id,
                status: acc.status,
                depends_on: Vec::new(),
                files,
                commands: acc.commands,
                evidence_refs: acc.evidence_refs,
            }
        })
        .collect::<Vec<_>>();

    packages.sort_by(|a, b| a.id.cmp(&b.id));

    for i in 0..packages.len() {
        let cur_files: HashSet<&str> = packages[i].files.iter().map(String::as_str).collect();
        if cur_files.is_empty() {
            continue;
        }
        let mut dependency: Option<String> = None;
        for j in (0..i).rev() {
            let prev_files: HashSet<&str> = packages[j].files.iter().map(String::as_str).collect();
            if !prev_files.is_empty() && !cur_files.is_disjoint(&prev_files) {
                dependency = Some(packages[j].id.clone());
                break;
            }
        }
        if let Some(dep) = dependency {
            packages[i].depends_on.push(dep);
        }
    }

    packages
}

fn package_key_for_event(event: &Event) -> String {
    if let Some(task_id) = event
        .task_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        return task_id.to_string();
    }
    if let Some(group_id) = event.semantic_group_id() {
        return group_id.to_string();
    }
    "main".to_string()
}

fn find_objective_event(session: &Session) -> Option<&Event> {
    session
        .events
        .iter()
        .find(|event| matches!(event.event_type, EventType::UserMessage))
        .or_else(|| {
            session.events.iter().find(|event| {
                matches!(
                    event.event_type,
                    EventType::TaskStart { .. } | EventType::TaskEnd { .. }
                )
            })
        })
}

fn find_task_summary_event<'a>(events: &'a [Event], summary: &str) -> Option<&'a Event> {
    let normalized_target = collapse_whitespace(summary);
    events.iter().find(|event| {
        let EventType::TaskEnd {
            summary: Some(candidate),
        } = &event.event_type
        else {
            return false;
        };
        collapse_whitespace(candidate) == normalized_target
    })
}

fn find_decision_event<'a>(events: &'a [Event], decision: &str) -> Option<&'a Event> {
    if decision.to_ascii_lowercase().contains("turn aborted") {
        return events.iter().find(|event| {
            matches!(
                &event.event_type,
                EventType::Custom { kind } if kind == "turn_aborted"
            )
        });
    }
    if decision.to_ascii_lowercase().contains("open question") {
        return events
            .iter()
            .find(|event| event.attr_str("source") == Some("interactive_question"));
    }
    None
}

fn collect_open_questions(events: &[Event]) -> Vec<String> {
    let mut question_meta: BTreeMap<String, String> = BTreeMap::new();
    let mut asked_order = Vec::new();
    let mut answered_ids = HashSet::new();

    for event in events {
        if event.attr_str("source") == Some("interactive_question") {
            if let Some(items) = event
                .attributes
                .get("question_meta")
                .and_then(|v| v.as_array())
            {
                for item in items {
                    let Some(id) = item
                        .get("id")
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|v| !v.is_empty())
                    else {
                        continue;
                    };
                    let text = item
                        .get("question")
                        .or_else(|| item.get("header"))
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|v| !v.is_empty())
                        .unwrap_or(id);
                    if !question_meta.contains_key(id) {
                        asked_order.push(id.to_string());
                    }
                    question_meta.insert(id.to_string(), text.to_string());
                }
            } else if let Some(ids) = event
                .attributes
                .get("question_ids")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str())
                        .map(str::trim)
                        .filter(|v| !v.is_empty())
                        .map(String::from)
                        .collect::<Vec<_>>()
                })
            {
                for id in ids {
                    if !question_meta.contains_key(&id) {
                        asked_order.push(id.clone());
                    }
                    question_meta.entry(id.clone()).or_insert(id);
                }
            }
        }

        if event.attr_str("source") == Some("interactive") {
            if let Some(ids) = event
                .attributes
                .get("question_ids")
                .and_then(|v| v.as_array())
            {
                for id in ids
                    .iter()
                    .filter_map(|v| v.as_str())
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                {
                    answered_ids.insert(id.to_string());
                }
            }
        }
    }

    asked_order
        .into_iter()
        .filter(|id| !answered_ids.contains(id))
        .map(|id| {
            let text = question_meta
                .get(&id)
                .cloned()
                .unwrap_or_else(|| id.clone());
            format!("{id}: {text}")
        })
        .collect()
}

fn unresolved_failed_commands(checks_run: &[CheckRun]) -> Vec<String> {
    let mut unresolved = Vec::new();
    for (idx, run) in checks_run.iter().enumerate() {
        if run.status != "failed" {
            continue;
        }
        let resolved = checks_run
            .iter()
            .skip(idx + 1)
            .any(|later| later.command == run.command && later.status == "passed");
        if !resolved {
            unresolved.push(run.command.clone());
        }
    }
    dedupe_keep_order(&mut unresolved);
    unresolved
}

fn dedupe_keep_order(values: &mut Vec<String>) {
    let mut seen = HashSet::new();
    values.retain(|value| seen.insert(value.clone()));
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

fn collect_undefined_fields(
    objective_undefined_reason: Option<&str>,
    execution_contract: &ExecutionContract,
    evidence: &[EvidenceRef],
) -> Vec<UndefinedField> {
    let mut undefined = Vec::new();

    if let Some(reason) = objective_undefined_reason {
        undefined.push(UndefinedField {
            path: "objective".to_string(),
            undefined_reason: reason.to_string(),
        });
    }

    if let Some(reason) = execution_contract
        .rollback_hint_undefined_reason
        .as_deref()
        .or(execution_contract.rollback_hint_missing_reason.as_deref())
    {
        undefined.push(UndefinedField {
            path: "execution_contract.rollback_hint".to_string(),
            undefined_reason: reason.to_string(),
        });
    }

    if evidence.is_empty() {
        undefined.push(UndefinedField {
            path: "evidence".to_string(),
            undefined_reason:
                "No objective/task/decision evidence could be mapped to source events.".to_string(),
        });
    }

    undefined
}

fn event_source_type(event: &Event) -> String {
    event
        .source_raw_type()
        .map(String::from)
        .unwrap_or_else(|| match &event.event_type {
            EventType::UserMessage => "UserMessage".to_string(),
            EventType::AgentMessage => "AgentMessage".to_string(),
            EventType::SystemMessage => "SystemMessage".to_string(),
            EventType::Thinking => "Thinking".to_string(),
            EventType::ToolCall { .. } => "ToolCall".to_string(),
            EventType::ToolResult { .. } => "ToolResult".to_string(),
            EventType::FileRead { .. } => "FileRead".to_string(),
            EventType::CodeSearch { .. } => "CodeSearch".to_string(),
            EventType::FileSearch { .. } => "FileSearch".to_string(),
            EventType::FileEdit { .. } => "FileEdit".to_string(),
            EventType::FileCreate { .. } => "FileCreate".to_string(),
            EventType::FileDelete { .. } => "FileDelete".to_string(),
            EventType::ShellCommand { .. } => "ShellCommand".to_string(),
            EventType::ImageGenerate { .. } => "ImageGenerate".to_string(),
            EventType::VideoGenerate { .. } => "VideoGenerate".to_string(),
            EventType::AudioGenerate { .. } => "AudioGenerate".to_string(),
            EventType::WebSearch { .. } => "WebSearch".to_string(),
            EventType::WebFetch { .. } => "WebFetch".to_string(),
            EventType::TaskStart { .. } => "TaskStart".to_string(),
            EventType::TaskEnd { .. } => "TaskEnd".to_string(),
            EventType::Custom { kind } => format!("Custom:{kind}"),
        })
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
    events
        .iter()
        .filter(|e| matches!(&e.event_type, EventType::UserMessage))
        .filter_map(extract_text_from_event)
        .collect()
}

/// Pair adjacent User→Agent messages into conversations.
///
/// Filters to message events only, then uses `windows(2)` to find
/// UserMessage→AgentMessage pairs — no mutable tracking state needed.
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

    messages
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
        .collect()
}

// ─── Merge ───────────────────────────────────────────────────────────────────

/// Merge multiple session summaries into a single handoff context.
pub fn merge_summaries(summaries: &[HandoffSummary]) -> MergedHandoff {
    let session_ids: Vec<String> = summaries
        .iter()
        .map(|s| s.source_session_id.clone())
        .collect();
    let total_duration: u64 = summaries.iter().map(|s| s.duration_seconds).sum();
    let total_errors: Vec<String> = summaries
        .iter()
        .flat_map(|s| {
            s.errors
                .iter()
                .map(move |err| format!("[{}] {}", s.source_session_id, err))
        })
        .collect();

    let all_modified: HashMap<String, &str> = summaries
        .iter()
        .flat_map(|s| &s.files_modified)
        .fold(HashMap::new(), |mut map, fc| {
            map.entry(fc.path.clone()).or_insert(fc.action);
            map
        });

    // Compute sorted_read before consuming all_modified
    let mut sorted_read: Vec<String> = summaries
        .iter()
        .flat_map(|s| &s.files_read)
        .filter(|p| !all_modified.contains_key(p.as_str()))
        .cloned()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    sorted_read.sort();

    let mut sorted_modified: Vec<FileChange> = all_modified
        .into_iter()
        .map(|(path, action)| FileChange { path, action })
        .collect();
    sorted_modified.sort_by(|a, b| a.path.cmp(&b.path));

    MergedHandoff {
        source_session_ids: session_ids,
        summaries: summaries.to_vec(),
        all_files_modified: sorted_modified,
        all_files_read: sorted_read,
        total_duration_seconds: total_duration,
        total_errors,
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ValidationFinding {
    pub code: String,
    pub severity: String,
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct HandoffValidationReport {
    pub session_id: String,
    pub passed: bool,
    pub findings: Vec<ValidationFinding>,
}

pub fn validate_handoff_summary(summary: &HandoffSummary) -> HandoffValidationReport {
    let mut findings = Vec::new();

    if summary.objective.trim().is_empty() || summary.objective == "(objective unavailable)" {
        findings.push(ValidationFinding {
            code: "objective_missing".to_string(),
            severity: "warning".to_string(),
            message: "Objective is unavailable.".to_string(),
        });
    }

    let unresolved_failures = unresolved_failed_commands(&summary.verification.checks_run);
    if !unresolved_failures.is_empty() && summary.execution_contract.next_actions.is_empty() {
        findings.push(ValidationFinding {
            code: "next_actions_missing".to_string(),
            severity: "warning".to_string(),
            message: "Unresolved failed checks exist but no next action was generated.".to_string(),
        });
    }

    if !summary.files_modified.is_empty() && summary.verification.checks_run.is_empty() {
        findings.push(ValidationFinding {
            code: "verification_missing".to_string(),
            severity: "warning".to_string(),
            message: "Files were modified but no verification check was recorded.".to_string(),
        });
    }

    if summary.evidence.is_empty() {
        findings.push(ValidationFinding {
            code: "evidence_missing".to_string(),
            severity: "warning".to_string(),
            message: "No evidence references were generated.".to_string(),
        });
    } else if !summary
        .evidence
        .iter()
        .any(|ev| ev.claim.starts_with("objective:"))
    {
        findings.push(ValidationFinding {
            code: "objective_evidence_missing".to_string(),
            severity: "warning".to_string(),
            message: "Objective exists but objective evidence is missing.".to_string(),
        });
    }

    if has_work_package_cycle(&summary.work_packages) {
        findings.push(ValidationFinding {
            code: "work_package_cycle".to_string(),
            severity: "error".to_string(),
            message: "work_packages.depends_on contains a cycle.".to_string(),
        });
    }

    HandoffValidationReport {
        session_id: summary.source_session_id.clone(),
        passed: findings.is_empty(),
        findings,
    }
}

pub fn validate_handoff_summaries(summaries: &[HandoffSummary]) -> Vec<HandoffValidationReport> {
    summaries.iter().map(validate_handoff_summary).collect()
}

fn has_work_package_cycle(packages: &[WorkPackage]) -> bool {
    let mut state: HashMap<&str, u8> = HashMap::new();
    let deps: HashMap<&str, Vec<&str>> = packages
        .iter()
        .map(|wp| {
            (
                wp.id.as_str(),
                wp.depends_on.iter().map(String::as_str).collect::<Vec<_>>(),
            )
        })
        .collect();

    fn dfs<'a>(
        node: &'a str,
        state: &mut HashMap<&'a str, u8>,
        deps: &HashMap<&'a str, Vec<&'a str>>,
    ) -> bool {
        match state.get(node).copied() {
            Some(1) => return true,
            Some(2) => return false,
            _ => {}
        }
        state.insert(node, 1);
        if let Some(children) = deps.get(node) {
            for child in children {
                if !deps.contains_key(child) {
                    continue;
                }
                if dfs(child, state, deps) {
                    return true;
                }
            }
        }
        state.insert(node, 2);
        false
    }

    for node in deps.keys().copied() {
        if dfs(node, &mut state, &deps) {
            return true;
        }
    }
    false
}

// ─── Markdown generation ─────────────────────────────────────────────────────

/// Generate a v2 Markdown handoff document from a single session summary.
pub fn generate_handoff_markdown_v2(summary: &HandoffSummary) -> String {
    let mut md = String::new();
    md.push_str("# Session Handoff\n\n");
    append_v2_markdown_sections(&mut md, summary);
    md
}

/// Generate a v2 Markdown handoff document from merged summaries.
pub fn generate_merged_handoff_markdown_v2(merged: &MergedHandoff) -> String {
    let mut md = String::new();
    md.push_str("# Merged Session Handoff\n\n");
    md.push_str(&format!(
        "**Sessions:** {} | **Total Duration:** {}\n\n",
        merged.source_session_ids.len(),
        format_duration(merged.total_duration_seconds)
    ));

    for (idx, summary) in merged.summaries.iter().enumerate() {
        md.push_str(&format!(
            "---\n\n## Session {} — {}\n\n",
            idx + 1,
            summary.source_session_id
        ));
        append_v2_markdown_sections(&mut md, summary);
        md.push('\n');
    }

    md
}

fn append_v2_markdown_sections(md: &mut String, summary: &HandoffSummary) {
    md.push_str("## Objective\n");
    md.push_str(&summary.objective);
    md.push_str("\n\n");

    md.push_str("## Current State\n");
    md.push_str(&format!(
        "- **Tool:** {} ({})\n- **Duration:** {}\n- **Messages:** {} | Tool calls: {} | Events: {}\n",
        summary.tool,
        summary.model,
        format_duration(summary.duration_seconds),
        summary.stats.message_count,
        summary.stats.tool_call_count,
        summary.stats.event_count
    ));
    if !summary.execution_contract.done_definition.is_empty() {
        md.push_str("- **Done:**\n");
        for done in &summary.execution_contract.done_definition {
            md.push_str(&format!("  - {done}\n"));
        }
    }
    md.push('\n');

    md.push_str("## Next Actions (ordered)\n");
    if summary.execution_contract.next_actions.is_empty() {
        md.push_str("_(none)_\n");
    } else {
        for (idx, action) in summary.execution_contract.next_actions.iter().enumerate() {
            md.push_str(&format!("{}. {}\n", idx + 1, action));
        }
    }
    md.push('\n');

    md.push_str("## Verification\n");
    if summary.verification.checks_run.is_empty() {
        md.push_str("- checks_run: _(none)_\n");
    } else {
        for check in &summary.verification.checks_run {
            let code = check
                .exit_code
                .map(|c| c.to_string())
                .unwrap_or_else(|| "?".to_string());
            md.push_str(&format!(
                "- [{}] `{}` (exit: {}, event: {})\n",
                check.status, check.command, code, check.event_id
            ));
        }
    }
    if !summary.verification.required_checks_missing.is_empty() {
        md.push_str("- required_checks_missing:\n");
        for item in &summary.verification.required_checks_missing {
            md.push_str(&format!("  - {item}\n"));
        }
    }
    md.push('\n');

    md.push_str("## Blockers / Decisions\n");
    if summary.uncertainty.decision_required.is_empty()
        && summary.uncertainty.open_questions.is_empty()
    {
        md.push_str("_(none)_\n");
    } else {
        for item in &summary.uncertainty.decision_required {
            md.push_str(&format!("- {item}\n"));
        }
        if !summary.uncertainty.open_questions.is_empty() {
            md.push_str("- open_questions:\n");
            for item in &summary.uncertainty.open_questions {
                md.push_str(&format!("  - {item}\n"));
            }
        }
    }
    md.push('\n');

    md.push_str("## Evidence Index\n");
    if summary.evidence.is_empty() {
        md.push_str("_(none)_\n");
    } else {
        for ev in &summary.evidence {
            md.push_str(&format!(
                "- `{}` {} ({}, {}, {})\n",
                ev.id, ev.claim, ev.event_id, ev.source_type, ev.timestamp
            ));
        }
    }
    md.push('\n');

    md.push_str("## Conversations\n");
    if summary.key_conversations.is_empty() {
        md.push_str("_(none)_\n");
    } else {
        for (idx, conv) in summary.key_conversations.iter().enumerate() {
            md.push_str(&format!(
                "### {}. User\n{}\n\n### {}. Agent\n{}\n\n",
                idx + 1,
                truncate_str(&conv.user, 300),
                idx + 1,
                truncate_str(&conv.agent, 300)
            ));
        }
    }

    md.push_str("## User Messages\n");
    if summary.user_messages.is_empty() {
        md.push_str("_(none)_\n");
    } else {
        for (idx, msg) in summary.user_messages.iter().enumerate() {
            md.push_str(&format!("{}. {}\n", idx + 1, truncate_str(msg, 150)));
        }
    }
}

/// Generate a Markdown handoff document from a single session summary.
pub fn generate_handoff_markdown(summary: &HandoffSummary) -> String {
    const MAX_TASK_SUMMARIES_DISPLAY: usize = 5;
    let mut md = String::new();

    md.push_str("# Session Handoff\n\n");

    // Objective
    md.push_str("## Objective\n");
    md.push_str(&summary.objective);
    md.push_str("\n\n");

    // Summary
    md.push_str("## Summary\n");
    md.push_str(&format!(
        "- **Tool:** {} ({})\n",
        summary.tool, summary.model
    ));
    md.push_str(&format!(
        "- **Duration:** {}\n",
        format_duration(summary.duration_seconds)
    ));
    md.push_str(&format!(
        "- **Messages:** {} | Tool calls: {} | Events: {}\n",
        summary.stats.message_count, summary.stats.tool_call_count, summary.stats.event_count
    ));
    md.push('\n');

    if !summary.task_summaries.is_empty() {
        md.push_str("## Task Summaries\n");
        for (idx, task_summary) in summary
            .task_summaries
            .iter()
            .take(MAX_TASK_SUMMARIES_DISPLAY)
            .enumerate()
        {
            md.push_str(&format!("{}. {}\n", idx + 1, task_summary));
        }
        if summary.task_summaries.len() > MAX_TASK_SUMMARIES_DISPLAY {
            md.push_str(&format!(
                "- ... and {} more\n",
                summary.task_summaries.len() - MAX_TASK_SUMMARIES_DISPLAY
            ));
        }
        md.push('\n');
    }

    // Files Modified
    if !summary.files_modified.is_empty() {
        md.push_str("## Files Modified\n");
        for fc in &summary.files_modified {
            md.push_str(&format!("- `{}` ({})\n", fc.path, fc.action));
        }
        md.push('\n');
    }

    // Files Read
    if !summary.files_read.is_empty() {
        md.push_str("## Files Read\n");
        for path in &summary.files_read {
            md.push_str(&format!("- `{path}`\n"));
        }
        md.push('\n');
    }

    // Shell Commands
    if !summary.shell_commands.is_empty() {
        md.push_str("## Shell Commands\n");
        for cmd in &summary.shell_commands {
            let code_str = match cmd.exit_code {
                Some(c) => c.to_string(),
                None => "?".to_string(),
            };
            md.push_str(&format!(
                "- `{}` → {}\n",
                truncate_str(&cmd.command, 80),
                code_str
            ));
        }
        md.push('\n');
    }

    // Errors
    if !summary.errors.is_empty() {
        md.push_str("## Errors\n");
        for err in &summary.errors {
            md.push_str(&format!("- {err}\n"));
        }
        md.push('\n');
    }

    // Key Conversations (user + agent pairs)
    if !summary.key_conversations.is_empty() {
        md.push_str("## Key Conversations\n");
        for (i, conv) in summary.key_conversations.iter().enumerate() {
            md.push_str(&format!(
                "### {}. User\n{}\n\n### {}. Agent\n{}\n\n",
                i + 1,
                truncate_str(&conv.user, 300),
                i + 1,
                truncate_str(&conv.agent, 300),
            ));
        }
    }

    // User Messages (fallback list)
    if summary.key_conversations.is_empty() && !summary.user_messages.is_empty() {
        md.push_str("## User Messages\n");
        for (i, msg) in summary.user_messages.iter().enumerate() {
            md.push_str(&format!("{}. {}\n", i + 1, truncate_str(msg, 150)));
        }
        md.push('\n');
    }

    md
}

/// Generate a Markdown handoff document from a merged multi-session handoff.
pub fn generate_merged_handoff_markdown(merged: &MergedHandoff) -> String {
    const MAX_TASK_SUMMARIES_DISPLAY: usize = 3;
    let mut md = String::new();

    md.push_str("# Merged Session Handoff\n\n");
    md.push_str(&format!(
        "**Sessions:** {} | **Total Duration:** {}\n\n",
        merged.source_session_ids.len(),
        format_duration(merged.total_duration_seconds)
    ));

    // Per-session summaries
    for (i, s) in merged.summaries.iter().enumerate() {
        md.push_str(&format!(
            "---\n\n## Session {} — {}\n\n",
            i + 1,
            s.source_session_id
        ));
        md.push_str(&format!("**Objective:** {}\n\n", s.objective));
        md.push_str(&format!(
            "- **Tool:** {} ({}) | **Duration:** {}\n",
            s.tool,
            s.model,
            format_duration(s.duration_seconds)
        ));
        md.push_str(&format!(
            "- **Messages:** {} | Tool calls: {} | Events: {}\n\n",
            s.stats.message_count, s.stats.tool_call_count, s.stats.event_count
        ));

        if !s.task_summaries.is_empty() {
            md.push_str("### Task Summaries\n");
            for (j, task_summary) in s
                .task_summaries
                .iter()
                .take(MAX_TASK_SUMMARIES_DISPLAY)
                .enumerate()
            {
                md.push_str(&format!("{}. {}\n", j + 1, task_summary));
            }
            if s.task_summaries.len() > MAX_TASK_SUMMARIES_DISPLAY {
                md.push_str(&format!(
                    "- ... and {} more\n",
                    s.task_summaries.len() - MAX_TASK_SUMMARIES_DISPLAY
                ));
            }
            md.push('\n');
        }

        // Key Conversations for this session
        if !s.key_conversations.is_empty() {
            md.push_str("### Conversations\n");
            for (j, conv) in s.key_conversations.iter().enumerate() {
                md.push_str(&format!(
                    "**{}. User:** {}\n\n**{}. Agent:** {}\n\n",
                    j + 1,
                    truncate_str(&conv.user, 200),
                    j + 1,
                    truncate_str(&conv.agent, 200),
                ));
            }
        }
    }

    // Combined files
    md.push_str("---\n\n## All Files Modified\n");
    if merged.all_files_modified.is_empty() {
        md.push_str("_(none)_\n");
    } else {
        for fc in &merged.all_files_modified {
            md.push_str(&format!("- `{}` ({})\n", fc.path, fc.action));
        }
    }
    md.push('\n');

    if !merged.all_files_read.is_empty() {
        md.push_str("## All Files Read\n");
        for path in &merged.all_files_read {
            md.push_str(&format!("- `{path}`\n"));
        }
        md.push('\n');
    }

    // Errors
    if !merged.total_errors.is_empty() {
        md.push_str("## All Errors\n");
        for err in &merged.total_errors {
            md.push_str(&format!("- {err}\n"));
        }
        md.push('\n');
    }

    md
}

// ─── Summary HAIL generation ─────────────────────────────────────────────────

/// Generate a summary HAIL session from an original session.
///
/// Filters events to only include important ones and truncates content.
pub fn generate_handoff_hail(session: &Session) -> Session {
    let mut summary_session = Session {
        version: session.version.clone(),
        session_id: format!("handoff-{}", session.session_id),
        agent: session.agent.clone(),
        context: SessionContext {
            title: Some(format!(
                "Handoff: {}",
                session.context.title.as_deref().unwrap_or("(untitled)")
            )),
            description: session.context.description.clone(),
            tags: {
                let mut tags = session.context.tags.clone();
                if !tags.contains(&"handoff".to_string()) {
                    tags.push("handoff".to_string());
                }
                tags
            },
            created_at: session.context.created_at,
            updated_at: chrono::Utc::now(),
            related_session_ids: vec![session.session_id.clone()],
            attributes: HashMap::new(),
        },
        events: Vec::new(),
        stats: session.stats.clone(),
    };

    for event in &session.events {
        let keep = matches!(
            &event.event_type,
            EventType::UserMessage
                | EventType::AgentMessage
                | EventType::FileEdit { .. }
                | EventType::FileCreate { .. }
                | EventType::FileDelete { .. }
                | EventType::TaskStart { .. }
                | EventType::TaskEnd { .. }
        ) || matches!(&event.event_type, EventType::ShellCommand { exit_code, .. } if *exit_code != Some(0));

        if !keep {
            continue;
        }

        // Truncate content blocks
        let truncated_blocks: Vec<ContentBlock> = event
            .content
            .blocks
            .iter()
            .map(|block| match block {
                ContentBlock::Text { text } => ContentBlock::Text {
                    text: truncate_str(text, 300),
                },
                ContentBlock::Code {
                    code,
                    language,
                    start_line,
                } => ContentBlock::Code {
                    code: truncate_str(code, 300),
                    language: language.clone(),
                    start_line: *start_line,
                },
                other => other.clone(),
            })
            .collect();

        summary_session.events.push(Event {
            event_id: event.event_id.clone(),
            timestamp: event.timestamp,
            event_type: event.event_type.clone(),
            task_id: event.task_id.clone(),
            content: Content {
                blocks: truncated_blocks,
            },
            duration_ms: event.duration_ms,
            attributes: HashMap::new(), // strip detailed attributes
        });
    }

    // Recompute stats for the filtered events
    summary_session.recompute_stats();

    summary_session
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

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

    if let Some(title) = session.context.title.as_deref().map(str::trim) {
        if !title.is_empty() {
            return truncate_str(&collapse_whitespace(title), 200);
        }
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

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{testing, Agent};

    fn make_agent() -> Agent {
        testing::agent()
    }

    fn make_event(event_type: EventType, text: &str) -> Event {
        testing::event(event_type, text)
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(45), "45s");
        assert_eq!(format_duration(90), "1m 30s");
        assert_eq!(format_duration(750), "12m 30s");
        assert_eq!(format_duration(3661), "1h 1m 1s");
    }

    #[test]
    fn test_handoff_summary_from_session() {
        let mut session = Session::new("test-id".to_string(), make_agent());
        session.stats = Stats {
            event_count: 10,
            message_count: 3,
            tool_call_count: 5,
            duration_seconds: 750,
            ..Default::default()
        };
        session
            .events
            .push(make_event(EventType::UserMessage, "Fix the build error"));
        session
            .events
            .push(make_event(EventType::AgentMessage, "I'll fix it now"));
        session.events.push(make_event(
            EventType::FileEdit {
                path: "src/main.rs".to_string(),
                diff: None,
            },
            "",
        ));
        session.events.push(make_event(
            EventType::FileRead {
                path: "Cargo.toml".to_string(),
            },
            "",
        ));
        session.events.push(make_event(
            EventType::ShellCommand {
                command: "cargo build".to_string(),
                exit_code: Some(0),
            },
            "",
        ));
        session.events.push(make_event(
            EventType::TaskEnd {
                summary: Some("Build now passes in local env".to_string()),
            },
            "",
        ));

        let summary = HandoffSummary::from_session(&session);

        assert_eq!(summary.source_session_id, "test-id");
        assert_eq!(summary.objective, "Fix the build error");
        assert_eq!(summary.files_modified.len(), 1);
        assert_eq!(summary.files_modified[0].path, "src/main.rs");
        assert_eq!(summary.files_modified[0].action, "edited");
        assert_eq!(summary.files_read, vec!["Cargo.toml"]);
        assert_eq!(summary.shell_commands.len(), 1);
        assert_eq!(
            summary.task_summaries,
            vec!["Build now passes in local env".to_string()]
        );
        assert_eq!(summary.key_conversations.len(), 1);
        assert_eq!(summary.key_conversations[0].user, "Fix the build error");
        assert_eq!(summary.key_conversations[0].agent, "I'll fix it now");
    }

    #[test]
    fn test_handoff_objective_falls_back_to_task_title() {
        let mut session = Session::new("task-title-fallback".to_string(), make_agent());
        session.context.title = Some("session-019c-example.jsonl".to_string());
        session.events.push(make_event(
            EventType::TaskStart {
                title: Some("Refactor auth middleware for oauth callback".to_string()),
            },
            "",
        ));

        let summary = HandoffSummary::from_session(&session);
        assert_eq!(
            summary.objective,
            "Refactor auth middleware for oauth callback"
        );
    }

    #[test]
    fn test_handoff_task_summaries_are_deduplicated() {
        let mut session = Session::new("task-summary-dedupe".to_string(), make_agent());
        session.events.push(make_event(
            EventType::TaskEnd {
                summary: Some("Add worker profile guard".to_string()),
            },
            "",
        ));
        session.events.push(make_event(
            EventType::TaskEnd {
                summary: Some(" ".to_string()),
            },
            "",
        ));
        session.events.push(make_event(
            EventType::TaskEnd {
                summary: Some("Add worker profile guard".to_string()),
            },
            "",
        ));
        session.events.push(make_event(
            EventType::TaskEnd {
                summary: Some("Hide teams nav for worker profile".to_string()),
            },
            "",
        ));

        let summary = HandoffSummary::from_session(&session);
        assert_eq!(
            summary.task_summaries,
            vec![
                "Add worker profile guard".to_string(),
                "Hide teams nav for worker profile".to_string()
            ]
        );
    }

    #[test]
    fn test_files_read_excludes_modified() {
        let mut session = Session::new("test-id".to_string(), make_agent());
        session
            .events
            .push(make_event(EventType::UserMessage, "test"));
        session.events.push(make_event(
            EventType::FileRead {
                path: "src/main.rs".to_string(),
            },
            "",
        ));
        session.events.push(make_event(
            EventType::FileEdit {
                path: "src/main.rs".to_string(),
                diff: None,
            },
            "",
        ));
        session.events.push(make_event(
            EventType::FileRead {
                path: "README.md".to_string(),
            },
            "",
        ));

        let summary = HandoffSummary::from_session(&session);
        assert_eq!(summary.files_read, vec!["README.md"]);
        assert_eq!(summary.files_modified.len(), 1);
    }

    #[test]
    fn test_file_create_not_overwritten_by_edit() {
        let mut session = Session::new("test-id".to_string(), make_agent());
        session
            .events
            .push(make_event(EventType::UserMessage, "test"));
        session.events.push(make_event(
            EventType::FileCreate {
                path: "new_file.rs".to_string(),
            },
            "",
        ));
        session.events.push(make_event(
            EventType::FileEdit {
                path: "new_file.rs".to_string(),
                diff: None,
            },
            "",
        ));

        let summary = HandoffSummary::from_session(&session);
        assert_eq!(summary.files_modified[0].action, "created");
    }

    #[test]
    fn test_shell_error_captured() {
        let mut session = Session::new("test-id".to_string(), make_agent());
        session
            .events
            .push(make_event(EventType::UserMessage, "test"));
        session.events.push(make_event(
            EventType::ShellCommand {
                command: "cargo test".to_string(),
                exit_code: Some(1),
            },
            "",
        ));

        let summary = HandoffSummary::from_session(&session);
        assert_eq!(summary.errors.len(), 1);
        assert!(summary.errors[0].contains("cargo test"));
    }

    #[test]
    fn test_generate_handoff_markdown() {
        let mut session = Session::new("test-id".to_string(), make_agent());
        session.stats = Stats {
            event_count: 10,
            message_count: 3,
            tool_call_count: 5,
            duration_seconds: 750,
            ..Default::default()
        };
        session
            .events
            .push(make_event(EventType::UserMessage, "Fix the build error"));
        session
            .events
            .push(make_event(EventType::AgentMessage, "I'll fix it now"));
        session.events.push(make_event(
            EventType::FileEdit {
                path: "src/main.rs".to_string(),
                diff: None,
            },
            "",
        ));
        session.events.push(make_event(
            EventType::ShellCommand {
                command: "cargo build".to_string(),
                exit_code: Some(0),
            },
            "",
        ));
        session.events.push(make_event(
            EventType::TaskEnd {
                summary: Some("Compile error fixed by updating trait bounds".to_string()),
            },
            "",
        ));

        let summary = HandoffSummary::from_session(&session);
        let md = generate_handoff_markdown(&summary);

        assert!(md.contains("# Session Handoff"));
        assert!(md.contains("Fix the build error"));
        assert!(md.contains("claude-code (claude-opus-4-6)"));
        assert!(md.contains("12m 30s"));
        assert!(md.contains("## Task Summaries"));
        assert!(md.contains("Compile error fixed by updating trait bounds"));
        assert!(md.contains("`src/main.rs` (edited)"));
        assert!(md.contains("`cargo build` → 0"));
        assert!(md.contains("## Key Conversations"));
    }

    #[test]
    fn test_merge_summaries() {
        let mut s1 = Session::new("session-a".to_string(), make_agent());
        s1.stats.duration_seconds = 100;
        s1.events.push(make_event(EventType::UserMessage, "task A"));
        s1.events.push(make_event(
            EventType::FileEdit {
                path: "a.rs".to_string(),
                diff: None,
            },
            "",
        ));

        let mut s2 = Session::new("session-b".to_string(), make_agent());
        s2.stats.duration_seconds = 200;
        s2.events.push(make_event(EventType::UserMessage, "task B"));
        s2.events.push(make_event(
            EventType::FileEdit {
                path: "b.rs".to_string(),
                diff: None,
            },
            "",
        ));

        let sum1 = HandoffSummary::from_session(&s1);
        let sum2 = HandoffSummary::from_session(&s2);
        let merged = merge_summaries(&[sum1, sum2]);

        assert_eq!(merged.source_session_ids.len(), 2);
        assert_eq!(merged.total_duration_seconds, 300);
        assert_eq!(merged.all_files_modified.len(), 2);
    }

    #[test]
    fn test_generate_handoff_hail() {
        let mut session = Session::new("test-id".to_string(), make_agent());
        session
            .events
            .push(make_event(EventType::UserMessage, "Hello"));
        session
            .events
            .push(make_event(EventType::AgentMessage, "Hi there"));
        session.events.push(make_event(
            EventType::FileRead {
                path: "foo.rs".to_string(),
            },
            "",
        ));
        session.events.push(make_event(
            EventType::FileEdit {
                path: "foo.rs".to_string(),
                diff: Some("+added line".to_string()),
            },
            "",
        ));
        session.events.push(make_event(
            EventType::ShellCommand {
                command: "cargo build".to_string(),
                exit_code: Some(0),
            },
            "",
        ));

        let hail = generate_handoff_hail(&session);

        assert!(hail.session_id.starts_with("handoff-"));
        assert_eq!(hail.context.related_session_ids, vec!["test-id"]);
        assert!(hail.context.tags.contains(&"handoff".to_string()));
        // FileRead and successful ShellCommand should be filtered out
        assert_eq!(hail.events.len(), 3); // UserMessage, AgentMessage, FileEdit
                                          // Verify HAIL roundtrip
        let jsonl = hail.to_jsonl().unwrap();
        let parsed = Session::from_jsonl(&jsonl).unwrap();
        assert_eq!(parsed.session_id, hail.session_id);
    }

    #[test]
    fn test_generate_handoff_markdown_v2_section_order() {
        let mut session = Session::new("v2-sections".to_string(), make_agent());
        session
            .events
            .push(make_event(EventType::UserMessage, "Implement handoff v2"));
        session.events.push(make_event(
            EventType::FileEdit {
                path: "crates/core/src/handoff.rs".to_string(),
                diff: None,
            },
            "",
        ));
        session.events.push(make_event(
            EventType::ShellCommand {
                command: "cargo test".to_string(),
                exit_code: Some(0),
            },
            "",
        ));

        let summary = HandoffSummary::from_session(&session);
        let md = generate_handoff_markdown_v2(&summary);

        let order = [
            "## Objective",
            "## Current State",
            "## Next Actions (ordered)",
            "## Verification",
            "## Blockers / Decisions",
            "## Evidence Index",
            "## Conversations",
            "## User Messages",
        ];

        let mut last_idx = 0usize;
        for section in order {
            let idx = md.find(section).unwrap();
            assert!(
                idx >= last_idx,
                "section order mismatch for {section}: idx={idx}, last={last_idx}"
            );
            last_idx = idx;
        }
    }

    #[test]
    fn test_execution_contract_and_verification_from_failed_command() {
        let mut session = Session::new("failed-check".to_string(), make_agent());
        session
            .events
            .push(make_event(EventType::UserMessage, "Fix failing tests"));
        session.events.push(make_event(
            EventType::FileEdit {
                path: "src/lib.rs".to_string(),
                diff: None,
            },
            "",
        ));
        session.events.push(make_event(
            EventType::ShellCommand {
                command: "cargo test".to_string(),
                exit_code: Some(1),
            },
            "",
        ));

        let summary = HandoffSummary::from_session(&session);
        assert!(summary
            .verification
            .checks_failed
            .contains(&"cargo test".to_string()));
        assert!(summary
            .execution_contract
            .next_actions
            .iter()
            .any(|action| action.contains("cargo test")));
        assert_eq!(
            summary.execution_contract.ordered_commands.first(),
            Some(&"cargo test".to_string())
        );
        assert!(summary.execution_contract.rollback_hint.is_none());
        assert!(summary
            .execution_contract
            .rollback_hint_missing_reason
            .is_some());
        assert!(summary
            .execution_contract
            .rollback_hint_undefined_reason
            .is_some());
    }

    #[test]
    fn test_validate_handoff_summary_flags_missing_objective() {
        let session = Session::new("missing-objective".to_string(), make_agent());
        let summary = HandoffSummary::from_session(&session);
        assert!(summary.objective_undefined_reason.is_some());
        assert!(summary
            .undefined_fields
            .iter()
            .any(|f| f.path == "objective"));
        let report = validate_handoff_summary(&summary);

        assert!(!report.passed);
        assert!(report
            .findings
            .iter()
            .any(|f| f.code == "objective_missing"));
    }

    #[test]
    fn test_validate_handoff_summary_flags_cycle() {
        let mut session = Session::new("cycle-case".to_string(), make_agent());
        session
            .events
            .push(make_event(EventType::UserMessage, "test"));
        let mut summary = HandoffSummary::from_session(&session);
        summary.work_packages = vec![
            WorkPackage {
                id: "a".to_string(),
                title: "A".to_string(),
                status: "pending".to_string(),
                depends_on: vec!["b".to_string()],
                files: Vec::new(),
                commands: Vec::new(),
                evidence_refs: Vec::new(),
            },
            WorkPackage {
                id: "b".to_string(),
                title: "B".to_string(),
                status: "pending".to_string(),
                depends_on: vec!["a".to_string()],
                files: Vec::new(),
                commands: Vec::new(),
                evidence_refs: Vec::new(),
            },
        ];

        let report = validate_handoff_summary(&summary);
        assert!(report
            .findings
            .iter()
            .any(|f| f.code == "work_package_cycle"));
    }

    #[test]
    fn test_validate_handoff_summary_requires_next_actions_for_failed_checks() {
        let mut session = Session::new("missing-next-action".to_string(), make_agent());
        session
            .events
            .push(make_event(EventType::UserMessage, "test"));
        let mut summary = HandoffSummary::from_session(&session);
        summary.verification.checks_run = vec![CheckRun {
            command: "cargo test".to_string(),
            status: "failed".to_string(),
            exit_code: Some(1),
            event_id: "evt-1".to_string(),
        }];
        summary.execution_contract.next_actions.clear();

        let report = validate_handoff_summary(&summary);
        assert!(report
            .findings
            .iter()
            .any(|f| f.code == "next_actions_missing"));
    }

    #[test]
    fn test_validate_handoff_summary_flags_missing_objective_evidence() {
        let mut session = Session::new("missing-objective-evidence".to_string(), make_agent());
        session
            .events
            .push(make_event(EventType::UserMessage, "keep objective"));
        let mut summary = HandoffSummary::from_session(&session);
        summary.evidence = vec![EvidenceRef {
            id: "evidence-1".to_string(),
            claim: "task_done: something".to_string(),
            event_id: "evt".to_string(),
            timestamp: "2026-02-01T00:00:00Z".to_string(),
            source_type: "TaskEnd".to_string(),
        }];

        let report = validate_handoff_summary(&summary);
        assert!(report
            .findings
            .iter()
            .any(|f| f.code == "objective_evidence_missing"));
    }
}
