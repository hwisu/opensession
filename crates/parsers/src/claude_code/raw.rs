use serde::Deserialize;

/// Top-level entry in the Claude Code JSONL file.
/// Each line is one of these.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum RawEntry {
    #[serde(rename = "user")]
    User(RawConversationEntry),
    #[serde(rename = "assistant")]
    Assistant(RawConversationEntry),
    #[serde(rename = "file-history-snapshot")]
    FileHistorySnapshot {},
    #[serde(rename = "system")]
    System(RawSystemEntry),
    #[serde(rename = "progress")]
    Progress(RawProgressEntry),
    #[serde(rename = "queue-operation")]
    QueueOperation(RawQueueOperationEntry),
    #[serde(rename = "summary")]
    Summary(RawSummaryEntry),
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawConversationEntry {
    pub(crate) uuid: String,
    #[serde(default)]
    pub(crate) session_id: Option<String>,
    pub(crate) timestamp: String,
    pub(crate) message: RawMessage,
    #[serde(default)]
    pub(crate) cwd: Option<String>,
    #[serde(default)]
    pub(crate) git_branch: Option<String>,
    #[serde(default)]
    pub(crate) version: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    agent_id: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    slug: Option<String>,
    #[allow(dead_code)]
    #[serde(default, rename = "costUSD")]
    cost_usd: Option<f64>,
    #[serde(default)]
    pub(crate) usage: Option<RawUsage>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawSystemEntry {
    #[serde(default)]
    pub(crate) uuid: Option<String>,
    #[serde(default)]
    pub(crate) session_id: Option<String>,
    #[serde(default)]
    pub(crate) timestamp: Option<String>,
    #[serde(default)]
    pub(crate) content: Option<String>,
    #[serde(default)]
    pub(crate) subtype: Option<String>,
    #[serde(default)]
    pub(crate) level: Option<String>,
    #[serde(default)]
    pub(crate) cwd: Option<String>,
    #[serde(default)]
    pub(crate) git_branch: Option<String>,
    #[serde(default)]
    pub(crate) version: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawProgressEntry {
    #[serde(default)]
    pub(crate) uuid: Option<String>,
    #[serde(default)]
    pub(crate) session_id: Option<String>,
    #[serde(default)]
    pub(crate) timestamp: Option<String>,
    #[serde(default)]
    pub(crate) data: Option<serde_json::Value>,
    #[serde(default)]
    pub(crate) tool_use_id: Option<String>,
    #[serde(default)]
    pub(crate) parent_tool_use_id: Option<String>,
    #[serde(default)]
    pub(crate) cwd: Option<String>,
    #[serde(default)]
    pub(crate) git_branch: Option<String>,
    #[serde(default)]
    pub(crate) version: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawQueueOperationEntry {
    #[serde(default)]
    pub(crate) session_id: Option<String>,
    #[serde(default)]
    pub(crate) timestamp: Option<String>,
    #[serde(default)]
    pub(crate) operation: Option<String>,
    #[serde(default)]
    pub(crate) content: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawSummaryEntry {
    #[serde(default)]
    pub(crate) uuid: Option<String>,
    #[serde(default)]
    pub(crate) session_id: Option<String>,
    #[serde(default)]
    pub(crate) timestamp: Option<String>,
    #[serde(default)]
    pub(crate) leaf_uuid: Option<String>,
    #[serde(default)]
    pub(crate) summary: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub(crate) struct RawUsage {
    #[serde(default)]
    pub(crate) input_tokens: u64,
    #[serde(default)]
    pub(crate) output_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub(crate) struct RawMessage {
    pub(crate) role: String,
    pub(crate) content: RawContent,
    #[serde(default)]
    pub(crate) model: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum RawContent {
    Text(String),
    Blocks(Vec<RawContentBlock>),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum RawContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "thinking")]
    Thinking {
        #[serde(default)]
        thinking: Option<String>,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        #[serde(default)]
        id: Option<String>,
        name: String,
        #[serde(default)]
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        #[serde(default)]
        tool_use_id: Option<String>,
        #[serde(default)]
        content: ToolResultContent,
        #[serde(default)]
        is_error: bool,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Default, Deserialize)]
#[serde(untagged)]
pub(crate) enum ToolResultContent {
    Text(String),
    Blocks(Vec<ToolResultBlock>),
    #[default]
    Null,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum ToolResultBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(other)]
    Other,
}
