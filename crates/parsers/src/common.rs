//! Shared content transformation helpers used by multiple parsers.
//!
//! These handle the "heavy lifting" of transforming raw tool output
//! into clean, typed ContentBlocks so the frontend can be a dumb renderer.

use opensession_core::trace::{
    Content, ContentBlock, ATTR_SEMANTIC_CALL_ID, ATTR_SEMANTIC_GROUP_ID, ATTR_SEMANTIC_TOOL_KIND,
    ATTR_SOURCE_RAW_TYPE, ATTR_SOURCE_SCHEMA_VERSION,
};
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::LazyLock;

// ── First-wins metadata helper ──────────────────────────────────────────────

/// Assign `source` to `target` if `target` is still `None` (first-wins semantics).
///
/// Used by parsers to collect metadata from the first occurrence in a session,
/// replacing the repeated `if x.is_none() { x = val; }` pattern.
pub fn set_first<T>(target: &mut Option<T>, source: Option<T>) {
    if target.is_none() {
        *target = source;
    }
}

// ── Shared semantic metadata helpers ────────────────────────────────────────

/// Normalize cross-tool role labels into a canonical role string.
///
/// Output values are intentionally stringly-typed for lightweight reuse
/// across parser modules without introducing new enums to public APIs.
pub fn normalize_role_label(role: &str) -> Option<&'static str> {
    match role.trim().to_ascii_lowercase().as_str() {
        "user" | "human" => Some("user"),
        "assistant" | "agent" | "model" | "gemini" => Some("assistant"),
        "system" => Some("system"),
        "thinking" | "reasoning" | "thought" => Some("thinking"),
        _ => None,
    }
}

/// Infer a semantic tool kind from a raw tool name.
pub fn infer_tool_kind(name: &str) -> &'static str {
    let lower = name.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return "other";
    }
    if matches!(
        lower.as_str(),
        "read"
            | "read_file"
            | "view"
            | "cat"
            | "open"
            | "fileread"
            | "readfile"
            | "list_dir"
            | "ls"
    ) {
        return "file_read";
    }
    if matches!(
        lower.as_str(),
        "edit"
            | "write"
            | "create"
            | "delete"
            | "apply_patch"
            | "str_replace_editor"
            | "edit_file"
            | "reapply"
            | "write_file"
            | "fileedit"
    ) {
        return "file_write";
    }
    if matches!(
        lower.as_str(),
        "bash" | "shell" | "exec_command" | "run_terminal_cmd" | "execute_command"
    ) {
        return "shell";
    }
    if matches!(
        lower.as_str(),
        "grep" | "search" | "code_search" | "grep_search" | "file_search" | "glob" | "find"
    ) {
        return "search";
    }
    if lower.starts_with("web") || matches!(lower.as_str(), "fetch" | "browser") {
        return "web";
    }
    if lower.contains("task") || lower.contains("subagent") {
        return "task";
    }
    "other"
}

/// Add non-breaking source metadata attributes to an event.
pub fn attach_source_attrs(
    attrs: &mut HashMap<String, Value>,
    schema_version: Option<&str>,
    raw_type: Option<&str>,
) {
    if let Some(version) = schema_version.map(str::trim).filter(|v| !v.is_empty()) {
        attrs.insert(
            ATTR_SOURCE_SCHEMA_VERSION.to_string(),
            Value::String(version.to_string()),
        );
    }
    if let Some(raw) = raw_type.map(str::trim).filter(|v| !v.is_empty()) {
        attrs.insert(
            ATTR_SOURCE_RAW_TYPE.to_string(),
            Value::String(raw.to_string()),
        );
    }
}

/// Add non-breaking semantic metadata attributes to an event.
pub fn attach_semantic_attrs(
    attrs: &mut HashMap<String, Value>,
    group_id: Option<&str>,
    call_id: Option<&str>,
    tool_kind: Option<&str>,
) {
    if let Some(group_id) = group_id.map(str::trim).filter(|v| !v.is_empty()) {
        attrs.insert(
            ATTR_SEMANTIC_GROUP_ID.to_string(),
            Value::String(group_id.to_string()),
        );
    }
    if let Some(call_id) = call_id.map(str::trim).filter(|v| !v.is_empty()) {
        attrs.insert(
            ATTR_SEMANTIC_CALL_ID.to_string(),
            Value::String(call_id.to_string()),
        );
    }
    if let Some(tool_kind) = tool_kind.map(str::trim).filter(|v| !v.is_empty()) {
        attrs.insert(
            ATTR_SEMANTIC_TOOL_KIND.to_string(),
            Value::String(tool_kind.to_string()),
        );
    }
}

// ── System reminder stripping ───────────────────────────────────────────────

static SYSTEM_REMINDER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<system-reminder>.*?</system-reminder>").unwrap());

/// Strip <system-reminder> blocks from text
pub fn strip_system_reminders(text: &str) -> String {
    SYSTEM_REMINDER_RE.replace_all(text, "").trim().to_string()
}

// ── Line-number detection (cat -n output and NNNNN| format) ─────────────────

/// Matches line number prefixes:  `  1→code` or `00001| code`
static LINE_NUM_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^ *\d+[→|]").unwrap());

/// Captures: (line_number, rest_of_line)
/// `→` has no trailing space; `|` consumes optional trailing space
static LINE_NUM_CAPTURE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^ *(\d+)(?:→|\| ?)(.*)$").unwrap());

/// Detect if text looks like line-numbered file content
/// (cat -n output with `→` separator, or `00001|` format)
pub fn is_line_numbered_output(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().take(5).collect();
    if lines.is_empty() {
        return false;
    }
    let match_count = lines
        .iter()
        .filter(|l| LINE_NUM_RE.is_match(l) || l.trim().is_empty())
        .count();
    match_count as f64 >= lines.len() as f64 * 0.6
}

/// Parse line-numbered output: strip line number prefixes, return clean code and start line
pub fn parse_line_numbered_output(text: &str) -> (String, u32) {
    let mut start_line = 1u32;
    let mut code_lines: Vec<&str> = Vec::new();

    for line in text.lines() {
        if let Some(caps) = LINE_NUM_CAPTURE_RE.captures(line) {
            if code_lines.is_empty() {
                start_line = caps[1].parse().unwrap_or(1);
            }
            code_lines.push(caps.get(2).map_or("", |m| m.as_str()));
        } else if line.trim().is_empty() {
            code_lines.push("");
        }
    }

    let code = code_lines.join("\n");
    let code = code.trim_end().to_string();
    (code, start_line)
}

// ── Language detection ──────────────────────────────────────────────────────

/// Detect programming language from file path extension
pub fn detect_language(file_path: &str) -> Option<String> {
    let basename = file_path.rsplit('/').next().unwrap_or(file_path);

    // Special filenames
    match basename {
        "Dockerfile" | "Makefile" => return Some("bash".to_string()),
        "Cargo.toml" | "pyproject.toml" => return Some("toml".to_string()),
        _ => {}
    }

    let ext = basename.rsplit('.').next()?.to_lowercase();
    let lang = match ext.as_str() {
        "ts" | "tsx" => "typescript",
        "js" | "jsx" => "javascript",
        "py" => "python",
        "rs" => "rust",
        "go" => "go",
        "java" => "java",
        "kt" => "kotlin",
        "swift" => "swift",
        "rb" => "ruby",
        "cpp" | "c" | "h" | "hpp" => "cpp",
        "cs" => "csharp",
        "css" | "scss" => "css",
        "html" | "svelte" | "vue" => "html",
        "xml" => "xml",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "md" => "markdown",
        "sql" => "sql",
        "sh" | "bash" | "zsh" => "bash",
        "diff" => "diff",
        "gradle" | "kts" => "kotlin",
        "properties" => "properties",
        _ => return None,
    };
    Some(lang.to_string())
}

// ── XML/HTML tag extraction ─────────────────────────────────────────────────

/// Extract content between XML-like tags: `<tag>content</tag>`
pub fn extract_tag_content(text: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = text.find(&open)?;
    let end = text.find(&close)?;
    if start + open.len() > end {
        return None;
    }
    let content = &text[start + open.len()..end];
    let trimmed = content.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

// ── ToolResult content building ─────────────────────────────────────────────

/// Metadata about a tool_use for matching with its ToolResult
#[derive(Debug, Clone)]
pub struct ToolUseInfo {
    pub name: String,
    pub file_path: Option<String>,
}

/// Build structured Content for a ToolResult from raw text.
/// Detects line-numbered file content → Code blocks with language and start_line.
pub fn build_tool_result_content(raw_text: &str, tool_info: &ToolUseInfo) -> Content {
    if raw_text.is_empty() {
        return Content::empty();
    }

    // Always strip system reminders
    let cleaned = strip_system_reminders(raw_text);
    if cleaned.trim().is_empty() {
        return Content::empty();
    }

    // Read tool results: detect line-numbered output → Code block with language + start_line
    if matches!(
        tool_info.name.as_str(),
        "Read" | "read_file" | "read" | "view"
    ) && is_line_numbered_output(&cleaned)
    {
        let (code, start_line) = parse_line_numbered_output(&cleaned);
        let language = tool_info.file_path.as_deref().and_then(detect_language);
        return Content {
            blocks: vec![ContentBlock::Code {
                code,
                language,
                start_line: Some(start_line),
            }],
        };
    }

    // All other results: cleaned text
    Content::text(cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_strip_system_reminders() {
        let input = "hello\n<system-reminder>\nsome reminder\n</system-reminder>\nworld";
        let result = strip_system_reminders(input);
        assert_eq!(result, "hello\n\nworld");
    }

    #[test]
    fn test_strip_system_reminders_multiple() {
        let input =
            "<system-reminder>first</system-reminder>text<system-reminder>second</system-reminder>";
        let result = strip_system_reminders(input);
        assert_eq!(result, "text");
    }

    #[test]
    fn test_is_line_numbered_cat_n() {
        let text =
            "     1→use std::io;\n     2→\n     3→fn main() {\n     4→    println!(\"hello\");\n     5→}";
        assert!(is_line_numbered_output(text));
    }

    #[test]
    fn test_is_line_numbered_pipe_format() {
        let text =
            "00001| /* Import CSS modules */\n00002| @import 'reset.css';\n00003| \n00004| body {";
        assert!(is_line_numbered_output(text));
    }

    #[test]
    fn test_is_line_numbered_not() {
        let text = "This is just regular text\nwith no line numbers";
        assert!(!is_line_numbered_output(text));
    }

    #[test]
    fn test_parse_line_numbered_cat_n() {
        let text = "     1→use std::io;\n     2→\n     3→fn main() {}";
        let (code, start_line) = parse_line_numbered_output(text);
        assert_eq!(start_line, 1);
        assert_eq!(code, "use std::io;\n\nfn main() {}");
    }

    #[test]
    fn test_parse_line_numbered_pipe_format() {
        let text = "00001| /* CSS */\n00002| body {\n00003|   color: red;\n00004| }";
        let (code, start_line) = parse_line_numbered_output(text);
        assert_eq!(start_line, 1);
        assert_eq!(code, "/* CSS */\nbody {\n  color: red;\n}");
    }

    #[test]
    fn test_parse_line_numbered_offset() {
        let text = "    10→    let x = 1;\n    11→    let y = 2;";
        let (code, start_line) = parse_line_numbered_output(text);
        assert_eq!(start_line, 10);
        assert_eq!(code, "    let x = 1;\n    let y = 2;");
    }

    #[test]
    fn test_detect_language() {
        assert_eq!(detect_language("/foo/bar.rs"), Some("rust".to_string()));
        assert_eq!(
            detect_language("/foo/bar.ts"),
            Some("typescript".to_string())
        );
        assert_eq!(detect_language("/foo/bar.py"), Some("python".to_string()));
        assert_eq!(detect_language("Dockerfile"), Some("bash".to_string()));
        assert_eq!(detect_language("/foo/bar.kt"), Some("kotlin".to_string()));
        assert_eq!(detect_language("/foo/bar.xyz"), None);
    }

    #[test]
    fn test_extract_tag_content() {
        assert_eq!(
            extract_tag_content("<task>\nhello world\n</task>", "task"),
            Some("hello world".to_string())
        );
        assert_eq!(
            extract_tag_content("<user_message>hi</user_message>", "user_message"),
            Some("hi".to_string())
        );
        assert_eq!(extract_tag_content("<task></task>", "task"), None);
        assert_eq!(extract_tag_content("no tags here", "task"), None);
    }

    #[test]
    fn test_build_tool_result_content_read_cat_n() {
        let info = ToolUseInfo {
            name: "Read".to_string(),
            file_path: Some("/tmp/test.rs".to_string()),
        };
        let content = build_tool_result_content("     1→use std::io;\n     2→fn main() {}", &info);
        assert_eq!(content.blocks.len(), 1);
        match &content.blocks[0] {
            ContentBlock::Code {
                code,
                language,
                start_line,
            } => {
                assert_eq!(code, "use std::io;\nfn main() {}");
                assert_eq!(language.as_deref(), Some("rust"));
                assert_eq!(*start_line, Some(1));
            }
            _ => panic!("Expected Code block"),
        }
    }

    #[test]
    fn test_build_tool_result_content_read_pipe_format() {
        let info = ToolUseInfo {
            name: "read_file".to_string(),
            file_path: Some("/tmp/style.css".to_string()),
        };
        let content = build_tool_result_content(
            "00001| /* CSS */\n00002| body {\n00003|   color: red;\n00004| }",
            &info,
        );
        assert_eq!(content.blocks.len(), 1);
        match &content.blocks[0] {
            ContentBlock::Code {
                code,
                language,
                start_line,
            } => {
                assert_eq!(code, "/* CSS */\nbody {\n  color: red;\n}");
                assert_eq!(language.as_deref(), Some("css"));
                assert_eq!(*start_line, Some(1));
            }
            _ => panic!("Expected Code block"),
        }
    }

    #[test]
    fn test_build_tool_result_content_with_reminders() {
        let info = ToolUseInfo {
            name: "Read".to_string(),
            file_path: Some("/tmp/test.txt".to_string()),
        };
        let content = build_tool_result_content(
            "     1→hello\n<system-reminder>ignore me</system-reminder>\n     2→world",
            &info,
        );
        assert_eq!(content.blocks.len(), 1);
        match &content.blocks[0] {
            ContentBlock::Code {
                code, start_line, ..
            } => {
                assert_eq!(code, "hello\n\nworld");
                assert_eq!(*start_line, Some(1));
                assert!(!code.contains("system-reminder"));
            }
            _ => panic!("Expected Code block"),
        }
    }

    #[test]
    fn test_build_tool_result_content_non_read() {
        let info = ToolUseInfo {
            name: "Bash".to_string(),
            file_path: None,
        };
        let content =
            build_tool_result_content("some output<system-reminder>r</system-reminder>", &info);
        assert_eq!(content.blocks.len(), 1);
        match &content.blocks[0] {
            ContentBlock::Text { text } => {
                assert_eq!(text, "some output");
            }
            _ => panic!("Expected Text block"),
        }
    }

    #[test]
    fn test_normalize_role_label() {
        assert_eq!(normalize_role_label("user"), Some("user"));
        assert_eq!(normalize_role_label("assistant"), Some("assistant"));
        assert_eq!(normalize_role_label("gemini"), Some("assistant"));
        assert_eq!(normalize_role_label("system"), Some("system"));
        assert_eq!(normalize_role_label("reasoning"), Some("thinking"));
        assert_eq!(normalize_role_label("unknown"), None);
    }

    #[test]
    fn test_infer_tool_kind() {
        assert_eq!(infer_tool_kind("Read"), "file_read");
        assert_eq!(infer_tool_kind("edit_file"), "file_write");
        assert_eq!(infer_tool_kind("exec_command"), "shell");
        assert_eq!(infer_tool_kind("WebSearch"), "web");
        assert_eq!(infer_tool_kind("Task"), "task");
        assert_eq!(infer_tool_kind("custom_tool"), "other");
    }

    #[test]
    fn test_attach_source_and_semantic_attrs() {
        let mut attrs = HashMap::new();
        attach_source_attrs(&mut attrs, Some("v3"), Some("bubble"));
        attach_semantic_attrs(&mut attrs, Some("turn-1"), Some("call-1"), Some("shell"));

        assert_eq!(
            attrs
                .get(ATTR_SOURCE_SCHEMA_VERSION)
                .and_then(|v| v.as_str()),
            Some("v3")
        );
        assert_eq!(
            attrs.get(ATTR_SOURCE_RAW_TYPE).and_then(|v| v.as_str()),
            Some("bubble")
        );
        assert_eq!(
            attrs.get(ATTR_SEMANTIC_GROUP_ID).and_then(|v| v.as_str()),
            Some("turn-1")
        );
        assert_eq!(
            attrs.get(ATTR_SEMANTIC_CALL_ID).and_then(|v| v.as_str()),
            Some("call-1")
        );
        assert_eq!(
            attrs.get(ATTR_SEMANTIC_TOOL_KIND).and_then(|v| v.as_str()),
            Some("shell")
        );
    }
}
