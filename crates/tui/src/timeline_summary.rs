#![allow(dead_code)]

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TimelineSummaryWindowKey {
    pub session_id: String,
    pub event_index: usize,
    pub window_id: u64,
}

#[derive(Debug, Clone)]
pub struct TimelineSummaryWindowRequest {
    pub key: TimelineSummaryWindowKey,
    pub context: String,
    pub visible_priority: bool,
    pub cache_lookup_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SummaryCliProbeResult {
    pub attempted_providers: Vec<String>,
    pub responsive_providers: Vec<String>,
    pub recommended_provider: Option<String>,
    pub errors: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default)]
pub struct SummaryRuntimeConfig {
    pub model: Option<String>,
    pub content_mode: Option<String>,
    pub openai_compat_endpoint: Option<String>,
    pub openai_compat_base: Option<String>,
    pub openai_compat_path: Option<String>,
    pub openai_compat_style: Option<String>,
    pub openai_compat_api_key: Option<String>,
    pub openai_compat_api_key_header: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TurnSummaryEventSpan {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TurnSummaryTurnMeta {
    pub turn_index: usize,
    pub anchor_event_index: usize,
    pub event_span: TurnSummaryEventSpan,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TurnSummaryPrompt {
    pub text: String,
    pub intent: String,
    pub constraints: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TurnSummaryOutcome {
    pub status: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileChange {
    pub path: String,
    pub op: String,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlanItem {
    pub step: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolAction {
    pub tool: String,
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TurnSummaryEvidence {
    pub modified_files: Vec<FileChange>,
    pub key_implementations: Vec<String>,
    pub agent_quotes: Vec<String>,
    pub agent_plan: Vec<PlanItem>,
    pub tool_actions: Vec<ToolAction>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BehaviorCard {
    #[serde(rename = "type")]
    pub card_type: String,
    pub title: String,
    pub lines: Vec<String>,
    pub severity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TimelineSummaryPayload {
    pub kind: String,
    pub version: String,
    pub scope: String,
    pub turn_meta: TurnSummaryTurnMeta,
    pub prompt: TurnSummaryPrompt,
    pub outcome: TurnSummaryOutcome,
    pub evidence: TurnSummaryEvidence,
    pub cards: Vec<BehaviorCard>,
    pub next_steps: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct TimelineSummaryCacheEntry {
    pub compact: String,
    pub payload: TimelineSummaryPayload,
    #[allow(dead_code)]
    pub raw: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SummaryProvider {
    Anthropic,
    OpenAi,
    OpenAiCompatible,
    Gemini,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SummaryCliTarget {
    Auto,
    Codex,
    Claude,
    Cursor,
    Gemini,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SummaryEngine {
    Api(SummaryProvider),
    Cli(SummaryCliTarget),
}

#[derive(Debug, Clone)]
struct ResolvedCliCommand {
    target: SummaryCliTarget,
    bin: String,
    pre_args: Vec<String>,
}

pub async fn generate_timeline_summary(
    context: &str,
    provider_hint: Option<&str>,
    agent_tool: Option<&str>,
    runtime: Option<&SummaryRuntimeConfig>,
) -> Result<TimelineSummaryCacheEntry> {
    let engine = resolve_engine(provider_hint, runtime)?;
    let summary_mode = runtime
        .and_then(|cfg| cfg.content_mode.as_deref())
        .map(|mode| mode.trim().to_ascii_lowercase())
        .filter(|mode| mode == "minimal")
        .unwrap_or_else(|| "normal".to_string());
    let mode_rule = if summary_mode == "minimal" {
        "- Use minimal mode: keep core outcomes, files, plan, and errors, but merge semantically equivalent low-signal read/open/list actions into grouped lines."
    } else {
        "- Use normal mode: preserve action-level detail when it carries meaning."
    };
    let prompt = format!(
        "Generate a strict JSON turn-summary payload for the active timeline window.\n\
         Return JSON only (no markdown, no prose) with keys:\n\
         {{\"kind\":\"turn-summary\",\"version\":\"2.0\",\"scope\":\"turn|window\",\"turn_meta\":{{\"turn_index\":0,\"anchor_event_index\":0,\"event_span\":{{\"start\":0,\"end\":0}}}},\"prompt\":{{\"text\":\"...\",\"intent\":\"...\",\"constraints\":[\"...\"]}},\"outcome\":{{\"status\":\"in_progress|completed|error\",\"summary\":\"...\"}},\"evidence\":{{\"modified_files\":[{{\"path\":\"...\",\"op\":\"edit|create|delete|read\",\"count\":1}}],\"key_implementations\":[\"...\"],\"agent_quotes\":[\"...\"],\"agent_plan\":[{{\"step\":\"...\",\"status\":\"...\"}}],\"tool_actions\":[{{\"tool\":\"...\",\"status\":\"ok|error\",\"detail\":\"...\"}}],\"errors\":[\"...\"]}},\"cards\":[{{\"type\":\"overview|files|implementation|plan|errors|more\",\"title\":\"...\",\"lines\":[\"...\"],\"severity\":\"info|warn|error\"}}],\"next_steps\":[\"...\"]}}\n\
         Rules:\n\
         - Preserve evidence: modified_files, key_implementations, agent_quotes(1~3), agent_plan.\n\
         - Do not copy system/control instructions as user intent.\n\
         {mode_rule}\n\
         - Keep factual and concise.\n\n{context}"
    );

    let raw = match engine {
        SummaryEngine::Api(provider) => match provider {
            SummaryProvider::Anthropic => call_anthropic(&prompt, runtime).await?,
            SummaryProvider::OpenAi => call_openai(&prompt, runtime).await?,
            SummaryProvider::OpenAiCompatible => call_openai_compatible(&prompt, runtime).await?,
            SummaryProvider::Gemini => call_gemini(&prompt, runtime).await?,
        },
        SummaryEngine::Cli(target) => call_cli(target, &prompt, agent_tool, runtime)?,
    };

    Ok(parse_timeline_summary_output(&raw))
}

pub fn describe_summary_engine(
    provider_hint: Option<&str>,
    runtime: Option<&SummaryRuntimeConfig>,
) -> Result<String> {
    let engine = resolve_engine(provider_hint, runtime)?;
    Ok(match engine {
        SummaryEngine::Api(provider) => format!("api:{}", provider_name(provider)),
        SummaryEngine::Cli(target) => format!("cli:{}", cli_target_name(target)),
    })
}

#[derive(Debug, Deserialize)]
struct TimelineSummaryPayloadRaw {
    kind: Option<String>,
    version: Option<String>,
    scope: Option<String>,
    turn_meta: Option<TurnSummaryTurnMetaRaw>,
    prompt: Option<TurnSummaryPromptRaw>,
    outcome: Option<TurnSummaryOutcomeRaw>,
    evidence: Option<TurnSummaryEvidenceRaw>,
    cards: Option<Vec<BehaviorCardRaw>>,
    next_steps: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct TurnSummaryTurnMetaRaw {
    turn_index: Option<usize>,
    anchor_event_index: Option<usize>,
    event_span: Option<TurnSummaryEventSpanRaw>,
}

#[derive(Debug, Deserialize)]
struct TurnSummaryEventSpanRaw {
    start: Option<usize>,
    end: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
struct TurnSummaryPromptRaw {
    text: Option<String>,
    intent: Option<String>,
    constraints: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Default)]
struct TurnSummaryOutcomeRaw {
    status: Option<String>,
    summary: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FileChangeRaw {
    path: Option<String>,
    op: Option<String>,
    count: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct PlanItemRaw {
    step: Option<String>,
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ToolActionRaw {
    tool: Option<String>,
    status: Option<String>,
    detail: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct TurnSummaryEvidenceRaw {
    modified_files: Option<Vec<FileChangeRaw>>,
    key_implementations: Option<Vec<String>>,
    agent_quotes: Option<Vec<String>>,
    agent_plan: Option<Vec<PlanItemRaw>>,
    tool_actions: Option<Vec<ToolActionRaw>>,
    errors: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct BehaviorCardRaw {
    #[serde(rename = "type")]
    card_type: Option<String>,
    title: Option<String>,
    lines: Option<Vec<String>>,
    severity: Option<String>,
}

pub fn parse_timeline_summary_output(raw: &str) -> TimelineSummaryCacheEntry {
    let trimmed = raw.trim();

    let parsed = if let Ok(payload) = serde_json::from_str::<TimelineSummaryPayloadRaw>(trimmed) {
        Some(payload)
    } else if let Some(json_obj) = extract_first_json_object(trimmed) {
        serde_json::from_str::<TimelineSummaryPayloadRaw>(json_obj).ok()
    } else {
        None
    };

    let payload = match parsed {
        Some(raw_payload) => normalize_turn_summary_payload(raw_payload, trimmed),
        None => fallback_turn_summary_payload(trimmed),
    };

    TimelineSummaryCacheEntry {
        compact: compact_turn_summary_payload(&payload),
        payload,
        raw: trimmed.to_string(),
    }
}

fn extract_first_json_object(raw: &str) -> Option<&str> {
    let bytes = raw.as_bytes();
    let mut depth = 0i32;
    let mut start = None;
    let mut in_string = false;
    let mut escape = false;

    for (idx, &byte) in bytes.iter().enumerate() {
        match byte {
            b'\\' if in_string => {
                escape = !escape;
                continue;
            }
            b'"' if !escape => in_string = !in_string,
            b'{' if !in_string => {
                if depth == 0 {
                    start = Some(idx);
                }
                depth += 1;
            }
            b'}' if !in_string && depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    let start = start?;
                    return Some(&raw[start..=idx]);
                }
            }
            _ => {}
        }

        if in_string {
            escape = false;
        }
    }

    None
}

fn normalize_turn_summary_payload(
    raw: TimelineSummaryPayloadRaw,
    fallback_text: &str,
) -> TimelineSummaryPayload {
    let fallback_text = sanitize_fallback_summary_text(fallback_text);
    let kind = raw.kind.unwrap_or_else(|| "turn-summary".to_string());
    let version = raw.version.unwrap_or_else(|| "2.0".to_string());
    let scope = raw.scope.unwrap_or_else(|| "turn".to_string());
    let fallback = clipped_plain_text(&fallback_text, 220);

    let turn_meta_raw = raw.turn_meta.unwrap_or(TurnSummaryTurnMetaRaw {
        turn_index: Some(0),
        anchor_event_index: Some(0),
        event_span: Some(TurnSummaryEventSpanRaw {
            start: Some(0),
            end: Some(0),
        }),
    });
    let span_raw = turn_meta_raw.event_span.unwrap_or(TurnSummaryEventSpanRaw {
        start: Some(0),
        end: Some(0),
    });
    let turn_meta = TurnSummaryTurnMeta {
        turn_index: turn_meta_raw.turn_index.unwrap_or(0),
        anchor_event_index: turn_meta_raw.anchor_event_index.unwrap_or(0),
        event_span: TurnSummaryEventSpan {
            start: span_raw.start.unwrap_or(0),
            end: span_raw.end.unwrap_or(0),
        },
    };

    let prompt_raw = raw.prompt.unwrap_or_default();
    let prompt = TurnSummaryPrompt {
        text: non_empty_owned(prompt_raw.text).unwrap_or_default(),
        intent: non_empty_owned(prompt_raw.intent).unwrap_or_else(|| {
            if fallback.is_empty() {
                "summarize turn execution".to_string()
            } else {
                fallback.clone()
            }
        }),
        constraints: prompt_raw
            .constraints
            .unwrap_or_default()
            .into_iter()
            .filter_map(|value| non_empty(Some(value.as_str())))
            .take(8)
            .collect(),
    };

    let outcome_raw = raw.outcome.unwrap_or_default();
    let outcome = TurnSummaryOutcome {
        status: non_empty_owned(outcome_raw.status).unwrap_or_else(|| "in_progress".to_string()),
        summary: non_empty_owned(outcome_raw.summary).unwrap_or_else(|| {
            if fallback.is_empty() {
                "summary unavailable for this turn".to_string()
            } else {
                fallback.clone()
            }
        }),
    };

    let evidence_raw = raw.evidence.unwrap_or_default();
    let modified_files = evidence_raw
        .modified_files
        .unwrap_or_default()
        .into_iter()
        .filter_map(|file| {
            let path = non_empty_owned(file.path)?;
            let op = non_empty_owned(file.op).unwrap_or_else(|| "edit".to_string());
            Some(FileChange {
                path,
                op,
                count: file.count.unwrap_or(1).max(1),
            })
        })
        .take(24)
        .collect();
    let key_implementations: Vec<String> = evidence_raw
        .key_implementations
        .unwrap_or_default()
        .into_iter()
        .filter_map(|entry| non_empty(Some(entry.as_str())))
        .take(24)
        .collect();
    let agent_quotes: Vec<String> = evidence_raw
        .agent_quotes
        .unwrap_or_default()
        .into_iter()
        .filter_map(|quote| non_empty(Some(quote.as_str())))
        .map(|quote| clipped_plain_text(&quote, 220))
        .take(3)
        .collect();
    let agent_plan: Vec<PlanItem> = evidence_raw
        .agent_plan
        .unwrap_or_default()
        .into_iter()
        .filter_map(|entry| {
            let step = non_empty_owned(entry.step)?;
            let status = non_empty_owned(entry.status).unwrap_or_else(|| "unknown".to_string());
            Some(PlanItem { step, status })
        })
        .take(20)
        .collect();
    let tool_actions: Vec<ToolAction> = evidence_raw
        .tool_actions
        .unwrap_or_default()
        .into_iter()
        .filter_map(|entry| {
            let tool = non_empty_owned(entry.tool)?;
            let status = non_empty_owned(entry.status).unwrap_or_else(|| "ok".to_string());
            let detail = non_empty_owned(entry.detail).unwrap_or_default();
            Some(ToolAction {
                tool,
                status,
                detail,
            })
        })
        .take(20)
        .collect();
    let errors: Vec<String> = evidence_raw
        .errors
        .unwrap_or_default()
        .into_iter()
        .filter_map(|entry| non_empty(Some(entry.as_str())))
        .take(16)
        .collect();
    let evidence = TurnSummaryEvidence {
        modified_files,
        key_implementations,
        agent_quotes,
        agent_plan,
        tool_actions,
        errors,
    };

    let mut cards: Vec<BehaviorCard> = raw
        .cards
        .unwrap_or_default()
        .into_iter()
        .filter_map(|card| {
            let card_type = normalize_card_type(card.card_type.as_deref());
            let title = non_empty_owned(card.title).unwrap_or_else(|| "Summary".to_string());
            let lines = card
                .lines
                .unwrap_or_default()
                .into_iter()
                .filter_map(|line| non_empty(Some(line.as_str())))
                .take(10)
                .collect::<Vec<_>>();
            if lines.is_empty() {
                return None;
            }
            Some(BehaviorCard {
                card_type,
                title,
                lines,
                severity: normalize_severity(card.severity.as_deref()),
            })
        })
        .collect();

    let mut payload = TimelineSummaryPayload {
        kind,
        version,
        scope,
        turn_meta,
        prompt,
        outcome,
        evidence,
        cards: Vec::new(),
        next_steps: raw
            .next_steps
            .unwrap_or_default()
            .into_iter()
            .filter_map(|step| non_empty(Some(step.as_str())))
            .take(8)
            .collect(),
    };

    if cards.is_empty() {
        cards = default_behavior_cards(&payload);
    }
    payload.cards = clamp_cards(cards, 24);
    payload
}

fn fallback_turn_summary_payload(raw_text: &str) -> TimelineSummaryPayload {
    let visible = sanitize_fallback_summary_text(raw_text);
    let clipped = clipped_plain_text(&visible, 180);
    let fallback_line = if clipped.is_empty() {
        "summary unavailable for this window".to_string()
    } else {
        clipped
    };
    TimelineSummaryPayload {
        kind: "turn-summary".to_string(),
        version: "2.0".to_string(),
        scope: "turn".to_string(),
        turn_meta: TurnSummaryTurnMeta::default(),
        prompt: TurnSummaryPrompt {
            text: String::new(),
            intent: "summarize turn execution".to_string(),
            constraints: Vec::new(),
        },
        outcome: TurnSummaryOutcome {
            status: "in_progress".to_string(),
            summary: fallback_line.clone(),
        },
        evidence: TurnSummaryEvidence::default(),
        cards: vec![BehaviorCard {
            card_type: "overview".to_string(),
            title: "Overview".to_string(),
            lines: vec![fallback_line],
            severity: "warn".to_string(),
        }],
        next_steps: Vec::new(),
    }
}

fn sanitize_fallback_summary_text(raw_text: &str) -> String {
    let lines = raw_text
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }

            if looks_like_internal_summary_template(line) {
                return None;
            }

            if line.starts_with('`') {
                return None;
            }

            Some(line.to_string())
        })
        .collect::<Vec<_>>();

    let joined = lines.join(" ");
    let joined = joined.replace(['{', '}'], "");
    let joined = joined.trim();
    if joined.is_empty() {
        String::new()
    } else {
        joined.to_string()
    }
}

fn looks_like_internal_summary_template(line: &str) -> bool {
    let normalized = line.to_ascii_lowercase();
    let compact = normalized
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>();
    const MARKERS: &[&str] = &[
        "you are generating a turn-summary payload",
        "you are generating a turn summary payload",
        "generate a turn-summary payload",
        "generate turn-summary",
        "generate turn summary payload",
        "you are generating a turn-summary json",
        "you are generating a turn-summary",
        "generate strict json",
        "return json only",
        "using this schema",
        "rules:",
        "keep factual and concise",
        "generate a strict json",
        "generate a strict json-only",
        "hail-summary",
        "generate concise semantic timeline summary",
        "summarize this coding timeline window",
        "generate a concise semantic timeline summary for this window",
        "preserve evidence",
        "do not copy system/control instructions",
        "normal mode:",
        "return turn-summary json (v2)",
        "active timeline window",
        "\"kind\"",
        "\"version\"",
        "\"scope\"",
        "\"implementations\"",
        "\"key_\"",
        "\"turn_meta\"",
        "\"prompt\"",
        "\"outcome\"",
        "\"evidence\"",
        "\"cards\"",
        "\"next_steps\"",
        "\"key_implementations\"",
        "\"modified_files\"",
        "\"agent_quotes\"",
        "\"agent_plan\"",
        "\"tool_actions\"",
        "realtime_scope",
        "summary_scope",
        "tui_layout",
        "interactive response",
        "user's prompt",
        "user prompt",
    ];
    MARKERS.iter().any(|marker| {
        let compact_marker = marker
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>();
        normalized.contains(marker) || compact.contains(&compact_marker)
    }) || looks_like_internal_schema_fragment(&compact)
}

fn looks_like_internal_schema_fragment(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }

    let has_json_like_chars = text.contains('{')
        || text.contains('}')
        || text.contains('[')
        || text.contains(']')
        || text.contains(':')
        || text.contains('\"');

    if !has_json_like_chars {
        return false;
    }

    const FIELD_MARKERS: &[&str] = &[
        "\"kind\"",
        "\"version\"",
        "\"scope\"",
        "\"turn_meta\"",
        "\"key_\"",
        "\"prompt\"",
        "\"outcome\"",
        "\"evidence\"",
        "\"cards\"",
        "\"next_steps\"",
        "\"modified_files\"",
        "\"key_implementations\"",
        "\"agent_quotes\"",
        "\"agent_plan\"",
        "\"tool_actions\"",
        "\"implementations\"",
        "\"agent\"",
        "\"kind\"",
        "\"summary\"",
        "\"status\"",
        "\"summary_scope\"",
        "\"realtime_scope\"",
        "\"tui_layout\"",
        "preserve evidence",
        "do not copy system/control instructions",
        "rules:",
        "keep factual and concise",
    ];

    FIELD_MARKERS
        .iter()
        .any(|marker| text.contains(marker) || text.contains(&marker.replace('\"', "")))
}

fn default_behavior_cards(payload: &TimelineSummaryPayload) -> Vec<BehaviorCard> {
    if payload.scope.trim().eq_ignore_ascii_case("window") {
        let mut cards = Vec::new();
        let primary = primary_window_action(payload)
            .or_else(|| non_empty(Some(payload.outcome.summary.as_str())))
            .unwrap_or_else(|| "No primary action captured".to_string());
        cards.push(BehaviorCard {
            card_type: "overview".to_string(),
            title: "Primary Action".to_string(),
            lines: vec![primary],
            severity: if payload.outcome.status.eq_ignore_ascii_case("error") {
                "error".to_string()
            } else {
                "info".to_string()
            },
        });
        if !payload.evidence.modified_files.is_empty() {
            cards.push(BehaviorCard {
                card_type: "files".to_string(),
                title: "Affected Files".to_string(),
                lines: payload
                    .evidence
                    .modified_files
                    .iter()
                    .take(4)
                    .map(|f| format!("{} ({}, x{})", f.path, f.op, f.count))
                    .collect(),
                severity: "info".to_string(),
            });
        }
        if !payload.evidence.errors.is_empty() {
            cards.push(BehaviorCard {
                card_type: "errors".to_string(),
                title: "Action Errors".to_string(),
                lines: payload.evidence.errors.iter().take(4).cloned().collect(),
                severity: "error".to_string(),
            });
        }
        return cards;
    }

    let mut cards = Vec::new();
    let overview_line = if payload.outcome.summary.trim().is_empty() {
        "No outcome summary".to_string()
    } else {
        payload.outcome.summary.trim().to_string()
    };
    cards.push(BehaviorCard {
        card_type: "overview".to_string(),
        title: "Overview".to_string(),
        lines: vec![overview_line],
        severity: if payload.outcome.status.eq_ignore_ascii_case("error") {
            "error".to_string()
        } else {
            "info".to_string()
        },
    });

    if !payload.evidence.modified_files.is_empty() {
        cards.push(BehaviorCard {
            card_type: "files".to_string(),
            title: "Modified Files".to_string(),
            lines: payload
                .evidence
                .modified_files
                .iter()
                .take(8)
                .map(|f| format!("{} ({}, x{})", f.path, f.op, f.count))
                .collect(),
            severity: "info".to_string(),
        });
    }
    if !payload.evidence.key_implementations.is_empty() {
        cards.push(BehaviorCard {
            card_type: "implementation".to_string(),
            title: "Key Implementation".to_string(),
            lines: payload
                .evidence
                .key_implementations
                .iter()
                .take(8)
                .cloned()
                .collect(),
            severity: "info".to_string(),
        });
    }
    if !payload.evidence.agent_plan.is_empty() {
        cards.push(BehaviorCard {
            card_type: "plan".to_string(),
            title: "Agent Plan".to_string(),
            lines: payload
                .evidence
                .agent_plan
                .iter()
                .take(8)
                .map(|item| format!("[{}] {}", item.status, item.step))
                .collect(),
            severity: "info".to_string(),
        });
    }
    if !payload.evidence.errors.is_empty() {
        cards.push(BehaviorCard {
            card_type: "errors".to_string(),
            title: "Errors".to_string(),
            lines: payload.evidence.errors.iter().take(8).cloned().collect(),
            severity: "error".to_string(),
        });
    }
    cards
}

fn clamp_cards(mut cards: Vec<BehaviorCard>, max_cards: usize) -> Vec<BehaviorCard> {
    if cards.len() <= max_cards {
        return cards;
    }
    let hidden = cards.len().saturating_sub(max_cards.saturating_sub(1));
    cards.truncate(max_cards.saturating_sub(1));
    cards.push(BehaviorCard {
        card_type: "more".to_string(),
        title: "More".to_string(),
        lines: vec![format!("{hidden} additional cards omitted")],
        severity: "info".to_string(),
    });
    cards
}

fn normalize_card_type(value: Option<&str>) -> String {
    match value.unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "overview" => "overview".to_string(),
        "files" => "files".to_string(),
        "implementation" => "implementation".to_string(),
        "plan" => "plan".to_string(),
        "errors" => "errors".to_string(),
        "more" => "more".to_string(),
        _ => "overview".to_string(),
    }
}

fn normalize_severity(value: Option<&str>) -> String {
    match value.unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "error" => "error".to_string(),
        "warn" | "warning" => "warn".to_string(),
        _ => "info".to_string(),
    }
}

fn compact_turn_summary_payload(payload: &TimelineSummaryPayload) -> String {
    let kind_ok = payload.kind.eq_ignore_ascii_case("turn-summary");
    let scope = if payload.scope.trim().is_empty() {
        "turn".to_string()
    } else {
        payload.scope.trim().to_string()
    };
    if scope.eq_ignore_ascii_case("window") {
        let mut parts: Vec<String> = Vec::new();
        if let Some(action) = primary_window_action(payload) {
            parts.push(format!("action: {}", clipped_plain_text(&action, 140)));
        }
        let summary = payload.outcome.summary.trim();
        if parts.is_empty() && !summary.is_empty() {
            parts.push(format!("outcome: {}", clipped_plain_text(summary, 140)));
        }
        if let Some(error) = payload.evidence.errors.first() {
            parts.push(format!("error: {}", clipped_plain_text(error, 120)));
        }
        if let Some(step) = payload.next_steps.first() {
            let trimmed = step.trim();
            if !trimmed.is_empty() {
                parts.push(format!("next: {}", clipped_plain_text(trimmed, 120)));
            }
        }
        if parts.is_empty() {
            return "summary unavailable for this window".to_string();
        }

        let prefix = if kind_ok {
            "[turn-summary:window]".to_string()
        } else {
            "[summary:window]".to_string()
        };
        let mut out = format!("{prefix} {}", parts.join(" | "));
        if out.chars().count() > 220 {
            out = out.chars().take(217).collect::<String>() + "...";
        }
        return out;
    }

    let mut parts: Vec<String> = Vec::new();

    let summary = payload.outcome.summary.trim();
    if !summary.is_empty() {
        parts.push(format!("outcome: {summary}"));
    }
    if !payload.evidence.modified_files.is_empty() {
        parts.push(format!("files:{}", payload.evidence.modified_files.len()));
    }
    if !payload.evidence.key_implementations.is_empty() {
        parts.push(format!(
            "impl:{}",
            payload.evidence.key_implementations.len()
        ));
    }
    if !payload.evidence.agent_plan.is_empty() {
        parts.push(format!("plan:{}", payload.evidence.agent_plan.len()));
    }
    if !payload.evidence.errors.is_empty() {
        parts.push(format!("errors:{}", payload.evidence.errors.len()));
    }
    if let Some(step) = payload.next_steps.first() {
        let trimmed = step.trim();
        if !trimmed.is_empty() {
            parts.push(format!("next: {trimmed}"));
        }
    }

    if parts.is_empty() {
        return "summary unavailable for this window".to_string();
    }

    let prefix = if kind_ok {
        format!("[turn-summary:{scope}]")
    } else {
        format!("[summary:{scope}]")
    };
    let mut out = format!("{prefix} {}", parts.join(" | "));
    if out.chars().count() > 220 {
        out = out.chars().take(217).collect::<String>() + "...";
    }
    out
}

fn primary_window_action(payload: &TimelineSummaryPayload) -> Option<String> {
    if let Some(action) = payload.evidence.tool_actions.first() {
        let tool = action.tool.trim();
        let status = action.status.trim();
        let detail = action.detail.trim();
        if !tool.is_empty() || !detail.is_empty() {
            let mut line = String::new();
            if !tool.is_empty() {
                line.push_str(tool);
            }
            if !status.is_empty() {
                if !line.is_empty() {
                    line.push(' ');
                }
                line.push('(');
                line.push_str(status);
                line.push(')');
            }
            if !detail.is_empty() {
                if !line.is_empty() {
                    line.push_str(": ");
                }
                line.push_str(detail);
            }
            if !line.is_empty() {
                return Some(line);
            }
        }
    }

    if let Some(file) = payload.evidence.modified_files.first() {
        let path = file.path.trim();
        if !path.is_empty() {
            let op = file.op.trim();
            if op.is_empty() {
                return Some(format!("file {path} (x{})", file.count));
            }
            return Some(format!("{op} {path} (x{})", file.count));
        }
    }

    if let Some(line) = payload.evidence.key_implementations.first() {
        let line = line.trim();
        if !line.is_empty() {
            return Some(line.to_string());
        }
    }

    if let Some(plan) = payload.evidence.agent_plan.first() {
        let step = plan.step.trim();
        if !step.is_empty() {
            let status = plan.status.trim();
            if status.is_empty() {
                return Some(step.to_string());
            }
            return Some(format!("[{status}] {step}"));
        }
    }

    None
}

fn non_empty_owned(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn clipped_plain_text(value: &str, max_chars: usize) -> String {
    let compact = value.replace('\n', " ").trim().to_string();
    if compact.chars().count() <= max_chars {
        compact
    } else {
        let mut out = String::new();
        for ch in compact.chars().take(max_chars.saturating_sub(3)) {
            out.push(ch);
        }
        out.push_str("...");
        out
    }
}

pub async fn probe_summary_cli_providers(
    agent_tool: Option<&str>,
) -> Result<SummaryCliProbeResult> {
    let candidates = detect_cli_candidates(SummaryCliTarget::Auto, agent_tool);
    let mut grouped: Vec<(SummaryCliTarget, Vec<ResolvedCliCommand>)> = Vec::new();

    for candidate in candidates {
        if let Some((_, group)) = grouped
            .iter_mut()
            .find(|(target, _)| *target == candidate.target)
        {
            group.push(candidate);
        } else {
            grouped.push((candidate.target, vec![candidate]));
        }
    }

    let mut attempted = Vec::new();
    let mut responsive = Vec::new();
    let mut errors: Vec<(String, String)> = Vec::new();

    for (target, commands) in grouped {
        if target == SummaryCliTarget::Auto {
            continue;
        }
        let provider = provider_for_target(target).to_string();
        let installed: Vec<ResolvedCliCommand> = commands
            .into_iter()
            .filter(|candidate| command_exists(&candidate.bin))
            .collect();
        if installed.is_empty() {
            continue;
        }
        attempted.push(provider.clone());

        let mut passed = false;
        let mut last_err = None;
        for candidate in installed {
            match probe_cli_candidate(&candidate, "hello", agent_tool) {
                Ok(_) => {
                    responsive.push(provider.clone());
                    passed = true;
                    break;
                }
                Err(err) => {
                    last_err = Some(err.to_string());
                }
            }
        }
        if !passed {
            errors.push((
                provider.clone(),
                last_err.unwrap_or_else(|| "probe failed".to_string()),
            ));
        }
    }

    if attempted.is_empty() {
        bail!("no installed summary CLI found");
    }

    Ok(SummaryCliProbeResult {
        attempted_providers: attempted,
        recommended_provider: responsive.first().cloned(),
        responsive_providers: responsive,
        errors,
    })
}

fn resolve_engine(
    provider_hint: Option<&str>,
    runtime: Option<&SummaryRuntimeConfig>,
) -> Result<SummaryEngine> {
    match provider_hint.map(|v| v.to_ascii_lowercase()) {
        Some(p) if p == "anthropic" => Ok(SummaryEngine::Api(SummaryProvider::Anthropic)),
        Some(p) if p == "openai" => Ok(SummaryEngine::Api(SummaryProvider::OpenAi)),
        Some(p) if p == "openai-compatible" => {
            Ok(SummaryEngine::Api(SummaryProvider::OpenAiCompatible))
        }
        Some(p) if p == "gemini" => Ok(SummaryEngine::Api(SummaryProvider::Gemini)),
        Some(p) if p == "auto" || p.is_empty() => resolve_auto_provider(runtime),
        Some(p) if p == "cli" || p == "cli:auto" => Ok(SummaryEngine::Cli(SummaryCliTarget::Auto)),
        Some(p) if p == "cli:codex" => Ok(SummaryEngine::Cli(SummaryCliTarget::Codex)),
        Some(p) if p == "cli:claude" => Ok(SummaryEngine::Cli(SummaryCliTarget::Claude)),
        Some(p) if p == "cli:cursor" => Ok(SummaryEngine::Cli(SummaryCliTarget::Cursor)),
        Some(p) if p == "cli:gemini" => Ok(SummaryEngine::Cli(SummaryCliTarget::Gemini)),
        Some(other) => bail!("unsupported summary provider: {other}"),
        None => resolve_auto_provider(runtime),
    }
}

fn resolve_auto_provider(runtime: Option<&SummaryRuntimeConfig>) -> Result<SummaryEngine> {
    if std::env::var("ANTHROPIC_API_KEY").is_ok() {
        return Ok(SummaryEngine::Api(SummaryProvider::Anthropic));
    }
    if std::env::var("OPENAI_API_KEY").is_ok() {
        return Ok(SummaryEngine::Api(SummaryProvider::OpenAi));
    }
    if has_openai_compatible_endpoint_config(runtime) {
        return Ok(SummaryEngine::Api(SummaryProvider::OpenAiCompatible));
    }
    if std::env::var("GEMINI_API_KEY").is_ok() || std::env::var("GOOGLE_API_KEY").is_ok() {
        return Ok(SummaryEngine::Api(SummaryProvider::Gemini));
    }
    if env_trimmed("OPS_TL_SUM_CLI_BIN").is_some() {
        return Ok(SummaryEngine::Cli(SummaryCliTarget::Auto));
    }
    bail!("no summary API key found and no CLI summary binary configured")
}

fn call_cli(
    target: SummaryCliTarget,
    prompt: &str,
    agent_tool: Option<&str>,
    runtime: Option<&SummaryRuntimeConfig>,
) -> Result<String> {
    let command = resolve_cli_command(target, agent_tool)?;
    let (args, codex_output_file) = build_cli_args(&command, prompt, runtime);
    let output = run_with_timeout(&command.bin, &args, summary_cli_timeout())
        .with_context(|| format!("failed to execute summary CLI: {}", command.bin))?;
    extract_cli_output(&output, codex_output_file)
}

fn resolve_cli_command(
    target: SummaryCliTarget,
    agent_tool: Option<&str>,
) -> Result<ResolvedCliCommand> {
    if let Some(raw) = env_trimmed("OPS_TL_SUM_CLI_BIN") {
        let (bin, mut pre_args) = parse_bin_and_args(&raw)?;
        let resolved_target = if target == SummaryCliTarget::Auto {
            infer_cli_target(&bin, agent_tool).unwrap_or(SummaryCliTarget::Codex)
        } else {
            target
        };
        if pre_args.is_empty() {
            pre_args.extend(default_pre_args(resolved_target));
        }
        return Ok(ResolvedCliCommand {
            target: resolved_target,
            bin,
            pre_args,
        });
    }

    for candidate in detect_cli_candidates(target, agent_tool) {
        if command_exists(&candidate.bin) {
            return Ok(candidate);
        }
    }
    bail!("could not resolve CLI summary binary")
}

fn detect_cli_candidates(
    target: SummaryCliTarget,
    agent_tool: Option<&str>,
) -> Vec<ResolvedCliCommand> {
    let from_tool = agent_tool
        .map(|t| t.to_ascii_lowercase())
        .unwrap_or_default();

    let preferred_targets: Vec<SummaryCliTarget> = match target {
        SummaryCliTarget::Codex => vec![SummaryCliTarget::Codex],
        SummaryCliTarget::Claude => vec![SummaryCliTarget::Claude],
        SummaryCliTarget::Cursor => vec![SummaryCliTarget::Cursor],
        SummaryCliTarget::Gemini => vec![SummaryCliTarget::Gemini],
        SummaryCliTarget::Auto => {
            if from_tool.contains("codex") {
                vec![
                    SummaryCliTarget::Codex,
                    SummaryCliTarget::Claude,
                    SummaryCliTarget::Cursor,
                    SummaryCliTarget::Gemini,
                ]
            } else if from_tool.contains("claude") {
                vec![
                    SummaryCliTarget::Claude,
                    SummaryCliTarget::Codex,
                    SummaryCliTarget::Cursor,
                    SummaryCliTarget::Gemini,
                ]
            } else if from_tool.contains("cursor") {
                vec![
                    SummaryCliTarget::Cursor,
                    SummaryCliTarget::Codex,
                    SummaryCliTarget::Claude,
                    SummaryCliTarget::Gemini,
                ]
            } else if from_tool.contains("gemini") {
                vec![
                    SummaryCliTarget::Gemini,
                    SummaryCliTarget::Codex,
                    SummaryCliTarget::Claude,
                    SummaryCliTarget::Cursor,
                ]
            } else {
                vec![
                    SummaryCliTarget::Codex,
                    SummaryCliTarget::Claude,
                    SummaryCliTarget::Cursor,
                    SummaryCliTarget::Gemini,
                ]
            }
        }
    };

    let mut candidates = Vec::new();
    for preferred in preferred_targets {
        candidates.extend(cli_candidates_for_target(preferred));
    }
    candidates
}

fn cli_candidates_for_target(target: SummaryCliTarget) -> Vec<ResolvedCliCommand> {
    match target {
        SummaryCliTarget::Auto => Vec::new(),
        SummaryCliTarget::Codex => vec![ResolvedCliCommand {
            target,
            bin: "codex".to_string(),
            pre_args: default_pre_args(target),
        }],
        SummaryCliTarget::Claude => vec![ResolvedCliCommand {
            target,
            bin: "claude".to_string(),
            pre_args: default_pre_args(target),
        }],
        SummaryCliTarget::Cursor => vec![
            ResolvedCliCommand {
                target,
                bin: "cursor".to_string(),
                pre_args: default_pre_args(target),
            },
            ResolvedCliCommand {
                target,
                bin: "cursor-agent".to_string(),
                pre_args: Vec::new(),
            },
        ],
        SummaryCliTarget::Gemini => vec![ResolvedCliCommand {
            target,
            bin: "gemini".to_string(),
            pre_args: default_pre_args(target),
        }],
    }
}

fn default_pre_args(target: SummaryCliTarget) -> Vec<String> {
    match target {
        SummaryCliTarget::Codex => vec!["exec".to_string()],
        SummaryCliTarget::Cursor => vec!["agent".to_string()],
        SummaryCliTarget::Auto | SummaryCliTarget::Claude | SummaryCliTarget::Gemini => Vec::new(),
    }
}

fn add_default_noninteractive_args(target: SummaryCliTarget, args: &mut Vec<String>) {
    match target {
        SummaryCliTarget::Codex => {}
        SummaryCliTarget::Claude => {
            args.push("--print".to_string());
            args.push("--output-format".to_string());
            args.push("text".to_string());
        }
        SummaryCliTarget::Cursor => {
            args.push("--print".to_string());
            args.push("--output-format".to_string());
            args.push("text".to_string());
        }
        SummaryCliTarget::Gemini => {
            args.push("--output-format".to_string());
            args.push("text".to_string());
        }
        SummaryCliTarget::Auto => {}
    }
}

fn add_prompt_arg(target: SummaryCliTarget, args: &mut Vec<String>, prompt: &str) {
    if target == SummaryCliTarget::Gemini && !has_flag(args, "--prompt", "-p") {
        args.push("--prompt".to_string());
    }
    args.push(prompt.to_string());
}

fn has_flag(args: &[String], long: &str, short: &str) -> bool {
    args.iter().any(|arg| arg == long || arg == short)
}

fn parse_bin_and_args(raw: &str) -> Result<(String, Vec<String>)> {
    let tokens: Vec<String> = raw
        .split_whitespace()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let Some((bin, rest)) = tokens.split_first() else {
        bail!("OPS_TL_SUM_CLI_BIN is empty");
    };
    Ok((bin.clone(), rest.to_vec()))
}

fn infer_cli_target(bin: &str, agent_tool: Option<&str>) -> Option<SummaryCliTarget> {
    let lower = bin.to_ascii_lowercase();
    if lower.contains("codex") {
        return Some(SummaryCliTarget::Codex);
    }
    if lower.contains("claude") {
        return Some(SummaryCliTarget::Claude);
    }
    if lower.contains("cursor") {
        return Some(SummaryCliTarget::Cursor);
    }
    if lower.contains("gemini") {
        return Some(SummaryCliTarget::Gemini);
    }

    let from_tool = agent_tool?.to_ascii_lowercase();
    if from_tool.contains("codex") {
        Some(SummaryCliTarget::Codex)
    } else if from_tool.contains("claude") {
        Some(SummaryCliTarget::Claude)
    } else if from_tool.contains("cursor") {
        Some(SummaryCliTarget::Cursor)
    } else if from_tool.contains("gemini") {
        Some(SummaryCliTarget::Gemini)
    } else {
        None
    }
}

fn provider_for_target(target: SummaryCliTarget) -> &'static str {
    match target {
        SummaryCliTarget::Codex => "cli:codex",
        SummaryCliTarget::Claude => "cli:claude",
        SummaryCliTarget::Cursor => "cli:cursor",
        SummaryCliTarget::Gemini => "cli:gemini",
        SummaryCliTarget::Auto => "cli:auto",
    }
}

fn provider_name(provider: SummaryProvider) -> &'static str {
    match provider {
        SummaryProvider::Anthropic => "anthropic",
        SummaryProvider::OpenAi => "openai",
        SummaryProvider::OpenAiCompatible => "openai-compatible",
        SummaryProvider::Gemini => "gemini",
    }
}

fn cli_target_name(target: SummaryCliTarget) -> &'static str {
    match target {
        SummaryCliTarget::Auto => "auto",
        SummaryCliTarget::Codex => "codex",
        SummaryCliTarget::Claude => "claude",
        SummaryCliTarget::Cursor => "cursor",
        SummaryCliTarget::Gemini => "gemini",
    }
}

fn build_cli_args(
    command: &ResolvedCliCommand,
    prompt: &str,
    runtime: Option<&SummaryRuntimeConfig>,
) -> (Vec<String>, Option<PathBuf>) {
    let mut args = command.pre_args.clone();
    if let Some(raw) = env_trimmed("OPS_TL_SUM_CLI_ARGS") {
        args.extend(raw.split_whitespace().map(|s| s.to_string()));
    } else {
        add_default_noninteractive_args(command.target, &mut args);
    }

    if let Some(model) = summary_model_override(runtime) {
        if !has_flag(&args, "--model", "-m") {
            args.push("--model".to_string());
            args.push(model);
        }
    }

    let mut codex_output_file = None;
    if command.target == SummaryCliTarget::Codex && !has_flag(&args, "--output-last-message", "-o")
    {
        let path = build_temp_output_file("opensession-timeline-summary-codex");
        args.push("--output-last-message".to_string());
        args.push(path.to_string_lossy().to_string());
        codex_output_file = Some(path);
    }

    add_prompt_arg(command.target, &mut args, prompt);
    (args, codex_output_file)
}

fn extract_cli_output(
    output: &std::process::Output,
    codex_output_file: Option<PathBuf>,
) -> Result<String> {
    if !output.status.success() {
        let status_text = match output.status.code() {
            Some(code) => format!("exit code {code}"),
            None => "terminated by signal".to_string(),
        };
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            "no stdout/stderr output".to_string()
        };
        let compact = detail.replace('\n', " ");
        let clipped = if compact.chars().count() > 220 {
            let mut out = String::new();
            for ch in compact.chars().take(217) {
                out.push(ch);
            }
            out.push_str("...");
            out
        } else {
            compact
        };
        bail!("summary CLI failed ({status_text}): {clipped}");
    }

    if let Some(path) = codex_output_file {
        if let Ok(last_message) = fs::read_to_string(&path) {
            let _ = fs::remove_file(&path);
            if !last_message.trim().is_empty() {
                return Ok(last_message);
            }
        } else {
            let _ = fs::remove_file(&path);
        }
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn probe_cli_candidate(
    command: &ResolvedCliCommand,
    prompt: &str,
    _agent_tool: Option<&str>,
) -> Result<String> {
    let (args, codex_output_file) = build_cli_args(command, prompt, None);
    let output = run_with_timeout(&command.bin, &args, Duration::from_secs(8))
        .with_context(|| format!("failed to execute summary CLI: {}", command.bin))?;
    let text = extract_cli_output(&output, codex_output_file)?;
    if text.trim().is_empty() {
        bail!("summary CLI returned an empty response");
    }
    Ok(text)
}

fn run_with_timeout(bin: &str, args: &[String], timeout: Duration) -> Result<std::process::Output> {
    let mut child = Command::new(bin)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to spawn summary CLI: {bin}"))?;

    let started = Instant::now();
    loop {
        if let Some(_status) = child.try_wait()? {
            return child
                .wait_with_output()
                .context("failed to read summary CLI output");
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            bail!("summary CLI probe timed out after {}s", timeout.as_secs());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn summary_cli_timeout() -> Duration {
    let ms = env_trimmed("OPS_TL_SUM_CLI_TIMEOUT_MS")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(45_000)
        .max(1_000);
    Duration::from_millis(ms)
}

fn build_temp_output_file(prefix: &str) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("{prefix}-{}-{now}.txt", std::process::id()))
}

fn env_trimmed(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn env_first(names: &[&str]) -> Option<String> {
    for name in names {
        if let Some(value) = env_trimmed(name) {
            return Some(value);
        }
    }
    None
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

fn config_or_env(configured: Option<&str>, env_names: &[&str]) -> Option<String> {
    non_empty(configured).or_else(|| env_first(env_names))
}

fn has_openai_compatible_endpoint_config(runtime: Option<&SummaryRuntimeConfig>) -> bool {
    config_or_env(
        runtime.and_then(|cfg| cfg.openai_compat_endpoint.as_deref()),
        &["OPS_TL_SUM_ENDPOINT"],
    )
    .is_some()
        || config_or_env(
            runtime.and_then(|cfg| cfg.openai_compat_base.as_deref()),
            &["OPS_TL_SUM_BASE", "OPENAI_BASE_URL"],
        )
        .is_some()
}

fn summary_model_override(runtime: Option<&SummaryRuntimeConfig>) -> Option<String> {
    config_or_env(
        runtime.and_then(|cfg| cfg.model.as_deref()),
        &["OPS_TL_SUM_MODEL"],
    )
}

fn command_exists(command: &str) -> bool {
    Command::new("sh")
        .arg("-lc")
        .arg(format!("command -v {command} >/dev/null 2>&1"))
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

async fn call_anthropic(prompt: &str, runtime: Option<&SummaryRuntimeConfig>) -> Result<String> {
    let api_key = std::env::var("ANTHROPIC_API_KEY").context("ANTHROPIC_API_KEY not set")?;
    let model =
        summary_model_override(runtime).unwrap_or_else(|| "claude-sonnet-4-5-20250929".to_string());

    let request_body = serde_json::json!({
        "model": model,
        "max_tokens": 256,
        "messages": [{"role": "user", "content": prompt}]
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&request_body)
        .send()
        .await
        .context("failed to call Anthropic API")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("Anthropic API error (HTTP {status}): {body}");
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .context("failed to parse Anthropic response")?;
    Ok(body
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|block| block.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string())
}

async fn call_openai(prompt: &str, runtime: Option<&SummaryRuntimeConfig>) -> Result<String> {
    let api_key = std::env::var("OPENAI_API_KEY").context("OPENAI_API_KEY not set")?;
    let base_url = std::env::var("OPENAI_BASE_URL")
        .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
    let model = summary_model_override(runtime).unwrap_or_else(|| "gpt-4o-mini".to_string());

    let request_body = serde_json::json!({
        "model": model,
        "max_tokens": 256,
        "messages": [{"role": "user", "content": prompt}]
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let resp = client
        .post(format!("{base_url}/chat/completions"))
        .header("Authorization", format!("Bearer {api_key}"))
        .header("content-type", "application/json")
        .json(&request_body)
        .send()
        .await
        .context("failed to call OpenAI API")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("OpenAI API error (HTTP {status}): {body}");
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .context("failed to parse OpenAI response")?;
    Ok(body
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|msg| msg.get("content"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string())
}

async fn call_openai_compatible(
    prompt: &str,
    runtime: Option<&SummaryRuntimeConfig>,
) -> Result<String> {
    let endpoint = openai_compatible_endpoint_url(runtime);
    let model = summary_model_override(runtime).unwrap_or_else(|| "gpt-4o-mini".to_string());
    let style = summary_openai_compat_style(&endpoint, runtime);

    let request_body = if style == "responses" {
        serde_json::json!({
            "model": model,
            "max_output_tokens": 256,
            "input": prompt
        })
    } else {
        serde_json::json!({
            "model": model,
            "max_tokens": 256,
            "messages": [{"role": "user", "content": prompt}]
        })
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let mut req = client
        .post(&endpoint)
        .header("content-type", "application/json")
        .json(&request_body);

    if let Some(api_key) = config_or_env(
        runtime.and_then(|cfg| cfg.openai_compat_api_key.as_deref()),
        &["OPS_TL_SUM_KEY", "OPENAI_API_KEY"],
    ) {
        if let Some(header_name) = config_or_env(
            runtime.and_then(|cfg| cfg.openai_compat_api_key_header.as_deref()),
            &["OPS_TL_SUM_KEY_HEADER"],
        ) {
            req = req.header(header_name, api_key);
        } else {
            req = req.header("Authorization", format!("Bearer {api_key}"));
        }
    }

    let resp = req
        .send()
        .await
        .context("failed to call OpenAI-compatible API")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("OpenAI-compatible API error (HTTP {status}): {body}");
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .context("failed to parse OpenAI-compatible response")?;
    let text = extract_openai_compatible_text(&body);
    if text.trim().is_empty() {
        bail!("OpenAI-compatible API returned an empty response");
    }
    Ok(text)
}

fn openai_compatible_endpoint_url(runtime: Option<&SummaryRuntimeConfig>) -> String {
    if let Some(full) = config_or_env(
        runtime.and_then(|cfg| cfg.openai_compat_endpoint.as_deref()),
        &["OPS_TL_SUM_ENDPOINT"],
    ) {
        return full;
    }

    let base = config_or_env(
        runtime.and_then(|cfg| cfg.openai_compat_base.as_deref()),
        &["OPS_TL_SUM_BASE", "OPENAI_BASE_URL"],
    )
    .unwrap_or_else(|| "https://api.openai.com/v1".to_string());

    let path = config_or_env(
        runtime.and_then(|cfg| cfg.openai_compat_path.as_deref()),
        &["OPS_TL_SUM_PATH"],
    )
    .unwrap_or_else(|| "/chat/completions".to_string());

    let base_lower = base.to_ascii_lowercase();
    if base_lower.contains("/chat/completions") || base_lower.contains("/responses") {
        return base;
    }

    let normalized_path = if path.starts_with('/') {
        path
    } else {
        format!("/{path}")
    };
    format!("{}{}", base.trim_end_matches('/'), normalized_path)
}

fn summary_openai_compat_style(endpoint: &str, runtime: Option<&SummaryRuntimeConfig>) -> String {
    if let Some(style) = config_or_env(
        runtime.and_then(|cfg| cfg.openai_compat_style.as_deref()),
        &["OPS_TL_SUM_STYLE"],
    ) {
        let normalized = style.to_ascii_lowercase();
        if normalized == "responses" || normalized == "chat" {
            return normalized;
        }
    }

    if endpoint.to_ascii_lowercase().contains("/responses") {
        "responses".to_string()
    } else {
        "chat".to_string()
    }
}

fn extract_openai_compatible_text(body: &serde_json::Value) -> String {
    if let Some(text) = body.get("output_text").and_then(|v| v.as_str()) {
        return text.to_string();
    }

    if let Some(text) = body
        .get("choices")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|msg| msg.get("content"))
        .and_then(|content| content.as_str())
    {
        return text.to_string();
    }

    if let Some(content) = body
        .get("choices")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|msg| msg.get("content"))
        .and_then(|content| content.as_array())
    {
        let mut parts = Vec::new();
        for block in content {
            if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                if !text.trim().is_empty() {
                    parts.push(text.trim().to_string());
                }
            }
        }
        if !parts.is_empty() {
            return parts.join("\n");
        }
    }

    if let Some(content_arr) = body.get("output").and_then(|v| v.as_array()) {
        let mut parts = Vec::new();
        for item in content_arr {
            let Some(blocks) = item.get("content").and_then(|v| v.as_array()) else {
                continue;
            };
            for block in blocks {
                if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                    if !text.trim().is_empty() {
                        parts.push(text.trim().to_string());
                    }
                }
            }
        }
        if !parts.is_empty() {
            return parts.join("\n");
        }
    }

    String::new()
}

async fn call_gemini(prompt: &str, runtime: Option<&SummaryRuntimeConfig>) -> Result<String> {
    let api_key = std::env::var("GEMINI_API_KEY")
        .or_else(|_| std::env::var("GOOGLE_API_KEY"))
        .context("GEMINI_API_KEY or GOOGLE_API_KEY not set")?;
    let model = summary_model_override(runtime).unwrap_or_else(|| "gemini-2.0-flash".to_string());

    let url =
        format!("https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent");
    let request_body = serde_json::json!({
        "contents": [{"parts": [{"text": prompt}]}],
        "generationConfig": {"maxOutputTokens": 256}
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let resp = client
        .post(&url)
        .header("x-goog-api-key", &api_key)
        .header("content-type", "application/json")
        .json(&request_body)
        .send()
        .await
        .context("failed to call Gemini API")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("Gemini API error (HTTP {status}): {body}");
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .context("failed to parse Gemini response")?;
    Ok(body
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
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::parse_timeline_summary_output;

    #[test]
    fn parse_turn_summary_v2_with_evidence_cards() {
        let raw = r#"{
          "kind":"turn-summary",
          "version":"2.0",
          "scope":"turn",
          "turn_meta":{"turn_index":2,"anchor_event_index":101,"event_span":{"start":95,"end":111}},
          "prompt":{"text":"Add view mode","intent":"Implement turn summary redesign","constraints":["preserve evidence"]},
          "outcome":{"status":"completed","summary":"Turn renderer switched to prompt-row + card rows."},
          "evidence":{
            "modified_files":[{"path":"crates/tui/src/views/session_detail.rs","op":"edit","count":4}],
            "key_implementations":["Added PromptRow/SummaryCardRow layout"],
            "agent_quotes":["\"I will refactor render_turn_view first.\""],
            "agent_plan":[{"step":"wire row model","status":"completed"}],
            "tool_actions":[{"tool":"apply_patch","status":"ok","detail":"updated renderer"}],
            "errors":[]
          },
          "cards":[
            {"type":"overview","title":"Overview","lines":["Layout switched"],"severity":"info"},
            {"type":"files","title":"Files","lines":["session_detail.rs updated"],"severity":"info"}
          ],
          "next_steps":["Run tui tests"]
        }"#;

        let parsed = parse_timeline_summary_output(raw);
        assert_eq!(parsed.payload.kind, "turn-summary");
        assert_eq!(parsed.payload.version, "2.0");
        assert_eq!(parsed.payload.scope, "turn");
        assert_eq!(parsed.payload.turn_meta.turn_index, 2);
        assert_eq!(parsed.payload.evidence.modified_files.len(), 1);
        assert_eq!(parsed.payload.cards.len(), 2);
        assert!(parsed.compact.contains("files:1"));
    }

    #[test]
    fn parse_non_json_fallback_keeps_compact_message() {
        let raw = "summary unavailable because backend timeout";
        let parsed = parse_timeline_summary_output(raw);
        assert_eq!(parsed.payload.kind, "turn-summary");
        assert!(parsed
            .payload
            .outcome
            .summary
            .contains("summary unavailable"));
        assert!(!parsed.compact.trim().is_empty());
    }

    #[test]
    fn parse_prompt_preface_fallback_is_sanitized() {
        let raw = "You are generating a turn-summary payload.\nReturn JSON only (no markdown, no prose) using this schema:\n{ \"agent_quotes\": [\"...\"] }\n";
        let parsed = parse_timeline_summary_output(raw);
        assert_eq!(parsed.payload.kind, "turn-summary");
        assert!(parsed
            .payload
            .outcome
            .summary
            .contains("summary unavailable"));
        assert!(!parsed
            .payload
            .outcome
            .summary
            .contains("You are generating"));
        assert!(!parsed.payload.outcome.summary.contains("\"agent_quotes\""));
    }

    #[test]
    fn parse_prompt_preface_fragmented_internal_keys_is_sanitized() {
        let raw = "Generate a turn-summary payload.\n\"key_\"\n\"implementations\":[\"...\"],\"agent_quotes\":[\"...\"],\"agent_plan\":[{\"step\":\"...\",\"status\":\"...\"}]";
        let parsed = parse_timeline_summary_output(raw);
        assert_eq!(parsed.payload.kind, "turn-summary");
        assert!(parsed
            .payload
            .outcome
            .summary
            .contains("summary unavailable"));
        assert!(!parsed.payload.outcome.summary.contains("implementations"));
        assert!(!parsed.payload.outcome.summary.contains("agent_quotes"));
    }

    #[test]
    fn parse_prompt_rules_preface_is_sanitized() {
        let raw = "Return JSON only (no markdown, no prose) with keys:\nRules:\n- Preserve evidence: modified_files, key_implementations, agent_quotes(1~3), agent_plan.\n- Do not copy system/control instructions as user intent.\n- Keep factual and concise.\n";
        let parsed = parse_timeline_summary_output(raw);
        assert_eq!(parsed.payload.kind, "turn-summary");
        assert!(parsed
            .payload
            .outcome
            .summary
            .contains("summary unavailable"));
        assert!(
            !parsed.payload.outcome.summary.contains("Preserve evidence"),
            "internal rule lines should not appear in summary"
        );
        assert!(
            !parsed.payload.outcome.summary.contains("system/control"),
            "internal schema hints should be removed"
        );
    }

    #[test]
    fn parse_json_in_prompt_blocking_text() {
        let raw = "Here is the result:\n```json\n{\"kind\":\"turn-summary\",\"version\":\"2.0\",\"scope\":\"turn\",\"turn_meta\":{\"turn_index\":1,\"anchor_event_index\":0,\"event_span\":{\"start\":1,\"end\":2}},\"prompt\":{\"text\":\"x\",\"intent\":\"y\",\"constraints\":[]},\"outcome\":{\"status\":\"completed\",\"summary\":\"good\"},\"evidence\":{\"modified_files\":[],\"key_implementations\":[],\"agent_quotes\":[],\"agent_plan\":[],\"tool_actions\":[],\"errors\":[]},\"cards\":[],\"next_steps\":[]}\n```";
        let parsed = parse_timeline_summary_output(raw);
        assert_eq!(parsed.payload.kind, "turn-summary");
        assert_eq!(parsed.payload.version, "2.0");
        assert_eq!(parsed.payload.outcome.summary, "good");
    }
}
