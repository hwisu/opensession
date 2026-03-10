use opensession_runtime_config::{SummaryOutputShape, SummaryResponseStyle, SummarySettings};
use serde::{Deserialize, Serialize};

const DEFAULT_SUMMARY_CHAR_LIMIT: usize = 560;
const DEFAULT_AUTH_SECURITY_CHAR_LIMIT: usize = 320;
const DEFAULT_LAYER_SUMMARY_CHAR_LIMIT: usize = 260;
const DEFAULT_MAX_LAYER_ITEMS: usize = 10;
const DEFAULT_MAX_FILES_PER_LAYER: usize = 14;

#[derive(Debug, Clone, Copy)]
struct SummaryNormalizationLimits {
    summary_chars: usize,
    auth_security_chars: usize,
    layer_summary_chars: usize,
    max_layer_items: usize,
    max_files_per_layer: usize,
}

fn summary_limits(settings: &SummarySettings) -> SummaryNormalizationLimits {
    let (summary_chars, auth_security_chars, layer_summary_chars) = match settings.response.style {
        SummaryResponseStyle::Compact => (280, 160, 120),
        SummaryResponseStyle::Standard => (
            DEFAULT_SUMMARY_CHAR_LIMIT,
            DEFAULT_AUTH_SECURITY_CHAR_LIMIT,
            DEFAULT_LAYER_SUMMARY_CHAR_LIMIT,
        ),
        SummaryResponseStyle::Detailed => (960, 520, 360),
    };
    let (max_layer_items, max_files_per_layer) = match settings.response.shape {
        SummaryOutputShape::Layered => (DEFAULT_MAX_LAYER_ITEMS, DEFAULT_MAX_FILES_PER_LAYER),
        SummaryOutputShape::FileList => (16, 20),
        SummaryOutputShape::SecurityFirst => (12, 14),
    };
    SummaryNormalizationLimits {
        summary_chars,
        auth_security_chars,
        layer_summary_chars,
        max_layer_items,
        max_files_per_layer,
    }
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

pub fn parse_semantic_summary_or_fallback(
    raw: &str,
    settings: &SummarySettings,
) -> SemanticSummary {
    let limits = summary_limits(settings);
    match parse_semantic_summary(raw) {
        Ok(summary) => summary.normalize(limits),
        Err(_) => SemanticSummary::from_plain_fallback(raw, limits),
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
        SemanticSummary, normalize_summary_text, parse_semantic_summary,
        parse_semantic_summary_or_fallback,
    };
    use opensession_runtime_config::{SummaryOutputShape, SummaryResponseStyle, SummarySettings};

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
        settings.response.style = SummaryResponseStyle::Compact;
        let parsed = parse_semantic_summary_or_fallback(&"x".repeat(400), &settings);
        assert!(parsed.changes.chars().count() <= 280);
    }

    #[test]
    fn parse_semantic_summary_fallback_applies_file_list_shape_limits() {
        let mut settings = SummarySettings::default();
        settings.response.shape = SummaryOutputShape::FileList;
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
