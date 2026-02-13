//! HAIL JSONL format: streaming serialization/deserialization
//!
//! A `.hail.jsonl` file has the structure:
//! ```jsonl
//! {"type":"header","version":"hail-1.0.0","session_id":"...","agent":{...},"context":{...}}
//! {"type":"event","event_id":"e1","timestamp":"...","event_type":{...},"content":{...},...}
//! {"type":"event","event_id":"e2","timestamp":"...","event_type":{...},"content":{...},...}
//! {"type":"stats","event_count":42,"message_count":10,...}
//! ```
//!
//! The header line contains session metadata (no events).
//! Each event is one line.
//! The last line is aggregate stats (optional on write, recomputed on read if missing).

use crate::trace::{Agent, Event, Session, SessionContext, Stats};
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, Write};

/// A single line in a HAIL JSONL file
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum HailLine {
    /// First line: session metadata
    #[serde(rename = "header")]
    Header {
        version: String,
        session_id: String,
        agent: Agent,
        context: SessionContext,
    },
    /// Middle lines: one event per line
    #[serde(rename = "event")]
    Event(Event),
    /// Last line: aggregate stats
    #[serde(rename = "stats")]
    Stats(Stats),
}

/// Error types for JSONL operations
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum JsonlError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("JSON error at line {line}: {source}")]
    Json {
        line: usize,
        source: serde_json::Error,
    },
    #[error("Missing header line")]
    MissingHeader,
    #[error("Unexpected line type at line {0}: expected header")]
    UnexpectedLineType(usize),
}

/// Write a Session as HAIL JSONL to a writer
pub fn write_jsonl<W: Write>(session: &Session, mut writer: W) -> Result<(), JsonlError> {
    // Line 1: header
    let header = HailLine::Header {
        version: session.version.clone(),
        session_id: session.session_id.clone(),
        agent: session.agent.clone(),
        context: session.context.clone(),
    };
    serde_json::to_writer(&mut writer, &header)
        .map_err(|e| JsonlError::Json { line: 1, source: e })?;
    writer.write_all(b"\n")?;

    // Lines 2..N: events
    for (i, event) in session.events.iter().enumerate() {
        let line = HailLine::Event(event.clone());
        serde_json::to_writer(&mut writer, &line).map_err(|e| JsonlError::Json {
            line: i + 2,
            source: e,
        })?;
        writer.write_all(b"\n")?;
    }

    // Last line: stats
    let stats_line = HailLine::Stats(session.stats.clone());
    serde_json::to_writer(&mut writer, &stats_line).map_err(|e| JsonlError::Json {
        line: session.events.len() + 2,
        source: e,
    })?;
    writer.write_all(b"\n")?;

    Ok(())
}

/// Write a Session as HAIL JSONL to a String
pub fn to_jsonl_string(session: &Session) -> Result<String, JsonlError> {
    let mut buf = Vec::new();
    write_jsonl(session, &mut buf)?;
    // Safe: serde_json always produces valid UTF-8
    Ok(String::from_utf8(buf).unwrap())
}

/// Read a Session from HAIL JSONL reader
pub fn read_jsonl<R: BufRead>(reader: R) -> Result<Session, JsonlError> {
    let mut lines = reader.lines();

    // Line 1: header
    let header_str = lines.next().ok_or(JsonlError::MissingHeader)??;
    let header: HailLine =
        serde_json::from_str(&header_str).map_err(|e| JsonlError::Json { line: 1, source: e })?;

    let (version, session_id, agent, context) = match header {
        HailLine::Header {
            version,
            session_id,
            agent,
            context,
        } => (version, session_id, agent, context),
        _ => return Err(JsonlError::UnexpectedLineType(1)),
    };

    let mut events = Vec::new();
    let mut stats = None;
    let mut line_num = 1usize;

    for line_result in lines {
        line_num += 1;
        let line_str = line_result?;
        if line_str.is_empty() {
            continue;
        }

        let hail_line: HailLine =
            serde_json::from_str(&line_str).map_err(|e| JsonlError::Json {
                line: line_num,
                source: e,
            })?;

        match hail_line {
            HailLine::Event(event) => events.push(event),
            HailLine::Stats(s) => stats = Some(s),
            HailLine::Header { .. } => {
                // Ignore duplicate headers
            }
        }
    }

    let has_stats = stats.is_some();
    let mut session = Session {
        version,
        session_id,
        agent,
        context,
        events,
        stats: stats.unwrap_or_default(),
    };

    // If no stats line was present, recompute
    if !has_stats {
        session.recompute_stats();
    }

    Ok(session)
}

/// Read a Session from a HAIL JSONL string
pub fn from_jsonl_str(s: &str) -> Result<Session, JsonlError> {
    read_jsonl(io::BufReader::new(s.as_bytes()))
}

/// Read only the header (first line) from HAIL JSONL — useful for listing sessions
/// without loading all events
pub fn read_header<R: BufRead>(
    reader: R,
) -> Result<(String, String, Agent, SessionContext), JsonlError> {
    let mut lines = reader.lines();
    let header_str = lines.next().ok_or(JsonlError::MissingHeader)??;
    let header: HailLine =
        serde_json::from_str(&header_str).map_err(|e| JsonlError::Json { line: 1, source: e })?;

    match header {
        HailLine::Header {
            version,
            session_id,
            agent,
            context,
        } => Ok((version, session_id, agent, context)),
        _ => Err(JsonlError::UnexpectedLineType(1)),
    }
}

/// Read header + stats (first and last line) without loading events.
/// Returns (version, session_id, agent, context, stats_or_none)
pub fn read_header_and_stats(
    data: &str,
) -> Result<(String, String, Agent, SessionContext, Option<Stats>), JsonlError> {
    let mut lines = data.lines();

    // First line: header
    let header_str = lines.next().ok_or(JsonlError::MissingHeader)?;
    let header: HailLine =
        serde_json::from_str(header_str).map_err(|e| JsonlError::Json { line: 1, source: e })?;

    let (version, session_id, agent, context) = match header {
        HailLine::Header {
            version,
            session_id,
            agent,
            context,
        } => (version, session_id, agent, context),
        _ => return Err(JsonlError::UnexpectedLineType(1)),
    };

    // Try to read last non-empty line for stats
    let mut last_line = None;
    let mut line_num = 1usize;
    for line in lines {
        line_num += 1;
        if !line.is_empty() {
            last_line = Some((line_num, line));
        }
    }

    let stats = if let Some((_ln, last)) = last_line {
        match serde_json::from_str::<HailLine>(last) {
            Ok(HailLine::Stats(s)) => Some(s),
            Ok(_) => None,
            Err(_) => None, // Last line isn't stats, that's ok (will recompute)
        }
    } else {
        None
    };

    Ok((version, session_id, agent, context, stats))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::{Content, EventType};
    use chrono::Utc;
    use std::collections::HashMap;

    fn make_test_session() -> Session {
        let mut session = Session::new(
            "test-jsonl-123".to_string(),
            Agent {
                provider: "anthropic".to_string(),
                model: "claude-opus-4-6".to_string(),
                tool: "claude-code".to_string(),
                tool_version: Some("1.2.3".to_string()),
            },
        );
        session.context.title = Some("Test JSONL session".to_string());

        let ts = Utc::now();
        session.events.push(Event {
            event_id: "e1".to_string(),
            timestamp: ts,
            event_type: EventType::UserMessage,
            task_id: None,
            content: Content::text("Hello, can you help me?"),
            duration_ms: None,
            attributes: HashMap::new(),
        });
        session.events.push(Event {
            event_id: "e2".to_string(),
            timestamp: ts,
            event_type: EventType::AgentMessage,
            task_id: None,
            content: Content::text("Sure! What do you need?"),
            duration_ms: None,
            attributes: HashMap::new(),
        });
        session.events.push(Event {
            event_id: "e3".to_string(),
            timestamp: ts,
            event_type: EventType::FileRead {
                path: "/tmp/test.rs".to_string(),
            },
            task_id: Some("t1".to_string()),
            content: Content::code("fn main() {}", Some("rust".to_string())),
            duration_ms: Some(50),
            attributes: HashMap::new(),
        });

        session.recompute_stats();
        session
    }

    #[test]
    fn test_jsonl_roundtrip() {
        let session = make_test_session();
        let jsonl = to_jsonl_string(&session).unwrap();

        // Should have exactly 5 lines (header + 3 events + stats)
        let lines: Vec<&str> = jsonl.trim().lines().collect();
        assert_eq!(lines.len(), 5);

        // First line should be header
        assert!(lines[0].contains("\"type\":\"header\""));
        assert!(lines[0].contains("hail-1.0.0"));

        // Middle lines should be events
        assert!(lines[1].contains("\"type\":\"event\""));
        assert!(lines[2].contains("\"type\":\"event\""));
        assert!(lines[3].contains("\"type\":\"event\""));

        // Last line should be stats
        assert!(lines[4].contains("\"type\":\"stats\""));

        // Roundtrip
        let parsed = from_jsonl_str(&jsonl).unwrap();
        assert_eq!(parsed.version, "hail-1.0.0");
        assert_eq!(parsed.session_id, "test-jsonl-123");
        assert_eq!(parsed.events.len(), 3);
        assert_eq!(parsed.stats.message_count, 2);
        assert_eq!(parsed.stats.tool_call_count, 1);
        assert_eq!(parsed.stats.event_count, 3);
        assert_eq!(parsed.agent.tool, "claude-code");
        assert_eq!(parsed.context.title, Some("Test JSONL session".to_string()));
    }

    #[test]
    fn test_jsonl_empty_session() {
        let session = Session::new(
            "empty-session".to_string(),
            Agent {
                provider: "openai".to_string(),
                model: "gpt-4o".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );

        let jsonl = to_jsonl_string(&session).unwrap();
        let lines: Vec<&str> = jsonl.trim().lines().collect();
        assert_eq!(lines.len(), 2); // header + stats only

        let parsed = from_jsonl_str(&jsonl).unwrap();
        assert_eq!(parsed.events.len(), 0);
        assert_eq!(parsed.stats.event_count, 0);
    }

    #[test]
    fn test_read_header_only() {
        let session = make_test_session();
        let jsonl = to_jsonl_string(&session).unwrap();

        let (version, session_id, agent, context) =
            read_header(io::BufReader::new(jsonl.as_bytes())).unwrap();

        assert_eq!(version, "hail-1.0.0");
        assert_eq!(session_id, "test-jsonl-123");
        assert_eq!(agent.tool, "claude-code");
        assert_eq!(context.title, Some("Test JSONL session".to_string()));
    }

    #[test]
    fn test_read_header_and_stats() {
        let session = make_test_session();
        let jsonl = to_jsonl_string(&session).unwrap();

        let (version, session_id, _agent, _context, stats) = read_header_and_stats(&jsonl).unwrap();

        assert_eq!(version, "hail-1.0.0");
        assert_eq!(session_id, "test-jsonl-123");
        let stats = stats.unwrap();
        assert_eq!(stats.event_count, 3);
        assert_eq!(stats.message_count, 2);
    }

    #[test]
    fn test_missing_stats_recomputes() {
        // Manually construct JSONL without stats line
        let session = make_test_session();
        let jsonl = to_jsonl_string(&session).unwrap();

        // Remove last line (stats)
        let without_stats: String = jsonl.lines().take(4).collect::<Vec<_>>().join("\n") + "\n";

        let parsed = from_jsonl_str(&without_stats).unwrap();
        assert_eq!(parsed.stats.event_count, 3);
        assert_eq!(parsed.stats.message_count, 2);
    }

    #[test]
    fn test_hailline_serde_tag() {
        let header = HailLine::Header {
            version: "hail-1.0.0".to_string(),
            session_id: "s1".to_string(),
            agent: Agent {
                provider: "test".to_string(),
                model: "test".to_string(),
                tool: "test".to_string(),
                tool_version: None,
            },
            context: SessionContext::default(),
        };

        let json = serde_json::to_string(&header).unwrap();
        assert!(json.contains("\"type\":\"header\""));

        let parsed: HailLine = serde_json::from_str(&json).unwrap();
        match parsed {
            HailLine::Header { version, .. } => assert_eq!(version, "hail-1.0.0"),
            _ => panic!("Expected Header"),
        }
    }

    #[test]
    fn test_jsonl_preserves_task_ids() {
        let session = make_test_session();
        let jsonl = to_jsonl_string(&session).unwrap();
        let parsed = from_jsonl_str(&jsonl).unwrap();

        // Event e3 has task_id "t1"
        assert_eq!(parsed.events[2].task_id, Some("t1".to_string()));
        // Events e1, e2 have no task_id
        assert_eq!(parsed.events[0].task_id, None);
    }
}
