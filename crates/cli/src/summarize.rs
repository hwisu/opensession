//! LLM-based session summarization via multiple AI providers.
//!
//! Used by `opensession handoff --summarize` to generate rich AI summaries.
//! Supports Anthropic, OpenAI-compatible, and Google Gemini APIs.

use anyhow::{bail, Context, Result};
use opensession_core::extract::truncate_str;
use opensession_core::{ContentBlock, EventType, Session};

/// Structured LLM summary output.
#[derive(Debug, Clone)]
pub struct LlmSummary {
    pub key_decisions: Vec<String>,
    pub patterns_discovered: Vec<String>,
    pub architecture_notes: String,
    pub next_steps: Vec<String>,
}

/// Summarize using the specified provider (or auto-detect from available env vars).
pub async fn llm_summarize(sessions: &[Session]) -> Result<LlmSummary> {
    llm_summarize_with_provider(sessions, None).await
}

/// Summarize using a specific provider name, or auto-detect.
pub async fn llm_summarize_with_provider(
    sessions: &[Session],
    provider: Option<&str>,
) -> Result<LlmSummary> {
    let transcript = build_transcript(sessions);
    let prompt = build_prompt(&transcript);

    let response_text = match provider {
        Some("openai") => call_openai(&prompt).await?,
        Some("gemini") => call_gemini(&prompt).await?,
        Some("claude") | Some("anthropic") => call_anthropic(&prompt).await?,
        Some(other) => {
            bail!("Unknown AI provider: '{other}'. Use 'claude', 'openai', or 'gemini'.")
        }
        None => {
            // Auto-detect from available env vars
            if std::env::var("ANTHROPIC_API_KEY").is_ok() {
                call_anthropic(&prompt).await?
            } else if std::env::var("OPENAI_API_KEY").is_ok() {
                call_openai(&prompt).await?
            } else if std::env::var("GEMINI_API_KEY").is_ok()
                || std::env::var("GOOGLE_API_KEY").is_ok()
            {
                call_gemini(&prompt).await?
            } else {
                bail!(
                    "No API key found. Set one of: ANTHROPIC_API_KEY, OPENAI_API_KEY, GEMINI_API_KEY"
                )
            }
        }
    };

    Ok(parse_llm_response(&response_text))
}

// ─── Provider implementations ────────────────────────────────────────────────

async fn call_anthropic(prompt: &str) -> Result<String> {
    let api_key = std::env::var("ANTHROPIC_API_KEY").context("ANTHROPIC_API_KEY not set")?;

    let model = std::env::var("OPENSESSION_SUMMARIZE_MODEL")
        .unwrap_or_else(|_| "claude-sonnet-4-5-20250929".to_string());

    let request_body = serde_json::json!({
        "model": model,
        "max_tokens": 2048,
        "messages": [{"role": "user", "content": prompt}]
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;
    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&request_body)
        .send()
        .await
        .context("Failed to call Anthropic API")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("Anthropic API error (HTTP {status}): {body}");
    }

    let body: serde_json::Value = resp.json().await.context("Failed to parse API response")?;
    let text = body
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|block| block.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();

    Ok(text)
}

async fn call_openai(prompt: &str) -> Result<String> {
    let api_key = std::env::var("OPENAI_API_KEY").context("OPENAI_API_KEY not set")?;

    let base_url = std::env::var("OPENAI_BASE_URL")
        .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());

    let model =
        std::env::var("OPENSESSION_SUMMARIZE_MODEL").unwrap_or_else(|_| "gpt-4o".to_string());

    let request_body = serde_json::json!({
        "model": model,
        "max_tokens": 2048,
        "messages": [{"role": "user", "content": prompt}]
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;
    let resp = client
        .post(format!("{base_url}/chat/completions"))
        .header("Authorization", format!("Bearer {api_key}"))
        .header("content-type", "application/json")
        .json(&request_body)
        .send()
        .await
        .context("Failed to call OpenAI API")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("OpenAI API error (HTTP {status}): {body}");
    }

    let body: serde_json::Value = resp.json().await.context("Failed to parse API response")?;
    let text = body
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|msg| msg.get("content"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();

    Ok(text)
}

async fn call_gemini(prompt: &str) -> Result<String> {
    let api_key = std::env::var("GEMINI_API_KEY")
        .or_else(|_| std::env::var("GOOGLE_API_KEY"))
        .context("GEMINI_API_KEY or GOOGLE_API_KEY not set")?;

    let model = std::env::var("OPENSESSION_SUMMARIZE_MODEL")
        .unwrap_or_else(|_| "gemini-2.0-flash".to_string());

    let url =
        format!("https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent");

    let request_body = serde_json::json!({
        "contents": [{"parts": [{"text": prompt}]}],
        "generationConfig": {"maxOutputTokens": 2048}
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;
    let resp = client
        .post(&url)
        .header("x-goog-api-key", &api_key)
        .header("content-type", "application/json")
        .json(&request_body)
        .send()
        .await
        .context("Failed to call Gemini API")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("Gemini API error (HTTP {status}): {body}");
    }

    let body: serde_json::Value = resp.json().await.context("Failed to parse API response")?;
    let text = body
        .get("candidates")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|candidate| candidate.get("content"))
        .and_then(|content| content.get("parts"))
        .and_then(|parts| parts.as_array())
        .and_then(|arr| arr.first())
        .and_then(|part| part.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();

    Ok(text)
}

// ─── Shared helpers ──────────────────────────────────────────────────────────

fn build_prompt(transcript: &str) -> String {
    format!(
        "You are analyzing AI coding session transcript(s) to produce a structured handoff summary.\n\
        \n\
        Here is the session transcript:\n\
        \n\
        <transcript>\n\
        {transcript}\n\
        </transcript>\n\
        \n\
        Please provide a structured summary with exactly these sections. \
        Output each section as a heading followed by bullet points (one per line, starting with `- `).\n\
        \n\
        ## Key Decisions\n\
        List the most important decisions made during this session.\n\
        \n\
        ## Patterns Discovered\n\
        List any patterns, conventions, or recurring themes found in the codebase.\n\
        \n\
        ## Architecture Notes\n\
        A paragraph describing relevant architecture observations.\n\
        \n\
        ## Next Steps\n\
        List suggested follow-up actions for the next session."
    )
}

/// Build a truncated transcript from sessions for the LLM prompt.
fn build_transcript(sessions: &[Session]) -> String {
    let mut parts = Vec::new();
    let max_total_chars = 50_000; // Keep under token limits
    let mut total_chars = 0;

    for (i, session) in sessions.iter().enumerate() {
        if total_chars >= max_total_chars {
            break;
        }

        if sessions.len() > 1 {
            let header = format!(
                "=== Session {} ({}) ===\nTool: {} ({})\n",
                i + 1,
                session.session_id,
                session.agent.tool,
                session.agent.model,
            );
            total_chars += header.len();
            parts.push(header);
        }

        for event in &session.events {
            if total_chars >= max_total_chars {
                parts.push("... (truncated)".to_string());
                break;
            }

            let line = match &event.event_type {
                EventType::UserMessage => {
                    let text = extract_event_text(event);
                    format!("User: {}", truncate_str(&text, 500))
                }
                EventType::AgentMessage => {
                    let text = extract_event_text(event);
                    format!("Agent: {}", truncate_str(&text, 500))
                }
                EventType::FileEdit { path, .. } => format!("[File Edit] {path}"),
                EventType::FileCreate { path } => format!("[File Create] {path}"),
                EventType::FileDelete { path } => format!("[File Delete] {path}"),
                EventType::ShellCommand {
                    command, exit_code, ..
                } => {
                    let code = exit_code.map(|c| c.to_string()).unwrap_or("?".into());
                    format!("[Shell] {} → {}", truncate_str(command, 100), code)
                }
                EventType::ToolResult {
                    is_error: true,
                    name,
                    ..
                } => format!("[Error] {name}"),
                _ => continue,
            };

            total_chars += line.len() + 1;
            parts.push(line);
        }
    }

    parts.join("\n")
}

fn extract_event_text(event: &opensession_core::Event) -> String {
    for block in &event.content.blocks {
        if let ContentBlock::Text { text } = block {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }
    String::new()
}

/// Parse the LLM response into structured sections.
///
/// Two-phase approach: first categorize each line by section, then extract content.
fn parse_llm_response(text: &str) -> LlmSummary {
    #[derive(Clone, Copy, PartialEq)]
    enum Section {
        None,
        KeyDecisions,
        Patterns,
        Architecture,
        NextSteps,
    }

    fn detect_section(line: &str) -> Option<Section> {
        if line.contains("Key Decisions") {
            Some(Section::KeyDecisions)
        } else if line.contains("Patterns Discovered")
            || (line.contains("Patterns") && line.starts_with('#'))
        {
            Some(Section::Patterns)
        } else if line.contains("Architecture Notes")
            || (line.contains("Architecture") && line.starts_with('#'))
        {
            Some(Section::Architecture)
        } else if line.contains("Next Steps") {
            Some(Section::NextSteps)
        } else {
            None
        }
    }

    fn extract_bullet(line: &str) -> Option<String> {
        line.strip_prefix("- ")
            .or_else(|| line.strip_prefix("* "))
            .map(String::from)
    }

    // Phase 1: Tag each content line with its section
    let mut current = Section::None;
    let tagged: Vec<(Section, &str)> = text
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if let Some(section) = detect_section(trimmed) {
                current = section;
                return None;
            }
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            Some((current, trimmed))
        })
        .collect();

    // Phase 2: Extract content by section
    let bullets_for = |section: Section| -> Vec<String> {
        tagged
            .iter()
            .filter(|(s, _)| *s == section)
            .filter_map(|(_, line)| extract_bullet(line))
            .collect()
    };

    let architecture_notes = tagged
        .iter()
        .filter(|(s, _)| *s == Section::Architecture)
        .map(|(_, line)| *line)
        .collect::<Vec<_>>()
        .join(" ");

    LlmSummary {
        key_decisions: bullets_for(Section::KeyDecisions),
        patterns_discovered: bullets_for(Section::Patterns),
        architecture_notes,
        next_steps: bullets_for(Section::NextSteps),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opensession_core::testing;
    use opensession_core::{Agent, Content, ContentBlock, Event, EventType, Session};

    fn make_agent() -> Agent {
        testing::agent()
    }

    fn make_event(event_type: EventType, content: Content) -> Event {
        testing::event_with_content(event_type, content)
    }

    // ── parse_llm_response tests ────────────────────────────────────────

    #[test]
    fn test_parse_llm_response() {
        let text = r#"## Key Decisions
- Used Axum for the web framework
- Chose SQLite over PostgreSQL for simplicity

## Patterns Discovered
- All routes follow REST conventions
- Error handling uses `ServiceError` enum

## Architecture Notes
The codebase follows a layered architecture with separate crates for core types,
API types, and the server implementation.

## Next Steps
- Add integration tests
- Implement rate limiting
"#;

        let summary = parse_llm_response(text);
        assert_eq!(summary.key_decisions.len(), 2);
        assert_eq!(summary.patterns_discovered.len(), 2);
        assert!(!summary.architecture_notes.is_empty());
        assert_eq!(summary.next_steps.len(), 2);
        assert!(summary.architecture_notes.contains("layered architecture"));
    }

    #[test]
    fn test_parse_llm_response_empty() {
        let summary = parse_llm_response("");
        assert!(summary.key_decisions.is_empty());
        assert!(summary.patterns_discovered.is_empty());
        assert!(summary.architecture_notes.is_empty());
        assert!(summary.next_steps.is_empty());
    }

    #[test]
    fn test_parse_llm_response_no_sections() {
        let text = "Just some random text without any section headers.";
        let summary = parse_llm_response(text);
        assert!(summary.key_decisions.is_empty());
        assert!(summary.next_steps.is_empty());
    }

    #[test]
    fn test_parse_llm_response_asterisk_bullets() {
        let text = "## Key Decisions\n* Used Rust\n* Chose Axum\n";
        let summary = parse_llm_response(text);
        assert_eq!(summary.key_decisions.len(), 2);
        assert_eq!(summary.key_decisions[0], "Used Rust");
        assert_eq!(summary.key_decisions[1], "Chose Axum");
    }

    #[test]
    fn test_parse_llm_response_multiline_architecture() {
        let text = "## Architecture Notes\nFirst line.\nSecond line.\nThird line.\n";
        let summary = parse_llm_response(text);
        assert!(summary.architecture_notes.contains("First line."));
        assert!(summary.architecture_notes.contains("Second line."));
        assert!(summary.architecture_notes.contains("Third line."));
    }

    // ── extract_event_text tests ────────────────────────────────────────

    #[test]
    fn test_extract_event_text_with_content() {
        let event = make_event(EventType::UserMessage, Content::text("Hello world"));
        assert_eq!(extract_event_text(&event), "Hello world");
    }

    #[test]
    fn test_extract_event_text_empty() {
        let event = make_event(EventType::UserMessage, Content::empty());
        assert_eq!(extract_event_text(&event), "");
    }

    #[test]
    fn test_extract_event_text_whitespace_only() {
        let event = make_event(
            EventType::UserMessage,
            Content {
                blocks: vec![ContentBlock::Text {
                    text: "   \n  ".to_string(),
                }],
            },
        );
        assert_eq!(extract_event_text(&event), "");
    }

    // ── build_transcript tests ──────────────────────────────────────────

    #[test]
    fn test_build_transcript_single_session() {
        let mut session = Session::new("s1".to_string(), make_agent());
        session
            .events
            .push(make_event(EventType::UserMessage, Content::text("Fix bug")));
        session.events.push(make_event(
            EventType::AgentMessage,
            Content::text("I'll fix it"),
        ));

        let transcript = build_transcript(&[session]);
        assert!(transcript.contains("User: Fix bug"));
        assert!(transcript.contains("Agent: I'll fix it"));
        // Single session should not have session header
        assert!(!transcript.contains("=== Session"));
    }

    #[test]
    fn test_build_transcript_multi_session() {
        let mut s1 = Session::new("s1".to_string(), make_agent());
        s1.events
            .push(make_event(EventType::UserMessage, Content::text("Task 1")));

        let mut s2 = Session::new("s2".to_string(), make_agent());
        s2.events
            .push(make_event(EventType::UserMessage, Content::text("Task 2")));

        let transcript = build_transcript(&[s1, s2]);
        assert!(transcript.contains("=== Session 1"));
        assert!(transcript.contains("=== Session 2"));
        assert!(transcript.contains("User: Task 1"));
        assert!(transcript.contains("User: Task 2"));
    }

    #[test]
    fn test_build_transcript_file_events() {
        let mut session = Session::new("s1".to_string(), make_agent());
        session.events.push(make_event(
            EventType::FileEdit {
                path: "src/main.rs".to_string(),
                diff: None,
            },
            Content::empty(),
        ));
        session.events.push(make_event(
            EventType::FileCreate {
                path: "src/new.rs".to_string(),
            },
            Content::empty(),
        ));
        session.events.push(make_event(
            EventType::FileDelete {
                path: "src/old.rs".to_string(),
            },
            Content::empty(),
        ));

        let transcript = build_transcript(&[session]);
        assert!(transcript.contains("[File Edit] src/main.rs"));
        assert!(transcript.contains("[File Create] src/new.rs"));
        assert!(transcript.contains("[File Delete] src/old.rs"));
    }

    #[test]
    fn test_build_transcript_shell_command() {
        let mut session = Session::new("s1".to_string(), make_agent());
        session.events.push(make_event(
            EventType::ShellCommand {
                command: "cargo test".to_string(),
                exit_code: Some(0),
            },
            Content::empty(),
        ));

        let transcript = build_transcript(&[session]);
        assert!(transcript.contains("[Shell] cargo test"));
        assert!(transcript.contains("→ 0"));
    }

    #[test]
    fn test_build_transcript_truncation() {
        let mut session = Session::new("s1".to_string(), make_agent());
        // Add enough events to exceed 50K chars
        for i in 0..2000 {
            session.events.push(make_event(
                EventType::UserMessage,
                Content::text(format!("Message {i}: {}", "x".repeat(50))),
            ));
        }

        let transcript = build_transcript(&[session]);
        assert!(transcript.len() <= 60_000); // Some overhead but bounded
        assert!(transcript.contains("... (truncated)"));
    }

    // ── build_prompt tests ──────────────────────────────────────────────

    #[test]
    fn test_build_prompt_contains_transcript() {
        let transcript = "User: Hello\nAgent: Hi there";
        let prompt = build_prompt(transcript);
        assert!(prompt.contains(transcript));
        assert!(prompt.contains("<transcript>"));
        assert!(prompt.contains("</transcript>"));
        assert!(prompt.contains("## Key Decisions"));
        assert!(prompt.contains("## Next Steps"));
    }
}
