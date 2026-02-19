use anyhow::{anyhow, Context, Result};
use opensession_core::trace::Session;
use std::fmt;
use std::io::Write;

use crate::SessionParser;

/// Parser candidate ranked by confidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseCandidate {
    pub id: String,
    pub confidence: u8,
    pub reason: String,
}

/// Parser preview output used by the ingest API.
#[derive(Debug, Clone)]
pub struct ParsePreview {
    pub parser_used: String,
    pub parser_candidates: Vec<ParseCandidate>,
    pub session: Session,
    pub warnings: Vec<String>,
    pub native_adapter: Option<String>,
}

/// Structured parse error for ingest preview.
#[derive(Debug, Clone)]
pub enum ParseError {
    InvalidParserHint {
        hint: String,
    },
    ParserSelectionRequired {
        message: String,
        parser_candidates: Vec<ParseCandidate>,
    },
    ParseFailed {
        message: String,
        parser_candidates: Vec<ParseCandidate>,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidParserHint { hint } => write!(f, "unsupported parser_hint: {hint}"),
            Self::ParserSelectionRequired { message, .. } => f.write_str(message),
            Self::ParseFailed { message, .. } => f.write_str(message),
        }
    }
}

impl std::error::Error for ParseError {}

const SUPPORTED_PARSER_IDS: &[&str] = &[
    "hail",
    "codex",
    "claude-code",
    "gemini",
    "amp",
    "cline",
    "cursor",
    "opencode",
];

/// Detect parser candidates from filename and content text.
pub fn detect_candidates(filename: &str, content: &str) -> Vec<ParseCandidate> {
    let mut candidates: Vec<ParseCandidate> = Vec::new();
    let lower_name = filename.to_ascii_lowercase();
    let trimmed = content.trim();

    if lower_name.ends_with(".hail.jsonl") {
        add_candidate(&mut candidates, "hail", 95, "filename suffix .hail.jsonl");
    }
    if lower_name.ends_with(".jsonl") {
        add_candidate(&mut candidates, "hail", 70, "jsonl extension");
        add_candidate(&mut candidates, "codex", 64, "jsonl extension");
        add_candidate(&mut candidates, "claude-code", 62, "jsonl extension");
        add_candidate(&mut candidates, "gemini", 50, "jsonl extension");
    }
    if lower_name.ends_with(".json") {
        add_candidate(&mut candidates, "gemini", 56, "json extension");
        add_candidate(&mut candidates, "amp", 46, "json extension");
        add_candidate(&mut candidates, "opencode", 44, "json extension");
        add_candidate(&mut candidates, "hail", 34, "json extension");
    }
    if lower_name.ends_with(".vscdb") {
        add_candidate(&mut candidates, "cursor", 92, "vscdb extension");
    }
    if lower_name.ends_with("api_conversation_history.json") {
        add_candidate(
            &mut candidates,
            "cline",
            88,
            "Cline conversation entrypoint filename",
        );
    }

    if looks_like_hail_jsonl(trimmed) {
        add_candidate(&mut candidates, "hail", 100, "HAIL header line");
    }
    if looks_like_hail_json(trimmed) {
        add_candidate(&mut candidates, "hail", 86, "HAIL JSON object fields");
    }
    if looks_like_codex_jsonl(trimmed) {
        add_candidate(&mut candidates, "codex", 90, "Codex event markers");
    }
    if looks_like_claude_jsonl(trimmed) {
        add_candidate(
            &mut candidates,
            "claude-code",
            88,
            "Claude message record markers",
        );
    }
    if looks_like_gemini_json(trimmed) {
        add_candidate(
            &mut candidates,
            "gemini",
            84,
            "Gemini session schema fields",
        );
    }
    if looks_like_amp_json(trimmed) {
        add_candidate(&mut candidates, "amp", 66, "Amp thread schema fields");
    }
    if looks_like_opencode_json(trimmed) {
        add_candidate(
            &mut candidates,
            "opencode",
            60,
            "OpenCode provider/model schema fields",
        );
    }

    candidates.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then_with(|| a.id.cmp(&b.id))
    });
    candidates
}

/// Parse content bytes with hint + detector fallback.
pub fn preview_parse_bytes(
    filename: &str,
    content_bytes: &[u8],
    parser_hint: Option<&str>,
) -> Result<ParsePreview, ParseError> {
    let content = std::str::from_utf8(content_bytes).map_err(|_| ParseError::ParseFailed {
        message: "input is not valid UTF-8 text".to_string(),
        parser_candidates: Vec::new(),
    })?;

    let parser_candidates = detect_candidates(filename, content);
    let mut warnings = Vec::new();

    if let Some(hint) = parser_hint.map(str::trim).filter(|v| !v.is_empty()) {
        if !SUPPORTED_PARSER_IDS.contains(&hint) {
            return Err(ParseError::InvalidParserHint {
                hint: hint.to_string(),
            });
        }

        match parse_with_parser_id(hint, filename, content) {
            Ok(session) => {
                return Ok(ParsePreview {
                    parser_used: hint.to_string(),
                    parser_candidates,
                    session,
                    warnings,
                    native_adapter: native_adapter_for_parser(hint),
                });
            }
            Err(err) => {
                warnings.push(format!("parser_hint '{hint}' failed: {err}"));
            }
        }
    }

    let mut attempted = Vec::<String>::new();
    let hint_id = parser_hint.map(str::trim).unwrap_or_default().to_string();
    for candidate in &parser_candidates {
        let candidate_id = candidate.id.clone();
        if !hint_id.is_empty() && candidate_id == hint_id {
            continue;
        }
        attempted.push(candidate_id.clone());
        match parse_with_parser_id(&candidate_id, filename, content) {
            Ok(session) => {
                return Ok(ParsePreview {
                    parser_used: candidate_id.clone(),
                    parser_candidates,
                    session,
                    warnings,
                    native_adapter: native_adapter_for_parser(&candidate_id),
                });
            }
            Err(err) => {
                warnings.push(format!("parser '{candidate_id}' failed: {err}"));
            }
        }
    }

    if parser_candidates.len() > 1 || (parser_candidates.len() == 1 && !hint_id.is_empty()) {
        return Err(ParseError::ParserSelectionRequired {
            message: if attempted.is_empty() {
                "could not determine parser from source".to_string()
            } else {
                "auto-detection failed; choose a parser and retry".to_string()
            },
            parser_candidates,
        });
    }

    Err(ParseError::ParseFailed {
        message: if attempted.is_empty() {
            "no parser candidate matched the input".to_string()
        } else {
            "all parser attempts failed".to_string()
        },
        parser_candidates,
    })
}

fn add_candidate(candidates: &mut Vec<ParseCandidate>, id: &str, confidence: u8, reason: &str) {
    if let Some(existing) = candidates.iter_mut().find(|c| c.id == id) {
        if confidence > existing.confidence {
            existing.confidence = confidence;
            existing.reason = reason.to_string();
        }
        return;
    }
    candidates.push(ParseCandidate {
        id: id.to_string(),
        confidence,
        reason: reason.to_string(),
    });
}

fn looks_like_hail_jsonl(content: &str) -> bool {
    let Some(first_line) = content.lines().find(|line| !line.trim().is_empty()) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(first_line) else {
        return false;
    };
    value.get("type").and_then(|v| v.as_str()) == Some("header")
        && value.get("version").is_some()
        && value.get("session_id").is_some()
}

fn looks_like_hail_json(content: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(content) else {
        return false;
    };
    value.get("version").is_some()
        && value.get("session_id").is_some()
        && value.get("agent").is_some()
        && value.get("context").is_some()
        && value.get("events").is_some()
}

fn looks_like_codex_jsonl(content: &str) -> bool {
    content.contains("\"type\":\"session_meta\"")
        || content.contains("\"type\": \"session_meta\"")
        || content.contains("\"type\":\"response_item\"")
        || content.contains("\"type\":\"event_msg\"")
}

fn looks_like_claude_jsonl(content: &str) -> bool {
    (content.contains("\"type\":\"user\"") || content.contains("\"type\":\"assistant\""))
        && content.contains("\"message\"")
}

fn looks_like_gemini_json(content: &str) -> bool {
    content.contains("\"messages\"")
        && (content.contains("\"session_id\"") || content.contains("\"sessionId\""))
}

fn looks_like_amp_json(content: &str) -> bool {
    content.contains("\"agentMode\"")
        || (content.contains("\"messages\"") && content.contains("\"tool_use\""))
}

fn looks_like_opencode_json(content: &str) -> bool {
    content.contains("\"providerID\"")
        || content.contains("\"providerId\"")
        || content.contains("\"modelID\"")
        || content.contains("\"modelId\"")
}

fn parse_hail_content(content: &str) -> Result<Session> {
    if let Ok(session) = Session::from_jsonl(content) {
        return Ok(session);
    }

    serde_json::from_str::<Session>(content).context("input is neither HAIL JSONL nor HAIL JSON")
}

fn parse_with_parser_id(parser_id: &str, filename: &str, content: &str) -> Result<Session> {
    match parser_id {
        "hail" => parse_hail_content(content),
        "codex" => parse_with_temp_file(filename, content, |path| {
            crate::codex::CodexParser.parse(path)
        }),
        "claude-code" => parse_with_temp_file(filename, content, |path| {
            crate::claude_code::ClaudeCodeParser.parse(path)
        }),
        "gemini" => parse_with_temp_file(filename, content, |path| {
            crate::gemini::GeminiParser.parse(path)
        }),
        "amp" => parse_with_temp_file(filename, content, |path| crate::amp::AmpParser.parse(path)),
        "cline" => parse_with_temp_file(filename, content, |path| {
            crate::cline::ClineParser.parse(path)
        }),
        "cursor" => parse_with_temp_file(filename, content, |path| {
            crate::cursor::CursorParser.parse(path)
        }),
        "opencode" => parse_with_temp_file(filename, content, |path| {
            crate::opencode::OpenCodeParser.parse(path)
        }),
        _ => Err(anyhow!("unsupported parser id: {parser_id}")),
    }
}

fn parse_with_temp_file<F>(filename: &str, content: &str, parser: F) -> Result<Session>
where
    F: FnOnce(&std::path::Path) -> Result<Session>,
{
    let suffix = temp_suffix_from_filename(filename);
    let mut tmp = tempfile::Builder::new()
        .prefix("opensession-ingest-")
        .suffix(&suffix)
        .tempfile()
        .context("failed to create temporary parse file")?;
    tmp.write_all(content.as_bytes())
        .context("failed to write temporary parse file")?;
    parser(tmp.path())
}

fn temp_suffix_from_filename(filename: &str) -> String {
    let mut safe = filename
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-' || *c == '_')
        .collect::<String>();
    if safe.is_empty() {
        safe = "session.txt".to_string();
    }
    if safe.len() > 80 {
        safe = safe[safe.len().saturating_sub(80)..].to_string();
    }
    if !safe.starts_with('.') {
        safe.insert(0, '.');
    }
    safe
}

fn native_adapter_for_parser(parser_id: &str) -> Option<String> {
    match parser_id {
        "codex" | "claude-code" | "gemini" | "amp" | "cline" | "cursor" | "opencode" => {
            Some(parser_id.to_string())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_hail_jsonl() -> String {
        [
            r#"{"type":"header","version":"hail-1.0.0","session_id":"s1","agent":{"provider":"openai","model":"gpt-5","tool":"codex"},"context":{"title":"t","description":"d","tags":[],"created_at":"2026-02-01T00:00:00Z","updated_at":"2026-02-01T00:00:00Z","related_session_ids":[],"attributes":{}}}"#,
            r#"{"type":"event","event_id":"e1","timestamp":"2026-02-01T00:00:00Z","event_type":{"type":"UserMessage"},"content":{"blocks":[{"type":"Text","text":"hello"}]},"attributes":{}}"#,
            r#"{"type":"stats","event_count":1,"message_count":1,"tool_call_count":0,"task_count":0,"duration_seconds":0,"total_input_tokens":0,"total_output_tokens":0,"user_message_count":1,"files_changed":0,"lines_added":0,"lines_removed":0}"#,
        ]
        .join("\n")
    }

    #[test]
    fn detect_hail_from_header_line() {
        let candidates = detect_candidates("sample.jsonl", &minimal_hail_jsonl());
        assert!(!candidates.is_empty());
        assert_eq!(candidates[0].id, "hail");
        assert_eq!(candidates[0].confidence, 100);
    }

    #[test]
    fn parser_hint_falls_back_when_hint_fails() {
        let preview = preview_parse_bytes(
            "session.hail.jsonl",
            minimal_hail_jsonl().as_bytes(),
            Some("cursor"),
        )
        .expect("fallback should parse as hail");
        assert_eq!(preview.parser_used, "hail");
        assert!(preview
            .warnings
            .iter()
            .any(|w| w.contains("parser_hint 'cursor' failed")));
    }

    #[test]
    fn invalid_parser_hint_errors() {
        let err = preview_parse_bytes("session.jsonl", b"{}", Some("nope"))
            .expect_err("unknown parser hint must fail");
        match err {
            ParseError::InvalidParserHint { hint } => assert_eq!(hint, "nope"),
            _ => panic!("unexpected error kind"),
        }
    }

    #[test]
    fn parser_selection_required_when_auto_detect_fails() {
        let err = preview_parse_bytes("cursor-session.vscdb", b"not-a-sqlite-db", Some("cursor"))
            .expect_err("cursor hint with invalid vscdb should request parser selection");
        match err {
            ParseError::ParserSelectionRequired {
                parser_candidates, ..
            } => {
                assert_eq!(parser_candidates.len(), 1);
                assert_eq!(parser_candidates[0].id, "cursor");
            }
            _ => panic!("expected parser selection required"),
        }
    }
}
