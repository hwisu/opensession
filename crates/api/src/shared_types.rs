use serde::{Deserialize, Serialize};

/// Sort order for session listings.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum SortOrder {
    #[default]
    Recent,
    Popular,
    Longest,
}

impl SortOrder {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Recent => "recent",
            Self::Popular => "popular",
            Self::Longest => "longest",
        }
    }
}

impl std::fmt::Display for SortOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Time range filter for queries.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum TimeRange {
    #[serde(rename = "24h")]
    Hours24,
    #[serde(rename = "7d")]
    Days7,
    #[serde(rename = "30d")]
    Days30,
    #[default]
    #[serde(rename = "all")]
    All,
}

impl TimeRange {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Hours24 => "24h",
            Self::Days7 => "7d",
            Self::Days30 => "30d",
            Self::All => "all",
        }
    }
}

impl std::fmt::Display for TimeRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Type of link between two sessions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum LinkType {
    Handoff,
    Related,
    Parent,
    Child,
}

impl LinkType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Handoff => "handoff",
            Self::Related => "related",
            Self::Parent => "parent",
            Self::Child => "child",
        }
    }
}

impl std::fmt::Display for LinkType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Safely convert `u64` to `i64`, saturating at `i64::MAX` instead of wrapping.
pub fn saturating_i64(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}
