pub mod error;
pub mod ops;
pub mod store;
pub mod url;

#[cfg(test)]
pub(crate) mod test_utils;

pub use error::{GitStorageError, Result};
pub use store::NativeGitStorage;
pub use url::generate_raw_url;

/// Branch name used for storing session data.
pub const SESSIONS_BRANCH: &str = "opensession/sessions";

/// Ref path for the sessions branch.
pub const SESSIONS_REF: &str = "refs/heads/opensession/sessions";
