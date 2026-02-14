use anyhow::{bail, Context, Result};
use serde::Deserialize;
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
}

#[derive(Debug, Clone)]
pub struct SummaryCliProbeResult {
    pub attempted_providers: Vec<String>,
    pub responsive_providers: Vec<String>,
    pub recommended_provider: Option<String>,
    pub errors: Vec<(String, String)>,
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
) -> Result<String> {
    let engine = resolve_engine(provider_hint)?;
    let prompt = format!(
        "You are generating a HAIL-summary payload.\n\
         Return JSON only (no markdown, no prose) using this schema:\n\
         {{\"kind\":\"HAIL-summary\",\"version\":\"1.0\",\"scope\":\"window|turn\",\"intent\":\"...\",\"progress\":\"...\",\"changes\":[\"...\"],\"next\":\"...\"}}\n\
         Rules:\n\
         - intent/progress/next must be concise plain text\n\
         - changes is a short array (0~3)\n\
         - keep each field short and factual\n\n{context}"
    );

    let raw = match engine {
        SummaryEngine::Api(provider) => match provider {
            SummaryProvider::Anthropic => call_anthropic(&prompt).await?,
            SummaryProvider::OpenAi => call_openai(&prompt).await?,
            SummaryProvider::OpenAiCompatible => call_openai_compatible(&prompt).await?,
            SummaryProvider::Gemini => call_gemini(&prompt).await?,
        },
        SummaryEngine::Cli(target) => call_cli(target, &prompt, agent_tool)?,
    };

    Ok(normalize_hail_summary_output(&raw))
}

#[derive(Debug, Deserialize)]
struct HailSummaryPayload {
    kind: Option<String>,
    version: Option<String>,
    scope: Option<String>,
    intent: Option<String>,
    progress: Option<String>,
    changes: Option<Vec<String>>,
    next: Option<String>,
}

fn normalize_hail_summary_output(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Ok(payload) = serde_json::from_str::<HailSummaryPayload>(trimmed) {
        return compact_hail_payload(payload);
    }

    if let Some(json_start) = trimmed.find('{') {
        if let Some(json_end) = trimmed.rfind('}') {
            if json_end > json_start {
                let candidate = &trimmed[json_start..=json_end];
                if let Ok(payload) = serde_json::from_str::<HailSummaryPayload>(candidate) {
                    return compact_hail_payload(payload);
                }
            }
        }
    }

    let mut fallback = trimmed.replace('\n', " ");
    if fallback.chars().count() > 180 {
        fallback = fallback.chars().take(177).collect::<String>() + "...";
    }
    fallback
}

fn compact_hail_payload(payload: HailSummaryPayload) -> String {
    let kind_ok = payload
        .kind
        .as_deref()
        .is_some_and(|k| k.eq_ignore_ascii_case("HAIL-summary"));
    let scope = payload.scope.unwrap_or_else(|| "window".to_string());
    let mut parts: Vec<String> = Vec::new();

    if let Some(intent) = payload.intent {
        let v = intent.trim();
        if !v.is_empty() {
            parts.push(format!("intent: {v}"));
        }
    }
    if let Some(progress) = payload.progress {
        let v = progress.trim();
        if !v.is_empty() {
            parts.push(format!("progress: {v}"));
        }
    }
    if let Some(changes) = payload.changes {
        let compact_changes: Vec<String> = changes
            .into_iter()
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
            .take(2)
            .collect();
        if !compact_changes.is_empty() {
            parts.push(format!("changes: {}", compact_changes.join(", ")));
        }
    }
    if let Some(next) = payload.next {
        let v = next.trim();
        if !v.is_empty() {
            parts.push(format!("next: {v}"));
        }
    }

    if parts.is_empty() {
        return "summary unavailable for this window".to_string();
    }

    let prefix = if kind_ok {
        if payload
            .version
            .as_deref()
            .is_some_and(|v| !v.trim().is_empty())
        {
            format!("[HAIL-summary:{scope}]")
        } else {
            format!("[HAIL-summary:{scope}]")
        }
    } else {
        format!("[summary:{scope}]")
    };
    let mut out = format!("{prefix} {}", parts.join(" | "));
    if out.chars().count() > 220 {
        out = out.chars().take(217).collect::<String>() + "...";
    }
    out
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

fn resolve_engine(provider_hint: Option<&str>) -> Result<SummaryEngine> {
    match provider_hint.map(|v| v.to_ascii_lowercase()) {
        Some(p) if p == "anthropic" => Ok(SummaryEngine::Api(SummaryProvider::Anthropic)),
        Some(p) if p == "openai" => Ok(SummaryEngine::Api(SummaryProvider::OpenAi)),
        Some(p) if p == "openai-compatible" => {
            Ok(SummaryEngine::Api(SummaryProvider::OpenAiCompatible))
        }
        Some(p) if p == "gemini" => Ok(SummaryEngine::Api(SummaryProvider::Gemini)),
        Some(p) if p == "auto" || p.is_empty() => resolve_auto_provider(),
        Some(p) if p == "cli" || p == "cli:auto" => Ok(SummaryEngine::Cli(SummaryCliTarget::Auto)),
        Some(p) if p == "cli:codex" => Ok(SummaryEngine::Cli(SummaryCliTarget::Codex)),
        Some(p) if p == "cli:claude" => Ok(SummaryEngine::Cli(SummaryCliTarget::Claude)),
        Some(p) if p == "cli:cursor" => Ok(SummaryEngine::Cli(SummaryCliTarget::Cursor)),
        Some(p) if p == "cli:gemini" => Ok(SummaryEngine::Cli(SummaryCliTarget::Gemini)),
        Some(other) => bail!("unsupported summary provider: {other}"),
        None => resolve_auto_provider(),
    }
}

fn resolve_auto_provider() -> Result<SummaryEngine> {
    if std::env::var("ANTHROPIC_API_KEY").is_ok() {
        return Ok(SummaryEngine::Api(SummaryProvider::Anthropic));
    }
    if std::env::var("OPENAI_API_KEY").is_ok() {
        return Ok(SummaryEngine::Api(SummaryProvider::OpenAi));
    }
    if has_openai_compatible_endpoint_config() {
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

fn call_cli(target: SummaryCliTarget, prompt: &str, agent_tool: Option<&str>) -> Result<String> {
    let command = resolve_cli_command(target, agent_tool)?;
    let (args, codex_output_file) = build_cli_args(&command, prompt);

    let output = Command::new(&command.bin)
        .args(&args)
        .output()
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
        .unwrap_or_else(String::new);

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
    if target == SummaryCliTarget::Gemini {
        if !has_flag(args, "--prompt", "-p") {
            args.push("--prompt".to_string());
        }
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

fn build_cli_args(command: &ResolvedCliCommand, prompt: &str) -> (Vec<String>, Option<PathBuf>) {
    let mut args = command.pre_args.clone();
    if let Some(raw) = env_trimmed("OPS_TL_SUM_CLI_ARGS") {
        args.extend(raw.split_whitespace().map(|s| s.to_string()));
    } else {
        add_default_noninteractive_args(command.target, &mut args);
    }

    if let Some(model) = summary_model_override() {
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
    let (args, codex_output_file) = build_cli_args(command, prompt);
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

fn has_openai_compatible_endpoint_config() -> bool {
    env_first(&["OPS_TL_SUM_ENDPOINT", "OPS_TL_SUM_BASE", "OPENAI_BASE_URL"]).is_some()
}

fn summary_model_override() -> Option<String> {
    env_first(&["OPS_TL_SUM_MODEL"])
}

fn command_exists(command: &str) -> bool {
    Command::new("sh")
        .arg("-lc")
        .arg(format!("command -v {command} >/dev/null 2>&1"))
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

async fn call_anthropic(prompt: &str) -> Result<String> {
    let api_key = std::env::var("ANTHROPIC_API_KEY").context("ANTHROPIC_API_KEY not set")?;
    let model =
        summary_model_override().unwrap_or_else(|| "claude-sonnet-4-5-20250929".to_string());

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

async fn call_openai(prompt: &str) -> Result<String> {
    let api_key = std::env::var("OPENAI_API_KEY").context("OPENAI_API_KEY not set")?;
    let base_url = std::env::var("OPENAI_BASE_URL")
        .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
    let model = summary_model_override().unwrap_or_else(|| "gpt-4o-mini".to_string());

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

async fn call_openai_compatible(prompt: &str) -> Result<String> {
    let endpoint = openai_compatible_endpoint_url();
    let model = summary_model_override().unwrap_or_else(|| "gpt-4o-mini".to_string());
    let style = summary_openai_compat_style(&endpoint);

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

    if let Some(api_key) = env_first(&["OPS_TL_SUM_KEY", "OPENAI_API_KEY"]) {
        if let Some(header_name) = env_first(&["OPS_TL_SUM_KEY_HEADER"]) {
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

fn openai_compatible_endpoint_url() -> String {
    if let Some(full) = env_first(&["OPS_TL_SUM_ENDPOINT"]) {
        return full;
    }

    let base = env_first(&["OPS_TL_SUM_BASE", "OPENAI_BASE_URL"])
        .unwrap_or_else(|| "https://api.openai.com/v1".to_string());

    let path = env_first(&["OPS_TL_SUM_PATH"]).unwrap_or_else(|| "/chat/completions".to_string());

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

fn summary_openai_compat_style(endpoint: &str) -> String {
    if let Some(style) = env_first(&["OPS_TL_SUM_STYLE"]) {
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

async fn call_gemini(prompt: &str) -> Result<String> {
    let api_key = std::env::var("GEMINI_API_KEY")
        .or_else(|_| std::env::var("GOOGLE_API_KEY"))
        .context("GEMINI_API_KEY or GOOGLE_API_KEY not set")?;
    let model = summary_model_override().unwrap_or_else(|| "gemini-2.0-flash".to_string());

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
