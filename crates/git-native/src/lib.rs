pub mod context;
pub mod error;
pub mod handoff_artifact_store;
pub mod ops;
pub mod refs;
pub mod store;
pub mod url;

#[cfg(test)]
pub(crate) mod test_utils;

pub use context::{extract_git_context, normalize_repo_name, GitContext};
pub use error::{GitStorageError, Result};
pub use handoff_artifact_store::{
    artifact_ref_name, list_handoff_artifact_refs, load_handoff_artifact, store_handoff_artifact,
};
pub use refs::{branch_ledger_ref, encode_branch_component, resolve_ledger_branch};
pub use store::{
    store_blob_at_ref, NativeGitStorage, PruneStats, SessionSummaryLedgerRecord,
    StoredSummaryRecord,
};
pub use url::generate_raw_url;

/// Ref prefix used for per-branch session ledgers.
pub const BRANCH_LEDGER_REF_PREFIX: &str = "refs/opensession/branches";

/// Ref used for semantic summary ledger blobs.
pub const SUMMARY_LEDGER_REF: &str = "refs/opensession/summaries";

/// Ref prefix used for handoff artifacts.
pub const HANDOFF_ARTIFACTS_REF_PREFIX: &str = "refs/opensession/handoff/artifacts";
