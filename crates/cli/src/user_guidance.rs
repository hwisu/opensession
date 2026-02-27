use anyhow::anyhow;

pub fn guided_error<S, I, T>(reason: S, next_steps: I) -> anyhow::Error
where
    S: Into<String>,
    I: IntoIterator<Item = T>,
    T: Into<String>,
{
    guided_error_impl(reason.into(), next_steps, None)
}

pub fn guided_error_with_doc<S, I, T, D>(reason: S, next_steps: I, doc_ref: D) -> anyhow::Error
where
    S: Into<String>,
    I: IntoIterator<Item = T>,
    T: Into<String>,
    D: Into<String>,
{
    guided_error_impl(reason.into(), next_steps, Some(doc_ref.into()))
}

fn guided_error_impl<I, T>(reason: String, next_steps: I, doc_ref: Option<String>) -> anyhow::Error
where
    I: IntoIterator<Item = T>,
    T: Into<String>,
{
    let mut normalized_steps = next_steps
        .into_iter()
        .map(Into::into)
        .map(|step| step.trim().to_string())
        .filter(|step| !step.is_empty())
        .collect::<Vec<_>>();
    if normalized_steps.is_empty() {
        normalized_steps.push("run `opensession --help`".to_string());
    }

    let mut body = reason.trim().to_string();
    body.push_str("\nnext:");
    for step in normalized_steps {
        body.push_str("\n- ");
        body.push_str(&step);
    }

    if let Some(doc_ref) = doc_ref
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        body.push_str("\ndocs: ");
        body.push_str(&doc_ref);
    }

    anyhow!(body)
}

#[cfg(test)]
mod tests {
    use super::{guided_error, guided_error_with_doc};

    #[test]
    fn guided_error_renders_reason_and_next_steps() {
        let err = guided_error(
            "share --web requires a remote source uri",
            [
                "run `opensession share <uri> --git --remote origin`",
                "run `opensession share <remote_uri> --web`",
            ],
        );
        let msg = err.to_string();
        assert!(msg.contains("share --web requires a remote source uri"));
        assert!(msg.contains("next:"));
        assert!(msg.contains("- run `opensession share <uri> --git --remote origin`"));
        assert!(msg.contains("- run `opensession share <remote_uri> --web`"));
    }

    #[test]
    fn guided_error_with_doc_appends_docs_reference() {
        let err = guided_error_with_doc(
            "missing config",
            ["run `opensession config init`"],
            "README.md#share",
        );
        let msg = err.to_string();
        assert!(msg.contains("next:"));
        assert!(msg.contains("docs: README.md#share"));
    }

    #[test]
    fn guided_error_injects_default_step_when_empty() {
        let err = guided_error("unexpected failure", std::iter::empty::<String>());
        let msg = err.to_string();
        assert!(msg.contains("next:"));
        assert!(msg.contains("- run `opensession --help`"));
    }
}
