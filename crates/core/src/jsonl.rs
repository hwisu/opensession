//! ACP semantic JSONL persistence with legacy HAIL v1 read compatibility.
//!
//! Canonical persisted format:
//! ```jsonl
//! {"type":"session.new","sessionId":"...","cwd":"/repo","_meta":{"opensession":{...}}}
//! {"type":"session.update","sessionId":"...","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"hi"}},"_meta":{"opensession":{"event":{...}}}}
//! {"type":"session.end","sessionId":"...","_meta":{"opensession":{"stats":{...}}}}
//! ```
//!
//! Legacy HAIL v1 `header/event/stats` JSONL remains readable via dual-read support.

use crate::session::ATTR_CWD;
use crate::trace::{
    Agent, Content, ContentBlock, Event, EventType, Session, SessionContext, Stats,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::io::{self, BufRead, Read, Write};

const TRACEPARENT_KEY: &str = "traceparent";
const TRACESTATE_KEY: &str = "tracestate";
const BAGGAGE_KEY: &str = "baggage";

const JOB_PROTOCOL_KEY: &str = "opensession.job.protocol";
const JOB_SYSTEM_KEY: &str = "opensession.job.system";
const JOB_ID_KEY: &str = "opensession.job.id";
const JOB_TITLE_KEY: &str = "opensession.job.title";
const JOB_RUN_ID_KEY: &str = "opensession.job.run_id";
const JOB_ATTEMPT_KEY: &str = "opensession.job.attempt";
const JOB_STAGE_KEY: &str = "opensession.job.stage";
const JOB_REVIEW_KIND_KEY: &str = "opensession.job.review_kind";
const JOB_STATUS_KEY: &str = "opensession.job.status";
const JOB_THREAD_ID_KEY: &str = "opensession.job.thread_id";
const JOB_ARTIFACTS_KEY: &str = "opensession.job.artifacts";
const REVIEW_NAMESPACE_PREFIX: &str = "opensession.review.";
const HANDOFF_NAMESPACE_PREFIX: &str = "opensession.handoff.";
const DIFF_NAMESPACE_PREFIX: &str = "opensession.diff.";

const MCP_SERVER_KEYS: [&str; 3] = ["mcpServers", "mcp_servers", "acp.session.mcp_servers"];

/// A single line in a legacy HAIL v1 JSONL file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum HailLine {
    #[serde(rename = "header")]
    Header {
        version: String,
        session_id: String,
        agent: Agent,
        context: SessionContext,
    },
    #[serde(rename = "event")]
    Event(Event),
    #[serde(rename = "stats")]
    Stats(Stats),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
enum AcpSemanticLine {
    #[serde(rename = "session.new")]
    New {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
        #[serde(rename = "mcpServers", skip_serializing_if = "Option::is_none")]
        mcp_servers: Option<Vec<Value>>,
        #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
        meta: Option<Value>,
    },
    #[serde(rename = "session.update")]
    Update {
        #[serde(rename = "sessionId")]
        session_id: String,
        update: AcpSessionUpdate,
        #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
        meta: Option<Value>,
    },
    #[serde(rename = "session.end")]
    End {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "stopReason", skip_serializing_if = "Option::is_none")]
        stop_reason: Option<String>,
        #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
        meta: Option<Value>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "sessionUpdate", rename_all = "snake_case")]
enum AcpSessionUpdate {
    UserMessageChunk {
        content: AcpContentBlock,
    },
    AgentMessageChunk {
        content: AcpContentBlock,
    },
    SystemMessageChunk {
        content: AcpContentBlock,
    },
    AgentThoughtChunk {
        content: AcpContentBlock,
    },
    ToolCall {
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        title: String,
        kind: AcpToolKind,
        status: AcpToolStatus,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        content: Vec<AcpToolCallContent>,
        #[serde(rename = "rawInput", skip_serializing_if = "Option::is_none")]
        raw_input: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        locations: Option<Vec<AcpLocation>>,
    },
    ToolCallUpdate {
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        status: AcpToolStatus,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        content: Vec<AcpToolCallContent>,
        #[serde(rename = "rawOutput", skip_serializing_if = "Option::is_none")]
        raw_output: Option<Value>,
    },
    SessionInfoUpdate {
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(rename = "updatedAt", skip_serializing_if = "Option::is_none")]
        updated_at: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AcpToolKind {
    Read,
    Edit,
    Delete,
    Move,
    Search,
    Execute,
    Think,
    Fetch,
    Other,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AcpToolStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AcpLocation {
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AcpContentBlock {
    Text {
        text: String,
    },
    ResourceLink {
        uri: String,
        #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AcpToolCallContent {
    Content {
        content: AcpContentBlock,
    },
    Diff {
        path: String,
        #[serde(rename = "oldText", skip_serializing_if = "Option::is_none")]
        old_text: Option<String>,
        #[serde(rename = "newText")]
        new_text: String,
    },
}

#[derive(Debug, Default)]
struct EventBuilder {
    event_id: String,
    timestamp: Option<DateTime<Utc>>,
    event_type: Option<EventType>,
    task_id: Option<String>,
    duration_ms: Option<u64>,
    attributes: Option<HashMap<String, Value>>,
    content: Option<Content>,
}

impl EventBuilder {
    fn finish(mut self) -> Event {
        Event {
            event_id: self.event_id,
            timestamp: self.timestamp.unwrap_or_else(Utc::now),
            event_type: self.event_type.unwrap_or_else(|| EventType::Custom {
                kind: "acp_update".to_string(),
            }),
            task_id: self.task_id.take(),
            content: self.content.unwrap_or_else(Content::empty),
            duration_ms: self.duration_ms,
            attributes: self.attributes.take().unwrap_or_default(),
        }
    }
}

/// Error types for JSONL operations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum JsonlError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("JSON error at line {line}: {source}")]
    Json {
        line: usize,
        source: serde_json::Error,
    },
    #[error("Missing header line")]
    MissingHeader,
    #[error("Unexpected line type at line {0}: expected header or session.new")]
    UnexpectedLineType(usize),
}

fn json_to_writer_line<W: Write, T: Serialize>(
    writer: &mut W,
    value: &T,
    line: usize,
) -> Result<(), JsonlError> {
    serde_json::to_writer(writer, value).map_err(|source| JsonlError::Json { line, source })
}

fn json_from_str_line<T: DeserializeOwned>(input: &str, line: usize) -> Result<T, JsonlError> {
    serde_json::from_str(input).map_err(|source| JsonlError::Json { line, source })
}

fn parse_line_type(input: &str, line: usize) -> Result<Option<String>, JsonlError> {
    let value: Value = json_from_str_line(input, line)?;
    Ok(value
        .get("type")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned))
}

fn object_from_hashmap(map: &HashMap<String, Value>) -> Map<String, Value> {
    map.iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn hashmap_from_object(value: &Value) -> Option<HashMap<String, Value>> {
    let object = value.as_object()?;
    Some(
        object
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    )
}

fn string_from_object(object: &Map<String, Value>, key: &str) -> Option<String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn insert_nested_object(root: &mut Map<String, Value>, path: &[&str], value: Value) {
    if path.is_empty() {
        return;
    }

    if path.len() == 1 {
        root.insert(path[0].to_string(), value);
        return;
    }

    let entry = root
        .entry(path[0].to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    if let Some(child) = entry.as_object_mut() {
        insert_nested_object(child, &path[1..], value);
    }
}

fn take_prefixed_namespace(attributes: &mut Map<String, Value>, prefix: &str) -> Option<Value> {
    let mut namespace = Map::new();
    let keys = attributes
        .keys()
        .filter(|key| key.starts_with(prefix))
        .cloned()
        .collect::<Vec<_>>();

    for key in keys {
        let Some(value) = attributes.remove(&key) else {
            continue;
        };
        let suffix = key.trim_start_matches(prefix);
        if suffix.is_empty() {
            continue;
        }
        let path = suffix
            .split('.')
            .map(str::trim)
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        if path.is_empty() {
            continue;
        }
        insert_nested_object(&mut namespace, &path, value);
    }

    if namespace.is_empty() {
        None
    } else {
        Some(Value::Object(namespace))
    }
}

fn restore_prefixed_namespace(
    value: &Map<String, Value>,
    prefix: &str,
    attributes: &mut HashMap<String, Value>,
) {
    fn flatten(
        prefix: &str,
        path: &mut Vec<String>,
        value: &Value,
        attributes: &mut HashMap<String, Value>,
    ) {
        if let Some(object) = value.as_object() {
            for (key, child) in object {
                path.push(key.clone());
                flatten(prefix, path, child, attributes);
                path.pop();
            }
            return;
        }

        if path.is_empty() {
            return;
        }
        attributes.insert(format!("{prefix}{}", path.join(".")), value.clone());
    }

    flatten(
        prefix,
        &mut Vec::new(),
        &Value::Object(value.clone()),
        attributes,
    );
}

fn datetime_from_object(object: &Map<String, Value>, key: &str) -> Option<DateTime<Utc>> {
    object
        .get(key)
        .and_then(Value::as_str)
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
}

fn json_array_of_strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn opensession_meta_object(meta: &Option<Value>) -> Option<&Map<String, Value>> {
    meta.as_ref()?.as_object()?.get("opensession")?.as_object()
}

fn job_meta_from_attributes(attributes: &mut Map<String, Value>) -> Option<Value> {
    let mut job = Map::new();
    let mut take = |key: &str, out_key: &str| {
        if let Some(value) = attributes.remove(key) {
            job.insert(out_key.to_string(), value);
        }
    };
    take(JOB_PROTOCOL_KEY, "protocol");
    take(JOB_SYSTEM_KEY, "system");
    take(JOB_ID_KEY, "jobId");
    take(JOB_TITLE_KEY, "jobTitle");
    take(JOB_RUN_ID_KEY, "runId");
    take(JOB_ATTEMPT_KEY, "attempt");
    take(JOB_STAGE_KEY, "stage");
    take(JOB_REVIEW_KIND_KEY, "reviewKind");
    take(JOB_STATUS_KEY, "status");
    take(JOB_THREAD_ID_KEY, "threadId");
    take(JOB_ARTIFACTS_KEY, "artifacts");

    if job.is_empty() {
        None
    } else {
        Some(Value::Object(job))
    }
}

fn restore_job_attributes(job: &Map<String, Value>, attributes: &mut HashMap<String, Value>) {
    let mut restore = |input_key: &str, output_key: &str| {
        if let Some(value) = job.get(input_key) {
            attributes.insert(output_key.to_string(), value.clone());
        }
    };
    restore("protocol", JOB_PROTOCOL_KEY);
    restore("system", JOB_SYSTEM_KEY);
    restore("jobId", JOB_ID_KEY);
    restore("jobTitle", JOB_TITLE_KEY);
    restore("runId", JOB_RUN_ID_KEY);
    restore("attempt", JOB_ATTEMPT_KEY);
    restore("stage", JOB_STAGE_KEY);
    restore("reviewKind", JOB_REVIEW_KIND_KEY);
    restore("status", JOB_STATUS_KEY);
    restore("threadId", JOB_THREAD_ID_KEY);
    restore("artifacts", JOB_ARTIFACTS_KEY);
}

fn take_string_attr(attributes: &mut Map<String, Value>, key: &str) -> Option<String> {
    attributes
        .remove(key)
        .and_then(|value| value.as_str().map(str::trim).map(ToOwned::to_owned))
        .filter(|value| !value.is_empty())
}

fn take_trace_meta(attributes: &mut Map<String, Value>, meta: &mut Map<String, Value>) {
    for key in [TRACEPARENT_KEY, TRACESTATE_KEY, BAGGAGE_KEY] {
        if let Some(value) = take_string_attr(attributes, key) {
            meta.insert(key.to_string(), Value::String(value));
        }
    }
}

fn restore_trace_meta(meta: &Option<Value>, attributes: &mut HashMap<String, Value>) {
    let Some(meta_object) = meta.as_ref().and_then(Value::as_object) else {
        return;
    };
    for key in [TRACEPARENT_KEY, TRACESTATE_KEY, BAGGAGE_KEY] {
        if let Some(value) = meta_object
            .get(key)
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
        {
            attributes.insert(key.to_string(), Value::String(value));
        }
    }
}

fn take_mcp_servers(attributes: &mut Map<String, Value>) -> Option<Vec<Value>> {
    for key in MCP_SERVER_KEYS {
        if let Some(Value::Array(items)) = attributes.remove(key) {
            return Some(items);
        }
    }
    None
}

fn hail_block_to_acp_block(block: &ContentBlock) -> AcpContentBlock {
    match block {
        ContentBlock::Text { text } => AcpContentBlock::Text { text: text.clone() },
        ContentBlock::Code { code, .. } => AcpContentBlock::Text { text: code.clone() },
        ContentBlock::Json { data } => AcpContentBlock::Text {
            text: serde_json::to_string_pretty(data).unwrap_or_else(|_| data.to_string()),
        },
        ContentBlock::Image { url, mime, alt } => AcpContentBlock::ResourceLink {
            uri: url.clone(),
            mime_type: Some(mime.clone()),
            title: alt.clone(),
        },
        ContentBlock::Video { url, mime }
        | ContentBlock::Audio { url, mime }
        | ContentBlock::Reference {
            uri: url,
            media_type: mime,
        } => AcpContentBlock::ResourceLink {
            uri: url.clone(),
            mime_type: Some(mime.clone()),
            title: None,
        },
        ContentBlock::File { path, content } => content
            .as_ref()
            .map(|text| AcpContentBlock::Text { text: text.clone() })
            .unwrap_or_else(|| AcpContentBlock::ResourceLink {
                uri: path.clone(),
                mime_type: None,
                title: Some(path.clone()),
            }),
    }
}

fn acp_block_to_hail_block(block: &AcpContentBlock) -> ContentBlock {
    match block {
        AcpContentBlock::Text { text } => ContentBlock::Text { text: text.clone() },
        AcpContentBlock::ResourceLink { uri, mime_type, .. } => ContentBlock::Reference {
            uri: uri.clone(),
            media_type: mime_type
                .clone()
                .unwrap_or_else(|| "application/octet-stream".to_string()),
        },
    }
}

fn tool_call_content_from_blocks(blocks: &[ContentBlock]) -> Vec<AcpToolCallContent> {
    blocks
        .iter()
        .map(|block| AcpToolCallContent::Content {
            content: hail_block_to_acp_block(block),
        })
        .collect()
}

fn fallback_content_from_update(update: &AcpSessionUpdate) -> Content {
    match update {
        AcpSessionUpdate::UserMessageChunk { content }
        | AcpSessionUpdate::AgentMessageChunk { content }
        | AcpSessionUpdate::SystemMessageChunk { content }
        | AcpSessionUpdate::AgentThoughtChunk { content } => Content {
            blocks: vec![acp_block_to_hail_block(content)],
        },
        AcpSessionUpdate::ToolCall { content, .. }
        | AcpSessionUpdate::ToolCallUpdate { content, .. } => Content {
            blocks: content
                .iter()
                .map(|item| match item {
                    AcpToolCallContent::Content { content } => acp_block_to_hail_block(content),
                    AcpToolCallContent::Diff {
                        path,
                        old_text,
                        new_text,
                    } => ContentBlock::Json {
                        data: serde_json::json!({
                            "path": path,
                            "oldText": old_text,
                            "newText": new_text,
                        }),
                    },
                })
                .collect(),
        },
        AcpSessionUpdate::SessionInfoUpdate { .. } => Content::empty(),
    }
}

fn tool_status_for_event(event: &Event) -> AcpToolStatus {
    match &event.event_type {
        EventType::ToolCall { .. } => AcpToolStatus::Pending,
        EventType::ToolResult { is_error, .. } => {
            if *is_error {
                AcpToolStatus::Failed
            } else {
                AcpToolStatus::Completed
            }
        }
        EventType::ShellCommand {
            exit_code: Some(code),
            ..
        } => {
            if *code == 0 {
                AcpToolStatus::Completed
            } else {
                AcpToolStatus::Failed
            }
        }
        _ => AcpToolStatus::Completed,
    }
}

fn tool_kind_for_event(event: &Event) -> AcpToolKind {
    if let Some(kind) = event.semantic_tool_kind() {
        return match kind.trim().to_ascii_lowercase().as_str() {
            "read" => AcpToolKind::Read,
            "edit" => AcpToolKind::Edit,
            "delete" => AcpToolKind::Delete,
            "move" => AcpToolKind::Move,
            "search" => AcpToolKind::Search,
            "execute" => AcpToolKind::Execute,
            "think" => AcpToolKind::Think,
            "fetch" => AcpToolKind::Fetch,
            _ => AcpToolKind::Other,
        };
    }

    match &event.event_type {
        EventType::FileRead { .. } => AcpToolKind::Read,
        EventType::FileEdit { .. } | EventType::FileCreate { .. } => AcpToolKind::Edit,
        EventType::FileDelete { .. } => AcpToolKind::Delete,
        EventType::CodeSearch { .. }
        | EventType::FileSearch { .. }
        | EventType::WebSearch { .. } => AcpToolKind::Search,
        EventType::ShellCommand { .. } => AcpToolKind::Execute,
        EventType::WebFetch { .. } => AcpToolKind::Fetch,
        EventType::Thinking => AcpToolKind::Think,
        EventType::ToolCall { name } | EventType::ToolResult { name, .. } => {
            let lowered = name.trim().to_ascii_lowercase();
            if lowered.contains("read") {
                AcpToolKind::Read
            } else if lowered.contains("edit")
                || lowered.contains("write")
                || lowered.contains("create")
            {
                AcpToolKind::Edit
            } else if lowered.contains("delete") {
                AcpToolKind::Delete
            } else if lowered.contains("search") || lowered.contains("grep") {
                AcpToolKind::Search
            } else if lowered.contains("fetch") {
                AcpToolKind::Fetch
            } else if lowered.contains("exec")
                || lowered.contains("bash")
                || lowered.contains("command")
            {
                AcpToolKind::Execute
            } else {
                AcpToolKind::Other
            }
        }
        _ => AcpToolKind::Other,
    }
}

fn tool_title_for_event(event: &Event) -> String {
    match &event.event_type {
        EventType::ToolCall { name } | EventType::ToolResult { name, .. } => name.clone(),
        EventType::FileRead { path } => format!("Read {path}"),
        EventType::CodeSearch { query } => format!("Code search: {query}"),
        EventType::FileSearch { pattern } => format!("File search: {pattern}"),
        EventType::FileEdit { path, .. } => format!("Edit {path}"),
        EventType::FileCreate { path } => format!("Create {path}"),
        EventType::FileDelete { path } => format!("Delete {path}"),
        EventType::ShellCommand { command, .. } => command.clone(),
        EventType::ImageGenerate { prompt }
        | EventType::VideoGenerate { prompt }
        | EventType::AudioGenerate { prompt } => prompt.clone(),
        EventType::WebSearch { query } => format!("Web search: {query}"),
        EventType::WebFetch { url } => format!("Fetch {url}"),
        EventType::TaskStart { title } => title.clone().unwrap_or_else(|| "task_start".to_string()),
        EventType::TaskEnd { summary } => summary.clone().unwrap_or_else(|| "task_end".to_string()),
        EventType::Custom { kind } => kind.clone(),
        _ => "tool_call".to_string(),
    }
}

fn raw_input_for_event(event: &Event) -> Option<Value> {
    match &event.event_type {
        EventType::ToolCall { name } => Some(Value::String(name.clone())),
        EventType::FileRead { path }
        | EventType::FileCreate { path }
        | EventType::FileDelete { path } => Some(serde_json::json!({ "path": path })),
        EventType::FileEdit { path, .. } => Some(serde_json::json!({ "path": path })),
        EventType::CodeSearch { query } | EventType::WebSearch { query } => {
            Some(serde_json::json!({ "query": query }))
        }
        EventType::FileSearch { pattern } => Some(serde_json::json!({ "pattern": pattern })),
        EventType::ShellCommand { command, .. } => Some(serde_json::json!({ "command": command })),
        EventType::WebFetch { url } => Some(serde_json::json!({ "url": url })),
        EventType::ImageGenerate { prompt }
        | EventType::VideoGenerate { prompt }
        | EventType::AudioGenerate { prompt } => Some(serde_json::json!({ "prompt": prompt })),
        _ => event.content.blocks.iter().find_map(|block| match block {
            ContentBlock::Json { data } => Some(data.clone()),
            _ => None,
        }),
    }
}

fn raw_output_for_event(event: &Event) -> Option<Value> {
    match &event.event_type {
        EventType::ToolResult { is_error, name, .. } => Some(serde_json::json!({
            "name": name,
            "isError": is_error,
        })),
        EventType::ShellCommand {
            exit_code: Some(code),
            ..
        } => Some(serde_json::json!({ "exitCode": code })),
        _ => event.content.blocks.iter().find_map(|block| match block {
            ContentBlock::Json { data } => Some(data.clone()),
            _ => None,
        }),
    }
}

fn locations_for_event(event: &Event) -> Option<Vec<AcpLocation>> {
    let path = match &event.event_type {
        EventType::FileRead { path }
        | EventType::FileEdit { path, .. }
        | EventType::FileCreate { path }
        | EventType::FileDelete { path } => Some(path.clone()),
        _ => None,
    }?;
    Some(vec![AcpLocation { path, line: None }])
}

fn tool_call_id_for_event(event: &Event) -> String {
    event
        .semantic_call_id()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("call_{}", event.event_id))
}

fn build_event_meta(
    event: &Event,
    chunk_index: usize,
    chunk_count: usize,
    include_original_content: bool,
) -> Value {
    let mut event_attributes = object_from_hashmap(&event.attributes);
    let mut event_meta = Map::new();
    event_meta.insert("eventId".to_string(), Value::String(event.event_id.clone()));
    event_meta.insert(
        "timestamp".to_string(),
        Value::String(event.timestamp.to_rfc3339()),
    );
    if let Some(task_id) = event.task_id.as_ref() {
        event_meta.insert("taskId".to_string(), Value::String(task_id.clone()));
    }
    if let Some(duration_ms) = event.duration_ms {
        event_meta.insert("durationMs".to_string(), Value::Number(duration_ms.into()));
    }
    event_meta.insert(
        "originalEventType".to_string(),
        serde_json::to_value(&event.event_type).unwrap_or(Value::Null),
    );
    if include_original_content {
        event_meta.insert(
            "originalContent".to_string(),
            serde_json::to_value(&event.content).unwrap_or(Value::Null),
        );
    }
    if !event_attributes.is_empty() {
        event_meta.insert(
            "attributes".to_string(),
            Value::Object(event_attributes.clone()),
        );
    }
    event_meta.insert(
        "chunkIndex".to_string(),
        Value::Number((chunk_index as u64).into()),
    );
    event_meta.insert(
        "chunkCount".to_string(),
        Value::Number((chunk_count as u64).into()),
    );

    let mut opensession = Map::new();
    opensession.insert("event".to_string(), Value::Object(event_meta));
    let mut diff = take_prefixed_namespace(&mut event_attributes, DIFF_NAMESPACE_PREFIX)
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    if let EventType::FileEdit {
        path,
        diff: Some(unified),
    } = &event.event_type
    {
        diff.entry("path".to_string())
            .or_insert_with(|| Value::String(path.clone()));
        diff.entry("unified".to_string())
            .or_insert_with(|| Value::String(unified.clone()));
    }
    if !diff.is_empty() {
        opensession.insert("diff".to_string(), Value::Object(diff));
    }

    let mut meta = Map::new();
    take_trace_meta(&mut event_attributes, &mut meta);
    if let Some(attributes) = opensession
        .get_mut("event")
        .and_then(Value::as_object_mut)
        .and_then(|event_meta| event_meta.get_mut("attributes"))
        .and_then(Value::as_object_mut)
    {
        *attributes = event_attributes;
        if attributes.is_empty() {
            opensession
                .get_mut("event")
                .and_then(Value::as_object_mut)
                .expect("event meta object")
                .remove("attributes");
        }
    }
    meta.insert("opensession".to_string(), Value::Object(opensession));
    Value::Object(meta)
}

fn session_new_meta(session: &Session) -> (Option<String>, Option<Vec<Value>>, Option<Value>) {
    let mut attributes = object_from_hashmap(&session.context.attributes);
    let cwd = take_string_attr(&mut attributes, ATTR_CWD)
        .or_else(|| take_string_attr(&mut attributes, "working_directory"));
    let mcp_servers = take_mcp_servers(&mut attributes);
    let job = job_meta_from_attributes(&mut attributes);
    let review = take_prefixed_namespace(&mut attributes, REVIEW_NAMESPACE_PREFIX);
    let handoff = take_prefixed_namespace(&mut attributes, HANDOFF_NAMESPACE_PREFIX);

    let mut context = Map::new();
    if let Some(title) = session.context.title.as_ref() {
        context.insert("title".to_string(), Value::String(title.clone()));
    }
    if let Some(description) = session.context.description.as_ref() {
        context.insert(
            "description".to_string(),
            Value::String(description.clone()),
        );
    }
    if !session.context.tags.is_empty() {
        context.insert(
            "tags".to_string(),
            Value::Array(
                session
                    .context
                    .tags
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }
    context.insert(
        "createdAt".to_string(),
        Value::String(session.context.created_at.to_rfc3339()),
    );
    context.insert(
        "updatedAt".to_string(),
        Value::String(session.context.updated_at.to_rfc3339()),
    );
    if !session.context.related_session_ids.is_empty() {
        context.insert(
            "relatedSessionIds".to_string(),
            Value::Array(
                session
                    .context
                    .related_session_ids
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }

    let mut meta = Map::new();
    take_trace_meta(&mut attributes, &mut meta);
    if !attributes.is_empty() {
        context.insert("attributes".to_string(), Value::Object(attributes));
    }

    let mut opensession = Map::new();
    opensession.insert(
        "agent".to_string(),
        serde_json::to_value(&session.agent).unwrap_or(Value::Null),
    );
    opensession.insert("context".to_string(), Value::Object(context));
    if let Some(job) = job {
        opensession.insert("job".to_string(), job);
    }
    if let Some(review) = review {
        opensession.insert("review".to_string(), review);
    }
    if let Some(handoff) = handoff {
        opensession.insert("handoff".to_string(), handoff);
    }
    opensession.insert(
        "source".to_string(),
        serde_json::json!({
            "sessionVersion": session.version,
            "canonicalFormat": "acp-semantic-jsonl",
        }),
    );

    meta.insert("opensession".to_string(), Value::Object(opensession));
    (cwd, mcp_servers, Some(Value::Object(meta)))
}

fn session_end_meta(session: &Session) -> Option<Value> {
    let mut opensession = Map::new();
    opensession.insert(
        "stats".to_string(),
        serde_json::to_value(&session.stats).unwrap_or(Value::Null),
    );
    opensession.insert(
        "source".to_string(),
        serde_json::json!({
            "sessionVersion": session.version,
            "canonicalFormat": "acp-semantic-jsonl",
        }),
    );

    let mut meta = Map::new();
    meta.insert("opensession".to_string(), Value::Object(opensession));
    Some(Value::Object(meta))
}

fn event_to_acp_lines(session_id: &str, event: &Event) -> Vec<AcpSemanticLine> {
    match &event.event_type {
        EventType::UserMessage => message_lines(session_id, event, |content| {
            AcpSessionUpdate::UserMessageChunk { content }
        }),
        EventType::AgentMessage => message_lines(session_id, event, |content| {
            AcpSessionUpdate::AgentMessageChunk { content }
        }),
        EventType::SystemMessage => message_lines(session_id, event, |content| {
            AcpSessionUpdate::SystemMessageChunk { content }
        }),
        EventType::Thinking => message_lines(session_id, event, |content| {
            AcpSessionUpdate::AgentThoughtChunk { content }
        }),
        EventType::ToolResult { .. } => vec![AcpSemanticLine::Update {
            session_id: session_id.to_string(),
            update: AcpSessionUpdate::ToolCallUpdate {
                tool_call_id: tool_call_id_for_event(event),
                status: tool_status_for_event(event),
                content: tool_call_content_from_blocks(&event.content.blocks),
                raw_output: raw_output_for_event(event),
            },
            meta: Some(build_event_meta(event, 0, 1, true)),
        }],
        _ => {
            let mut content = tool_call_content_from_blocks(&event.content.blocks);
            if let EventType::FileEdit {
                path,
                diff: Some(diff),
            } = &event.event_type
            {
                content.push(AcpToolCallContent::Diff {
                    path: path.clone(),
                    old_text: None,
                    new_text: diff.clone(),
                });
            }
            vec![AcpSemanticLine::Update {
                session_id: session_id.to_string(),
                update: AcpSessionUpdate::ToolCall {
                    tool_call_id: tool_call_id_for_event(event),
                    title: tool_title_for_event(event),
                    kind: tool_kind_for_event(event),
                    status: tool_status_for_event(event),
                    content,
                    raw_input: raw_input_for_event(event),
                    locations: locations_for_event(event),
                },
                meta: Some(build_event_meta(event, 0, 1, true)),
            }]
        }
    }
}

fn message_lines<F>(session_id: &str, event: &Event, make_update: F) -> Vec<AcpSemanticLine>
where
    F: Fn(AcpContentBlock) -> AcpSessionUpdate,
{
    let blocks = if event.content.blocks.is_empty() {
        vec![AcpContentBlock::Text {
            text: String::new(),
        }]
    } else {
        event
            .content
            .blocks
            .iter()
            .map(hail_block_to_acp_block)
            .collect::<Vec<_>>()
    };

    blocks
        .into_iter()
        .enumerate()
        .map(|(index, block)| AcpSemanticLine::Update {
            session_id: session_id.to_string(),
            update: make_update(block),
            meta: Some(build_event_meta(
                event,
                index,
                event.content.blocks.len().max(1),
                index == 0,
            )),
        })
        .collect()
}

fn read_all_to_string<R: Read>(mut reader: R) -> Result<String, JsonlError> {
    let mut buf = String::new();
    reader.read_to_string(&mut buf)?;
    Ok(buf)
}

fn first_non_empty_line(data: &str) -> Option<(usize, &str)> {
    data.lines()
        .enumerate()
        .find_map(|(index, line)| (!line.trim().is_empty()).then_some((index + 1, line)))
}

fn detect_is_legacy_hail(data: &str) -> Result<bool, JsonlError> {
    let Some((line_no, line)) = first_non_empty_line(data) else {
        return Err(JsonlError::MissingHeader);
    };
    match parse_line_type(line, line_no)? {
        Some(line_type) if line_type == "header" => Ok(true),
        Some(line_type) if line_type == "session.new" => Ok(false),
        _ => Err(JsonlError::UnexpectedLineType(line_no)),
    }
}

/// Write a session as ACP semantic JSONL to a writer.
pub fn write_jsonl<W: Write>(session: &Session, mut writer: W) -> Result<(), JsonlError> {
    let (cwd, mcp_servers, meta) = session_new_meta(session);
    let header = AcpSemanticLine::New {
        session_id: session.session_id.clone(),
        cwd,
        mcp_servers,
        meta,
    };
    json_to_writer_line(&mut writer, &header, 1)?;
    writer.write_all(b"\n")?;

    let mut line_no = 1usize;
    for event in &session.events {
        for line in event_to_acp_lines(&session.session_id, event) {
            line_no += 1;
            json_to_writer_line(&mut writer, &line, line_no)?;
            writer.write_all(b"\n")?;
        }
    }

    let end = AcpSemanticLine::End {
        session_id: session.session_id.clone(),
        stop_reason: Some("completed".to_string()),
        meta: session_end_meta(session),
    };
    json_to_writer_line(&mut writer, &end, line_no + 1)?;
    writer.write_all(b"\n")?;
    Ok(())
}

/// Write a session as canonical ACP semantic JSONL to a string.
pub fn to_jsonl_string(session: &Session) -> Result<String, JsonlError> {
    to_acp_semantic_jsonl_string(session)
}

/// Write a session as ACP semantic JSONL to a string explicitly.
pub fn to_acp_semantic_jsonl_string(session: &Session) -> Result<String, JsonlError> {
    let mut buf = Vec::new();
    write_jsonl(session, &mut buf)?;
    Ok(String::from_utf8(buf).unwrap())
}

/// Write a session as legacy HAIL v1 JSONL to a writer.
pub fn write_hail_v1_jsonl<W: Write>(session: &Session, mut writer: W) -> Result<(), JsonlError> {
    let header = HailLine::Header {
        version: session.version.clone(),
        session_id: session.session_id.clone(),
        agent: session.agent.clone(),
        context: session.context.clone(),
    };
    json_to_writer_line(&mut writer, &header, 1)?;
    writer.write_all(b"\n")?;

    for (index, event) in session.events.iter().enumerate() {
        json_to_writer_line(&mut writer, &HailLine::Event(event.clone()), index + 2)?;
        writer.write_all(b"\n")?;
    }

    json_to_writer_line(
        &mut writer,
        &HailLine::Stats(session.stats.clone()),
        session.events.len() + 2,
    )?;
    writer.write_all(b"\n")?;
    Ok(())
}

/// Write a session as legacy HAIL v1 JSONL to a string explicitly.
pub fn to_hail_v1_jsonl_string(session: &Session) -> Result<String, JsonlError> {
    let mut buf = Vec::new();
    write_hail_v1_jsonl(session, &mut buf)?;
    Ok(String::from_utf8(buf).unwrap())
}

/// Read a session from ACP semantic JSONL or legacy HAIL v1 JSONL.
pub fn read_jsonl<R: BufRead>(reader: R) -> Result<Session, JsonlError> {
    let text = read_all_to_string(reader)?;
    from_jsonl_str(&text)
}

/// Read a session from ACP semantic JSONL or legacy HAIL v1 JSONL.
pub fn from_jsonl_str(s: &str) -> Result<Session, JsonlError> {
    if detect_is_legacy_hail(s)? {
        from_hail_v1_jsonl_str(s)
    } else {
        from_acp_semantic_jsonl_str(s)
    }
}

/// Read a session from ACP semantic JSONL explicitly.
pub fn from_acp_semantic_jsonl_str(s: &str) -> Result<Session, JsonlError> {
    let mut lines = s
        .lines()
        .enumerate()
        .filter_map(|(index, line)| (!line.trim().is_empty()).then_some((index + 1, line)));

    let (header_line_no, header_str) = lines.next().ok_or(JsonlError::MissingHeader)?;
    let header: AcpSemanticLine = json_from_str_line(header_str, header_line_no)?;
    let AcpSemanticLine::New {
        session_id,
        cwd,
        mcp_servers,
        meta,
    } = header
    else {
        return Err(JsonlError::UnexpectedLineType(header_line_no));
    };

    let mut agent = Agent {
        provider: "unknown".to_string(),
        model: "unknown".to_string(),
        tool: "acp".to_string(),
        tool_version: None,
    };
    let mut context = SessionContext::default();
    let mut version = Session::CURRENT_VERSION.to_string();

    if let Some(opensession) = opensession_meta_object(&meta) {
        if let Some(agent_value) = opensession.get("agent") {
            if let Ok(parsed) = serde_json::from_value::<Agent>(agent_value.clone()) {
                agent = parsed;
            }
        }
        if let Some(context_value) = opensession.get("context").and_then(Value::as_object) {
            context.title = string_from_object(context_value, "title");
            context.description = string_from_object(context_value, "description");
            context.tags = json_array_of_strings(context_value.get("tags"));
            context.created_at =
                datetime_from_object(context_value, "createdAt").unwrap_or(context.created_at);
            context.updated_at =
                datetime_from_object(context_value, "updatedAt").unwrap_or(context.updated_at);
            context.related_session_ids =
                json_array_of_strings(context_value.get("relatedSessionIds"));
            if let Some(attributes) = context_value
                .get("attributes")
                .and_then(hashmap_from_object)
            {
                context.attributes.extend(attributes);
            }
        }
        if let Some(job) = opensession.get("job").and_then(Value::as_object) {
            restore_job_attributes(job, &mut context.attributes);
        }
        if let Some(review) = opensession.get("review").and_then(Value::as_object) {
            restore_prefixed_namespace(review, REVIEW_NAMESPACE_PREFIX, &mut context.attributes);
        }
        if let Some(handoff) = opensession.get("handoff").and_then(Value::as_object) {
            restore_prefixed_namespace(handoff, HANDOFF_NAMESPACE_PREFIX, &mut context.attributes);
        }
        if let Some(source) = opensession.get("source").and_then(Value::as_object) {
            if let Some(session_version) = string_from_object(source, "sessionVersion") {
                version = session_version;
            }
        }
    }
    restore_trace_meta(&meta, &mut context.attributes);

    if let Some(cwd) = cwd {
        context
            .attributes
            .insert(ATTR_CWD.to_string(), Value::String(cwd));
    }
    if let Some(mcp_servers) = mcp_servers {
        context
            .attributes
            .insert("mcp_servers".to_string(), Value::Array(mcp_servers));
    }

    let mut session = Session {
        version,
        session_id,
        agent,
        context,
        events: Vec::new(),
        stats: Stats::default(),
    };

    let mut current: Option<EventBuilder> = None;
    let mut stats = None;

    for (line_no, line_str) in lines {
        let line: AcpSemanticLine = json_from_str_line(line_str, line_no)?;
        match line {
            AcpSemanticLine::Update { update, meta, .. } => {
                let opensession = opensession_meta_object(&meta);
                let meta_event = opensession
                    .and_then(|object| object.get("event"))
                    .and_then(Value::as_object);

                let event_id = meta_event
                    .and_then(|object| string_from_object(object, "eventId"))
                    .unwrap_or_else(|| format!("event-{line_no}"));
                let same_event = current
                    .as_ref()
                    .map(|builder| builder.event_id == event_id)
                    .unwrap_or(false);

                if !same_event {
                    if let Some(builder) = current.take() {
                        session.events.push(builder.finish());
                    }
                    current = Some(EventBuilder {
                        event_id: event_id.clone(),
                        timestamp: meta_event
                            .and_then(|object| datetime_from_object(object, "timestamp")),
                        event_type: meta_event
                            .and_then(|object| object.get("originalEventType"))
                            .and_then(|value| {
                                serde_json::from_value::<EventType>(value.clone()).ok()
                            })
                            .or_else(|| derive_event_type_from_update(&update)),
                        task_id: meta_event.and_then(|object| string_from_object(object, "taskId")),
                        duration_ms: meta_event
                            .and_then(|object| object.get("durationMs"))
                            .and_then(Value::as_u64),
                        attributes: meta_event
                            .and_then(|object| object.get("attributes"))
                            .and_then(hashmap_from_object),
                        content: meta_event
                            .and_then(|object| object.get("originalContent"))
                            .and_then(|value| {
                                serde_json::from_value::<Content>(value.clone()).ok()
                            }),
                    });
                    if let Some(builder) = current.as_mut() {
                        if builder.content.is_none() {
                            builder.content = Some(fallback_content_from_update(&update));
                        }
                        if let Some(attributes) = builder.attributes.as_mut() {
                            if let Some(diff) = opensession
                                .and_then(|object| object.get("diff"))
                                .and_then(Value::as_object)
                            {
                                restore_prefixed_namespace(diff, DIFF_NAMESPACE_PREFIX, attributes);
                            }
                            restore_trace_meta(&meta, attributes);
                        } else {
                            let mut attributes = HashMap::new();
                            if let Some(diff) = opensession
                                .and_then(|object| object.get("diff"))
                                .and_then(Value::as_object)
                            {
                                restore_prefixed_namespace(
                                    diff,
                                    DIFF_NAMESPACE_PREFIX,
                                    &mut attributes,
                                );
                            }
                            restore_trace_meta(&meta, &mut attributes);
                            if !attributes.is_empty() {
                                builder.attributes = Some(attributes);
                            }
                        }
                    }
                } else if let Some(builder) = current.as_mut() {
                    if builder.content.is_none() {
                        let mut content = builder.content.take().unwrap_or_else(Content::empty);
                        content
                            .blocks
                            .extend(fallback_content_from_update(&update).blocks);
                        builder.content = Some(content);
                    }
                }
            }
            AcpSemanticLine::End { meta, .. } => {
                stats = opensession_meta_object(&meta)
                    .and_then(|object| object.get("stats"))
                    .and_then(|value| serde_json::from_value::<Stats>(value.clone()).ok());
            }
            AcpSemanticLine::New { .. } => {}
        }
    }

    if let Some(builder) = current.take() {
        session.events.push(builder.finish());
    }

    let had_stats = stats.is_some();
    session.stats = stats.unwrap_or_default();
    if !had_stats {
        session.recompute_stats();
    }
    Ok(session)
}

fn derive_event_type_from_update(update: &AcpSessionUpdate) -> Option<EventType> {
    Some(match update {
        AcpSessionUpdate::UserMessageChunk { .. } => EventType::UserMessage,
        AcpSessionUpdate::AgentMessageChunk { .. } => EventType::AgentMessage,
        AcpSessionUpdate::SystemMessageChunk { .. } => EventType::SystemMessage,
        AcpSessionUpdate::AgentThoughtChunk { .. } => EventType::Thinking,
        AcpSessionUpdate::ToolCall {
            title,
            tool_call_id,
            ..
        } => EventType::ToolCall {
            name: if title.trim().is_empty() {
                tool_call_id.clone()
            } else {
                title.clone()
            },
        },
        AcpSessionUpdate::ToolCallUpdate {
            tool_call_id,
            status,
            ..
        } => EventType::ToolResult {
            name: "tool_call".to_string(),
            is_error: matches!(status, AcpToolStatus::Failed),
            call_id: Some(tool_call_id.clone()),
        },
        AcpSessionUpdate::SessionInfoUpdate { .. } => EventType::Custom {
            kind: "session_info_update".to_string(),
        },
    })
}

/// Read a session from legacy HAIL v1 JSONL explicitly.
pub fn from_hail_v1_jsonl_str(s: &str) -> Result<Session, JsonlError> {
    let mut lines = s.lines().enumerate();

    let (header_line_no, header_str) = lines.next().ok_or(JsonlError::MissingHeader)?;
    let header: HailLine = json_from_str_line(header_str, header_line_no + 1)?;

    let (version, session_id, agent, context) = match header {
        HailLine::Header {
            version,
            session_id,
            agent,
            context,
        } => (version, session_id, agent, context),
        _ => return Err(JsonlError::UnexpectedLineType(header_line_no + 1)),
    };

    let mut events = Vec::new();
    let mut stats = None;

    for (line_no, line_str) in lines {
        if line_str.trim().is_empty() {
            continue;
        }
        match json_from_str_line::<HailLine>(line_str, line_no + 1)? {
            HailLine::Event(event) => events.push(event),
            HailLine::Stats(value) => stats = Some(value),
            HailLine::Header { .. } => {}
        }
    }

    let had_stats = stats.is_some();
    let mut session = Session {
        version,
        session_id,
        agent,
        context,
        events,
        stats: stats.unwrap_or_default(),
    };
    if !had_stats {
        session.recompute_stats();
    }
    Ok(session)
}

/// Read only session metadata without retaining event bodies.
pub fn read_header<R: BufRead>(
    reader: R,
) -> Result<(String, String, Agent, SessionContext), JsonlError> {
    let session = read_jsonl(reader)?;
    Ok((
        session.version,
        session.session_id,
        session.agent,
        session.context,
    ))
}

/// Read header + stats without exposing event bodies.
pub fn read_header_and_stats(
    data: &str,
) -> Result<(String, String, Agent, SessionContext, Option<Stats>), JsonlError> {
    let session = from_jsonl_str(data)?;
    Ok((
        session.version,
        session.session_id,
        session.agent,
        session.context,
        Some(session.stats),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::{ATTR_SEMANTIC_CALL_ID, Content, EventType};
    use chrono::Utc;

    fn make_test_session() -> Session {
        let mut session = Session::new(
            "test-jsonl-123".to_string(),
            Agent {
                provider: "anthropic".to_string(),
                model: "claude-opus-4-6".to_string(),
                tool: "claude-code".to_string(),
                tool_version: Some("1.2.3".to_string()),
            },
        );
        session.context.title = Some("Test JSONL session".to_string());
        session
            .context
            .attributes
            .insert("cwd".to_string(), Value::String("/tmp/repo".to_string()));
        session.context.attributes.insert(
            JOB_ID_KEY.to_string(),
            Value::String("AUTH-123".to_string()),
        );
        session.context.attributes.insert(
            JOB_PROTOCOL_KEY.to_string(),
            Value::String("agent_client_protocol".to_string()),
        );
        session.context.attributes.insert(
            JOB_SYSTEM_KEY.to_string(),
            Value::String("symphony".to_string()),
        );
        session.context.attributes.insert(
            JOB_TITLE_KEY.to_string(),
            Value::String("Fix auth".to_string()),
        );
        session.context.attributes.insert(
            JOB_RUN_ID_KEY.to_string(),
            Value::String("run-42".to_string()),
        );
        session
            .context
            .attributes
            .insert(JOB_ATTEMPT_KEY.to_string(), Value::Number(2.into()));
        session.context.attributes.insert(
            JOB_STAGE_KEY.to_string(),
            Value::String("review".to_string()),
        );
        session.context.attributes.insert(
            JOB_REVIEW_KIND_KEY.to_string(),
            Value::String("todo".to_string()),
        );
        session.context.attributes.insert(
            JOB_STATUS_KEY.to_string(),
            Value::String("pending".to_string()),
        );
        session.context.attributes.insert(
            JOB_ARTIFACTS_KEY.to_string(),
            serde_json::json!([{
                "kind": "handoff",
                "label": "handoff",
                "uri": "os://artifact/handoff/123"
            }]),
        );
        session.context.attributes.insert(
            "opensession.review.id".to_string(),
            Value::String("todo-review-1".to_string()),
        );
        session.context.attributes.insert(
            "opensession.review.qa.count".to_string(),
            Value::Number(2.into()),
        );
        session.context.attributes.insert(
            "opensession.handoff.artifact_uri".to_string(),
            Value::String("os://artifact/handoff/123".to_string()),
        );

        let ts = Utc::now();
        session.events.push(Event {
            event_id: "e1".to_string(),
            timestamp: ts,
            event_type: EventType::UserMessage,
            task_id: None,
            content: Content::text("Hello, can you help me?"),
            duration_ms: None,
            attributes: HashMap::new(),
        });
        let mut tool_attrs = HashMap::new();
        tool_attrs.insert(
            ATTR_SEMANTIC_CALL_ID.to_string(),
            Value::String("call-read-1".to_string()),
        );
        session.events.push(Event {
            event_id: "e2".to_string(),
            timestamp: ts,
            event_type: EventType::FileEdit {
                path: "src/lib.rs".to_string(),
                diff: Some("@@ -1 +1 @@\n-old\n+new".to_string()),
            },
            task_id: Some("task-1".to_string()),
            content: Content {
                blocks: vec![
                    ContentBlock::Text {
                        text: "Updated src/lib.rs".to_string(),
                    },
                    ContentBlock::Code {
                        code: "fn main() {}".to_string(),
                        language: Some("rust".to_string()),
                        start_line: Some(1),
                    },
                ],
            },
            duration_ms: Some(120),
            attributes: tool_attrs,
        });
        session.events[1].attributes.insert(
            "opensession.diff.language".to_string(),
            Value::String("rust".to_string()),
        );
        session.events.push(Event {
            event_id: "e3".to_string(),
            timestamp: ts,
            event_type: EventType::ToolResult {
                name: "Edit".to_string(),
                is_error: false,
                call_id: Some("call-read-1".to_string()),
            },
            task_id: Some("task-1".to_string()),
            content: Content::text("applied"),
            duration_ms: Some(40),
            attributes: HashMap::new(),
        });
        session.recompute_stats();
        session
    }

    #[test]
    fn canonical_write_uses_acp_semantic_lines() {
        let session = make_test_session();
        let jsonl = to_jsonl_string(&session).unwrap();
        let lines = jsonl.lines().collect::<Vec<_>>();
        assert_eq!(
            parse_line_type(lines[0], 1).unwrap().as_deref(),
            Some("session.new")
        );
        assert_eq!(
            parse_line_type(lines.last().unwrap(), lines.len())
                .unwrap()
                .as_deref(),
            Some("session.end")
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("\"sessionUpdate\":\"tool_call\""))
        );
    }

    #[test]
    fn canonical_acp_roundtrip_preserves_job_context_and_events() {
        let session = make_test_session();
        let jsonl = to_jsonl_string(&session).unwrap();
        let parsed = from_jsonl_str(&jsonl).unwrap();
        assert_eq!(parsed.version, Session::CURRENT_VERSION);
        assert_eq!(parsed.session_id, session.session_id);
        assert_eq!(parsed.events.len(), session.events.len());
        assert_eq!(
            parsed.context.attributes.get(JOB_ID_KEY),
            session.context.attributes.get(JOB_ID_KEY)
        );
        assert_eq!(parsed.stats.event_count, session.stats.event_count);
        assert!(matches!(
            parsed.events[1].event_type,
            EventType::FileEdit { .. }
        ));
        assert_eq!(
            parsed.context.attributes.get("opensession.review.id"),
            session.context.attributes.get("opensession.review.id")
        );
        assert_eq!(
            parsed
                .context
                .attributes
                .get("opensession.handoff.artifact_uri"),
            session
                .context
                .attributes
                .get("opensession.handoff.artifact_uri")
        );
        assert_eq!(
            parsed.events[1].attributes.get("opensession.diff.language"),
            session.events[1]
                .attributes
                .get("opensession.diff.language")
        );
        assert_eq!(
            parsed.events[1].attributes.get("opensession.diff.unified"),
            Some(&Value::String("@@ -1 +1 @@\n-old\n+new".to_string()))
        );
    }

    #[test]
    fn canonical_acp_persists_review_handoff_and_diff_namespaces() {
        let session = make_test_session();
        let jsonl = to_jsonl_string(&session).unwrap();
        let lines = jsonl.lines().collect::<Vec<_>>();
        let header: Value = serde_json::from_str(lines[0]).unwrap();
        let update: Value = lines
            .iter()
            .find_map(|line| {
                line.contains("\"eventId\":\"e2\"")
                    .then(|| serde_json::from_str::<Value>(line).unwrap())
            })
            .expect("file edit update line");

        let header_meta = header
            .get("_meta")
            .and_then(Value::as_object)
            .and_then(|meta| meta.get("opensession"))
            .and_then(Value::as_object)
            .expect("header opensession meta");
        assert_eq!(
            header_meta
                .get("review")
                .and_then(Value::as_object)
                .and_then(|review| review.get("id"))
                .and_then(Value::as_str),
            Some("todo-review-1")
        );
        assert_eq!(
            header_meta
                .get("handoff")
                .and_then(Value::as_object)
                .and_then(|handoff| handoff.get("artifact_uri"))
                .and_then(Value::as_str),
            Some("os://artifact/handoff/123")
        );

        let update_meta = update
            .get("_meta")
            .and_then(Value::as_object)
            .and_then(|meta| meta.get("opensession"))
            .and_then(Value::as_object)
            .expect("update opensession meta");
        assert_eq!(
            update_meta
                .get("diff")
                .and_then(Value::as_object)
                .and_then(|diff| diff.get("path"))
                .and_then(Value::as_str),
            Some("src/lib.rs")
        );
        assert_eq!(
            update_meta
                .get("diff")
                .and_then(Value::as_object)
                .and_then(|diff| diff.get("unified"))
                .and_then(Value::as_str),
            Some("@@ -1 +1 @@\n-old\n+new")
        );
        assert_eq!(
            update_meta
                .get("diff")
                .and_then(Value::as_object)
                .and_then(|diff| diff.get("language"))
                .and_then(Value::as_str),
            Some("rust")
        );
    }

    #[test]
    fn legacy_hail_v1_roundtrip_still_reads() {
        let session = make_test_session();
        let legacy = to_hail_v1_jsonl_string(&session).unwrap();
        let parsed = from_jsonl_str(&legacy).unwrap();
        assert_eq!(parsed.session_id, session.session_id);
        assert_eq!(parsed.events.len(), session.events.len());
        assert_eq!(parsed.stats.event_count, session.stats.event_count);
    }

    #[test]
    fn explicit_legacy_writer_keeps_header_event_stats_shape() {
        let session = make_test_session();
        let legacy = to_hail_v1_jsonl_string(&session).unwrap();
        let lines = legacy.lines().collect::<Vec<_>>();
        assert_eq!(
            parse_line_type(lines[0], 1).unwrap().as_deref(),
            Some("header")
        );
        assert_eq!(
            parse_line_type(lines.last().unwrap(), lines.len())
                .unwrap()
                .as_deref(),
            Some("stats")
        );
    }

    #[test]
    fn read_header_and_stats_supports_canonical_acp() {
        let session = make_test_session();
        let jsonl = to_jsonl_string(&session).unwrap();
        let (version, session_id, _agent, context, stats) = read_header_and_stats(&jsonl).unwrap();
        assert_eq!(version, Session::CURRENT_VERSION);
        assert_eq!(session_id, session.session_id);
        assert_eq!(context.title, session.context.title);
        assert_eq!(stats.unwrap().event_count, session.stats.event_count);
    }
}
