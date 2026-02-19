use std::io::Write;

use anyhow::Result;
use opensession_core::handoff::{
    generate_handoff_hail, generate_handoff_markdown_v2, generate_merged_handoff_markdown_v2,
    merge_summaries, HandoffSummary, HandoffValidationReport,
};
use opensession_core::Session;

/// Output format for session data.
#[derive(Debug, Clone, PartialEq, clap::ValueEnum)]
pub enum OutputFormat {
    Text,
    Markdown,
    Jsonl,
    Json,
    Hail,
    /// NDJSON stream: each line is an independent JSON envelope
    Stream,
}

/// Structured output envelope (Terraform/ripgrep pattern).
#[derive(Debug, serde::Serialize)]
pub struct OutputEnvelope {
    pub version: &'static str,
    #[serde(rename = "type")]
    pub data_type: String,
    #[serde(rename = "@message")]
    pub message: String,
    #[serde(rename = "@timestamp")]
    pub timestamp: String,
    pub data: serde_json::Value,
}

impl OutputEnvelope {
    pub fn new(data_type: &str, message: &str, data: serde_json::Value) -> Self {
        Self {
            version: "0.1",
            data_type: data_type.to_string(),
            message: message.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            data,
        }
    }
}

/// Machine-readable error for JSON output (Terraform diagnostic pattern).
#[derive(Debug, serde::Serialize)]
pub struct CliDiagnostic {
    pub version: &'static str,
    #[serde(rename = "type")]
    pub data_type: &'static str,
    #[serde(rename = "@level")]
    pub level: &'static str,
    #[serde(rename = "@message")]
    pub message: String,
    pub diagnostic: DiagnosticDetail,
}

#[derive(Debug, serde::Serialize)]
pub struct DiagnosticDetail {
    pub severity: &'static str,
    pub summary: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

impl CliDiagnostic {
    pub fn error(summary: &str, detail: &str, suggestion: Option<&str>) -> Self {
        Self {
            version: "0.1",
            data_type: "error",
            level: "error",
            message: summary.to_string(),
            diagnostic: DiagnosticDetail {
                severity: "error",
                summary: summary.to_string(),
                detail: detail.to_string(),
                suggestion: suggestion.map(String::from),
            },
        }
    }

    #[cfg(test)]
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

#[derive(Debug, Default, Clone)]
pub struct RenderOptions<'a> {
    pub validation_reports: Option<&'a [HandoffValidationReport]>,
}

/// Render sessions in the specified format.
pub fn render_output(
    sessions: &[Session],
    format: &OutputFormat,
    writer: &mut dyn Write,
) -> Result<()> {
    render_output_with_options(sessions, format, writer, &RenderOptions::default())
}

pub fn render_output_with_options(
    sessions: &[Session],
    format: &OutputFormat,
    writer: &mut dyn Write,
    options: &RenderOptions<'_>,
) -> Result<()> {
    // Hail format operates directly on sessions, not summaries
    if matches!(format, OutputFormat::Hail) {
        for (i, session) in sessions.iter().enumerate() {
            let hail = generate_handoff_hail(session);
            let jsonl = hail.to_jsonl()?;
            if i > 0 {
                writeln!(writer)?;
            }
            write!(writer, "{jsonl}")?;
        }
        return Ok(());
    }

    // Pre-compute summaries once for all other formats
    let summaries: Vec<HandoffSummary> =
        sessions.iter().map(HandoffSummary::from_session).collect();

    match format {
        OutputFormat::Text | OutputFormat::Markdown => {
            let md = if summaries.len() == 1 {
                generate_handoff_markdown_v2(&summaries[0])
            } else {
                let merged = merge_summaries(&summaries);
                generate_merged_handoff_markdown_v2(&merged)
            };
            write!(writer, "{md}")?;
        }
        OutputFormat::Jsonl => {
            for summary in &summaries {
                let report = find_validation_report(summary, options.validation_reports);
                let json = serde_json::to_string(&summary_to_json_v2(summary, report))?;
                writeln!(writer, "{json}")?;
            }
        }
        OutputFormat::Json => {
            let values: Vec<serde_json::Value> = summaries
                .iter()
                .map(|summary| {
                    let report = find_validation_report(summary, options.validation_reports);
                    summary_to_json_v2(summary, report)
                })
                .collect();
            let json = serde_json::to_string_pretty(&values)?;
            write!(writer, "{json}")?;
        }
        OutputFormat::Stream => {
            for summary in &summaries {
                let report = find_validation_report(summary, options.validation_reports);
                let data = summary_to_json_v2(summary, report);
                let envelope =
                    OutputEnvelope::new("session_summary", &format_summary_message(summary), data);
                let json = serde_json::to_string(&envelope)?;
                writeln!(writer, "{json}")?;
            }
        }
        OutputFormat::Hail => unreachable!(),
    }
    Ok(())
}

fn format_summary_message(s: &HandoffSummary) -> String {
    let duration = opensession_core::handoff::format_duration(s.duration_seconds);
    let tokens = s.stats.total_input_tokens + s.stats.total_output_tokens;
    let tok_str = if tokens >= 1000 {
        format!("{:.1}K tokens", tokens as f64 / 1000.0)
    } else {
        format!("{tokens} tokens")
    };
    format!("{} session ({duration}, {tok_str})", s.tool)
}

fn summary_to_json_v2(
    s: &HandoffSummary,
    validation: Option<&HandoffValidationReport>,
) -> serde_json::Value {
    let objective_json = if s.objective_undefined_reason.is_some() {
        serde_json::Value::Null
    } else {
        serde_json::Value::String(s.objective.clone())
    };
    let mut json = serde_json::json!({
        "session_id": s.source_session_id,
        "tool": s.tool,
        "model": s.model,
        "objective": objective_json,
        "objective_undefined_reason": s.objective_undefined_reason,
        "duration_seconds": s.duration_seconds,
        "stats": {
            "event_count": s.stats.event_count,
            "message_count": s.stats.message_count,
            "tool_call_count": s.stats.tool_call_count,
            "total_input_tokens": s.stats.total_input_tokens,
            "total_output_tokens": s.stats.total_output_tokens,
        },
        "files_modified": s.files_modified.iter().map(|f| {
            serde_json::json!({"path": f.path, "action": f.action})
        }).collect::<Vec<_>>(),
        "files_read": s.files_read,
        "key_conversations": s.key_conversations.iter().map(|conv| {
            serde_json::json!({"user": conv.user, "agent": conv.agent})
        }).collect::<Vec<_>>(),
        "user_messages": s.user_messages,
        "execution_contract": serde_json::to_value(&s.execution_contract).unwrap_or(serde_json::Value::Null),
        "uncertainty": serde_json::to_value(&s.uncertainty).unwrap_or(serde_json::Value::Null),
        "verification": serde_json::to_value(&s.verification).unwrap_or(serde_json::Value::Null),
        "evidence": serde_json::to_value(&s.evidence).unwrap_or(serde_json::Value::Null),
        "work_packages": serde_json::to_value(&s.work_packages).unwrap_or(serde_json::Value::Null),
        "undefined_fields": serde_json::to_value(&s.undefined_fields).unwrap_or(serde_json::Value::Null),
    });
    if let Some(validation) = validation {
        json["validation"] = serde_json::to_value(validation).unwrap_or(serde_json::Value::Null);
    }
    json
}

fn find_validation_report<'a>(
    summary: &HandoffSummary,
    reports: Option<&'a [HandoffValidationReport]>,
) -> Option<&'a HandoffValidationReport> {
    reports.and_then(|items| {
        items
            .iter()
            .find(|report| report.session_id == summary.source_session_id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_envelope_new() {
        let data = serde_json::json!({"key": "value"});
        let envelope = OutputEnvelope::new("test_type", "test message", data);
        assert_eq!(envelope.version, "0.1");
        assert_eq!(envelope.data_type, "test_type");
        assert_eq!(envelope.message, "test message");
        assert!(!envelope.timestamp.is_empty());
    }

    #[test]
    fn test_cli_diagnostic_error() {
        let diag = CliDiagnostic::error(
            "Not found",
            "Session not found at HEAD~5",
            Some("opensession index"),
        );
        assert_eq!(diag.version, "0.1");
        assert_eq!(diag.data_type, "error");
        assert_eq!(diag.level, "error");
        assert_eq!(diag.diagnostic.severity, "error");
        assert_eq!(
            diag.diagnostic.suggestion.as_deref(),
            Some("opensession index")
        );

        let json = diag.to_json();
        assert!(json.contains("Not found"));
        assert!(json.contains("opensession index"));
    }

    #[test]
    fn test_cli_diagnostic_no_suggestion() {
        let diag = CliDiagnostic::error("Error", "Something went wrong", None);
        let json = diag.to_json();
        assert!(!json.contains("suggestion"));
    }

    // ── Test helpers ────────────────────────────────────────────────────

    fn make_test_session() -> Session {
        use opensession_core::testing;
        use opensession_core::{Content, Event, EventType, Stats};
        use std::collections::HashMap;

        let mut session = Session::new("test-session-1".to_string(), testing::agent());
        session.context.title = Some("Fix authentication bug".to_string());
        session.stats = Stats {
            event_count: 10,
            message_count: 5,
            tool_call_count: 3,
            task_count: 1,
            duration_seconds: 120,
            total_input_tokens: 2000,
            total_output_tokens: 1500,
            ..Default::default()
        };
        // Add a user message event so HandoffSummary can extract objective
        let ts = session.context.created_at;
        session.events.push(Event {
            event_id: "e1".to_string(),
            timestamp: ts,
            event_type: EventType::UserMessage,
            task_id: None,
            content: Content::text("Fix the auth bug"),
            duration_ms: None,
            attributes: HashMap::new(),
        });
        session.events.push(Event {
            event_id: "e2".to_string(),
            timestamp: ts + chrono::Duration::seconds(120),
            event_type: EventType::FileEdit {
                path: "src/auth.rs".to_string(),
                diff: None,
            },
            task_id: None,
            content: Content::empty(),
            duration_ms: None,
            attributes: HashMap::new(),
        });
        session
    }

    // ── render_output tests ─────────────────────────────────────────────

    #[test]
    fn test_render_output_markdown_single() {
        let session = make_test_session();
        let mut buf = Vec::new();
        render_output(&[session], &OutputFormat::Markdown, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Fix the auth bug"));
    }

    #[test]
    fn test_render_output_markdown_multiple() {
        let s1 = make_test_session();
        let mut s2 = make_test_session();
        s2.session_id = "test-session-2".to_string();
        let mut buf = Vec::new();
        render_output(&[s1, s2], &OutputFormat::Markdown, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        // Merged handoff should produce output
        assert!(!output.is_empty());
    }

    #[test]
    fn test_render_output_jsonl() {
        let session = make_test_session();
        let mut buf = Vec::new();
        render_output(&[session], &OutputFormat::Jsonl, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = output.trim().lines().collect();
        assert_eq!(lines.len(), 1);
        let parsed: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(parsed["tool"], "claude-code");
    }

    #[test]
    fn test_render_output_json() {
        let session = make_test_session();
        let mut buf = Vec::new();
        render_output(&[session], &OutputFormat::Json, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        let arr = parsed.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["tool"], "claude-code");
    }

    #[test]
    fn test_render_output_hail() {
        let session = make_test_session();
        let mut buf = Vec::new();
        render_output(&[session], &OutputFormat::Hail, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        // HAIL JSONL starts with header line
        assert!(!output.is_empty());
        // Should be valid JSONL (multiple lines of JSON)
        for line in output.lines() {
            assert!(serde_json::from_str::<serde_json::Value>(line).is_ok());
        }
    }

    #[test]
    fn test_render_output_stream() {
        let session = make_test_session();
        let mut buf = Vec::new();
        render_output(&[session], &OutputFormat::Stream, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
        assert_eq!(parsed["version"], "0.1");
        assert_eq!(parsed["type"], "session_summary");
        assert!(parsed["@message"].as_str().unwrap().contains("claude-code"));
    }

    #[test]
    fn test_render_output_empty() {
        let mut buf = Vec::new();
        // Empty sessions for JSONL: no lines
        render_output(&[], &OutputFormat::Jsonl, &mut buf).unwrap();
        assert!(buf.is_empty());
        // Empty sessions for JSON: empty array
        render_output(&[], &OutputFormat::Json, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed.as_array().unwrap().len(), 0);
    }

    // ── summary_to_json tests ───────────────────────────────────────────

    #[test]
    fn test_summary_to_json_v2_populated() {
        let session = make_test_session();
        let summary = HandoffSummary::from_session(&session);
        let json = summary_to_json_v2(&summary, None);
        assert_eq!(json["session_id"], "test-session-1");
        assert_eq!(json["tool"], "claude-code");
        assert_eq!(json["model"], "claude-opus-4-6");
        assert!(json["objective"].is_string());
        assert!(json["stats"]["message_count"].as_u64().is_some());
        assert!(json["files_modified"].as_array().is_some());
        assert!(json.get("execution_contract").is_some());
        assert!(json.get("verification").is_some());
        assert!(json.get("undefined_fields").is_some());
        assert!(json.get("task_summaries").is_none());
        assert!(json.get("errors").is_none());
        assert!(json.get("shell_commands").is_none());
    }

    #[test]
    fn test_summary_to_json_v2_empty_collections() {
        use opensession_core::{Agent, Session};
        let session = Session::new(
            "empty-1".to_string(),
            Agent {
                provider: "test".to_string(),
                model: "test".to_string(),
                tool: "test-tool".to_string(),
                tool_version: None,
            },
        );
        let summary = HandoffSummary::from_session(&session);
        let json = summary_to_json_v2(&summary, None);
        assert!(json["objective"].is_null());
        assert!(json["objective_undefined_reason"].is_string());
        assert!(json["files_modified"].as_array().unwrap().is_empty());
        assert!(json["files_read"].as_array().unwrap().is_empty());
        assert!(json["verification"]["checks_run"].as_array().is_some());
        assert!(json["undefined_fields"].as_array().is_some());
    }

    #[test]
    fn test_render_output_default_markdown_is_v2() {
        let session = make_test_session();
        let mut buf = Vec::new();
        render_output(&[session], &OutputFormat::Markdown, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("## Next Actions (ordered)"));
        assert!(output.contains("## Evidence Index"));
    }

    // ── format_summary_message tests ────────────────────────────────────

    #[test]
    fn test_format_summary_message_small_tokens() {
        let session = make_test_session();
        let mut summary = HandoffSummary::from_session(&session);
        summary.stats.total_input_tokens = 200;
        summary.stats.total_output_tokens = 300;
        let msg = format_summary_message(&summary);
        assert!(msg.contains("500 tokens"));
        assert!(msg.contains("claude-code"));
    }

    #[test]
    fn test_format_summary_message_large_tokens() {
        let session = make_test_session();
        let mut summary = HandoffSummary::from_session(&session);
        summary.stats.total_input_tokens = 5000;
        summary.stats.total_output_tokens = 3000;
        let msg = format_summary_message(&summary);
        assert!(msg.contains("8.0K tokens"));
    }

    #[test]
    fn test_format_summary_message_tool_name() {
        let session = make_test_session();
        let summary = HandoffSummary::from_session(&session);
        let msg = format_summary_message(&summary);
        assert!(msg.starts_with("claude-code session"));
    }
}
