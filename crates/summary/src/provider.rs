use opensession_runtime_config::{
    SummaryOutputShape, SummaryProvider, SummaryResponseStyle, SummarySettings,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DEFAULT_OLLAMA_ENDPOINT: &str = "http://127.0.0.1:11434";
const DEFAULT_SUMMARY_CHAR_LIMIT: usize = 420;
const DEFAULT_AUTH_SECURITY_CHAR_LIMIT: usize = 260;
const DEFAULT_LAYER_SUMMARY_CHAR_LIMIT: usize = 200;
const DEFAULT_MAX_LAYER_ITEMS: usize = 8;
const DEFAULT_MAX_FILES_PER_LAYER: usize = 10;

#[derive(Debug, Clone, Copy)]
struct SummaryNormalizationLimits {
    summary_chars: usize,
    auth_security_chars: usize,
    layer_summary_chars: usize,
    max_layer_items: usize,
    max_files_per_layer: usize,
}

fn summary_limits(settings: &SummarySettings) -> SummaryNormalizationLimits {
    let (summary_chars, auth_security_chars, layer_summary_chars) = match settings.response_style {
        SummaryResponseStyle::Compact => (280, 160, 120),
        SummaryResponseStyle::Standard => (
            DEFAULT_SUMMARY_CHAR_LIMIT,
            DEFAULT_AUTH_SECURITY_CHAR_LIMIT,
            DEFAULT_LAYER_SUMMARY_CHAR_LIMIT,
        ),
        SummaryResponseStyle::Detailed => (720, 420, 320),
    };
    let (max_layer_items, max_files_per_layer) = match settings.output_shape {
        SummaryOutputShape::Layered => (DEFAULT_MAX_LAYER_ITEMS, DEFAULT_MAX_FILES_PER_LAYER),
        SummaryOutputShape::FileList => (16, 20),
        SummaryOutputShape::SecurityFirst => (10, 12),
    };
    SummaryNormalizationLimits {
        summary_chars,
        auth_security_chars,
        layer_summary_chars,
        max_layer_items,
        max_files_per_layer,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSummaryProfile {
    pub provider: SummaryProvider,
    pub endpoint: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticSummary {
    pub changes: String,
    pub auth_security: String,
    #[serde(default)]
    pub layer_file_changes: Vec<LayerFileChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LayerFileChange {
    pub layer: String,
    pub summary: String,
    #[serde(default)]
    pub files: Vec<String>,
}

impl SemanticSummary {
    fn normalize(mut self, limits: SummaryNormalizationLimits) -> Self {
        self.changes = normalize_summary_text_with_limit(&self.changes, limits.summary_chars);
        self.auth_security =
            normalize_summary_text_with_limit(&self.auth_security, limits.auth_security_chars);
        if self.auth_security.is_empty() {
            self.auth_security = "none detected".to_string();
        }

        self.layer_file_changes = self
            .layer_file_changes
            .into_iter()
            .filter_map(|item| {
                let layer = normalize_summary_text_with_limit(&item.layer, 40);
                if layer.is_empty() {
                    return None;
                }
                let summary =
                    normalize_summary_text_with_limit(&item.summary, limits.layer_summary_chars);
                let mut files = item
                    .files
                    .into_iter()
                    .map(|file| normalize_summary_text_with_limit(&file, 120))
                    .filter(|file| !file.is_empty())
                    .collect::<Vec<_>>();
                files.sort();
                files.dedup();
                files.truncate(limits.max_files_per_layer);

                Some(LayerFileChange {
                    layer,
                    summary,
                    files,
                })
            })
            .collect();
        self.layer_file_changes
            .sort_by(|lhs, rhs| lhs.layer.cmp(&rhs.layer));
        self.layer_file_changes.truncate(limits.max_layer_items);
        self
    }

    fn from_plain_fallback(text: &str, limits: SummaryNormalizationLimits) -> Self {
        Self {
            changes: normalize_summary_text_with_limit(text, limits.summary_chars),
            auth_security: "none detected".to_string(),
            layer_file_changes: Vec::new(),
        }
        .normalize(limits)
    }
}

pub fn detect_local_summary_profile() -> Option<LocalSummaryProfile> {
    detect_ollama_profile()
        .or_else(detect_codex_exec_profile)
        .or_else(detect_claude_cli_profile)
}

fn detect_ollama_profile() -> Option<LocalSummaryProfile> {
    let output = Command::new("ollama").arg("list").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let model = parse_ollama_list_output(&stdout).into_iter().next()?;
    Some(LocalSummaryProfile {
        provider: SummaryProvider::Ollama,
        endpoint: DEFAULT_OLLAMA_ENDPOINT.to_string(),
        model,
    })
}

fn detect_codex_exec_profile() -> Option<LocalSummaryProfile> {
    if !command_available("codex", &["exec", "--help"]) {
        return None;
    }
    Some(LocalSummaryProfile {
        provider: SummaryProvider::CodexExec,
        endpoint: String::new(),
        model: String::new(),
    })
}

fn detect_claude_cli_profile() -> Option<LocalSummaryProfile> {
    if !command_available("claude", &["--help"]) {
        return None;
    }
    Some(LocalSummaryProfile {
        provider: SummaryProvider::ClaudeCli,
        endpoint: String::new(),
        model: String::new(),
    })
}

fn command_available(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn parse_ollama_list_output(raw: &str) -> Vec<String> {
    let mut models = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lowered = trimmed.to_ascii_lowercase();
        if lowered.starts_with("name ")
            || lowered.starts_with("error")
            || lowered.starts_with("failed")
        {
            continue;
        }
        let Some(token) = trimmed.split_whitespace().next() else {
            continue;
        };
        let candidate = token.trim().to_string();
        if candidate.is_empty() || models.contains(&candidate) {
            continue;
        }
        models.push(candidate);
    }
    models
}

#[derive(Debug, Serialize)]
struct OllamaGenerateRequest<'a> {
    model: &'a str,
    prompt: &'a str,
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct OllamaGenerateResponse {
    response: String,
}

pub async fn generate_summary(
    settings: &SummarySettings,
    prompt: &str,
) -> Result<SemanticSummary, String> {
    if prompt.trim().is_empty() {
        return Err("summary prompt is empty".to_string());
    }
    if !settings.is_configured() {
        return Err("local summary provider is not configured".to_string());
    }

    match settings.provider {
        SummaryProvider::Disabled => Err("local summary provider is disabled".to_string()),
        SummaryProvider::Ollama => generate_with_ollama(settings, prompt).await,
        SummaryProvider::CodexExec => generate_with_codex_exec(settings, prompt).await,
        SummaryProvider::ClaudeCli => generate_with_claude_cli(settings, prompt).await,
    }
}

async fn generate_with_ollama(
    settings: &SummarySettings,
    prompt: &str,
) -> Result<SemanticSummary, String> {
    let endpoint = if settings.endpoint.trim().is_empty() {
        DEFAULT_OLLAMA_ENDPOINT
    } else {
        settings.endpoint.trim()
    };
    let url = format!("{}/api/generate", endpoint.trim_end_matches('/'));
    let model = settings.model.trim();
    if model.is_empty() {
        return Err("ollama model is empty".to_string());
    }

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|err| format!("failed to build local summary HTTP client: {err}"))?;

    let response = client
        .post(url)
        .json(&OllamaGenerateRequest {
            model,
            prompt,
            stream: false,
        })
        .send()
        .await
        .map_err(|err| format!("failed to call ollama summary API: {err}"))?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "ollama summary API returned {status}: {}",
            body.trim()
        ));
    }

    let payload: OllamaGenerateResponse = response
        .json()
        .await
        .map_err(|err| format!("failed to decode ollama summary response: {err}"))?;
    if payload.response.trim().is_empty() {
        return Err("ollama summary response was empty".to_string());
    }

    Ok(parse_semantic_summary_or_fallback(
        &payload.response,
        settings,
    ))
}

async fn generate_with_codex_exec(
    settings: &SummarySettings,
    prompt: &str,
) -> Result<SemanticSummary, String> {
    let output_path = temp_cli_output_path("codex-summary");

    let mut command = Command::new("codex");
    command
        .arg("exec")
        .arg("--skip-git-repo-check")
        .arg("--sandbox")
        .arg("read-only")
        .arg("--output-last-message")
        .arg(output_path.to_string_lossy().to_string());
    let model = settings.model.trim();
    if !model.is_empty() {
        command.arg("--model").arg(model);
    }
    command.arg(prompt);

    let output = run_command_with_timeout(command, Duration::from_secs(60))
        .map_err(|err| format!("failed to run codex exec summary: {err}"))?;

    let response = read_output_or_stdout(&output_path, &output);
    if response.trim().is_empty() {
        return Err("codex exec summary response was empty".to_string());
    }
    Ok(parse_semantic_summary_or_fallback(&response, settings))
}

async fn generate_with_claude_cli(
    settings: &SummarySettings,
    prompt: &str,
) -> Result<SemanticSummary, String> {
    let model = settings.model.trim().to_string();
    let timeout = Duration::from_secs(60);

    let mut command = Command::new("claude");
    command.arg("-c");
    if !model.is_empty() {
        command.arg("--model").arg(&model);
    }
    command.arg(prompt);

    let output = match run_command_with_timeout(command, timeout) {
        Ok(output) => output,
        Err(primary_error) => {
            let mut fallback = Command::new("claude");
            fallback
                .arg("--print")
                .arg("--output-format")
                .arg("text")
                .arg("--no-session-persistence")
                .arg("--tools")
                .arg("");
            if !model.is_empty() {
                fallback.arg("--model").arg(&model);
            }
            fallback.arg(prompt);

            run_command_with_timeout(fallback, timeout).map_err(|fallback_error| {
                format!(
                    "failed to run claude summary (`claude -c` => {primary_error}; fallback => {fallback_error})"
                )
            })?
        }
    };

    let response = String::from_utf8_lossy(&output.stdout).to_string();
    if response.trim().is_empty() {
        return Err("claude summary response was empty".to_string());
    }
    Ok(parse_semantic_summary_or_fallback(&response, settings))
}

fn parse_semantic_summary_or_fallback(raw: &str, settings: &SummarySettings) -> SemanticSummary {
    let limits = summary_limits(settings);
    match parse_semantic_summary(raw) {
        Ok(summary) => summary.normalize(limits),
        Err(_) => SemanticSummary::from_plain_fallback(raw, limits),
    }
}

fn read_output_or_stdout(path: &PathBuf, output: &Output) -> String {
    let file_text = fs::read_to_string(path).unwrap_or_default();
    let _ = fs::remove_file(path);
    if !file_text.trim().is_empty() {
        return file_text;
    }
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn temp_cli_output_path(prefix: &str) -> PathBuf {
    let pid = std::process::id();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("{prefix}-{pid}-{timestamp}.txt"))
}

fn run_command_with_timeout(mut command: Command, timeout: Duration) -> Result<Output, String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let program = command.get_program().to_string_lossy().to_string();
    let mut child = command
        .spawn()
        .map_err(|err| format!("failed to spawn `{program}`: {err}"))?;
    let started = Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(_status)) => {
                let output = child
                    .wait_with_output()
                    .map_err(|err| format!("failed to collect `{program}` output: {err}"))?;
                if output.status.success() {
                    return Ok(output);
                }
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let detail = if !stderr.is_empty() {
                    stderr
                } else if !stdout.is_empty() {
                    stdout
                } else {
                    format!("exit status {}", output.status)
                };
                return Err(format!("`{program}` failed: {detail}"));
            }
            Ok(None) => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "`{program}` timed out after {}s",
                        timeout.as_secs()
                    ));
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(err) => {
                return Err(format!("failed while waiting for `{program}`: {err}"));
            }
        }
    }
}

#[cfg(test)]
fn normalize_summary_text(raw: &str) -> String {
    normalize_summary_text_with_limit(raw, DEFAULT_SUMMARY_CHAR_LIMIT)
}

fn normalize_summary_text_with_limit(raw: &str, limit: usize) -> String {
    let compact = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= limit {
        return compact;
    }
    let mut out = String::new();
    for ch in compact.chars().take(limit.saturating_sub(1)) {
        out.push(ch);
    }
    out.push('…');
    out
}

fn parse_semantic_summary(raw: &str) -> Result<SemanticSummary, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("empty summary payload".to_string());
    }

    if let Ok(parsed) = serde_json::from_str::<SemanticSummary>(trimmed) {
        return Ok(parsed);
    }

    if let Some(json_block) = strip_markdown_json_fence(trimmed) {
        if let Ok(parsed) = serde_json::from_str::<SemanticSummary>(&json_block) {
            return Ok(parsed);
        }
    }

    if let Some(object_slice) = find_json_object_slice(trimmed) {
        if let Ok(parsed) = serde_json::from_str::<SemanticSummary>(object_slice) {
            return Ok(parsed);
        }
    }

    Err("failed to parse semantic summary JSON".to_string())
}

fn strip_markdown_json_fence(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if !trimmed.starts_with("```") {
        return None;
    }
    let mut lines = trimmed.lines();
    let first = lines.next()?.trim().to_ascii_lowercase();
    if !(first == "```json" || first == "```") {
        return None;
    }
    let remaining = lines.collect::<Vec<_>>().join("\n");
    let end_idx = remaining.rfind("```")?;
    Some(remaining[..end_idx].trim().to_string())
}

fn find_json_object_slice(raw: &str) -> Option<&str> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    if end <= start {
        return None;
    }
    Some(raw[start..=end].trim())
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_summary_text, parse_ollama_list_output, parse_semantic_summary,
        parse_semantic_summary_or_fallback, SemanticSummary,
    };
    use opensession_runtime_config::{SummaryOutputShape, SummaryResponseStyle, SummarySettings};

    #[test]
    fn parse_ollama_list_output_extracts_model_names() {
        let output = r#"
NAME                      ID              SIZE      MODIFIED
llama3.2:3b               a80c4f17acd5    2.0 GB    3 hours ago
qwen2.5-coder:7b          2b0496514337    4.7 GB    1 day ago
"#;

        let models = parse_ollama_list_output(output);
        assert_eq!(
            models,
            vec!["llama3.2:3b".to_string(), "qwen2.5-coder:7b".to_string()]
        );
    }

    #[test]
    fn parse_ollama_list_output_ignores_errors_and_empty_lines() {
        let output = "\nError: could not connect to ollama\n";
        assert!(parse_ollama_list_output(output).is_empty());
    }

    #[test]
    fn normalize_summary_text_collapses_whitespace_and_limits_length() {
        let raw = "  fixed   setup flow\nand  added    summary  cache ";
        assert_eq!(
            normalize_summary_text(raw),
            "fixed setup flow and added summary cache"
        );
    }

    #[test]
    fn parse_semantic_summary_accepts_plain_json() {
        let raw = r#"{
  "changes": "Updated session summary pipeline",
  "auth_security": "none detected",
  "layer_file_changes": [
    {"layer":"application","summary":"Added queue handling","files":["crates/summary/src/lib.rs"]}
  ]
}"#;

        let parsed = parse_semantic_summary(raw).expect("parse semantic summary");
        assert_eq!(parsed.changes, "Updated session summary pipeline");
        assert_eq!(parsed.auth_security, "none detected");
        assert_eq!(parsed.layer_file_changes.len(), 1);
        assert_eq!(parsed.layer_file_changes[0].layer, "application");
    }

    #[test]
    fn parse_semantic_summary_accepts_markdown_code_fence() {
        let raw = r#"```json
{"changes":"c","auth_security":"none","layer_file_changes":[]}
```"#;
        let parsed = parse_semantic_summary(raw).expect("parse fenced semantic summary");
        assert_eq!(
            parsed,
            SemanticSummary {
                changes: "c".to_string(),
                auth_security: "none".to_string(),
                layer_file_changes: Vec::new()
            }
        );
    }

    #[test]
    fn parse_semantic_summary_fallback_preserves_plain_text_changes() {
        let parsed = parse_semantic_summary_or_fallback(
            "updated auth token handling",
            &SummarySettings::default(),
        );
        assert_eq!(parsed.changes, "updated auth token handling");
        assert_eq!(parsed.auth_security, "none detected");
        assert!(parsed.layer_file_changes.is_empty());
    }

    #[test]
    fn parse_semantic_summary_fallback_applies_compact_style_limits() {
        let mut settings = SummarySettings::default();
        settings.response_style = SummaryResponseStyle::Compact;
        let parsed = parse_semantic_summary_or_fallback(&"x".repeat(400), &settings);
        assert!(parsed.changes.chars().count() <= 280);
    }

    #[test]
    fn parse_semantic_summary_fallback_applies_file_list_shape_limits() {
        let mut settings = SummarySettings::default();
        settings.output_shape = SummaryOutputShape::FileList;
        let payload = r#"{
  "changes":"summary",
  "auth_security":"none detected",
  "layer_file_changes":[
    {"layer":"application","summary":"changed","files":[
      "f01","f02","f03","f04","f05","f06","f07","f08","f09","f10",
      "f11","f12","f13","f14","f15","f16","f17","f18","f19","f20","f21"
    ]}
  ]
}"#;
        let parsed = parse_semantic_summary_or_fallback(payload, &settings);
        assert_eq!(parsed.layer_file_changes.len(), 1);
        assert_eq!(parsed.layer_file_changes[0].files.len(), 20);
    }
}
