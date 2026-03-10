use std::collections::{BTreeMap, HashMap, HashSet};

use crate::extract::truncate_str;
use crate::{Event, EventType, Session};

use super::{
    CheckRun, EvidenceRef, ExecutionContract, FileChange, OrderedStep, ShellCmd, Uncertainty,
    UndefinedField, Verification, WorkPackage, collapse_whitespace,
};

pub(super) fn build_execution_contract(
    task_summaries: &[String],
    verification: &Verification,
    uncertainty: &Uncertainty,
    shell_commands: &[ShellCmd],
    files_modified: &[FileChange],
    work_packages: &[WorkPackage],
) -> ExecutionContract {
    let ordered_steps = work_packages
        .iter()
        .filter(|pkg| is_material_work_package(pkg))
        .map(|pkg| OrderedStep {
            sequence: pkg.sequence,
            work_package_id: pkg.id.clone(),
            title: pkg.title.clone(),
            status: pkg.status.clone(),
            depends_on: pkg.depends_on.clone(),
            started_at: pkg.started_at.clone(),
            completed_at: pkg.completed_at.clone(),
            evidence_refs: pkg.evidence_refs.clone(),
        })
        .collect::<Vec<_>>();

    let mut done_definition = ordered_steps
        .iter()
        .filter(|step| step.status == "completed")
        .map(|step| {
            let pkg = work_packages
                .iter()
                .find(|pkg| pkg.id == step.work_package_id)
                .expect("ordered step must map to existing work package");
            let mut details = Vec::new();
            if let Some(outcome) = pkg.outcome.as_deref() {
                details.push(format!("outcome: {}", truncate_str(outcome, 140)));
            }
            let footprint = work_package_footprint(pkg);
            if !footprint.is_empty() {
                details.push(footprint);
            }
            let at = step
                .completed_at
                .as_deref()
                .or(step.started_at.as_deref())
                .unwrap_or("time-unavailable");
            if details.is_empty() {
                format!("[{}] Completed `{}` at {}.", step.sequence, step.title, at)
            } else {
                format!(
                    "[{}] Completed `{}` at {} ({}).",
                    step.sequence,
                    step.title,
                    at,
                    details.join("; ")
                )
            }
        })
        .collect::<Vec<_>>();

    if !verification.checks_passed.is_empty() {
        let keep = verification
            .checks_passed
            .iter()
            .take(3)
            .map(|check| format!("`{check}`"))
            .collect::<Vec<_>>();
        let extra = verification.checks_passed.len().saturating_sub(3);
        if extra > 0 {
            done_definition.push(format!(
                "Verification passed: {} (+{} more).",
                keep.join(", "),
                extra
            ));
        } else {
            done_definition.push(format!("Verification passed: {}.", keep.join(", ")));
        }
    }

    if !files_modified.is_empty() {
        let keep = files_modified
            .iter()
            .take(3)
            .map(|file| format!("`{}`", file.path))
            .collect::<Vec<_>>();
        let extra = files_modified.len().saturating_sub(3);
        if extra > 0 {
            done_definition.push(format!(
                "Changed {} file(s): {} (+{} more).",
                files_modified.len(),
                keep.join(", "),
                extra
            ));
        } else {
            done_definition.push(format!(
                "Changed {} file(s): {}.",
                files_modified.len(),
                keep.join(", ")
            ));
        }
    }

    if done_definition.is_empty() {
        done_definition.extend(task_summaries.iter().take(5).cloned());
    }
    dedupe_keep_order(&mut done_definition);

    let mut next_actions = unresolved_failed_commands(&verification.checks_run)
        .into_iter()
        .map(|cmd| format!("Fix and re-run `{cmd}` until the check passes."))
        .collect::<Vec<_>>();
    next_actions.extend(
        verification
            .required_checks_missing
            .iter()
            .map(|missing| format!("Add/restore verification check: {missing}")),
    );
    next_actions.extend(ordered_steps.iter().filter_map(|step| {
        if step.status == "completed" || step.depends_on.is_empty() {
            return None;
        }
        Some(format!(
            "[{}] After dependencies [{}], execute `{}` ({}).",
            step.sequence,
            step.depends_on.join(", "),
            step.title,
            step.work_package_id
        ))
    }));
    next_actions.extend(
        uncertainty
            .open_questions
            .iter()
            .map(|q| format!("Resolve open question: {q}")),
    );
    let mut parallel_actions = ordered_steps
        .iter()
        .filter(|step| {
            step.status != "completed"
                && step.depends_on.is_empty()
                && step.work_package_id != "main"
        })
        .map(|step| {
            let at = step.started_at.as_deref().unwrap_or("time-unavailable");
            format!(
                "[{}] `{}` ({}) — start: {}",
                step.sequence, step.title, step.work_package_id, at
            )
        })
        .collect::<Vec<_>>();

    if done_definition.is_empty()
        && next_actions.is_empty()
        && parallel_actions.is_empty()
        && ordered_steps.is_empty()
    {
        next_actions.push(
            "Define completion criteria and run at least one verification command.".to_string(),
        );
    }
    dedupe_keep_order(&mut next_actions);
    dedupe_keep_order(&mut parallel_actions);

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
        parallel_actions,
        ordered_steps,
        ordered_commands,
        rollback_hint,
        rollback_hint_missing_reason: rollback_hint_missing_reason.clone(),
        rollback_hint_undefined_reason: rollback_hint_missing_reason,
    }
}

fn work_package_footprint(pkg: &WorkPackage) -> String {
    let mut details = Vec::new();
    if !pkg.files.is_empty() {
        details.push(format!("files: {}", pkg.files.len()));
    }
    if !pkg.commands.is_empty() {
        details.push(format!("commands: {}", pkg.commands.len()));
    }
    details.join(", ")
}

pub(super) fn collect_evidence(
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

pub(super) fn build_work_packages(events: &[Event], evidence: &[EvidenceRef]) -> Vec<WorkPackage> {
    #[derive(Default)]
    struct WorkPackageAcc {
        title: Option<String>,
        status: String,
        outcome: Option<String>,
        first_ts: Option<chrono::DateTime<chrono::Utc>>,
        first_idx: Option<usize>,
        completed_ts: Option<chrono::DateTime<chrono::Utc>>,
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
    for (event_idx, event) in events.iter().enumerate() {
        let key = package_key_for_event(event);
        let acc = grouped
            .entry(key.clone())
            .or_insert_with(|| WorkPackageAcc {
                status: "pending".to_string(),
                ..Default::default()
            });

        if acc.first_ts.is_none() {
            acc.first_ts = Some(event.timestamp);
            acc.first_idx = Some(event_idx);
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
            EventType::TaskEnd { summary } => {
                acc.status = "completed".to_string();
                acc.completed_ts = Some(event.timestamp);
                if let Some(summary) = summary
                    .as_deref()
                    .map(collapse_whitespace)
                    .filter(|summary| !summary.is_empty())
                {
                    acc.outcome = Some(summary.clone());
                    if acc.title.is_none() {
                        acc.title = Some(truncate_str(&summary, 160));
                    }
                }
            }
            EventType::FileEdit { path, .. }
            | EventType::FileCreate { path }
            | EventType::FileDelete { path } => {
                acc.files.insert(path.clone());
                if acc.status == "pending" {
                    acc.status = "in_progress".to_string();
                }
            }
            EventType::ShellCommand { command, .. } => {
                acc.commands.push(collapse_whitespace(command));
                if acc.status == "pending" {
                    acc.status = "in_progress".to_string();
                }
            }
            _ => {}
        }
    }

    let mut by_first_seen = grouped
        .into_iter()
        .map(|(id, mut acc)| {
            dedupe_keep_order(&mut acc.commands);
            dedupe_keep_order(&mut acc.evidence_refs);
            let mut files: Vec<String> = acc.files.into_iter().collect();
            files.sort();
            (
                acc.first_ts,
                acc.first_idx.unwrap_or(usize::MAX),
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
                    sequence: 0,
                    started_at: acc.first_ts.map(|ts| ts.to_rfc3339()),
                    completed_at: acc.completed_ts.map(|ts| ts.to_rfc3339()),
                    outcome: acc.outcome,
                    depends_on: Vec::new(),
                    files,
                    commands: acc.commands,
                    evidence_refs: acc.evidence_refs,
                },
            )
        })
        .collect::<Vec<_>>();

    by_first_seen.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.id.cmp(&b.2.id))
    });

    let mut packages = by_first_seen
        .into_iter()
        .map(|(_, _, package)| package)
        .collect::<Vec<_>>();

    for (idx, package) in packages.iter_mut().enumerate() {
        package.sequence = (idx + 1) as u32;
    }

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

    packages.retain(|pkg| pkg.id == "main" || is_material_work_package(pkg));
    let known_ids: HashSet<String> = packages.iter().map(|pkg| pkg.id.clone()).collect();
    for pkg in &mut packages {
        pkg.depends_on.retain(|dep| known_ids.contains(dep));
    }

    packages
}

fn is_generic_work_package_title(id: &str, title: &str) -> bool {
    title == "Main flow" || title == format!("Task {id}")
}

pub(super) fn is_material_work_package(pkg: &WorkPackage) -> bool {
    if !pkg.files.is_empty() || !pkg.commands.is_empty() || pkg.outcome.is_some() {
        return true;
    }

    if pkg.id == "main" {
        return pkg.status != "pending";
    }

    if !is_generic_work_package_title(&pkg.id, &pkg.title) {
        return true;
    }

    pkg.status == "completed" && !pkg.evidence_refs.is_empty()
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

pub(super) fn collect_open_questions(events: &[Event]) -> Vec<String> {
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

        if event.attr_str("source") == Some("interactive")
            && let Some(ids) = event
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

pub(super) fn unresolved_failed_commands(checks_run: &[CheckRun]) -> Vec<String> {
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

pub(super) fn dedupe_keep_order(values: &mut Vec<String>) {
    let mut seen = HashSet::new();
    values.retain(|value| seen.insert(value.clone()));
}

pub(super) fn collect_undefined_fields(
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
