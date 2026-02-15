//! Deployment profile flags shared by server and worker runtimes.

/// Env var controlling anonymous public feed listing on the Axum server.
pub const ENV_PUBLIC_FEED_ENABLED: &str = "OPENSESSION_PUBLIC_FEED_ENABLED";

/// Env var controlling whether team APIs are mounted on the Worker runtime.
pub const ENV_TEAM_API_ENABLED: &str = "ENABLE_TEAM_API";

/// Parse a human-friendly boolean env flag value.
///
/// Accepted truthy values:
/// - `1`
/// - `true`
/// - `yes`
/// - `on`
pub fn parse_bool_flag(raw: Option<&str>, default: bool) -> bool {
    raw.map(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
    .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::parse_bool_flag;

    #[test]
    fn parses_truthy_values() {
        for value in ["1", "true", "TRUE", "yes", "on"] {
            assert!(parse_bool_flag(Some(value), false));
        }
    }

    #[test]
    fn parses_falsy_values() {
        for value in ["0", "false", "no", "off", ""] {
            assert!(!parse_bool_flag(Some(value), true));
        }
    }

    #[test]
    fn uses_default_for_missing_value() {
        assert!(parse_bool_flag(None, true));
        assert!(!parse_bool_flag(None, false));
    }
}
