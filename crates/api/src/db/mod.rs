//! Shared database schema, migrations, and query builders.
//!
//! Used by: Axum server, Cloudflare Worker, local DB (TUI/Daemon).

pub mod api_keys;
pub mod migrations;
pub mod oauth;
pub mod sessions;
pub mod tables;
pub mod users;

// Re-export tables for convenience
pub use tables::*;
