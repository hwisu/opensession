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

        // auth (12)
        $mac!(auth::register_email);
        $mac!(auth::register_duplicate_email);
        $mac!(auth::register_duplicate_nickname);
        $mac!(auth::register_bad_password);
        $mac!(auth::login);
        $mac!(auth::verify_jwt);
        $mac!(auth::verify_api_key);
        $mac!(auth::me_endpoint);
        $mac!(auth::refresh_token);
        $mac!(auth::logout);
        $mac!(auth::change_password);
        $mac!(auth::regenerate_key);

        // sessions (10)
        $mac!(sessions::upload_session);
        $mac!(sessions::upload_requires_membership);
        $mac!(sessions::list_sessions);
        $mac!(sessions::list_sessions_pagination);
        $mac!(sessions::list_sessions_search);
        $mac!(sessions::list_sessions_sort);
        $mac!(sessions::get_session_detail);
        $mac!(sessions::get_session_not_found);
        $mac!(sessions::get_session_raw);
        $mac!(sessions::linked_sessions);

        // teams (11)
        $mac!(teams::create_team);
        $mac!(teams::create_team_any_user);
        $mac!(teams::list_teams);
        $mac!(teams::get_team_detail);
        $mac!(teams::update_team);
        $mac!(teams::update_team_non_admin);
        $mac!(teams::add_member);
        $mac!(teams::add_member_duplicate);
        $mac!(teams::remove_member);
        $mac!(teams::remove_member_non_admin);
        $mac!(teams::team_session_scoping);

        // sync (3)
        $mac!(sync::sync_pull_basic);
        $mac!(sync::sync_pull_cursor);
        $mac!(sync::sync_pull_non_member);
    };
}

/// Invoke `$mac!(module::name)` for Docker-only specs.
/// Currently empty — all specs are now common across targets.
#[macro_export]
macro_rules! for_each_docker_only_spec {
    ($mac:ident) => {};
}
