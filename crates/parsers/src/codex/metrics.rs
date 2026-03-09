pub(super) fn extract_token_counts(
    payload: &serde_json::Value,
) -> Option<(Option<u64>, Option<u64>)> {
    let pick = |v: &serde_json::Value, keys: &[&str]| -> Option<u64> {
        for key in keys {
            if let Some(num) = v.get(*key).and_then(|value| value.as_u64()) {
                return Some(num);
            }
            if let Some(num) = v
                .get(*key)
                .and_then(|value| value.as_i64())
                .filter(|value| *value >= 0)
                .map(|value| value as u64)
            {
                return Some(num);
            }
        }
        None
    };

    let usage_pick_input = |value: &serde_json::Value| {
        pick(
            value,
            &[
                "input_tokens",
                "prompt_tokens",
                "inputTokens",
                "promptTokens",
                "token_input",
                "tokenInput",
            ],
        )
    };
    let usage_pick_output = |value: &serde_json::Value| {
        pick(
            value,
            &[
                "output_tokens",
                "completion_tokens",
                "outputTokens",
                "completionTokens",
                "token_output",
                "tokenOutput",
            ],
        )
    };
    fn info_usage<'a>(
        info: &'a serde_json::Value,
        snake_case_key: &str,
        camel_case_key: &str,
    ) -> Option<&'a serde_json::Value> {
        if let Some(value) = info.get(snake_case_key) {
            Some(value)
        } else {
            info.get(camel_case_key)
        }
    }

    let input = pick(
        payload,
        &[
            "input_tokens",
            "prompt_tokens",
            "inputTokens",
            "promptTokens",
            "token_input",
            "tokenInput",
        ],
    )
    .or_else(|| payload.get("usage").and_then(usage_pick_input))
    .or_else(|| {
        payload
            .get("info")
            .and_then(|info| info_usage(info, "last_token_usage", "lastTokenUsage"))
            .and_then(usage_pick_input)
    })
    .or_else(|| {
        payload
            .get("info")
            .and_then(|info| info_usage(info, "total_token_usage", "totalTokenUsage"))
            .and_then(usage_pick_input)
    });
    let output = pick(
        payload,
        &[
            "output_tokens",
            "completion_tokens",
            "outputTokens",
            "completionTokens",
            "token_output",
            "tokenOutput",
        ],
    )
    .or_else(|| payload.get("usage").and_then(usage_pick_output))
    .or_else(|| {
        payload
            .get("info")
            .and_then(|info| info_usage(info, "last_token_usage", "lastTokenUsage"))
            .and_then(usage_pick_output)
    })
    .or_else(|| {
        payload
            .get("info")
            .and_then(|info| info_usage(info, "total_token_usage", "totalTokenUsage"))
            .and_then(usage_pick_output)
    });
    if input.is_none() && output.is_none() {
        None
    } else {
        Some((input, output))
    }
}

pub(super) fn extract_total_token_counts(
    payload: &serde_json::Value,
) -> Option<(Option<u64>, Option<u64>)> {
    let pick = |v: &serde_json::Value, keys: &[&str]| -> Option<u64> {
        for key in keys {
            if let Some(num) = v.get(*key).and_then(|value| value.as_u64()) {
                return Some(num);
            }
            if let Some(num) = v
                .get(*key)
                .and_then(|value| value.as_i64())
                .filter(|value| *value >= 0)
                .map(|value| value as u64)
            {
                return Some(num);
            }
        }
        None
    };

    let total_usage = payload.get("info").and_then(|info| {
        info.get("total_token_usage")
            .or_else(|| info.get("totalTokenUsage"))
    })?;

    let input = pick(
        total_usage,
        &[
            "input_tokens",
            "prompt_tokens",
            "inputTokens",
            "promptTokens",
            "token_input",
            "tokenInput",
        ],
    );
    let output = pick(
        total_usage,
        &[
            "output_tokens",
            "completion_tokens",
            "outputTokens",
            "completionTokens",
            "token_output",
            "tokenOutput",
        ],
    );

    if input.is_none() && output.is_none() {
        None
    } else {
        Some((input, output))
    }
}
