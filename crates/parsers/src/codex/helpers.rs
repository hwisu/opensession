use super::*;

pub(super) fn parse_function_output(raw: &str) -> (String, bool, Option<u64>) {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
        let mut output = v
            .get("output")
            .and_then(|v| v.as_str())
            .unwrap_or(raw)
            .to_string();
        let exit_code = v
            .get("metadata")
            .and_then(|m| m.get("exit_code"))
            .and_then(|c| c.as_i64());
        let duration = v
            .get("metadata")
            .and_then(|m| m.get("duration_seconds"))
            .and_then(|d| d.as_f64())
            .map(|s| (s * 1000.0) as u64);
        let is_error = exit_code.is_some_and(|c| c != 0);
        if is_low_signal_output_marker(&output) || output.trim().is_empty() {
            let metadata = v.get("metadata");
            let recovered = [
                v.get("stdout"),
                v.get("stderr"),
                v.get("message"),
                v.get("result"),
                v.get("content"),
                metadata.and_then(|value| value.get("stdout")),
                metadata.and_then(|value| value.get("stderr")),
                metadata.and_then(|value| value.get("message")),
            ]
            .into_iter()
            .flatten()
            .filter_map(|value| value.as_str())
            .map(str::trim)
            .find(|value| !value.is_empty() && !is_low_signal_output_marker(value))
            .map(str::to_string);

            if let Some(restored) = recovered {
                output = restored;
            } else if let Ok(serialized) = serde_json::to_string(&v) {
                if !serialized.trim().is_empty() && !is_low_signal_output_marker(&serialized) {
                    output = serialized;
                }
            }
        }
        (output, is_error, duration)
    } else {
        (raw.to_string(), false, None)
    }
}

pub(super) fn is_low_signal_output_marker(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty()
        && trimmed.chars().count() <= 8
        && trimmed.chars().all(|ch| {
            matches!(
                ch,
                '.' | '\u{00B7}' | '\u{2022}' | '-' | '_' | '=' | '~' | '`'
            )
        })
}

pub(super) fn extract_message_text_blocks(content: Option<&serde_json::Value>) -> String {
    let Some(content) = content else {
        return String::new();
    };
    if let Some(text) = content.as_str() {
        return text.trim().to_string();
    }
    let Some(blocks) = content.as_array() else {
        return String::new();
    };

    blocks
        .iter()
        .filter_map(|block| {
            if let Some(text) = block.as_str() {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    return None;
                }
                return Some(trimmed.to_string());
            }
            let btype = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match btype {
                "text" | "input_text" | "output_text" => block
                    .get("text")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(String::from),
                _ => block
                    .get("text")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(String::from),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn looks_like_injected_codex_user_text(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }

    if looks_like_summary_batch_prompt(trimmed) {
        return true;
    }

    let lower = trimmed.to_ascii_lowercase();

    if lower.contains("apply_patch was requested via exec_command")
        && lower.contains("use the apply_patch tool instead")
    {
        return true;
    }

    lower == "agents.md instructions"
        || lower.starts_with("# agents.md instructions")
        || lower.contains("<instructions>")
        || lower.contains("</instructions>")
        || lower.contains("<environment_context>")
        || lower.contains("</environment_context>")
        || lower.contains("<subagent_notification")
        || lower.contains("&lt;subagent_notification")
        || lower.contains("</subagent_notification>")
        || lower.contains("subagent_notification>")
        || lower.contains("<turn_aborted>")
        || lower.contains("</turn_aborted>")
}

pub(super) fn looks_like_summary_batch_prompt(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    lower.starts_with("convert a real coding session into semantic compression.")
        && lower.contains("pipeline: session -> hail compact -> semantic summary")
        && lower.contains("hail_compact=")
}

pub(super) fn codex_desktop_parent_session_id(payload: &serde_json::Value) -> Option<String> {
    non_empty_json_str(payload.pointer("/source/subagent/thread_spawn/parent_thread_id"))
        .or_else(|| non_empty_json_str(payload.pointer("/source/subagent/parent_thread_id")))
        .or_else(|| non_empty_json_str(payload.pointer("/source/parent_thread_id")))
        .or_else(|| non_empty_json_str(payload.get("parent_thread_id")))
}

pub(super) fn codex_desktop_payload_is_auxiliary(payload: &serde_json::Value) -> bool {
    if payload.pointer("/source/subagent").is_some() {
        return true;
    }

    payload
        .get("agent_role")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some_and(|role| {
            matches!(
                role.to_ascii_lowercase().as_str(),
                "awaiter" | "worker" | "explorer" | "subagent"
            )
        })
}

pub(super) fn non_empty_json_str(value: Option<&serde_json::Value>) -> Option<String> {
    value
        .and_then(|entry| entry.as_str())
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
}

pub(super) fn json_object_string(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    match value {
        serde_json::Value::Object(map) => {
            for key in keys {
                if let Some(s) = map.get(*key).and_then(|entry| entry.as_str()) {
                    let trimmed = s.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
            for nested in map.values() {
                if let Some(found) = json_object_string(nested, keys) {
                    return Some(found);
                }
            }
            None
        }
        serde_json::Value::Array(values) => {
            for nested in values {
                if let Some(found) = json_object_string(nested, keys) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

pub(super) fn parse_timestamp(ts: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(ts)
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S%.f")
                .map(|ndt| ndt.and_utc())
        })
        .with_context(|| format!("Failed to parse timestamp: {}", ts))
}

pub(super) fn load_codex_agent_identity() -> (String, String) {
    let model = read_codex_model_from_config().unwrap_or_else(|| "unknown".to_string());
    let provider = read_codex_provider_from_config()
        .or_else(|| infer_provider_from_model(&model))
        .unwrap_or_else(|| "openai".to_string());
    (provider, model)
}

pub(super) fn codex_config_path() -> Option<PathBuf> {
    if let Ok(codex_home) = std::env::var("CODEX_HOME") {
        let home = codex_home.trim();
        if !home.is_empty() {
            return Some(PathBuf::from(home).join("config.toml"));
        }
    }
    let home = std::env::var("HOME").ok()?;
    let home = home.trim();
    if home.is_empty() {
        return None;
    }
    Some(PathBuf::from(home).join(".codex").join("config.toml"))
}

pub(super) fn read_codex_model_from_config() -> Option<String> {
    read_codex_setting_from_config("model")
}

pub(super) fn read_codex_provider_from_config() -> Option<String> {
    read_codex_setting_from_config("provider")
        .or_else(|| read_codex_setting_from_config("model_provider"))
        .and_then(|provider| {
            let normalized = provider.trim().to_ascii_lowercase();
            if normalized.is_empty() || normalized == "auto" {
                None
            } else {
                Some(normalized)
            }
        })
}

pub(super) fn read_codex_setting_from_config(key: &str) -> Option<String> {
    let path = codex_config_path()?;
    let text = std::fs::read_to_string(path).ok()?;
    parse_codex_config_value(&text, key)
}

pub(super) fn parse_codex_config_value(config_toml: &str, key: &str) -> Option<String> {
    let value: toml::Value = toml::from_str(config_toml).ok()?;
    let active_profile = value
        .get("profile")
        .or_else(|| value.get("default_profile"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(profile) = active_profile {
        if let Some(profile_value) = value
            .get("profiles")
            .and_then(|profiles| profiles.get(profile))
            .and_then(|entry| entry.get(key))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Some(profile_value.to_string());
        }
    }
    if let Some(defaults_value) = value
        .get("defaults")
        .and_then(|defaults| defaults.get(key))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Some(defaults_value.to_string());
    }
    value
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

pub(super) fn infer_provider_from_model(model: &str) -> Option<String> {
    let lower = model.trim().to_ascii_lowercase();
    if lower.is_empty() || lower == "unknown" {
        return None;
    }
    if lower.contains("claude") {
        return Some("anthropic".to_string());
    }
    if lower.contains("gemini") {
        return Some("google".to_string());
    }
    if lower.contains("gpt")
        || lower.contains("openai")
        || lower.contains("codex")
        || lower.starts_with("o1")
        || lower.starts_with("o3")
        || lower.starts_with("o4")
    {
        return Some("openai".to_string());
    }
    None
}

/// Extract a shell command string from function arguments.
/// Handles: `{cmd: "..."}`, `{command: ["bash", "-lc", "cmd"]}`, `{command: "cmd"}`.
pub(super) fn extract_shell_command(args: &serde_json::Value) -> String {
    if let Some(cmd) = args.get("cmd").and_then(|v| v.as_str()) {
        return cmd.to_string();
    }
    if let Some(arr) = args.get("command").and_then(|v| v.as_array()) {
        let parts: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
        // Skip shell prefix (e.g. "bash -lc") and take the actual command
        if parts.len() >= 3 {
            return parts[2..].join(" ");
        }
        return parts.join(" ");
    }
    if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
        return cmd.to_string();
    }
    String::new()
}

pub(super) fn normalize_codex_function_name(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

pub(super) fn extract_patch_target_path(args: &serde_json::Value) -> Option<String> {
    if let Some(path) = args
        .get("path")
        .or_else(|| args.get("file"))
        .or_else(|| args.get("file_path"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return Some(path.to_string());
    }

    for key in ["input", "patch"] {
        if let Some(path) = args
            .get(key)
            .and_then(|v| v.as_str())
            .and_then(extract_patch_target_path_from_text)
        {
            return Some(path);
        }
    }

    None
}

pub(super) fn extract_patch_target_path_from_text(input: &str) -> Option<String> {
    const PREFIXES: [&str; 3] = ["*** Update File:", "*** Add File:", "*** Delete File:"];
    for line in input.lines() {
        let trimmed = line.trim();
        for prefix in PREFIXES {
            if let Some(rest) = trimmed.strip_prefix(prefix) {
                let path = rest.trim().trim_matches('"').trim_matches('\'').trim();
                if !path.is_empty() {
                    return Some(path.to_string());
                }
            }
        }
    }
    None
}

pub(super) fn classify_codex_function(name: &str, args: &serde_json::Value) -> EventType {
    let normalized_name = normalize_codex_function_name(name);
    match normalized_name {
        "exec_command" | "shell" => {
            let cmd = extract_shell_command(args);
            EventType::ShellCommand {
                command: cmd,
                exit_code: None,
            }
        }
        "write_stdin" => EventType::ToolCall {
            name: "write_stdin".to_string(),
        },
        "apply_diff" | "apply_patch" => {
            let path = extract_patch_target_path(args).unwrap_or_else(|| "unknown".to_string());
            EventType::FileEdit { path, diff: None }
        }
        "create_file" | "write_file" => {
            let path = args
                .get("path")
                .or_else(|| args.get("file_path"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            EventType::FileCreate { path }
        }
        "read_file" => {
            let path = args
                .get("path")
                .or_else(|| args.get("file_path"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            EventType::FileRead { path }
        }
        _ => EventType::ToolCall {
            name: canonical_tool_name(normalized_name),
        },
    }
}

pub(super) fn codex_function_content(name: &str, args: &serde_json::Value) -> Content {
    match normalize_codex_function_name(name) {
        "exec_command" | "shell" => {
            let cmd = extract_shell_command(args);
            Content {
                blocks: vec![ContentBlock::Code {
                    code: cmd,
                    language: Some("bash".to_string()),
                    start_line: None,
                }],
            }
        }
        _ => Content {
            blocks: vec![ContentBlock::Json { data: args.clone() }],
        },
    }
}
