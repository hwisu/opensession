use std::cmp::Ordering;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::Session;

pub const HANDOFF_ARTIFACT_VERSION: &str = "1";
pub const HANDOFF_MERGE_POLICY_TIME_ASC: &str = "time_asc";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HandoffPayloadFormat {
    Json,
    Jsonl,
}

impl std::fmt::Display for HandoffPayloadFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json => write!(f, "json"),
            Self::Jsonl => write!(f, "jsonl"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandoffArtifactSource {
    pub session_id: String,
    pub tool: String,
    pub model: String,
    pub source_path: String,
    pub source_mtime_ms: u64,
    pub source_size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffArtifact {
    pub version: String,
    pub artifact_id: String,
    pub created_at: DateTime<Utc>,
    pub merge_policy: String,
    pub sources: Vec<HandoffArtifactSource>,
    pub payload_format: HandoffPayloadFormat,
    pub payload: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub derived_markdown: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandoffSourceStaleReason {
    pub session_id: String,
    pub source_path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceFingerprint {
    pub mtime_ms: u64,
    pub size: u64,
}

pub fn merge_time_order(
    left_created_at: DateTime<Utc>,
    left_session_id: &str,
    right_created_at: DateTime<Utc>,
    right_session_id: &str,
) -> Ordering {
    left_created_at
        .cmp(&right_created_at)
        .then_with(|| left_session_id.cmp(right_session_id))
}

pub fn sort_sessions_time_asc(sessions: &mut [Session]) {
    sessions.sort_by(|left, right| {
        merge_time_order(
            left.context.created_at,
            &left.session_id,
            right.context.created_at,
            &right.session_id,
        )
    });
}

pub fn source_from_session(
    session: &Session,
    source_path: impl Into<String>,
    fingerprint: SourceFingerprint,
) -> HandoffArtifactSource {
    HandoffArtifactSource {
        session_id: session.session_id.clone(),
        tool: session.agent.tool.clone(),
        model: session.agent.model.clone(),
        source_path: source_path.into(),
        source_mtime_ms: fingerprint.mtime_ms,
        source_size: fingerprint.size,
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};

    use crate::testing;

    use super::*;

    #[test]
    fn sort_sessions_time_asc_orders_by_time_then_session_id() {
        let now = Utc::now();
        let mut s1 = Session::new("session-z".to_string(), testing::agent());
        s1.context.created_at = now + Duration::seconds(10);
        let mut s2 = Session::new("session-b".to_string(), testing::agent());
        s2.context.created_at = now;
        let mut s3 = Session::new("session-a".to_string(), testing::agent());
        s3.context.created_at = now;

        let mut sessions = vec![s1, s2, s3];
        sort_sessions_time_asc(&mut sessions);

        let ids = sessions
            .into_iter()
            .map(|session| session.session_id)
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                "session-a".to_string(),
                "session-b".to_string(),
                "session-z".to_string()
            ]
        );
    }

    #[test]
    fn source_from_session_preserves_supplied_fingerprint() {
        let mut session = Session::new("session-1".to_string(), testing::agent());
        session.context.created_at = Utc::now();
        let source = source_from_session(
            &session,
            "/tmp/session.jsonl",
            SourceFingerprint {
                mtime_ms: 42,
                size: 128,
            },
        );

        assert_eq!(source.session_id, "session-1");
        assert_eq!(source.source_path, "/tmp/session.jsonl");
        assert_eq!(source.source_mtime_ms, 42);
        assert_eq!(source.source_size, 128);
    }
}
