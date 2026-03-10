use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

pub(super) fn parse_timestamp(ts: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(ts)
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S%.f")
                .map(|ndt| ndt.and_utc())
        })
        .or_else(|_| {
            ts.parse::<f64>()
                .ok()
                .and_then(|ms| DateTime::from_timestamp_millis(ms as i64))
                .ok_or_else(|| anyhow::anyhow!("Not a timestamp"))
        })
        .with_context(|| format!("Failed to parse Cursor timestamp: {ts}"))
}
