use opensession_runtime_config::{SummaryProvider, SummarySettings};
use opensession_summary::{SemanticSummary, parse_semantic_summary_or_fallback};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DEFAULT_OLLAMA_ENDPOINT: &str = "http://127.0.0.1:11434";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSummaryProfile {
    pub provider: SummaryProvider,
    pub endpoint: String,
    pub model: String,
}

pub fn detect_local_summary_profile() -> Option<LocalSummaryProfile> {
    first_available_profile([
        detect_ollama_profile(),
        detect_codex_exec_profile(),
        detect_claude_cli_profile(),
    ])
}

fn first_available_profile<const N: usize>(
    profiles: [Option<LocalSummaryProfile>; N],
) -> Option<LocalSummaryProfile> {
    profiles.into_iter().flatten().next()
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
    let raw = generate_text(settings, prompt).await?;
    Ok(parse_semantic_summary_or_fallback(&raw, settings))
}

pub async fn generate_text(settings: &SummarySettings, prompt: &str) -> Result<String, String> {
    if prompt.trim().is_empty() {
        return Err("summary prompt is empty".to_string());
    }
    if !settings.is_configured() {
        return Err("local summary provider is not configured".to_string());
    }

    match settings.provider.id {
        SummaryProvider::Disabled => Err("local summary provider is disabled".to_string()),
        SummaryProvider::Ollama => generate_text_with_ollama(settings, prompt).await,
        SummaryProvider::CodexExec => generate_text_with_codex_exec(settings, prompt).await,
        SummaryProvider::ClaudeCli => generate_text_with_claude_cli(settings, prompt).await,
    }
}

async fn generate_text_with_ollama(
    settings: &SummarySettings,
    prompt: &str,
) -> Result<String, String> {
    let endpoint = if settings.provider.endpoint.trim().is_empty() {
        DEFAULT_OLLAMA_ENDPOINT
    } else {
        settings.provider.endpoint.trim()
    };
    let url = format!("{}/api/generate", endpoint.trim_end_matches('/'));
    let model = settings.provider.model.trim();
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

    Ok(payload.response)
}

async fn generate_text_with_codex_exec(
    settings: &SummarySettings,
    prompt: &str,
) -> Result<String, String> {
    let output_path = temp_cli_output_path("codex-summary");

    let mut command = Command::new("codex");
    command
        .arg("exec")
        .arg("--skip-git-repo-check")
        .arg("--sandbox")
        .arg("read-only")
        .arg("--output-last-message")
        .arg(output_path.to_string_lossy().to_string());
    let model = settings.provider.model.trim();
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
    Ok(response)
}

async fn generate_text_with_claude_cli(
    settings: &SummarySettings,
    prompt: &str,
) -> Result<String, String> {
    let model = settings.provider.model.trim().to_string();
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
    Ok(response)
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
mod tests {
    use super::{
        LocalSummaryProfile, SummaryProvider, first_available_profile, parse_ollama_list_output,
    };

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
    fn first_available_profile_preserves_provider_priority() {
        let claude = Some(LocalSummaryProfile {
            provider: SummaryProvider::ClaudeCli,
            endpoint: String::new(),
            model: String::new(),
        });
        let codex = Some(LocalSummaryProfile {
            provider: SummaryProvider::CodexExec,
            endpoint: String::new(),
            model: String::new(),
        });

        let selected = first_available_profile([None, codex.clone(), claude]);
        assert_eq!(selected, codex);
    }
}
