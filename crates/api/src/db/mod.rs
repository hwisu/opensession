//! Shared database schema, migrations, and query builders.
//!
//! Used by: Axum server, Cloudflare Worker, local DB (TUI/Daemon).

pub mod invitations;
pub mod migrations;
pub mod oauth;
pub mod sessions;
pub mod sync;
pub mod tables;
pub mod team_invite_keys;
pub mod teams;
pub mod users;

// Re-export tables for convenience
pub use tables::*;
