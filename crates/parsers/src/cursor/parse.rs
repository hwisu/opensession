use super::time::parse_timestamp;
use super::transform::{
    classify_cursor_tool, extract_model_from_signature, infer_provider, parse_tool_result,
    resolve_tool_name, tool_call_content,
};
use super::types::{RawBubble, RawComposerData, RawComposerIndex, RawComposerMeta};
#[cfg(test)]
use super::types::{RawBubbleHeader, RawThinking, RawToolFormerData};
use crate::common::{attach_semantic_attrs, attach_source_attrs, infer_tool_kind};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use opensession_core::trace::{Agent, Content, Event, EventType, Session, SessionContext};
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

// ── Core parsing logic ─────────────────────────────────────────────────────

pub(super) fn parse_cursor_vscdb(path: &Path) -> Result<Session> {
    let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("Failed to open Cursor state.vscdb: {}", path.display()))?;

    // Legacy Cursor format stores full conversation JSON under composerData:*.
    let mut conversations = read_composer_data(&conn)?;

    // Modern workspace DBs may only expose metadata index (`composer.composerData`).
    // Resolve those IDs from companion globalStorage DB when needed.
    let composer_meta = read_composer_index_entries(&conn)?;
    if !composer_meta.is_empty() {
        let composer_ids: HashSet<String> = composer_meta
            .iter()
            .map(|meta| meta.composer_id.clone())
            .collect();
        if !composer_ids.is_empty() {
            let extra = read_composer_data_from_companion_global(path, &composer_ids)?;
            if !extra.is_empty() {
                merge_missing_composer_data(&mut conversations, extra);
            }
        }
        hydrate_conversation_meta(&mut conversations, &composer_meta);
    }

    if conversations.is_empty() {
        anyhow::bail!("No composer conversations found in {}", path.display());
    }

    // Pick the best conversation: most recent by lastUpdatedAt, breaking ties
    // with the largest conversation array length
    let best = conversations
        .iter()
        .max_by(|a, b| {
            let ts_a = a
                .last_updated_at
                .as_deref()
                .or(a.created_at.as_deref())
                .unwrap_or("");
            let ts_b = b
                .last_updated_at
                .as_deref()
                .or(b.created_at.as_deref())
                .unwrap_or("");
            ts_a.cmp(ts_b)
                .then_with(|| a.conversation.len().cmp(&b.conversation.len()))
        })
        .unwrap(); // safe: we checked conversations is not empty

    convert_conversation_to_session(best, path)
}

fn merge_missing_composer_data(target: &mut Vec<RawComposerData>, extra: Vec<RawComposerData>) {
    let mut seen: HashSet<String> = target
        .iter()
        .map(|entry| entry.composer_id.clone())
        .collect();
    for entry in extra {
        if seen.insert(entry.composer_id.clone()) {
            target.push(entry);
        }
    }
}

fn hydrate_conversation_meta(conversations: &mut [RawComposerData], meta: &[RawComposerMeta]) {
    let index: HashMap<&str, &RawComposerMeta> = meta
        .iter()
        .map(|entry| (entry.composer_id.as_str(), entry))
        .collect();

    for conversation in conversations {
        if let Some(meta) = index.get(conversation.composer_id.as_str()) {
            if conversation.name.is_none() {
                conversation.name = meta.name.clone();
            }
            if conversation.created_at.is_none() {
                conversation.created_at = meta.created_at.clone();
            }
            if conversation.last_updated_at.is_none() {
                conversation.last_updated_at = meta.last_updated_at.clone();
            }
        }
    }
}

fn read_composer_data_from_companion_global(
    workspace_db: &Path,
    composer_ids: &HashSet<String>,
) -> Result<Vec<RawComposerData>> {
    let Some(global_db) = companion_global_db_path(workspace_db) else {
        return Ok(Vec::new());
    };
    if !global_db.exists() || global_db == workspace_db {
        return Ok(Vec::new());
    }

    let conn = Connection::open_with_flags(&global_db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("Failed to open Cursor global DB: {}", global_db.display()))?;
    let all = read_composer_data(&conn)?;
    Ok(all
        .into_iter()
        .filter(|entry| composer_ids.contains(&entry.composer_id))
        .collect())
}

fn companion_global_db_path(path: &Path) -> Option<PathBuf> {
    let workspace_hash_dir = path.parent()?;
    let workspace_storage_dir = workspace_hash_dir.parent()?;
    let is_workspace_storage = workspace_storage_dir
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("workspaceStorage"));
    if !is_workspace_storage {
        return None;
    }
    let user_dir = workspace_storage_dir.parent()?;
    Some(user_dir.join("globalStorage").join("state.vscdb"))
}

fn read_composer_index_entries(conn: &Connection) -> Result<Vec<RawComposerMeta>> {
    let mut merged = Vec::new();
    if table_exists(conn, "cursorDiskKV") {
        merged.extend(read_composer_index_from_table(conn, "cursorDiskKV")?);
    }
    if table_exists(conn, "ItemTable") {
        merged.extend(read_composer_index_from_table(conn, "ItemTable")?);
    }

    let mut seen = HashSet::new();
    merged.retain(|entry| seen.insert(entry.composer_id.clone()));
    Ok(merged)
}

fn read_composer_index_from_table(conn: &Connection, table: &str) -> Result<Vec<RawComposerMeta>> {
    let sql = format!("SELECT value FROM {table} WHERE key = 'composer.composerData' LIMIT 1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    let Some(row) = rows.next()? else {
        return Ok(Vec::new());
    };

    let value_ref = row.get_ref(0)?;
    let raw = match value_ref {
        rusqlite::types::ValueRef::Text(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        rusqlite::types::ValueRef::Blob(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        _ => String::new(),
    };
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }

    match serde_json::from_str::<RawComposerIndex>(&raw) {
        Ok(parsed) => Ok(parsed.all_composers),
        Err(err) => {
            tracing::debug!(
                "Skipping unparseable composer.composerData payload: {}",
                err
            );
            Ok(Vec::new())
        }
    }
}

fn table_exists(conn: &Connection, table: &str) -> bool {
    conn.query_row(
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
        [table],
        |row| row.get(0),
    )
    .unwrap_or(false)
}

fn read_composer_data(conn: &Connection) -> Result<Vec<RawComposerData>> {
    if !table_exists(conn, "cursorDiskKV") {
        // Try ItemTable which is another known Cursor DB layout
        if table_exists(conn, "ItemTable") {
            return read_composer_data_from_item_table(conn);
        }

        anyhow::bail!("No cursorDiskKV or ItemTable table found in database");
    }

    // Read both composerData:* and bubbleId:* entries for v3 support
    let mut stmt = conn.prepare(
        "SELECT key, value FROM cursorDiskKV WHERE key LIKE 'composerData:%' OR key LIKE 'bubbleId:%'",
    )?;

    let mut composer_entries = Vec::new();
    let mut bubble_map: HashMap<String, String> = HashMap::new();

    let rows = stmt.query_map([], |row| {
        let key: String = row.get(0)?;
        // Cursor stores values as TEXT in some versions and BLOB in others.
        // Use get_ref to handle both storage types gracefully.
        let value_ref = row.get_ref(1)?;
        let value = match value_ref {
            rusqlite::types::ValueRef::Text(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            rusqlite::types::ValueRef::Blob(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            _ => String::new(),
        };
        Ok((key, value))
    })?;

    for row_result in rows {
        let (key, value) = row_result?;
        if key.starts_with("bubbleId:") {
            bubble_map.insert(key, value);
        } else {
            composer_entries.push((key, value));
        }
    }

    let mut conversations = Vec::new();
    for (key, value) in composer_entries {
        match serde_json::from_str::<RawComposerData>(&value) {
            Ok(mut data) => {
                resolve_v3_conversation(&mut data, &bubble_map);
                if !data.conversation.is_empty() {
                    conversations.push(data);
                }
            }
            Err(e) => {
                tracing::debug!("Skipping unparseable composerData entry {}: {}", key, e);
            }
        }
    }

    if conversations.is_empty() && table_exists(conn, "ItemTable") {
        return read_composer_data_from_item_table(conn);
    }

    Ok(conversations)
}

/// Alternative: some Cursor versions use ItemTable with (key, value) columns
fn read_composer_data_from_item_table(conn: &Connection) -> Result<Vec<RawComposerData>> {
    // Read both composerData:* and bubbleId:* entries for v3 support
    let mut stmt = conn.prepare(
        "SELECT key, value FROM ItemTable WHERE key LIKE 'composerData:%' OR key LIKE 'bubbleId:%'",
    )?;

    let mut composer_entries = Vec::new();
    let mut bubble_map: HashMap<String, String> = HashMap::new();

    let rows = stmt.query_map([], |row| {
        let key: String = row.get(0)?;
        let value: String = row.get(1)?;
        Ok((key, value))
    })?;

    for row_result in rows {
        let (key, value) = row_result?;
        if key.starts_with("bubbleId:") {
            bubble_map.insert(key, value);
        } else {
            composer_entries.push((key, value));
        }
    }

    let mut conversations = Vec::new();
    for (key, value) in composer_entries {
        match serde_json::from_str::<RawComposerData>(&value) {
            Ok(mut data) => {
                resolve_v3_conversation(&mut data, &bubble_map);
                if !data.conversation.is_empty() {
                    conversations.push(data);
                }
            }
            Err(e) => {
                tracing::debug!("Skipping unparseable composerData entry {}: {}", key, e);
            }
        }
    }

    Ok(conversations)
}

/// Resolve v3 conversation: when `_v >= 3`, the conversation is stored in separate
/// `bubbleId:<composerId>:<bubbleId>` keys instead of inline in composerData.
fn resolve_v3_conversation(data: &mut RawComposerData, bubble_map: &HashMap<String, String>) {
    if data.version.unwrap_or(0) < 3 {
        return;
    }
    let headers = match &data.full_conversation_headers_only {
        Some(h) if !h.is_empty() => h,
        _ => return,
    };
    let mut bubbles = Vec::with_capacity(headers.len());
    for header in headers {
        let key = format!("bubbleId:{}:{}", data.composer_id, header.bubble_id);
        if let Some(json) = bubble_map.get(&key) {
            match serde_json::from_str::<RawBubble>(json) {
                Ok(bubble) => bubbles.push(bubble),
                Err(e) => {
                    tracing::debug!("Failed to parse bubble {}: {}", key, e);
                }
            }
        } else {
            tracing::debug!("Bubble key not found in DB: {}", key);
        }
    }
    data.conversation = bubbles;
}

fn convert_conversation_to_session(data: &RawComposerData, source_path: &Path) -> Result<Session> {
    let session_id = data.composer_id.clone();

    // Determine timestamps
    let created_at = data
        .created_at
        .as_deref()
        .and_then(|s| parse_timestamp(s).ok())
        .unwrap_or_else(Utc::now);

    let updated_at = data
        .last_updated_at
        .as_deref()
        .and_then(|s| parse_timestamp(s).ok())
        .unwrap_or(created_at);

    // Extract model info from the first assistant bubble that has model_type
    // or from thinking.signature
    let model_name = data
        .conversation
        .iter()
        .find_map(|b| b.model_type.clone())
        .or_else(|| {
            data.conversation.iter().find_map(|b| {
                b.thinking
                    .as_ref()
                    .and_then(|t| t.signature.as_ref())
                    .and_then(|sig| extract_model_from_signature(sig))
            })
        })
        .unwrap_or_else(|| "unknown".to_string());

    let agent = Agent {
        provider: infer_provider(&model_name),
        model: model_name,
        tool: "cursor".to_string(),
        tool_version: None,
    };

    let mut attributes = HashMap::new();
    attributes.insert(
        "source".to_string(),
        serde_json::Value::String(source_path.display().to_string()),
    );
    if let Some(is_agentic) = data.is_agentic {
        attributes.insert(
            "is_agentic".to_string(),
            serde_json::Value::Bool(is_agentic),
        );
    }

    let context = SessionContext {
        title: data.name.clone(),
        description: None,
        tags: vec!["cursor".to_string()],
        created_at,
        updated_at,
        related_session_ids: Vec::new(),
        attributes,
    };

    let schema_version = if data.version.unwrap_or(0) >= 3 {
        "cursor-v3"
    } else {
        "cursor-v2"
    };
    let events = convert_bubbles_to_events(&data.conversation, created_at, schema_version);

    let mut session = Session::new(session_id, agent);
    session.context = context;
    session.events = events;
    session.recompute_stats();

    Ok(session)
}

fn convert_bubbles_to_events(
    bubbles: &[RawBubble],
    base_ts: DateTime<Utc>,
    schema_version: &str,
) -> Vec<Event> {
    let mut events = Vec::new();
    let mut event_counter: u32 = 0;

    for bubble in bubbles {
        let bubble_id = bubble
            .bubble_id
            .clone()
            .unwrap_or_else(|| format!("bubble-{}", event_counter));

        // Derive timestamp from timingInfo if available, otherwise increment from base
        let ts = bubble
            .timing_info
            .as_ref()
            .and_then(|ti| {
                ti.client_start_time
                    .or(ti.start_time)
                    .and_then(|ms| DateTime::from_timestamp_millis(ms as i64))
            })
            .unwrap_or_else(|| {
                base_ts + chrono::Duration::milliseconds(event_counter as i64 * 100)
            });

        // Compute duration from timing info
        let duration_ms = bubble.timing_info.as_ref().and_then(|ti| {
            let start = ti.client_start_time.or(ti.start_time)?;
            let end = ti.client_end_time.or(ti.end_time)?;
            let d = end - start;
            if d > 0.0 { Some(d as u64) } else { None }
        });

        match bubble.bubble_type {
            // type=1 → User message
            1 => {
                if let Some(text) = &bubble.text {
                    let cleaned = text.trim();
                    if !cleaned.is_empty() {
                        let mut attrs = HashMap::new();
                        attach_source_attrs(
                            &mut attrs,
                            Some(schema_version),
                            Some("bubble:user_message"),
                        );
                        attach_semantic_attrs(&mut attrs, Some(&bubble_id), None, None);
                        events.push(Event {
                            event_id: format!("{}-user", bubble_id),
                            timestamp: ts,
                            event_type: EventType::UserMessage,
                            task_id: None,
                            content: Content::text(cleaned),
                            duration_ms: None,
                            attributes: attrs,
                        });
                    }
                }
                event_counter += 1;
            }

            // type=2 → Assistant message, thinking, or tool call
            2 => {
                // Handle thinking block
                if let Some(thinking) = &bubble.thinking {
                    if let Some(text) = &thinking.text {
                        let cleaned = text.trim();
                        if !cleaned.is_empty() {
                            let mut attrs = HashMap::new();
                            attach_source_attrs(
                                &mut attrs,
                                Some(schema_version),
                                Some("bubble:thinking"),
                            );
                            attach_semantic_attrs(&mut attrs, Some(&bubble_id), None, None);
                            if let Some(sig) = &thinking.signature {
                                attrs.insert(
                                    "signature".to_string(),
                                    serde_json::Value::String(sig.clone()),
                                );
                            }
                            events.push(Event {
                                event_id: format!("{}-thinking", bubble_id),
                                timestamp: ts,
                                event_type: EventType::Thinking,
                                task_id: None,
                                content: Content::text(cleaned),
                                duration_ms: None,
                                attributes: attrs,
                            });
                        }
                    }
                }

                // Handle tool call (toolFormerData)
                if let Some(tool_data) = &bubble.tool_former_data {
                    let tool_name = resolve_tool_name(tool_data.tool, tool_data.name.as_deref());
                    let tool_kind = infer_tool_kind(&tool_name);
                    let task_id = format!("cursor-task-{}", bubble_id);
                    let task_title = tool_data
                        .name
                        .as_ref()
                        .filter(|name| !name.trim().is_empty())
                        .cloned()
                        .or_else(|| Some(tool_name.clone()));

                    // Parse rawArgs as JSON for structured tool info
                    let args: serde_json::Value = tool_data
                        .raw_args
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .and_then(|raw| {
                            serde_json::from_str(raw)
                                .ok()
                                .or_else(|| Some(serde_json::json!({ "raw": raw })))
                        })
                        .unwrap_or(serde_json::Value::Null);

                    let event_type = classify_cursor_tool(&tool_name, &args);
                    let content = tool_call_content(&tool_name, &args);

                    let mut attrs = HashMap::new();
                    attach_source_attrs(&mut attrs, Some(schema_version), Some("bubble:tool"));
                    let call_event_id = format!("{}-call", bubble_id);
                    attach_semantic_attrs(
                        &mut attrs,
                        Some(&bubble_id),
                        Some(&call_event_id),
                        Some(tool_kind),
                    );
                    if let Some(status) = &tool_data.status {
                        attrs.insert(
                            "status".to_string(),
                            serde_json::Value::String(status.clone()),
                        );
                    }
                    if let Some(decision) = &tool_data.user_decision {
                        attrs.insert(
                            "user_decision".to_string(),
                            serde_json::Value::String(decision.clone()),
                        );
                    }

                    events.push(Event {
                        event_id: format!("{}-task-start", bubble_id),
                        timestamp: ts,
                        event_type: EventType::TaskStart { title: task_title },
                        task_id: Some(task_id.clone()),
                        content: Content::empty(),
                        duration_ms: None,
                        attributes: {
                            let mut task_attrs = attrs.clone();
                            attach_source_attrs(
                                &mut task_attrs,
                                Some(schema_version),
                                Some("bubble:tool-task-start"),
                            );
                            task_attrs
                        },
                    });

                    // Emit ToolCall event
                    events.push(Event {
                        event_id: call_event_id.clone(),
                        timestamp: ts,
                        event_type,
                        task_id: Some(task_id.clone()),
                        content,
                        duration_ms,
                        attributes: attrs.clone(),
                    });

                    let is_terminal_status = tool_data
                        .status
                        .as_deref()
                        .is_some_and(|s| matches!(s, "completed" | "error" | "failed" | "done"));
                    let result_source = tool_data
                        .result
                        .as_deref()
                        .or(bubble.text.as_deref())
                        .filter(|text| !text.trim().is_empty());
                    if result_source.is_some() || is_terminal_status {
                        let result_content = result_source
                            .map(|text| parse_tool_result(&tool_name, text))
                            .unwrap_or_else(Content::empty);
                        let is_error = tool_data
                            .status
                            .as_deref()
                            .is_some_and(|s| s == "error" || s == "failed");
                        let mut result_attrs = attrs.clone();
                        attach_source_attrs(
                            &mut result_attrs,
                            Some(schema_version),
                            Some("bubble:tool-result"),
                        );
                        attach_semantic_attrs(
                            &mut result_attrs,
                            Some(&bubble_id),
                            Some(&call_event_id),
                            Some(tool_kind),
                        );
                        events.push(Event {
                            event_id: format!("{}-result", bubble_id),
                            timestamp: ts,
                            event_type: EventType::ToolResult {
                                name: tool_name.clone(),
                                is_error,
                                call_id: Some(call_event_id.clone()),
                            },
                            task_id: Some(task_id.clone()),
                            content: result_content,
                            duration_ms: None,
                            attributes: result_attrs,
                        });
                    }

                    let task_summary = tool_data
                        .status
                        .as_ref()
                        .filter(|status| !status.trim().is_empty())
                        .map(|status| format!("{tool_name} {status}"))
                        .or_else(|| Some(format!("{tool_name} finished")));
                    events.push(Event {
                        event_id: format!("{}-task-end", bubble_id),
                        timestamp: ts,
                        event_type: EventType::TaskEnd {
                            summary: task_summary,
                        },
                        task_id: Some(task_id),
                        content: Content::empty(),
                        duration_ms: None,
                        attributes: {
                            let mut task_end_attrs = attrs.clone();
                            attach_source_attrs(
                                &mut task_end_attrs,
                                Some(schema_version),
                                Some("bubble:tool-task-end"),
                            );
                            task_end_attrs
                        },
                    });

                    event_counter += 1;
                    continue; // toolFormerData bubbles don't have text content
                }

                // Handle regular assistant text
                if let Some(text) = &bubble.text {
                    let cleaned = text.trim();
                    if !cleaned.is_empty() {
                        let mut attrs = HashMap::new();
                        attach_source_attrs(
                            &mut attrs,
                            Some(schema_version),
                            Some("bubble:assistant_message"),
                        );
                        attach_semantic_attrs(&mut attrs, Some(&bubble_id), None, None);
                        if let Some(model) = &bubble.model_type {
                            attrs.insert(
                                "model".to_string(),
                                serde_json::Value::String(model.clone()),
                            );
                        }
                        events.push(Event {
                            event_id: format!("{}-agent", bubble_id),
                            timestamp: ts,
                            event_type: EventType::AgentMessage,
                            task_id: None,
                            content: Content::text(cleaned),
                            duration_ms,
                            attributes: attrs,
                        });
                    }
                }

                event_counter += 1;
            }

            // Unknown bubble type - skip
            _ => {
                tracing::debug!("Skipping unknown bubble type: {}", bubble.bubble_type);
                event_counter += 1;
            }
        }
    }

    events
}

// ── Timestamp parsing ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Datelike;

    #[test]
    fn test_can_parse_vscdb() {
        let parser = super::super::CursorParser;
        use crate::SessionParser;
        assert!(parser.can_parse(Path::new("/tmp/state.vscdb")));
        assert!(parser.can_parse(Path::new("state.vscdb")));
        assert!(!parser.can_parse(Path::new("state.db")));
        assert!(!parser.can_parse(Path::new("state.jsonl")));
    }

    #[test]
    fn test_parse_timestamp_iso() {
        let ts = parse_timestamp("2025-10-03T12:34:56.789Z").unwrap();
        assert_eq!(ts.year(), 2025);
    }

    #[test]
    fn test_parse_timestamp_epoch() {
        let ts = parse_timestamp("1696339200000").unwrap();
        // Should parse as a valid DateTime
        assert!(ts.year() >= 2023);
    }

    #[test]
    fn test_convert_bubbles_user_message() {
        let bubbles = vec![RawBubble {
            bubble_type: 1,
            bubble_id: Some("b1".to_string()),
            text: Some("Hello, help me with this".to_string()),
            thinking: None,
            tool_former_data: None,
            timing_info: None,
            model_type: None,
            checkpoint: None,
        }];
        let events = convert_bubbles_to_events(&bubbles, Utc::now(), "cursor-test-v2");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].event_type, EventType::UserMessage));
    }

    #[test]
    fn test_convert_bubbles_agent_message() {
        let bubbles = vec![RawBubble {
            bubble_type: 2,
            bubble_id: Some("b2".to_string()),
            text: Some("Here is my response".to_string()),
            thinking: None,
            tool_former_data: None,
            timing_info: None,
            model_type: Some("claude-3.5-sonnet".to_string()),
            checkpoint: None,
        }];
        let events = convert_bubbles_to_events(&bubbles, Utc::now(), "cursor-test-v2");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].event_type, EventType::AgentMessage));
    }

    #[test]
    fn test_convert_bubbles_thinking() {
        let bubbles = vec![RawBubble {
            bubble_type: 2,
            bubble_id: Some("b3".to_string()),
            text: None,
            thinking: Some(RawThinking {
                text: Some("Let me think about this...".to_string()),
                signature: Some("claude-sonnet-sig".to_string()),
            }),
            tool_former_data: None,
            timing_info: None,
            model_type: None,
            checkpoint: None,
        }];
        let events = convert_bubbles_to_events(&bubbles, Utc::now(), "cursor-test-v2");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].event_type, EventType::Thinking));
    }

    #[test]
    fn test_convert_bubbles_tool_call_with_result() {
        let bubbles = vec![RawBubble {
            bubble_type: 2,
            bubble_id: Some("b4".to_string()),
            text: None,
            thinking: None,
            tool_former_data: Some(RawToolFormerData {
                tool: Some(7),
                name: Some("edit_file".to_string()),
                status: Some("completed".to_string()),
                raw_args: Some(
                    r#"{"target_file":"/tmp/test.rs","code_edit":"fn main() {}"}"#.to_string(),
                ),
                result: Some(r#"{"diff":{"added":1},"isApplied":true}"#.to_string()),
                user_decision: Some("accepted".to_string()),
            }),
            timing_info: None,
            model_type: None,
            checkpoint: None,
        }];
        let events = convert_bubbles_to_events(&bubbles, Utc::now(), "cursor-test-v2");
        assert_eq!(events.len(), 4); // TaskStart + ToolCall + ToolResult + TaskEnd
        assert!(matches!(events[0].event_type, EventType::TaskStart { .. }));
        assert!(matches!(events[1].event_type, EventType::FileEdit { .. }));
        assert!(matches!(events[2].event_type, EventType::ToolResult { .. }));
        assert!(matches!(events[3].event_type, EventType::TaskEnd { .. }));
    }

    #[test]
    fn test_convert_bubbles_tool_call_terminal_without_result_still_emits_tool_result() {
        let bubbles = vec![RawBubble {
            bubble_type: 2,
            bubble_id: Some("b4b".to_string()),
            text: Some("fallback output".to_string()),
            thinking: None,
            tool_former_data: Some(RawToolFormerData {
                tool: Some(15),
                name: Some("run_terminal_cmd".to_string()),
                status: Some("completed".to_string()),
                raw_args: Some(r#"{"command":"echo hi"}"#.to_string()),
                result: None,
                user_decision: None,
            }),
            timing_info: None,
            model_type: None,
            checkpoint: None,
        }];
        let events = convert_bubbles_to_events(&bubbles, Utc::now(), "cursor-test-v3");
        assert!(events.iter().any(|event| matches!(
            event.event_type,
            EventType::ToolCall { .. } | EventType::ShellCommand { .. }
        )));
        assert!(
            events
                .iter()
                .any(|event| matches!(event.event_type, EventType::ToolResult { .. }))
        );
    }

    #[test]
    fn test_resolve_v3_conversation() {
        // Simulate v3 composerData with headers but no inline conversation
        let mut data = RawComposerData {
            composer_id: "comp-1".to_string(),
            name: Some("Test".to_string()),
            created_at: Some("2025-10-03T12:00:00.000Z".to_string()),
            last_updated_at: Some("2025-10-03T12:01:00.000Z".to_string()),
            conversation: vec![],
            is_agentic: Some(true),
            version: Some(3),
            full_conversation_headers_only: Some(vec![
                RawBubbleHeader {
                    bubble_id: "b1".to_string(),
                    bubble_type: 1,
                },
                RawBubbleHeader {
                    bubble_id: "b2".to_string(),
                    bubble_type: 2,
                },
            ]),
        };

        // Build bubble map with the separate bubble entries
        let mut bubble_map = HashMap::new();
        bubble_map.insert(
            "bubbleId:comp-1:b1".to_string(),
            serde_json::json!({
                "type": 1,
                "bubbleId": "b1",
                "text": "Hello from user"
            })
            .to_string(),
        );
        bubble_map.insert(
            "bubbleId:comp-1:b2".to_string(),
            serde_json::json!({
                "type": 2,
                "bubbleId": "b2",
                "text": "Hello from assistant",
                "modelType": "gpt-4"
            })
            .to_string(),
        );

        resolve_v3_conversation(&mut data, &bubble_map);

        assert_eq!(data.conversation.len(), 2);
        assert_eq!(data.conversation[0].bubble_type, 1);
        assert_eq!(
            data.conversation[0].text.as_deref(),
            Some("Hello from user")
        );
        assert_eq!(data.conversation[1].bubble_type, 2);
        assert_eq!(
            data.conversation[1].text.as_deref(),
            Some("Hello from assistant")
        );
    }

    #[test]
    fn test_resolve_v3_skips_old_versions() {
        let mut data = RawComposerData {
            composer_id: "comp-2".to_string(),
            name: None,
            created_at: None,
            last_updated_at: None,
            conversation: vec![RawBubble {
                bubble_type: 1,
                bubble_id: Some("b1".to_string()),
                text: Some("existing".to_string()),
                thinking: None,
                tool_former_data: None,
                timing_info: None,
                model_type: None,
                checkpoint: None,
            }],
            is_agentic: None,
            version: None, // no version = old format
            full_conversation_headers_only: None,
        };

        let bubble_map = HashMap::new();
        resolve_v3_conversation(&mut data, &bubble_map);

        // Should not modify existing conversation
        assert_eq!(data.conversation.len(), 1);
        assert_eq!(data.conversation[0].text.as_deref(), Some("existing"));
    }

    #[test]
    fn test_convert_bubbles_thinking_plus_text() {
        let bubbles = vec![RawBubble {
            bubble_type: 2,
            bubble_id: Some("b5".to_string()),
            text: Some("Here's what I found".to_string()),
            thinking: Some(RawThinking {
                text: Some("Analyzing the code...".to_string()),
                signature: None,
            }),
            tool_former_data: None,
            timing_info: None,
            model_type: None,
            checkpoint: None,
        }];
        let events = convert_bubbles_to_events(&bubbles, Utc::now(), "cursor-test-v2");
        assert_eq!(events.len(), 2); // Thinking + AgentMessage
        assert!(matches!(events[0].event_type, EventType::Thinking));
        assert!(matches!(events[1].event_type, EventType::AgentMessage));
    }

    #[test]
    fn test_companion_global_db_path_for_workspace_db() {
        let workspace_db = Path::new(
            "/Users/test/Library/Application Support/Cursor/User/workspaceStorage/abc/state.vscdb",
        );
        let global = companion_global_db_path(workspace_db).expect("global path");
        assert_eq!(
            global,
            PathBuf::from(
                "/Users/test/Library/Application Support/Cursor/User/globalStorage/state.vscdb"
            )
        );
    }

    #[test]
    fn test_hydrate_conversation_meta_fills_missing_fields() {
        let mut conversations = vec![RawComposerData {
            composer_id: "comp-1".to_string(),
            name: None,
            created_at: None,
            last_updated_at: None,
            conversation: vec![RawBubble {
                bubble_type: 1,
                bubble_id: Some("b1".to_string()),
                text: Some("hello".to_string()),
                thinking: None,
                tool_former_data: None,
                timing_info: None,
                model_type: None,
                checkpoint: None,
            }],
            is_agentic: None,
            version: None,
            full_conversation_headers_only: None,
        }];

        let meta = vec![RawComposerMeta {
            composer_id: "comp-1".to_string(),
            name: Some("Title".to_string()),
            created_at: Some("2026-02-14T12:00:00Z".to_string()),
            last_updated_at: Some("2026-02-14T13:00:00Z".to_string()),
        }];

        hydrate_conversation_meta(&mut conversations, &meta);

        assert_eq!(conversations[0].name.as_deref(), Some("Title"));
        assert_eq!(
            conversations[0].created_at.as_deref(),
            Some("2026-02-14T12:00:00Z")
        );
        assert_eq!(
            conversations[0].last_updated_at.as_deref(),
            Some("2026-02-14T13:00:00Z")
        );
    }
}
