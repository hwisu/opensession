#[derive(Debug, Clone, Default)]
pub(super) struct RequestUserInputCallMeta {
    pub(super) questions: Vec<InteractiveQuestionMeta>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct InteractiveQuestionMeta {
    pub(super) id: String,
    pub(super) header: Option<String>,
    pub(super) question: Option<String>,
}

pub(super) fn parse_request_user_input_call_meta(
    args: &serde_json::Value,
) -> RequestUserInputCallMeta {
    let mut questions = Vec::new();
    let Some(items) = args.get("questions").and_then(|v| v.as_array()) else {
        return RequestUserInputCallMeta { questions };
    };

    for item in items {
        let id = item
            .get("id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or("question")
            .to_string();
        let header = item
            .get("header")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string);
        let question = item
            .get("question")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string);
        questions.push(InteractiveQuestionMeta {
            id,
            header,
            question,
        });
    }

    RequestUserInputCallMeta { questions }
}

pub(super) fn render_interactive_questions(questions: &[InteractiveQuestionMeta]) -> String {
    let mut lines = Vec::new();
    for q in questions {
        let mut label = q.id.clone();
        if let Some(header) = q.header.as_deref() {
            label = format!("{label} ({header})");
        }
        let body = q.question.as_deref().unwrap_or("(no question text)");
        lines.push(format!("- {label}: {body}"));
    }
    if lines.is_empty() {
        "(no interactive questions)".to_string()
    } else {
        lines.join("\n")
    }
}

pub(super) fn parse_request_user_input_answers(
    output_text: &str,
) -> Option<(String, Vec<String>, serde_json::Value)> {
    let parsed: serde_json::Value = serde_json::from_str(output_text).ok()?;
    let answers = parsed.get("answers").and_then(|v| v.as_object())?;
    if answers.is_empty() {
        return None;
    }

    let mut question_ids: Vec<String> = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    for (question_id, value) in answers {
        question_ids.push(question_id.clone());
        let mut picks: Vec<String> = Vec::new();
        if let Some(arr) = value.get("answers").and_then(|v| v.as_array()) {
            for answer in arr {
                let rendered = answer
                    .as_str()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .or_else(|| {
                        answer
                            .as_object()
                            .and_then(|obj| obj.get("value").and_then(|v| v.as_str()))
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                    })
                    .unwrap_or_else(|| answer.to_string());
                if !rendered.trim().is_empty() {
                    picks.push(rendered);
                }
            }
        } else if let Some(s) = value.as_str() {
            if !s.trim().is_empty() {
                picks.push(s.trim().to_string());
            }
        } else if !value.is_null() {
            picks.push(value.to_string());
        }
        if picks.is_empty() {
            lines.push(format!("{question_id}: (no answer)"));
        } else {
            lines.push(format!("{question_id}: {}", picks.join(" | ")));
        }
    }

    let rendered = lines.join("\n");
    Some((rendered, question_ids, parsed))
}
