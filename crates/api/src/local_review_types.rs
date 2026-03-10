use crate::{JobArtifactRef, JobProtocol, JobReviewKind, JobStatus};
use opensession_core::trace::Session;
use serde::{Deserialize, Serialize};

/// Local review bundle generated from a PR range.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LocalReviewBundle {
    pub review_id: String,
    pub generated_at: String,
    pub pr: LocalReviewPrMeta,
    #[serde(default)]
    pub commits: Vec<LocalReviewCommit>,
    #[serde(default)]
    pub sessions: Vec<LocalReviewSession>,
}

/// PR metadata for a local review bundle.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LocalReviewPrMeta {
    pub url: String,
    pub owner: String,
    pub repo: String,
    pub number: u64,
    pub remote: String,
    pub base_sha: String,
    pub head_sha: String,
}

/// Reviewer-focused digest extracted from mapped sessions for a commit.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LocalReviewReviewerQa {
    pub question: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
}

/// Reviewer-focused digest extracted from mapped sessions for a commit.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LocalReviewReviewerDigest {
    #[serde(default)]
    pub qa: Vec<LocalReviewReviewerQa>,
    #[serde(default)]
    pub modified_files: Vec<String>,
    #[serde(default)]
    pub test_files: Vec<String>,
}

/// Commit row in a local review bundle.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LocalReviewCommit {
    pub sha: String,
    pub title: String,
    pub author_name: String,
    pub author_email: String,
    pub authored_at: String,
    #[serde(default)]
    pub session_ids: Vec<String>,
    #[serde(default)]
    pub reviewer_digest: LocalReviewReviewerDigest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_summary: Option<LocalReviewSemanticSummary>,
}

/// Layer/file summary section for local review semantic payloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LocalReviewLayerFileChange {
    pub layer: String,
    pub summary: String,
    #[serde(default)]
    pub files: Vec<String>,
}

/// Commit-level semantic summary used when session mappings are weak or absent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LocalReviewSemanticSummary {
    pub changes: String,
    pub auth_security: String,
    #[serde(default)]
    pub layer_file_changes: Vec<LocalReviewLayerFileChange>,
    pub source_kind: String,
    pub generation_kind: String,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(type = "any[]"))]
    pub diff_tree: Vec<serde_json::Value>,
}

/// Session payload mapped into a local review bundle.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LocalReviewSession {
    pub session_id: String,
    pub ledger_ref: String,
    pub hail_path: String,
    #[serde(default)]
    pub commit_shas: Vec<String>,
    #[cfg_attr(feature = "ts", ts(type = "any"))]
    pub session: Session,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct JobReviewBundleJob {
    pub protocol: JobProtocol,
    pub system: String,
    pub job_id: String,
    pub job_title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct JobReviewSelectedReview {
    pub session_id: String,
    pub run_id: String,
    pub attempt: i64,
    pub kind: JobReviewKind,
    pub status: JobStatus,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct JobReviewRun {
    pub run_id: String,
    pub attempt: i64,
    pub status: JobStatus,
    #[serde(default)]
    pub sessions: Vec<LocalReviewSession>,
    #[serde(default)]
    pub artifacts: Vec<JobArtifactRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct JobReviewBundle {
    pub job: JobReviewBundleJob,
    pub selected_review: JobReviewSelectedReview,
    #[serde(default)]
    pub runs: Vec<JobReviewRun>,
    pub review_digest: LocalReviewReviewerDigest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_summary: Option<LocalReviewSemanticSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handoff_artifact_uri: Option<String>,
}
