pub mod error;
pub mod handoff_artifact_store;
pub mod ops;
pub mod store;
pub mod url;

#[cfg(test)]
pub(crate) mod test_utils;

pub use error::{GitStorageError, Result};
pub use handoff_artifact_store::{
    artifact_ref_name, list_handoff_artifact_refs, load_handoff_artifact, store_handoff_artifact,
};
pub use store::{store_blob_at_ref, NativeGitStorage, PruneStats};
pub use url::generate_raw_url;

/// Branch name used for storing session data.
pub const SESSIONS_BRANCH: &str = "opensession/sessions";

/// Ref path for the sessions branch.
pub const SESSIONS_REF: &str = "refs/heads/opensession/sessions";

/// Ref prefix used for handoff artifacts.
pub const HANDOFF_ARTIFACTS_REF_PREFIX: &str = "refs/opensession/handoff/artifacts";
