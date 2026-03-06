use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use opensession_api::{
    DesktopApiError, DesktopChangeQuestionRequest, DesktopChangeQuestionResponse,
    DesktopChangeReadRequest, DesktopChangeReadResponse, DesktopChangeReaderScope,
    DesktopChangeReaderTtsRequest, DesktopChangeReaderTtsResponse,
    DesktopSessionSummaryResponse, DesktopSummaryProviderId,
};
use opensession_core::trace::{ContentBlock, Event, EventType, Session as HailSession};
use opensession_local_db::LocalDb;
use opensession_runtime_config::{
    ChangeReaderVoiceProvider, DaemonConfig, SummaryProvider,
};
use opensession_summary::provider::generate_text;
use serde_json::json;
use std::time::Duration;

use crate::app::session_summary::load_session_summary_for_runtime;
use crate::{
    CHANGE_READER_MAX_EVENTS, CHANGE_READER_MAX_LINE_CHARS, DesktopApiResult, desktop_error,
    load_normalized_session_body, load_runtime_config, map_change_reader_scope_from_runtime,
    map_summary_provider_id_from_runtime, open_local_db,
};

#[derive(Debug, Clone)]
struct ChangeReaderContextPayload {
    session_id: String,
    scope: DesktopChangeReaderScope,
    context: String,
    citations: Vec<String>,
    provider: Option<DesktopSummaryProviderId>,
    warning: Option<String>,
}

fn compact_ws(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn trim_chars(raw: &str, max_chars: usize) -> String {
    let normalized = compact_ws(raw);
    if normalized.chars().count() <= max_chars {
        return normalized;
    }
    let mut out = String::new();
    for ch in normalized.chars().take(max_chars.saturating_sub(1)) {
        out.push(ch);
    }
    out.push('…');
    out
}

fn json_value_compact(value: &serde_json::Value, max_chars: usize) -> String {
    let raw = serde_json::to_string(value).unwrap_or_else(|_| value.to_string());
    trim_chars(&raw, max_chars)
}

fn event_type_label(event_type: &EventType) -> &'static str {
    match event_type {
        EventType::UserMessage => "user",
        EventType::AgentMessage => "agent",
        EventType::SystemMessage => "system",
        EventType::Thinking => "thinking",
        EventType::ToolCall { .. } => "tool_call",
        EventType::ToolResult { .. } => "tool_result",
        EventType::FileRead { .. } => "file_read",
        EventType::CodeSearch { .. } => "code_search",
        EventType::FileSearch { .. } => "file_search",
        EventType::FileEdit { .. } => "file_edit",
        EventType::FileCreate { .. } => "file_create",
        EventType::FileDelete { .. } => "file_delete",
        EventType::ShellCommand { .. } => "shell",
        EventType::ImageGenerate { .. } => "image_generate",
        EventType::VideoGenerate { .. } => "video_generate",
        EventType::AudioGenerate { .. } => "audio_generate",
        EventType::WebSearch { .. } => "web_search",
        EventType::WebFetch { .. } => "web_fetch",
        EventType::TaskStart { .. } => "task_start",
        EventType::TaskEnd { .. } => "task_end",
        EventType::Custom { .. } => "custom",
        _ => "event",
    }
}

fn event_type_payload(event_type: &EventType) -> Option<String> {
    match event_type {
        EventType::ToolCall { name } => Some(format!("tool={name}")),
        EventType::ToolResult {
            name,
            is_error,
            call_id,
        } => Some(format!(
            "tool={} result={}{}",
            name,
            if *is_error { "error" } else { "ok" },
            call_id
                .as_deref()
                .map(|id| format!(" call_id={id}"))
                .unwrap_or_default()
        )),
        EventType::FileRead { path }
        | EventType::FileEdit { path, .. }
        | EventType::FileCreate { path }
        | EventType::FileDelete { path } => Some(format!("path={path}")),
        EventType::CodeSearch { query } | EventType::WebSearch { query } => {
            Some(format!("query={}", trim_chars(query, 90)))
        }
        EventType::FileSearch { pattern } => Some(format!("pattern={}", trim_chars(pattern, 90))),
        EventType::ShellCommand { command, exit_code } => Some(format!(
            "cmd={}{}",
            trim_chars(command, 120),
            exit_code
                .map(|code| format!(" exit_code={code}"))
                .unwrap_or_default()
        )),
        EventType::ImageGenerate { prompt }
        | EventType::VideoGenerate { prompt }
        | EventType::AudioGenerate { prompt } => Some(format!("prompt={}", trim_chars(prompt, 90))),
        EventType::WebFetch { url } => Some(format!("url={url}")),
        EventType::TaskStart { title } => title
            .as_deref()
            .map(|raw| format!("title={}", trim_chars(raw, 90))),
        EventType::TaskEnd { summary } => summary
            .as_deref()
            .map(|raw| format!("summary={}", trim_chars(raw, 90))),
        EventType::Custom { kind } => Some(format!("kind={kind}")),
        _ => None,
    }
}

fn event_content_excerpt(event: &Event) -> Option<String> {
    let mut parts = Vec::<String>::new();
    for block in &event.content.blocks {
        let rendered = match block {
            ContentBlock::Text { text } => trim_chars(text, CHANGE_READER_MAX_LINE_CHARS),
            ContentBlock::Code { code, .. } => {
                let first_line = code.lines().next().unwrap_or_default();
                format!("code: {}", trim_chars(first_line, 120))
            }
            ContentBlock::Image { alt, url, .. } => {
                let label = alt.as_deref().unwrap_or("image");
                format!("{label}: {url}")
            }
            ContentBlock::Video { url, .. } => format!("video: {url}"),
            ContentBlock::Audio { url, .. } => format!("audio: {url}"),
            ContentBlock::File { path, content } => {
                let head = content
                    .as_deref()
                    .map(|raw| trim_chars(raw, 80))
                    .unwrap_or_else(|| "content omitted".to_string());
                format!("file {path}: {head}")
            }
            ContentBlock::Json { data } => format!("json: {}", json_value_compact(data, 120)),
            ContentBlock::Reference { uri, .. } => format!("ref: {uri}"),
            _ => String::new(),
        };
        if rendered.trim().is_empty() {
            continue;
        }
        parts.push(rendered);
        if parts.len() >= 2 {
            break;
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" | "))
    }
}

fn summary_lines_for_reader(summary: &DesktopSessionSummaryResponse) -> Vec<String> {
    let mut lines = Vec::<String>::new();
    if let Some(summary_obj) = summary.summary.as_ref().and_then(|value| value.as_object()) {
        if let Some(changes) = summary_obj.get("changes").and_then(|value| value.as_str()) {
            if !changes.trim().is_empty() {
                lines.push(format!("changes: {}", trim_chars(changes, 280)));
            }
        }
        if let Some(auth_security) = summary_obj
            .get("auth_security")
            .and_then(|value| value.as_str())
        {
            if !auth_security.trim().is_empty() {
                lines.push(format!("auth_security: {}", trim_chars(auth_security, 200)));
            }
        }
        if let Some(layer_items) = summary_obj
            .get("layer_file_changes")
            .and_then(|value| value.as_array())
        {
            for item in layer_items.iter().take(12) {
                let layer = item
                    .get("layer")
                    .and_then(|value| value.as_str())
                    .unwrap_or("(layer)");
                let detail = item
                    .get("summary")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                let files = item
                    .get("files")
                    .and_then(|value| value.as_array())
                    .map(|entries| {
                        entries
                            .iter()
                            .filter_map(|entry| entry.as_str())
                            .take(5)
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                lines.push(format!(
                    "layer {}: {}{}",
                    trim_chars(layer, 60),
                    trim_chars(detail, 120),
                    if files.is_empty() {
                        String::new()
                    } else {
                        format!(" (files: {files})")
                    }
                ));
            }
        }
    } else if let Some(value) = &summary.summary {
        lines.push(format!(
            "semantic_summary_json: {}",
            json_value_compact(value, 260)
        ));
    }

    for layer in summary.diff_tree.iter().take(8) {
        let Some(layer_obj) = layer.as_object() else {
            continue;
        };
        let layer_name = layer_obj
            .get("layer")
            .and_then(|value| value.as_str())
            .unwrap_or("(layer)");
        let file_count = layer_obj
            .get("file_count")
            .and_then(|value| value.as_u64())
            .unwrap_or_default();
        let added = layer_obj
            .get("lines_added")
            .and_then(|value| value.as_u64())
            .unwrap_or_default();
        let removed = layer_obj
            .get("lines_removed")
            .and_then(|value| value.as_u64())
            .unwrap_or_default();
        lines.push(format!(
            "diff_layer {}: files={} +{} -{}",
            trim_chars(layer_name, 60),
            file_count,
            added,
            removed
        ));
    }

    if let Some(source_kind) = summary.source_kind.as_deref() {
        lines.push(format!("source_kind: {source_kind}"));
    }
    if let Some(generation_kind) = summary.generation_kind.as_deref() {
        lines.push(format!("generation_kind: {generation_kind}"));
    }
    if let Some(error) = summary.error.as_deref() {
        lines.push(format!("generation_error: {}", trim_chars(error, 160)));
    }
    lines
}

fn timeline_lines_for_reader(session: &HailSession) -> Vec<String> {
    session
        .events
        .iter()
        .take(CHANGE_READER_MAX_EVENTS)
        .map(|event| {
            let label = event_type_label(&event.event_type);
            let payload = event_type_payload(&event.event_type).unwrap_or_default();
            let content = event_content_excerpt(event).unwrap_or_default();
            let mut merged = format!("{} {}", event.timestamp.to_rfc3339(), label);
            if !payload.is_empty() {
                merged.push(' ');
                merged.push_str(&payload);
            }
            if !content.is_empty() {
                merged.push_str(" => ");
                merged.push_str(&content);
            }
            trim_chars(&merged, CHANGE_READER_MAX_LINE_CHARS)
        })
        .collect()
}

fn trim_context_to_limit(raw: String, max_chars: usize) -> String {
    if raw.chars().count() <= max_chars {
        return raw;
    }
    let mut out = String::new();
    for ch in raw.chars().take(max_chars.saturating_sub(24)) {
        out.push(ch);
    }
    out.push_str("\n\n[context truncated]");
    out
}

fn provider_for_change_reader(runtime: &DaemonConfig) -> Option<DesktopSummaryProviderId> {
    match runtime.summary.provider.id {
        SummaryProvider::Disabled => None,
        _ => Some(map_summary_provider_id_from_runtime(
            &runtime.summary.provider.id,
        )),
    }
}

fn build_change_reader_context(
    db: &LocalDb,
    runtime: &DaemonConfig,
    session_id: &str,
    scope_override: Option<DesktopChangeReaderScope>,
) -> DesktopApiResult<ChangeReaderContextPayload> {
    let scope = scope_override
        .unwrap_or_else(|| map_change_reader_scope_from_runtime(&runtime.change_reader.scope));
    let normalized_session = load_normalized_session_body(db, session_id)?;
    let session = HailSession::from_jsonl(&normalized_session).map_err(|error| {
        desktop_error(
            "desktop.change_reader_parse_failed",
            422,
            "failed to parse session payload for change reader",
            Some(json!({ "cause": error.to_string(), "session_id": session_id })),
        )
    })?;
    let summary = load_session_summary_for_runtime(db, runtime, session_id)?;
    let summary_lines = summary_lines_for_reader(&summary);
    let timeline_lines = timeline_lines_for_reader(&session);
    let mut citations = Vec::<String>::new();
    let mut chunks = vec![
        format!("session_id: {}", session.session_id),
        format!(
            "agent: tool={} provider={} model={}",
            session.agent.tool, session.agent.provider, session.agent.model
        ),
    ];
    if let Some(title) = session.context.title.as_deref() {
        if !title.trim().is_empty() {
            chunks.push(format!("title: {}", trim_chars(title, 120)));
        }
    }
    if let Some(description) = session.context.description.as_deref() {
        if !description.trim().is_empty() {
            chunks.push(format!("description: {}", trim_chars(description, 180)));
        }
    }

    let mut warning = None;
    if !summary_lines.is_empty() {
        citations.push("session.semantic_summary".to_string());
        chunks.push("[semantic_summary]".to_string());
        chunks.extend(summary_lines.into_iter().map(|line| format!("- {line}")));
    } else {
        warning =
            Some("semantic summary is not available; using timeline-derived context".to_string());
    }

    if matches!(scope, DesktopChangeReaderScope::FullContext)
        || (matches!(scope, DesktopChangeReaderScope::SummaryOnly) && citations.is_empty())
    {
        citations.push("session.timeline".to_string());
        chunks.push("[timeline_excerpt]".to_string());
        chunks.extend(timeline_lines.into_iter().map(|line| format!("- {line}")));
    }

    let max_context_chars = runtime.change_reader.max_context_chars.max(1) as usize;
    let context = trim_context_to_limit(chunks.join("\n"), max_context_chars);
    Ok(ChangeReaderContextPayload {
        session_id: session_id.to_string(),
        scope,
        context,
        citations,
        provider: provider_for_change_reader(runtime),
        warning,
    })
}

fn build_read_prompt(context: &str, scope: &DesktopChangeReaderScope) -> String {
    let scope_label = match scope {
        DesktopChangeReaderScope::SummaryOnly => "summary_only",
        DesktopChangeReaderScope::FullContext => "full_context",
    };
    format!(
        "You are OpenSession Change Reader.\n\
Use only the provided context and do not fabricate facts.\n\
Write a concise, human-readable Korean briefing about what changed.\n\
Include: 핵심 변경, 영향도/리스크, 확인할 테스트 1~2개.\n\
Scope={scope_label}\n\
\n\
Context:\n{context}\n"
    )
}

fn build_question_prompt(
    question: &str,
    context: &str,
    scope: &DesktopChangeReaderScope,
) -> String {
    let scope_label = match scope {
        DesktopChangeReaderScope::SummaryOnly => "summary_only",
        DesktopChangeReaderScope::FullContext => "full_context",
    };
    format!(
        "You are OpenSession Change Q&A assistant.\n\
Answer only from the given context. If evidence is insufficient, say clearly what is missing.\n\
Respond in Korean and keep it concise.\n\
Scope={scope_label}\n\
Question: {question}\n\
\n\
Context:\n{context}\n"
    )
}

fn fallback_change_narrative(context: &ChangeReaderContextPayload) -> String {
    let lines = context
        .context
        .lines()
        .filter(|line| line.starts_with("- "))
        .take(8)
        .map(|line| line.trim_start_matches("- ").to_string())
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return "변경을 설명할 수 있는 컨텍스트가 충분하지 않습니다.".to_string();
    }
    format!(
        "로컬 변경 브리핑(휴리스틱)\n{}",
        lines
            .into_iter()
            .map(|line| format!("- {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

fn tokenize_question(question: &str) -> Vec<String> {
    question
        .split(|ch: char| ch.is_whitespace() || ",.;:!?/()[]{}".contains(ch))
        .map(|token| token.trim().to_lowercase())
        .filter(|token| token.chars().count() >= 2)
        .take(10)
        .collect()
}

fn fallback_change_answer(question: &str, context: &ChangeReaderContextPayload) -> String {
    let tokens = tokenize_question(question);
    let mut matches = Vec::<String>::new();
    if !tokens.is_empty() {
        for line in context.context.lines() {
            let lowered = line.to_lowercase();
            if tokens.iter().any(|token| lowered.contains(token)) {
                matches.push(trim_chars(line, 180));
            }
            if matches.len() >= 5 {
                break;
            }
        }
    }
    if matches.is_empty() {
        return "질문에 바로 대응되는 근거를 현재 컨텍스트에서 찾지 못했습니다. full_context로 전환하거나 세션 요약을 재생성해 주세요."
            .to_string();
    }
    format!(
        "질문 답변(로컬 휴리스틱)\n{}\n\n근거:\n{}",
        trim_chars(
            &matches
                .first()
                .cloned()
                .unwrap_or_else(|| "근거를 찾지 못했습니다.".to_string()),
            220
        ),
        matches
            .into_iter()
            .take(4)
            .map(|line| format!("- {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

fn merge_warnings(primary: Option<String>, secondary: Option<String>) -> Option<String> {
    match (primary, secondary) {
        (Some(a), Some(b)) => Some(format!("{a}; {b}")),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn request_openai_tts_audio(
    api_key: &str,
    model: &str,
    voice: &str,
    text: &str,
) -> DesktopApiResult<Vec<u8>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(45))
        .build()
        .map_err(|error| {
            desktop_error(
                "desktop.change_reader_tts_client_failed",
                500,
                "failed to initialize TTS client",
                Some(json!({ "cause": error.to_string() })),
            )
        })?;

    let response = client
        .post("https://api.openai.com/v1/audio/speech")
        .bearer_auth(api_key)
        .json(&json!({
            "model": model,
            "voice": voice,
            "input": text,
            "format": "mp3"
        }))
        .send()
        .map_err(|error| {
            desktop_error(
                "desktop.change_reader_tts_request_failed",
                502,
                "failed to call OpenAI TTS API",
                Some(json!({
                    "cause": error.to_string(),
                    "hint": "check network connectivity and OpenAI API access"
                })),
            )
        })?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(desktop_error(
                "desktop.change_reader_tts_auth_failed",
                status.as_u16(),
                "OpenAI TTS authentication failed",
                Some(json!({
                    "hint": "verify Settings > Runtime Summary > Change Reader > Voice API key",
                    "response": trim_chars(&body, 300),
                })),
            ));
        }
        return Err(desktop_error(
            "desktop.change_reader_tts_provider_error",
            status.as_u16(),
            "OpenAI TTS API returned an error",
            Some(json!({
                "hint": "check configured model/voice and API quota",
                "response": trim_chars(&body, 300),
            })),
        ));
    }

    let payload = response.bytes().map_err(|error| {
        desktop_error(
            "desktop.change_reader_tts_decode_failed",
            500,
            "failed to decode TTS audio payload",
            Some(json!({ "cause": error.to_string() })),
        )
    })?;

    if payload.is_empty() {
        return Err(desktop_error(
            "desktop.change_reader_tts_empty_audio",
            502,
            "OpenAI TTS returned empty audio",
            Some(json!({
                "hint": "try a shorter input text or verify provider status"
            })),
        ));
    }

    Ok(payload.to_vec())
}

pub(crate) fn require_non_empty_request_field(
    raw: &str,
    code: &str,
    field_name: &str,
) -> DesktopApiResult<String> {
    let value = raw.trim().to_string();
    if value.is_empty() {
        return Err(desktop_error(
            code,
            400,
            format!("{field_name} is required"),
            None,
        ));
    }
    Ok(value)
}

fn change_reader_disabled_error() -> DesktopApiError {
    desktop_error(
        "desktop.change_reader_disabled",
        422,
        "change reader is disabled in runtime settings",
        Some(json!({ "hint": "Enable Change Reader in Settings > Runtime Summary" })),
    )
}

fn ensure_change_reader_enabled(runtime: &DaemonConfig) -> DesktopApiResult<()> {
    if runtime.change_reader.enabled {
        Ok(())
    } else {
        Err(change_reader_disabled_error())
    }
}

fn ensure_change_reader_qa_enabled(runtime: &DaemonConfig) -> DesktopApiResult<()> {
    if runtime.change_reader.qa_enabled {
        Ok(())
    } else {
        Err(desktop_error(
            "desktop.change_reader_qa_disabled",
            422,
            "change reader Q&A is disabled in runtime settings",
            Some(json!({ "hint": "Enable Q&A in Settings > Runtime Summary > Change Reader" })),
        ))
    }
}

fn load_change_reader_request_context(
    session_id: &str,
    scope: Option<DesktopChangeReaderScope>,
    require_qa: bool,
) -> DesktopApiResult<(DaemonConfig, ChangeReaderContextPayload)> {
    let session_id = require_non_empty_request_field(
        session_id,
        "desktop.change_reader_invalid_request",
        "session_id",
    )?;
    let runtime = load_runtime_config()?;
    ensure_change_reader_enabled(&runtime)?;
    if require_qa {
        ensure_change_reader_qa_enabled(&runtime)?;
    }
    let db = open_local_db()?;
    let context = build_change_reader_context(&db, &runtime, &session_id, scope)?;
    Ok((runtime, context))
}

#[tauri::command]
pub(crate) async fn desktop_read_session_changes(
    request: DesktopChangeReadRequest,
) -> DesktopApiResult<DesktopChangeReadResponse> {
    let (runtime, context) =
        load_change_reader_request_context(&request.session_id, request.scope, false)?;
    let prompt = build_read_prompt(&context.context, &context.scope);

    let (narrative, provider_warning) = if runtime.summary.is_configured() {
        match generate_text(&runtime.summary, &prompt).await {
            Ok(text) if !text.trim().is_empty() => (trim_chars(&text, 4000), None),
            Ok(_) => (
                fallback_change_narrative(&context),
                Some("provider returned empty response".to_string()),
            ),
            Err(error) => (
                fallback_change_narrative(&context),
                Some(format!("provider generation failed: {error}")),
            ),
        }
    } else {
        (
            fallback_change_narrative(&context),
            Some("summary provider is not configured; used local fallback".to_string()),
        )
    };

    Ok(DesktopChangeReadResponse {
        session_id: context.session_id,
        scope: context.scope,
        narrative,
        citations: context.citations,
        provider: context.provider,
        warning: merge_warnings(context.warning, provider_warning),
    })
}

#[tauri::command]
pub(crate) async fn desktop_ask_session_changes(
    request: DesktopChangeQuestionRequest,
) -> DesktopApiResult<DesktopChangeQuestionResponse> {
    let question = require_non_empty_request_field(
        &request.question,
        "desktop.change_reader_question_required",
        "question",
    )?;
    let (runtime, context) =
        load_change_reader_request_context(&request.session_id, request.scope, true)?;
    let prompt = build_question_prompt(&question, &context.context, &context.scope);
    let (answer, provider_warning) = if runtime.summary.is_configured() {
        match generate_text(&runtime.summary, &prompt).await {
            Ok(text) if !text.trim().is_empty() => (trim_chars(&text, 4000), None),
            Ok(_) => (
                fallback_change_answer(&question, &context),
                Some("provider returned empty response".to_string()),
            ),
            Err(error) => (
                fallback_change_answer(&question, &context),
                Some(format!("provider generation failed: {error}")),
            ),
        }
    } else {
        (
            fallback_change_answer(&question, &context),
            Some("summary provider is not configured; used local fallback".to_string()),
        )
    };

    Ok(DesktopChangeQuestionResponse {
        session_id: context.session_id,
        question,
        scope: context.scope,
        answer,
        citations: context.citations,
        provider: context.provider,
        warning: merge_warnings(context.warning, provider_warning),
    })
}

#[tauri::command]
pub(crate) fn desktop_change_reader_tts(
    request: DesktopChangeReaderTtsRequest,
) -> DesktopApiResult<DesktopChangeReaderTtsResponse> {
    let mut text = request.text.trim().to_string();
    if text.is_empty() {
        return Err(desktop_error(
            "desktop.change_reader_tts_text_required",
            400,
            "text is required for TTS",
            None,
        ));
    }

    let runtime = load_runtime_config()?;
    ensure_change_reader_enabled(&runtime)?;
    if !runtime.change_reader.voice.enabled {
        return Err(desktop_error(
            "desktop.change_reader_tts_disabled",
            422,
            "change reader voice is disabled in runtime settings",
            Some(json!({ "hint": "Enable voice in Settings > Runtime Summary > Change Reader" })),
        ));
    }

    let api_key = runtime.change_reader.voice.api_key.trim().to_string();
    if api_key.is_empty() {
        return Err(desktop_error(
            "desktop.change_reader_tts_api_key_missing",
            422,
            "change reader voice API key is not configured",
            Some(
                json!({ "hint": "Set OpenAI API key in Settings > Runtime Summary > Change Reader" }),
            ),
        ));
    }

    let model = if runtime.change_reader.voice.model.trim().is_empty() {
        "gpt-4o-mini-tts".to_string()
    } else {
        runtime.change_reader.voice.model.trim().to_string()
    };
    let voice = if runtime.change_reader.voice.voice.trim().is_empty() {
        "alloy".to_string()
    } else {
        runtime.change_reader.voice.voice.trim().to_string()
    };
    if !matches!(
        runtime.change_reader.voice.provider,
        ChangeReaderVoiceProvider::Openai
    ) {
        return Err(desktop_error(
            "desktop.change_reader_tts_provider_unsupported",
            422,
            "unsupported change reader voice provider",
            Some(json!({
                "hint": "Select openai provider in Settings > Runtime Summary > Change Reader"
            })),
        ));
    }

    let mut warning = None;
    if text.chars().count() > 4_000 {
        text = text.chars().take(4_000).collect();
        warning = Some("Input text was truncated to 4000 chars before TTS.".to_string());
    }

    let audio = request_openai_tts_audio(&api_key, &model, &voice, &text)?;
    Ok(DesktopChangeReaderTtsResponse {
        mime_type: "audio/mpeg".to_string(),
        audio_base64: BASE64_STANDARD.encode(audio),
        warning,
    })
}
