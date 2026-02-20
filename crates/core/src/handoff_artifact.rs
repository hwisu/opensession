use std::cmp::Ordering;
use std::path::Path;
use std::time::UNIX_EPOCH;

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

impl HandoffArtifact {
    pub fn stale_reasons(&self) -> Vec<HandoffSourceStaleReason> {
        stale_reasons(&self.sources)
    }

    pub fn is_stale(&self) -> bool {
        !self.stale_reasons().is_empty()
    }
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

pub fn source_fingerprint(path: &Path) -> std::io::Result<SourceFingerprint> {
    let metadata = std::fs::metadata(path)?;
    let mtime_ms = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0);
    Ok(SourceFingerprint {
        mtime_ms,
        size: metadata.len(),
    })
}

pub fn source_from_session(
    session: &Session,
    source_path: &Path,
) -> std::io::Result<HandoffArtifactSource> {
    let fp = source_fingerprint(source_path)?;
    Ok(HandoffArtifactSource {
        session_id: session.session_id.clone(),
        tool: session.agent.tool.clone(),
        model: session.agent.model.clone(),
        source_path: source_path.to_string_lossy().into_owned(),
        source_mtime_ms: fp.mtime_ms,
        source_size: fp.size,
    })
}

pub fn stale_reasons(sources: &[HandoffArtifactSource]) -> Vec<HandoffSourceStaleReason> {
    let mut reasons = Vec::new();
    for source in sources {
        let path = Path::new(&source.source_path);
        let metadata = match std::fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                reasons.push(HandoffSourceStaleReason {
                    session_id: source.session_id.clone(),
                    source_path: source.source_path.clone(),
                    reason: "missing_source_file".to_string(),
                });
                continue;
            }
            Err(_) => {
                reasons.push(HandoffSourceStaleReason {
                    session_id: source.session_id.clone(),
                    source_path: source.source_path.clone(),
                    reason: "unreadable_source_file".to_string(),
                });
                continue;
            }
        };

        let current_mtime_ms = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0);
        let current_size = metadata.len();

        if current_mtime_ms != source.source_mtime_ms || current_size != source.source_size {
            reasons.push(HandoffSourceStaleReason {
                session_id: source.session_id.clone(),
                source_path: source.source_path.clone(),
                reason: "source_fingerprint_changed".to_string(),
            });
        }
    }
    reasons
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
    fn stale_reasons_detects_fingerprint_changes() {
        let temp_path = std::env::temp_dir().join(format!(
            "opensession-handoff-artifact-{}.jsonl",
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::write(&temp_path, b"before").expect("write temp file");

        let mut session = Session::new("session-1".to_string(), testing::agent());
        session.context.created_at = Utc::now();
        let source = source_from_session(&session, &temp_path).expect("source fingerprint");
        assert!(stale_reasons(&[source.clone()]).is_empty());

        std::fs::write(&temp_path, b"after-after").expect("rewrite temp file");
        let reasons = stale_reasons(&[source]);
        assert_eq!(reasons.len(), 1);
        assert_eq!(reasons[0].reason, "source_fingerprint_changed");

        let _ = std::fs::remove_file(&temp_path);
    }
}
