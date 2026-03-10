use opensession_core::trace::Session;
use serde::{Deserialize, Serialize};

pub const ATTR_JOB_PROTOCOL: &str = "opensession.job.protocol";
pub const ATTR_JOB_SYSTEM: &str = "opensession.job.system";
pub const ATTR_JOB_ID: &str = "opensession.job.id";
pub const ATTR_JOB_TITLE: &str = "opensession.job.title";
pub const ATTR_JOB_RUN_ID: &str = "opensession.job.run_id";
pub const ATTR_JOB_ATTEMPT: &str = "opensession.job.attempt";
pub const ATTR_JOB_STAGE: &str = "opensession.job.stage";
pub const ATTR_JOB_REVIEW_KIND: &str = "opensession.job.review_kind";
pub const ATTR_JOB_STATUS: &str = "opensession.job.status";
pub const ATTR_JOB_THREAD_ID: &str = "opensession.job.thread_id";
pub const ATTR_JOB_ARTIFACTS: &str = "opensession.job.artifacts";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum JobProtocol {
    Opensession,
    AgentClientProtocol,
    AgentCommunicationProtocol,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum JobStage {
    Planning,
    Review,
    Execution,
    Handoff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum JobReviewKind {
    Todo,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum JobStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Cancelled,
}

impl std::fmt::Display for JobProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Opensession => "opensession",
            Self::AgentClientProtocol => "agent_client_protocol",
            Self::AgentCommunicationProtocol => "agent_communication_protocol",
        };
        f.write_str(value)
    }
}

impl std::fmt::Display for JobStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Planning => "planning",
            Self::Review => "review",
            Self::Execution => "execution",
            Self::Handoff => "handoff",
        };
        f.write_str(value)
    }
}

impl std::fmt::Display for JobReviewKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Todo => "todo",
            Self::Done => "done",
        };
        f.write_str(value)
    }
}

impl std::fmt::Display for JobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct JobArtifactRef {
    pub kind: String,
    pub label: String,
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(type = "Record<string, any>"))]
    pub metadata: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct JobManifest {
    pub protocol: JobProtocol,
    pub system: String,
    pub job_id: String,
    pub job_title: String,
    pub run_id: String,
    pub attempt: i64,
    pub stage: JobStage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_kind: Option<JobReviewKind>,
    pub status: JobStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub artifacts: Vec<JobArtifactRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct JobContext {
    pub protocol: JobProtocol,
    pub system: String,
    pub job_id: String,
    pub job_title: String,
    pub run_id: String,
    pub attempt: i64,
    pub stage: JobStage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_kind: Option<JobReviewKind>,
    pub status: JobStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    pub artifact_count: i64,
}

impl JobManifest {
    pub fn validate(&self) -> Result<(), String> {
        if self.system.trim().is_empty() {
            return Err("manifest.system is required".to_string());
        }
        if self.job_id.trim().is_empty() {
            return Err("manifest.job_id is required".to_string());
        }
        if self.job_title.trim().is_empty() {
            return Err("manifest.job_title is required".to_string());
        }
        if self.run_id.trim().is_empty() {
            return Err("manifest.run_id is required".to_string());
        }
        if self.attempt < 0 {
            return Err("manifest.attempt must be non-negative".to_string());
        }
        match (self.stage, self.review_kind) {
            (JobStage::Review, None) => {
                return Err("manifest.review_kind is required when stage=review".to_string());
            }
            (JobStage::Review, Some(_)) => {}
            (_, Some(_)) => {
                return Err("manifest.review_kind is only allowed when stage=review".to_string());
            }
            (_, None) => {}
        }
        for artifact in &self.artifacts {
            if artifact.kind.trim().is_empty() {
                return Err("manifest.artifacts[].kind is required".to_string());
            }
            if artifact.label.trim().is_empty() {
                return Err("manifest.artifacts[].label is required".to_string());
            }
            if artifact.uri.trim().is_empty() {
                return Err("manifest.artifacts[].uri is required".to_string());
            }
        }
        Ok(())
    }
}

impl JobContext {
    pub fn from_manifest(manifest: &JobManifest) -> Self {
        Self {
            protocol: manifest.protocol,
            system: manifest.system.clone(),
            job_id: manifest.job_id.clone(),
            job_title: manifest.job_title.clone(),
            run_id: manifest.run_id.clone(),
            attempt: manifest.attempt,
            stage: manifest.stage,
            review_kind: manifest.review_kind,
            status: manifest.status,
            thread_id: manifest.thread_id.clone(),
            artifact_count: manifest.artifacts.len() as i64,
        }
    }
}

pub fn apply_job_manifest(session: &mut Session, manifest: &JobManifest) {
    session.context.attributes.insert(
        ATTR_JOB_PROTOCOL.to_string(),
        serde_json::to_value(manifest.protocol).unwrap_or(serde_json::Value::Null),
    );
    session.context.attributes.insert(
        ATTR_JOB_SYSTEM.to_string(),
        serde_json::Value::String(manifest.system.clone()),
    );
    session.context.attributes.insert(
        ATTR_JOB_ID.to_string(),
        serde_json::Value::String(manifest.job_id.clone()),
    );
    session.context.attributes.insert(
        ATTR_JOB_TITLE.to_string(),
        serde_json::Value::String(manifest.job_title.clone()),
    );
    session.context.attributes.insert(
        ATTR_JOB_RUN_ID.to_string(),
        serde_json::Value::String(manifest.run_id.clone()),
    );
    session.context.attributes.insert(
        ATTR_JOB_ATTEMPT.to_string(),
        serde_json::Value::Number(manifest.attempt.into()),
    );
    session.context.attributes.insert(
        ATTR_JOB_STAGE.to_string(),
        serde_json::to_value(manifest.stage).unwrap_or(serde_json::Value::Null),
    );
    if let Some(review_kind) = manifest.review_kind {
        session.context.attributes.insert(
            ATTR_JOB_REVIEW_KIND.to_string(),
            serde_json::to_value(review_kind).unwrap_or(serde_json::Value::Null),
        );
    } else {
        session.context.attributes.remove(ATTR_JOB_REVIEW_KIND);
    }
    session.context.attributes.insert(
        ATTR_JOB_STATUS.to_string(),
        serde_json::to_value(manifest.status).unwrap_or(serde_json::Value::Null),
    );
    if let Some(thread_id) = manifest.thread_id.as_ref() {
        session.context.attributes.insert(
            ATTR_JOB_THREAD_ID.to_string(),
            serde_json::Value::String(thread_id.clone()),
        );
    } else {
        session.context.attributes.remove(ATTR_JOB_THREAD_ID);
    }
    session.context.attributes.insert(
        ATTR_JOB_ARTIFACTS.to_string(),
        serde_json::to_value(&manifest.artifacts).unwrap_or(serde_json::Value::Array(Vec::new())),
    );
}

pub fn job_manifest_from_session(session: &Session) -> Option<JobManifest> {
    job_manifest_from_attributes(&session.context.attributes)
}

pub fn job_manifest_from_attributes(
    attributes: &std::collections::HashMap<String, serde_json::Value>,
) -> Option<JobManifest> {
    let protocol =
        serde_json::from_value::<JobProtocol>(attributes.get(ATTR_JOB_PROTOCOL)?.clone()).ok()?;
    let system = attr_string(attributes, ATTR_JOB_SYSTEM)?;
    let job_id = attr_string(attributes, ATTR_JOB_ID)?;
    let job_title = attr_string(attributes, ATTR_JOB_TITLE)?;
    let run_id = attr_string(attributes, ATTR_JOB_RUN_ID)?;
    let attempt = attributes.get(ATTR_JOB_ATTEMPT)?.as_i64()?;
    let stage = serde_json::from_value::<JobStage>(attributes.get(ATTR_JOB_STAGE)?.clone()).ok()?;
    let review_kind = attributes
        .get(ATTR_JOB_REVIEW_KIND)
        .and_then(|value| serde_json::from_value::<JobReviewKind>(value.clone()).ok());
    let status =
        serde_json::from_value::<JobStatus>(attributes.get(ATTR_JOB_STATUS)?.clone()).ok()?;
    let thread_id = attr_optional_string(attributes, ATTR_JOB_THREAD_ID);
    let artifacts = attributes
        .get(ATTR_JOB_ARTIFACTS)
        .and_then(|value| serde_json::from_value::<Vec<JobArtifactRef>>(value.clone()).ok())
        .unwrap_or_default();

    Some(JobManifest {
        protocol,
        system,
        job_id,
        job_title,
        run_id,
        attempt,
        stage,
        review_kind,
        status,
        thread_id,
        artifacts,
    })
}

pub fn job_context_from_session(session: &Session) -> Option<JobContext> {
    job_manifest_from_session(session).map(|manifest| JobContext::from_manifest(&manifest))
}

fn attr_string(
    attributes: &std::collections::HashMap<String, serde_json::Value>,
    key: &str,
) -> Option<String> {
    attr_optional_string(attributes, key)
}

fn attr_optional_string(
    attributes: &std::collections::HashMap<String, serde_json::Value>,
    key: &str,
) -> Option<String> {
    attributes
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::{
        ATTR_JOB_ARTIFACTS, JobArtifactRef, JobContext, JobManifest, JobProtocol, JobReviewKind,
        JobStage, JobStatus, apply_job_manifest, job_manifest_from_session,
    };
    use opensession_core::trace::{Agent, Session};

    fn make_session() -> Session {
        Session::new(
            "job-session".to_string(),
            Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        )
    }

    #[test]
    fn manifest_validation_requires_review_kind_for_review_stage() {
        let manifest = JobManifest {
            protocol: JobProtocol::AgentCommunicationProtocol,
            system: "symphony".to_string(),
            job_id: "AUTH-123".to_string(),
            job_title: "Fix auth bug".to_string(),
            run_id: "run-1".to_string(),
            attempt: 1,
            stage: JobStage::Review,
            review_kind: None,
            status: JobStatus::Pending,
            thread_id: None,
            artifacts: Vec::new(),
        };

        assert_eq!(
            manifest.validate(),
            Err("manifest.review_kind is required when stage=review".to_string())
        );
    }

    #[test]
    fn apply_and_extract_job_manifest_round_trip() {
        let mut session = make_session();
        let manifest = JobManifest {
            protocol: JobProtocol::AgentClientProtocol,
            system: "symphony".to_string(),
            job_id: "AUTH-123".to_string(),
            job_title: "Fix auth bug".to_string(),
            run_id: "run-42".to_string(),
            attempt: 2,
            stage: JobStage::Review,
            review_kind: Some(JobReviewKind::Todo),
            status: JobStatus::InProgress,
            thread_id: Some("thread-9".to_string()),
            artifacts: vec![JobArtifactRef {
                kind: "handoff".to_string(),
                label: "handoff".to_string(),
                uri: "os://artifact/handoff/123".to_string(),
                mime_type: Some("application/json".to_string()),
                metadata: None,
            }],
        };

        apply_job_manifest(&mut session, &manifest);
        let parsed = job_manifest_from_session(&session).expect("manifest should parse");
        assert_eq!(parsed, manifest);
        assert!(session.context.attributes.contains_key(ATTR_JOB_ARTIFACTS));

        let context = JobContext::from_manifest(&manifest);
        assert_eq!(context.artifact_count, 1);
        assert_eq!(context.job_id, "AUTH-123");
    }
}
