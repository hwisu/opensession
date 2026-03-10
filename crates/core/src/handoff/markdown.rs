use crate::extract::truncate_str;

use super::{HandoffSummary, MergedHandoff, format_duration};

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
    if !summary.execution_contract.ordered_steps.is_empty() {
        md.push_str("- **Execution Timeline (ordered):**\n");
        for step in &summary.execution_contract.ordered_steps {
            let started = step.started_at.as_deref().unwrap_or("?");
            let completed = step.completed_at.as_deref().unwrap_or("-");
            if step.depends_on.is_empty() {
                md.push_str(&format!(
                    "  - [{}] `{}` [{}] status={} start={} done={}\n",
                    step.sequence,
                    step.title,
                    step.work_package_id,
                    step.status,
                    started,
                    completed
                ));
            } else {
                md.push_str(&format!(
                    "  - [{}] `{}` [{}] status={} start={} done={} deps=[{}]\n",
                    step.sequence,
                    step.title,
                    step.work_package_id,
                    step.status,
                    started,
                    completed,
                    step.depends_on.join(", ")
                ));
            }
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
    if !summary.execution_contract.parallel_actions.is_empty() {
        md.push_str("\nParallelizable Work Packages:\n");
        for action in &summary.execution_contract.parallel_actions {
            md.push_str(&format!("- {action}\n"));
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
        for evidence in &summary.evidence {
            md.push_str(&format!(
                "- `{}` {} ({}, {}, {})\n",
                evidence.id,
                evidence.claim,
                evidence.event_id,
                evidence.source_type,
                evidence.timestamp
            ));
        }
    }
    md.push('\n');

    md.push_str("## Conversations\n");
    if summary.key_conversations.is_empty() {
        md.push_str("_(none)_\n");
    } else {
        for (idx, conversation) in summary.key_conversations.iter().enumerate() {
            md.push_str(&format!(
                "### {}. User\n{}\n\n### {}. Agent\n{}\n\n",
                idx + 1,
                truncate_str(&conversation.user, 300),
                idx + 1,
                truncate_str(&conversation.agent, 300)
            ));
        }
    }

    md.push_str("## User Messages\n");
    if summary.user_messages.is_empty() {
        md.push_str("_(none)_\n");
    } else {
        for (idx, message) in summary.user_messages.iter().enumerate() {
            md.push_str(&format!("{}. {}\n", idx + 1, truncate_str(message, 150)));
        }
    }
}

/// Generate a Markdown handoff document from a single session summary.
pub fn generate_handoff_markdown(summary: &HandoffSummary) -> String {
    const MAX_TASK_SUMMARIES_DISPLAY: usize = 5;
    let mut md = String::new();

    md.push_str("# Session Handoff\n\n");

    md.push_str("## Objective\n");
    md.push_str(&summary.objective);
    md.push_str("\n\n");

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

    if !summary.files_modified.is_empty() {
        md.push_str("## Files Modified\n");
        for file_change in &summary.files_modified {
            md.push_str(&format!(
                "- `{}` ({})\n",
                file_change.path, file_change.action
            ));
        }
        md.push('\n');
    }

    if !summary.files_read.is_empty() {
        md.push_str("## Files Read\n");
        for path in &summary.files_read {
            md.push_str(&format!("- `{path}`\n"));
        }
        md.push('\n');
    }

    if !summary.shell_commands.is_empty() {
        md.push_str("## Shell Commands\n");
        for command in &summary.shell_commands {
            let code_str = match command.exit_code {
                Some(code) => code.to_string(),
                None => "?".to_string(),
            };
            md.push_str(&format!(
                "- `{}` → {}\n",
                truncate_str(&command.command, 80),
                code_str
            ));
        }
        md.push('\n');
    }

    if !summary.errors.is_empty() {
        md.push_str("## Errors\n");
        for error in &summary.errors {
            md.push_str(&format!("- {error}\n"));
        }
        md.push('\n');
    }

    if !summary.key_conversations.is_empty() {
        md.push_str("## Key Conversations\n");
        for (idx, conversation) in summary.key_conversations.iter().enumerate() {
            md.push_str(&format!(
                "### {}. User\n{}\n\n### {}. Agent\n{}\n\n",
                idx + 1,
                truncate_str(&conversation.user, 300),
                idx + 1,
                truncate_str(&conversation.agent, 300),
            ));
        }
    }

    if summary.key_conversations.is_empty() && !summary.user_messages.is_empty() {
        md.push_str("## User Messages\n");
        for (idx, message) in summary.user_messages.iter().enumerate() {
            md.push_str(&format!("{}. {}\n", idx + 1, truncate_str(message, 150)));
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

    for (idx, summary) in merged.summaries.iter().enumerate() {
        md.push_str(&format!(
            "---\n\n## Session {} — {}\n\n",
            idx + 1,
            summary.source_session_id
        ));
        md.push_str(&format!("**Objective:** {}\n\n", summary.objective));
        md.push_str(&format!(
            "- **Tool:** {} ({}) | **Duration:** {}\n",
            summary.tool,
            summary.model,
            format_duration(summary.duration_seconds)
        ));
        md.push_str(&format!(
            "- **Messages:** {} | Tool calls: {} | Events: {}\n\n",
            summary.stats.message_count, summary.stats.tool_call_count, summary.stats.event_count
        ));

        if !summary.task_summaries.is_empty() {
            md.push_str("### Task Summaries\n");
            for (task_idx, task_summary) in summary
                .task_summaries
                .iter()
                .take(MAX_TASK_SUMMARIES_DISPLAY)
                .enumerate()
            {
                md.push_str(&format!("{}. {}\n", task_idx + 1, task_summary));
            }
            if summary.task_summaries.len() > MAX_TASK_SUMMARIES_DISPLAY {
                md.push_str(&format!(
                    "- ... and {} more\n",
                    summary.task_summaries.len() - MAX_TASK_SUMMARIES_DISPLAY
                ));
            }
            md.push('\n');
        }

        if !summary.key_conversations.is_empty() {
            md.push_str("### Conversations\n");
            for (conversation_idx, conversation) in summary.key_conversations.iter().enumerate() {
                md.push_str(&format!(
                    "**{}. User:** {}\n\n**{}. Agent:** {}\n\n",
                    conversation_idx + 1,
                    truncate_str(&conversation.user, 200),
                    conversation_idx + 1,
                    truncate_str(&conversation.agent, 200),
                ));
            }
        }
    }

    md.push_str("---\n\n## All Files Modified\n");
    if merged.all_files_modified.is_empty() {
        md.push_str("_(none)_\n");
    } else {
        for file_change in &merged.all_files_modified {
            md.push_str(&format!(
                "- `{}` ({})\n",
                file_change.path, file_change.action
            ));
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

    if !merged.total_errors.is_empty() {
        md.push_str("## All Errors\n");
        for error in &merged.total_errors {
            md.push_str(&format!("- {error}\n"));
        }
        md.push('\n');
    }

    md
}
