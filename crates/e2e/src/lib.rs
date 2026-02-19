pub mod client;
pub mod fixtures;
pub mod runner;
pub mod specs;

/// Invoke `$mac!(module::name)` for every common (all-targets) E2E spec.
///
/// This is the **single source of truth** for the spec list. Adding a new spec
/// here automatically registers it in `runner::run_all`, `tests/docker.rs`, and
/// `tests/worker.rs`.
#[macro_export]
macro_rules! for_each_spec {
    ($mac:ident) => {
        // health (1)
        $mac!(health::health_check);

        // public sessions (3)
        $mac!(sessions::list_sessions_public);
        $mac!(sessions::get_session_not_found_public);
        $mac!(sessions::get_session_raw_not_found_public);
    };
}
