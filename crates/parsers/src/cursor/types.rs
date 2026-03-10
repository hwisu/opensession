use serde::Deserialize;

pub(super) mod string_or_number {
    use serde::{self, Deserialize, Deserializer};

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum StringOrNumber {
            String(String),
            Integer(i64),
            Float(f64),
        }

        match Option::<StringOrNumber>::deserialize(deserializer)? {
            Some(StringOrNumber::String(value)) => Ok(Some(value)),
            Some(StringOrNumber::Integer(value)) => Ok(Some(value.to_string())),
            Some(StringOrNumber::Float(value)) => Ok(Some(value.to_string())),
            None => Ok(None),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawComposerData {
    pub(super) composer_id: String,
    #[serde(default)]
    pub(super) name: Option<String>,
    #[serde(default, deserialize_with = "string_or_number::deserialize")]
    pub(super) created_at: Option<String>,
    #[serde(default, deserialize_with = "string_or_number::deserialize")]
    pub(super) last_updated_at: Option<String>,
    #[serde(default)]
    pub(super) conversation: Vec<RawBubble>,
    #[serde(default)]
    pub(super) is_agentic: Option<bool>,
    #[serde(default, rename = "_v")]
    pub(super) version: Option<u64>,
    #[serde(default)]
    pub(super) full_conversation_headers_only: Option<Vec<RawBubbleHeader>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawComposerIndex {
    #[serde(default)]
    pub(super) all_composers: Vec<RawComposerMeta>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawComposerMeta {
    pub(super) composer_id: String,
    #[serde(default)]
    pub(super) name: Option<String>,
    #[serde(default, deserialize_with = "string_or_number::deserialize")]
    pub(super) created_at: Option<String>,
    #[serde(default, deserialize_with = "string_or_number::deserialize")]
    pub(super) last_updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawBubbleHeader {
    pub(super) bubble_id: String,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    pub(super) bubble_type: u8,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub(super) struct RawBubble {
    #[serde(rename = "type")]
    pub(super) bubble_type: u8,
    #[serde(default)]
    pub(super) bubble_id: Option<String>,
    #[serde(default)]
    pub(super) text: Option<String>,
    #[serde(default)]
    pub(super) thinking: Option<RawThinking>,
    #[serde(default)]
    pub(super) tool_former_data: Option<RawToolFormerData>,
    #[serde(default)]
    pub(super) timing_info: Option<RawTimingInfo>,
    #[serde(default)]
    pub(super) model_type: Option<String>,
    #[serde(default)]
    pub(super) checkpoint: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawThinking {
    #[serde(default)]
    pub(super) text: Option<String>,
    #[serde(default)]
    pub(super) signature: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawToolFormerData {
    #[serde(default)]
    pub(super) tool: Option<u32>,
    #[serde(default)]
    pub(super) name: Option<String>,
    #[serde(default)]
    pub(super) status: Option<String>,
    #[serde(default)]
    pub(super) raw_args: Option<String>,
    #[serde(default)]
    pub(super) result: Option<String>,
    #[serde(default)]
    pub(super) user_decision: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawTimingInfo {
    #[serde(default)]
    pub(super) start_time: Option<f64>,
    #[serde(default)]
    pub(super) end_time: Option<f64>,
    #[serde(default)]
    pub(super) client_start_time: Option<f64>,
    #[serde(default)]
    pub(super) client_end_time: Option<f64>,
}
