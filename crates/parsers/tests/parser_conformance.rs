use opensession_core::trace::{
    ATTR_SEMANTIC_CALL_ID, ATTR_SOURCE_RAW_TYPE, ATTR_SOURCE_SCHEMA_VERSION, EventType,
};
use opensession_parsers::{ParserRegistry, SessionParser};
use rusqlite::Connection;
use std::path::{Path, PathBuf};

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn parser_for_fixture<'a>(registry: &'a ParserRegistry, path: &Path) -> &'a dyn SessionParser {
    registry
        .parser_for_path(path)
        .unwrap_or_else(|| panic!("parser not found for {}", path.display()))
}

fn stage_fixture(
    temp_root: &Path,
    fixtures: &Path,
    fixture_relative: &str,
    staged_relative: &str,
) -> PathBuf {
    let source = fixtures.join(fixture_relative);
    let staged = temp_root.join(staged_relative);
    std::fs::create_dir_all(
        staged
            .parent()
            .unwrap_or_else(|| panic!("missing parent for {}", staged.display())),
    )
    .unwrap_or_else(|_| panic!("create dir for {}", staged.display()));
    std::fs::copy(&source, &staged)
        .unwrap_or_else(|_| panic!("copy {} to {}", source.display(), staged.display()));
    staged
}

fn build_cursor_fixture_db(fixtures: &Path) -> PathBuf {
    let composer_path = fixtures.join("cursor/composer_data.json");
    let bubbles_path = fixtures.join("cursor/bubbles.json");
    let composer: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&composer_path)
            .unwrap_or_else(|_| panic!("read {}", composer_path.display())),
    )
    .expect("parse composer fixture");
    let bubbles: serde_json::Map<String, serde_json::Value> = serde_json::from_str(
        &std::fs::read_to_string(&bubbles_path)
            .unwrap_or_else(|_| panic!("read {}", bubbles_path.display())),
    )
    .expect("parse bubble fixture");

    let db_path = std::env::temp_dir().join(format!(
        "opensession-cursor-fixture-{}.vscdb",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    let conn = Connection::open(&db_path).expect("create sqlite db");
    conn.execute(
        "CREATE TABLE cursorDiskKV (key TEXT PRIMARY KEY, value TEXT)",
        [],
    )
    .expect("create cursorDiskKV");
    conn.execute(
        "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
        ("composerData:comp-fixture", composer.to_string()),
    )
    .expect("insert composerData");
    for (key, value) in bubbles {
        conn.execute(
            "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
            (key, value.to_string()),
        )
        .expect("insert bubble");
    }
    db_path
}

#[test]
fn parser_conformance_fixtures_cover_five_tools() {
    let fixtures = fixture_root();
    let registry = ParserRegistry::default();
    let staged = tempfile::tempdir().expect("create staged parser fixtures");

    let codex_fixture = stage_fixture(
        staged.path(),
        &fixtures,
        "codex/rollout-desktop.jsonl",
        ".codex/sessions/rollout-desktop.jsonl",
    );
    let codex_session = parser_for_fixture(&registry, &codex_fixture)
        .parse(&codex_fixture)
        .expect("parse codex fixture");
    assert!(
        codex_session
            .events
            .iter()
            .any(|event| matches!(event.event_type, EventType::Thinking))
    );
    assert!(codex_session.events.iter().any(|event| {
        matches!(
            event.event_type,
            EventType::Custom { ref kind } if kind == "token_count"
        )
    }));
    let codex_web_fixture = stage_fixture(
        staged.path(),
        &fixtures,
        "codex/web-search-actions.jsonl",
        ".codex/sessions/web-search-actions.jsonl",
    );
    let codex_web_session = parser_for_fixture(&registry, &codex_web_fixture)
        .parse(&codex_web_fixture)
        .expect("parse codex web fixture");
    assert!(codex_web_session.events.iter().any(|event| {
        matches!(
            &event.event_type,
            EventType::WebSearch { query } if query == "rust serde flatten"
        ) && event
            .attributes
            .get(ATTR_SOURCE_RAW_TYPE)
            .and_then(|value| value.as_str())
            == Some("web_search_call:search")
            && event
                .attributes
                .get(ATTR_SEMANTIC_CALL_ID)
                .and_then(|value| value.as_str())
                == Some("ws_fixture_1")
    }));
    assert!(codex_web_session.events.iter().any(|event| {
        matches!(
            &event.event_type,
            EventType::WebFetch { url } if url == "https://serde.rs/attr-flatten.html"
        ) && event
            .attributes
            .get(ATTR_SOURCE_RAW_TYPE)
            .and_then(|value| value.as_str())
            == Some("web_search_call:open_page")
    }));
    assert!(codex_web_session.events.iter().any(|event| {
        event
            .attributes
            .get(ATTR_SOURCE_RAW_TYPE)
            .and_then(|value| value.as_str())
            == Some("web_search_call:find_in_page")
            && event
                .attributes
                .get("web_search.pattern")
                .and_then(|value| value.as_str())
                == Some("flatten")
    }));
    assert!(codex_web_session.events.iter().any(|event| {
        matches!(
            event.event_type,
            EventType::Custom { ref kind } if kind == "token_count"
        ) && event
            .attributes
            .get("input_tokens")
            .and_then(|value| value.as_u64())
            == Some(19)
            && event
                .attributes
                .get("output_tokens")
                .and_then(|value| value.as_u64())
                == Some(7)
    }));
    assert!(codex_web_session.events.iter().any(|event| {
        matches!(
            event.event_type,
            EventType::Custom { ref kind } if kind == "context_compacted"
        ) && event.task_id.as_deref() == Some("turn_fixture_1")
    }));
    assert!(codex_web_session.events.iter().any(|event| {
        matches!(
            event.event_type,
            EventType::Custom { ref kind } if kind == "plan_completed"
        ) && event
            .attributes
            .get("plan_id")
            .and_then(|value| value.as_str())
            == Some("plan_fixture_1")
    }));

    let claude_fixture = stage_fixture(
        staged.path(),
        &fixtures,
        "claude/session-fallback.jsonl",
        ".claude/projects/demo/session-fallback.jsonl",
    );
    let claude_session = parser_for_fixture(&registry, &claude_fixture)
        .parse(&claude_fixture)
        .expect("parse claude fixture");
    assert!(claude_session.events.iter().any(|event| {
        matches!(
            event.event_type,
            EventType::ToolResult { ref name, .. } if name == "Read"
        )
    }));

    let gemini_fixture = stage_fixture(
        staged.path(),
        &fixtures,
        "gemini/session-parts.json",
        ".gemini/tmp/demo/chats/session-parts.json",
    );
    let gemini_session = parser_for_fixture(&registry, &gemini_fixture)
        .parse(&gemini_fixture)
        .expect("parse gemini fixture");
    assert!(
        gemini_session
            .events
            .iter()
            .any(|event| matches!(event.event_type, EventType::UserMessage))
    );
    assert!(
        gemini_session
            .events
            .iter()
            .any(|event| matches!(event.event_type, EventType::AgentMessage))
    );
    assert!(gemini_session.events.iter().all(|event| {
        event
            .attributes
            .get(ATTR_SOURCE_SCHEMA_VERSION)
            .and_then(|value| value.as_str())
            .is_some()
    }));
    let gemini_tool_fixture = stage_fixture(
        staged.path(),
        &fixtures,
        "gemini/session-toolcalls.json",
        ".gemini/tmp/demo/chats/session-toolcalls.json",
    );
    let gemini_tool_session = parser_for_fixture(&registry, &gemini_tool_fixture)
        .parse(&gemini_tool_fixture)
        .expect("parse gemini toolCalls fixture");
    assert!(gemini_tool_session.events.iter().any(|event| {
        matches!(
            &event.event_type,
            EventType::ToolCall { name } if name == "run_shell_command"
        )
    }));
    assert!(gemini_tool_session.events.iter().any(|event| {
        matches!(
            &event.event_type,
            EventType::ToolResult { name, call_id, .. }
                if name == "run_shell_command" && call_id.as_deref() == Some("call-1")
        )
    }));
    assert!(gemini_tool_session.events.iter().any(|event| {
        event
            .attributes
            .get(ATTR_SOURCE_SCHEMA_VERSION)
            .and_then(|value| value.as_str())
            == Some("gemini-json-v3-toolcalls")
    }));

    let opencode_fixture = fixtures.join("opencode/storage/session/project/ses_fixture.json");
    let opencode_session = parser_for_fixture(&registry, &opencode_fixture)
        .parse(&opencode_fixture)
        .expect("parse opencode fixture");
    assert_eq!(opencode_session.agent.provider, "openai");
    assert_eq!(opencode_session.agent.model, "gpt-5.2-codex");
    assert!(opencode_session.events.iter().any(|event| {
        matches!(event.event_type, EventType::AgentMessage)
            && event
                .attributes
                .get(ATTR_SOURCE_RAW_TYPE)
                .and_then(|v| v.as_str())
                == Some("part:text")
    }));
    let opencode_company_fixture =
        fixtures.join("opencode/storage/session/project/ses_company_logic.json");
    let opencode_company_session = parser_for_fixture(&registry, &opencode_company_fixture)
        .parse(&opencode_company_fixture)
        .expect("parse opencode company fixture");
    assert!(opencode_company_session.events.iter().any(|event| {
        matches!(event.event_type, EventType::Thinking)
            && event
                .attributes
                .get(ATTR_SOURCE_RAW_TYPE)
                .and_then(|v| v.as_str())
                == Some("part:reasoning")
    }));
    assert!(opencode_company_session.events.iter().any(|event| {
        matches!(event.event_type, EventType::UserMessage)
            && event
                .attributes
                .get(ATTR_SOURCE_RAW_TYPE)
                .and_then(|v| v.as_str())
                == Some("part:file")
    }));
    assert!(opencode_company_session.events.iter().any(|event| {
        matches!(
            &event.event_type,
            EventType::ToolResult { name, call_id, .. }
                if name == "grep" && call_id.as_deref() == Some("call_abc")
        )
    }));
    assert!(opencode_company_session.events.iter().any(|event| {
        matches!(&event.event_type, EventType::FileEdit { .. })
            && event
                .attributes
                .get(ATTR_SEMANTIC_CALL_ID)
                .and_then(|v| v.as_str())
                == Some("functions.edit:27")
    }));
    assert!(opencode_company_session.events.iter().any(|event| {
        matches!(
            &event.event_type,
            EventType::FileEdit { path, .. } if path == "/tmp/opencode-company/lib.rs"
        ) && event
            .attributes
            .get(ATTR_SOURCE_RAW_TYPE)
            .and_then(|v| v.as_str())
            == Some("part:patch:file")
    }));
    assert!(opencode_company_session.events.iter().any(|event| {
        matches!(event.event_type, EventType::TaskEnd { .. })
            && event.task_id.as_deref() == Some("functions.edit:27")
            && event
                .attributes
                .get(ATTR_SOURCE_RAW_TYPE)
                .and_then(|v| v.as_str())
                == Some("synthetic:task-end")
    }));

    let cursor_db = build_cursor_fixture_db(&fixtures);
    let cursor_session = parser_for_fixture(&registry, &cursor_db)
        .parse(&cursor_db)
        .expect("parse cursor fixture db");
    assert!(
        cursor_session
            .events
            .iter()
            .any(|event| matches!(event.event_type, EventType::TaskStart { .. }))
    );
    assert!(cursor_session.events.iter().any(|event| {
        matches!(
            event.event_type,
            EventType::ToolResult { ref name, .. } if name == "run_terminal_cmd"
        )
    }));
}
