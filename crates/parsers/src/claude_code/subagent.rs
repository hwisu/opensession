use super::parse::{parse_timestamp, process_assistant_entry, process_user_entry};
use super::raw::RawEntry;
use crate::common::ToolUseInfo;
use crate::common::set_first;
use anyhow::{Context, Result};
use chrono::Utc;
use opensession_core::trace::{Agent, Event, EventType, Session, SessionContext};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::io::BufRead;
use std::path::{Path, PathBuf};

fn is_subagent_file_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("agent-")
        || lower.starts_with("agent_")
        || lower.starts_with("subagent-")
        || lower.starts_with("subagent_")
}

fn collect_subagent_dirs(parent_path: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut seen = HashSet::new();
    let mut push_unique = |path: PathBuf| {
        if seen.insert(path.clone()) {
            dirs.push(path);
        }
    };

    push_unique(parent_path.with_extension("").join("subagents"));

    if let Some(parent_dir) = parent_path.parent() {
        push_unique(parent_dir.join("subagents"));
        push_unique(parent_dir.to_path_buf());
    }

    dirs
}

fn merge_subagent_session_ids_match(parent_session_id: &str, meta: &SubagentMeta) -> bool {
    meta.session_id
        .as_deref()
        .is_some_and(|id| id == parent_session_id)
        || meta
            .parent_session_id
            .as_deref()
            .is_some_and(|id| id == parent_session_id)
}

pub(super) fn merge_subagent_sessions(
    parent_path: &Path,
    parent_session_id: &str,
    session: &mut Session,
) {
    let mut subagent_files: Vec<_> = collect_subagent_dirs(parent_path)
        .into_iter()
        .filter(|dir| dir.is_dir())
        .flat_map(|dir| match std::fs::read_dir(dir) {
            Ok(entries) => entries
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
                .collect(),
            Err(_) => Vec::new(),
        })
        .collect();

    if subagent_files.is_empty() {
        return;
    }

    subagent_files.retain(|path| {
        if path == parent_path {
            return false;
        }

        let file_name = match path.file_name().and_then(|name| name.to_str()) {
            Some(name) => name,
            None => return false,
        };

        if file_name.starts_with('.') {
            return false;
        }

        let in_subagents_dir = path
            .parent()
            .and_then(|dir| dir.file_name())
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("subagents"));
        if in_subagents_dir && is_subagent_file_name(file_name) {
            return true;
        }

        let meta = read_subagent_meta(path);
        matches!(
            meta,
            Some(meta) if merge_subagent_session_ids_match(parent_session_id, &meta)
        )
    });

    subagent_files.sort();
    if subagent_files.is_empty() {
        return;
    }

    for subagent_path in subagent_files {
        let meta = read_subagent_meta(&subagent_path).unwrap_or(SubagentMeta {
            slug: None,
            agent_id: None,
            session_id: None,
            parent_session_id: None,
        });
        let file_agent_id = subagent_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("unknown")
            .to_string();

        let task_id = meta
            .agent_id
            .as_ref()
            .cloned()
            .unwrap_or_else(|| file_agent_id.clone());

        let sub_session = match parse_subagent_jsonl(&subagent_path) {
            Ok(session) => session,
            Err(error) => {
                tracing::warn!(
                    "Failed to parse subagent {}: {}",
                    subagent_path.display(),
                    error
                );
                continue;
            }
        };

        if sub_session.events.is_empty() {
            continue;
        }
        let task_title = meta
            .slug
            .as_ref()
            .cloned()
            .unwrap_or_else(|| task_id.clone());

        let sub_model = if sub_session.agent.model != "unknown" {
            Some(sub_session.agent.model.clone())
        } else {
            None
        };

        let start_ts = sub_session
            .events
            .first()
            .expect("subagent start")
            .timestamp;
        let end_ts = sub_session.events.last().expect("subagent end").timestamp;

        let mut start_attrs = HashMap::new();
        start_attrs.insert(
            "subagent_id".to_string(),
            serde_json::Value::String(task_id.clone()),
        );
        start_attrs.insert("merged_subagent".to_string(), serde_json::Value::Bool(true));
        if let Some(model) = sub_model.as_ref() {
            start_attrs.insert(
                "model".to_string(),
                serde_json::Value::String(model.clone()),
            );
        }

        session.events.push(Event {
            event_id: format!("{task_id}-start"),
            timestamp: start_ts,
            event_type: EventType::TaskStart {
                title: Some(task_title),
            },
            task_id: Some(task_id.clone()),
            content: opensession_core::trace::Content::text(""),
            duration_ms: None,
            attributes: start_attrs,
        });

        for mut event in sub_session.events {
            event.task_id = Some(task_id.clone());
            event.event_id = format!("{}:{}", task_id, event.event_id);
            event.attributes.insert(
                "subagent_id".to_string(),
                serde_json::Value::String(task_id.clone()),
            );
            event
                .attributes
                .insert("merged_subagent".to_string(), serde_json::Value::Bool(true));
            session.events.push(event);
        }

        let duration = (end_ts - start_ts).num_milliseconds().max(0) as u64;
        let mut end_attrs = HashMap::new();
        end_attrs.insert(
            "subagent_id".to_string(),
            serde_json::Value::String(task_id.clone()),
        );
        end_attrs.insert("merged_subagent".to_string(), serde_json::Value::Bool(true));
        session.events.push(Event {
            event_id: format!("{task_id}-end"),
            timestamp: end_ts,
            event_type: EventType::TaskEnd {
                summary: Some(format!(
                    "{} events, {}",
                    sub_session.stats.event_count, sub_session.agent.model
                )),
            },
            task_id: Some(task_id),
            content: opensession_core::trace::Content::text(""),
            duration_ms: Some(duration),
            attributes: end_attrs,
        });
    }

    session.events.sort_by_key(|event| event.timestamp);
}

#[derive(Debug)]
pub(super) struct SubagentMeta {
    pub(super) slug: Option<String>,
    pub(super) agent_id: Option<String>,
    pub(super) session_id: Option<String>,
    pub(super) parent_session_id: Option<String>,
}

pub(super) fn read_subagent_meta(path: &Path) -> Option<SubagentMeta> {
    let file = std::fs::File::open(path).ok()?;
    let mut reader = std::io::BufReader::new(file);
    let mut first_line = String::new();
    reader.read_line(&mut first_line).ok()?;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FirstLine {
        #[serde(default)]
        slug: Option<String>,
        #[serde(default)]
        agent_id: Option<String>,
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default, alias = "parentUuid", alias = "parentID", alias = "parentId")]
        parent_session_id: Option<String>,
    }

    let parsed: FirstLine = serde_json::from_str(&first_line).ok()?;
    Some(SubagentMeta {
        slug: parsed.slug,
        agent_id: parsed.agent_id,
        session_id: parsed.session_id,
        parent_session_id: parsed.parent_session_id,
    })
}

pub(super) fn parse_subagent_jsonl(path: &Path) -> Result<Session> {
    let meta = read_subagent_meta(path);
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open subagent JSONL: {}", path.display()))?;
    let reader = std::io::BufReader::new(file);

    let mut events: Vec<Event> = Vec::new();
    let mut model_name: Option<String> = None;
    let mut tool_version: Option<String> = None;
    let mut session_id: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut git_branch: Option<String> = None;
    let mut tool_use_info: HashMap<String, ToolUseInfo> = HashMap::new();

    for line_result in reader.lines() {
        let line = match line_result {
            Ok(line) => line,
            Err(_) => continue,
        };
        if line.trim().is_empty() {
            continue;
        }

        let entry: RawEntry = match serde_json::from_str(&line) {
            Ok(entry) => entry,
            Err(_) => continue,
        };

        match entry {
            RawEntry::FileHistorySnapshot {} | RawEntry::Unknown => continue,
            RawEntry::System(system) => {
                set_first(&mut session_id, system.session_id.clone());
                set_first(&mut tool_version, system.version.clone());
                set_first(&mut cwd, system.cwd.clone());
                set_first(&mut git_branch, system.git_branch.clone());
                events.push(super::parse::system_entry_to_event(&system, &events));
            }
            RawEntry::Progress(progress) => {
                set_first(&mut session_id, progress.session_id.clone());
                set_first(&mut tool_version, progress.version.clone());
                set_first(&mut cwd, progress.cwd.clone());
                set_first(&mut git_branch, progress.git_branch.clone());
                events.push(super::parse::progress_entry_to_event(&progress, &events));
            }
            RawEntry::QueueOperation(queue_op) => {
                set_first(&mut session_id, queue_op.session_id.clone());
                events.push(super::parse::queue_operation_entry_to_event(
                    &queue_op, &events,
                ));
            }
            RawEntry::Summary(summary) => {
                set_first(&mut session_id, summary.session_id.clone());
                events.push(super::parse::summary_entry_to_event(&summary, &events));
            }
            RawEntry::User(conv) => {
                set_first(&mut session_id, conv.session_id.clone());
                set_first(&mut tool_version, conv.version.clone());
                set_first(&mut cwd, conv.cwd.clone());
                set_first(&mut git_branch, conv.git_branch.clone());
                if let Ok(ts) = parse_timestamp(&conv.timestamp) {
                    process_user_entry(&conv, ts, &mut events, &tool_use_info);
                }
            }
            RawEntry::Assistant(conv) => {
                set_first(&mut session_id, conv.session_id.clone());
                set_first(&mut tool_version, conv.version.clone());
                set_first(&mut model_name, conv.message.model.clone());
                set_first(&mut git_branch, conv.git_branch.clone());
                if let Ok(ts) = parse_timestamp(&conv.timestamp) {
                    process_assistant_entry(&conv, ts, &mut events, &mut tool_use_info);
                }
            }
        }
    }

    let session_id = session_id.unwrap_or_else(|| {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("unknown")
            .to_string()
    });

    let agent = Agent {
        provider: "anthropic".to_string(),
        model: model_name.unwrap_or_else(|| "unknown".to_string()),
        tool: "claude-code".to_string(),
        tool_version,
    };

    let (created_at, updated_at) =
        if let (Some(first), Some(last)) = (events.first(), events.last()) {
            (first.timestamp, last.timestamp)
        } else {
            let now = Utc::now();
            (now, now)
        };

    let parent_session_id = meta
        .as_ref()
        .and_then(|value| value.parent_session_id.clone())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let mut attributes = HashMap::from([(
        "source_path".to_string(),
        serde_json::Value::String(path.to_string_lossy().to_string()),
    )]);
    attributes.insert(
        "session_role".to_string(),
        serde_json::Value::String(if parent_session_id.is_some() {
            "auxiliary".to_string()
        } else {
            "primary".to_string()
        }),
    );
    if let Some(parent_session_id) = parent_session_id.as_ref() {
        attributes.insert(
            "parent_session_id".to_string(),
            serde_json::Value::String(parent_session_id.clone()),
        );
    }
    if let Some(branch) = git_branch.as_ref() {
        attributes.insert(
            "git_branch".to_string(),
            serde_json::Value::String(branch.clone()),
        );
    }

    let context = SessionContext {
        title: None,
        description: None,
        tags: vec!["claude-code".to_string()],
        created_at,
        updated_at,
        related_session_ids: parent_session_id.clone().into_iter().collect(),
        attributes,
    };

    let mut session = Session::new(session_id, agent);
    session.context = context;
    session.events = events;
    session.recompute_stats();
    Ok(session)
}
