use opensession_core::trace::Session;
use serde::{Deserialize, Serialize};

/// Source descriptor for parser preview requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum ParseSource {
    Git {
        remote: String,
        r#ref: String,
        path: String,
    },
    Github {
        owner: String,
        repo: String,
        r#ref: String,
        path: String,
    },
    Inline {
        filename: String,
        content_base64: String,
    },
}

/// Candidate parser ranked by detection confidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ParseCandidate {
    pub id: String,
    pub confidence: u8,
    pub reason: String,
}

/// Request body for `POST /api/parse/preview`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ParsePreviewRequest {
    pub source: ParseSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parser_hint: Option<String>,
}

/// Response body for `POST /api/parse/preview`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ParsePreviewResponse {
    pub parser_used: String,
    #[serde(default)]
    pub parser_candidates: Vec<ParseCandidate>,
    #[cfg_attr(feature = "ts", ts(type = "any"))]
    pub session: Session,
    pub source: ParseSource,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_adapter: Option<String>,
}

/// Structured parser preview error response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ParsePreviewErrorResponse {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parser_candidates: Vec<ParseCandidate>,
}
