use super::*;
use crate::{Agent, testing};

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
    assert_eq!(hail.events.len(), 3);
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
    assert!(
        summary
            .verification
            .checks_failed
            .contains(&"cargo test".to_string())
    );
    assert!(
        summary
            .execution_contract
            .next_actions
            .iter()
            .any(|action| action.contains("cargo test"))
    );
    assert_eq!(
        summary.execution_contract.ordered_commands.first(),
        Some(&"cargo test".to_string())
    );
    assert!(summary.execution_contract.parallel_actions.is_empty());
    assert!(summary.execution_contract.rollback_hint.is_none());
    assert!(
        summary
            .execution_contract
            .rollback_hint_missing_reason
            .is_some()
    );
    assert!(
        summary
            .execution_contract
            .rollback_hint_undefined_reason
            .is_some()
    );
}

#[test]
fn test_validate_handoff_summary_flags_missing_objective() {
    let session = Session::new("missing-objective".to_string(), make_agent());
    let summary = HandoffSummary::from_session(&session);
    assert!(summary.objective_undefined_reason.is_some());
    assert!(
        summary
            .undefined_fields
            .iter()
            .any(|field| field.path == "objective")
    );
    let report = validate_handoff_summary(&summary);

    assert!(!report.passed);
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.code == "objective_missing")
    );
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
            sequence: 1,
            started_at: None,
            completed_at: None,
            outcome: None,
            depends_on: vec!["b".to_string()],
            files: Vec::new(),
            commands: Vec::new(),
            evidence_refs: Vec::new(),
        },
        WorkPackage {
            id: "b".to_string(),
            title: "B".to_string(),
            status: "pending".to_string(),
            sequence: 2,
            started_at: None,
            completed_at: None,
            outcome: None,
            depends_on: vec!["a".to_string()],
            files: Vec::new(),
            commands: Vec::new(),
            evidence_refs: Vec::new(),
        },
    ];

    let report = validate_handoff_summary(&summary);
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.code == "work_package_cycle")
    );
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
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.code == "next_actions_missing")
    );
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
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.code == "objective_evidence_missing")
    );
}

#[test]
fn test_execution_contract_includes_parallel_actions_for_independent_work_packages() {
    let mut session = Session::new("parallel-actions".to_string(), make_agent());
    session.events.push(make_event(
        EventType::UserMessage,
        "Refactor two independent modules",
    ));

    let mut auth_start = make_event(
        EventType::TaskStart {
            title: Some("Refactor auth".to_string()),
        },
        "",
    );
    auth_start.task_id = Some("auth".to_string());
    session.events.push(auth_start);

    let mut auth_edit = make_event(
        EventType::FileEdit {
            path: "src/auth.rs".to_string(),
            diff: None,
        },
        "",
    );
    auth_edit.task_id = Some("auth".to_string());
    session.events.push(auth_edit);

    let mut billing_start = make_event(
        EventType::TaskStart {
            title: Some("Refactor billing".to_string()),
        },
        "",
    );
    billing_start.task_id = Some("billing".to_string());
    session.events.push(billing_start);

    let mut billing_edit = make_event(
        EventType::FileEdit {
            path: "src/billing.rs".to_string(),
            diff: None,
        },
        "",
    );
    billing_edit.task_id = Some("billing".to_string());
    session.events.push(billing_edit);

    let summary = HandoffSummary::from_session(&session);
    assert!(
        summary
            .execution_contract
            .parallel_actions
            .iter()
            .any(|action| action.contains("auth"))
    );
    assert!(
        summary
            .execution_contract
            .parallel_actions
            .iter()
            .any(|action| action.contains("billing"))
    );
    let md = generate_handoff_markdown_v2(&summary);
    assert!(md.contains("Parallelizable Work Packages"));
}

#[test]
fn test_done_definition_prefers_material_signals() {
    let mut session = Session::new("material-signals".to_string(), make_agent());
    session
        .events
        .push(make_event(EventType::UserMessage, "Implement feature X"));
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
            exit_code: Some(0),
        },
        "",
    ));

    let summary = HandoffSummary::from_session(&session);
    assert!(
        summary
            .execution_contract
            .done_definition
            .iter()
            .any(|item| item.contains("Verification passed: `cargo test`"))
    );
    assert!(
        summary
            .execution_contract
            .done_definition
            .iter()
            .any(|item| item.contains("Changed 1 file(s): `src/lib.rs`"))
    );
    assert!(
        summary
            .execution_contract
            .ordered_steps
            .iter()
            .any(|step| step.work_package_id == "main")
    );
}

#[test]
fn test_ordered_steps_keep_temporal_and_task_context() {
    let mut session = Session::new("ordered-steps".to_string(), make_agent());
    session
        .events
        .push(make_event(EventType::UserMessage, "Process two tasks"));

    let mut task1_start = make_event(
        EventType::TaskStart {
            title: Some("Prepare migration".to_string()),
        },
        "",
    );
    task1_start.task_id = Some("task-1".to_string());
    session.events.push(task1_start);

    let mut task1_end = make_event(
        EventType::TaskEnd {
            summary: Some("Migration script prepared".to_string()),
        },
        "",
    );
    task1_end.task_id = Some("task-1".to_string());
    session.events.push(task1_end);

    let mut task2_start = make_event(
        EventType::TaskStart {
            title: Some("Run verification".to_string()),
        },
        "",
    );
    task2_start.task_id = Some("task-2".to_string());
    session.events.push(task2_start);

    let mut task2_cmd = make_event(
        EventType::ShellCommand {
            command: "cargo test".to_string(),
            exit_code: Some(0),
        },
        "",
    );
    task2_cmd.task_id = Some("task-2".to_string());
    session.events.push(task2_cmd);

    let summary = HandoffSummary::from_session(&session);
    let steps = &summary.execution_contract.ordered_steps;
    assert_eq!(steps.len(), 2);
    assert!(steps[0].sequence < steps[1].sequence);
    assert_eq!(steps[0].work_package_id, "task-1");
    assert_eq!(steps[1].work_package_id, "task-2");
    assert!(steps[0].completed_at.is_some());
    assert!(
        summary
            .work_packages
            .iter()
            .find(|pkg| pkg.id == "task-1")
            .and_then(|pkg| pkg.outcome.as_deref())
            .is_some()
    );
}

#[test]
fn test_validate_handoff_summary_flags_inconsistent_ordered_steps() {
    let mut session = Session::new("invalid-ordered-steps".to_string(), make_agent());
    session
        .events
        .push(make_event(EventType::UserMessage, "test ordered steps"));
    let mut summary = HandoffSummary::from_session(&session);
    summary.work_packages = vec![WorkPackage {
        id: "main".to_string(),
        title: "Main flow".to_string(),
        status: "completed".to_string(),
        sequence: 1,
        started_at: Some("2026-02-19T00:00:00Z".to_string()),
        completed_at: Some("2026-02-19T00:01:00Z".to_string()),
        outcome: Some("done".to_string()),
        depends_on: Vec::new(),
        files: vec!["src/lib.rs".to_string()],
        commands: Vec::new(),
        evidence_refs: Vec::new(),
    }];
    summary.execution_contract.ordered_steps = vec![OrderedStep {
        sequence: 1,
        work_package_id: "missing".to_string(),
        title: "missing".to_string(),
        status: "completed".to_string(),
        depends_on: Vec::new(),
        started_at: Some("2026-02-19T00:00:00Z".to_string()),
        completed_at: Some("2026-02-19T00:01:00Z".to_string()),
        evidence_refs: Vec::new(),
    }];

    let report = validate_handoff_summary(&summary);
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.code == "ordered_steps_inconsistent")
    );
}

#[test]
fn test_message_and_conversation_collections_are_condensed() {
    let mut session = Session::new("condense".to_string(), make_agent());

    for idx in 0..24 {
        session
            .events
            .push(make_event(EventType::UserMessage, &format!("user-{idx}")));
        session
            .events
            .push(make_event(EventType::AgentMessage, &format!("agent-{idx}")));
    }

    let summary = HandoffSummary::from_session(&session);
    assert_eq!(summary.user_messages.len(), MAX_USER_MESSAGES);
    assert_eq!(
        summary.user_messages.first().map(String::as_str),
        Some("user-0")
    );
    assert_eq!(
        summary.user_messages.last().map(String::as_str),
        Some("user-23")
    );

    assert_eq!(summary.key_conversations.len(), MAX_KEY_CONVERSATIONS);
    assert_eq!(
        summary
            .key_conversations
            .first()
            .map(|conversation| conversation.user.as_str()),
        Some("user-0")
    );
    assert_eq!(
        summary
            .key_conversations
            .last()
            .map(|conversation| conversation.user.as_str()),
        Some("user-23")
    );
}
