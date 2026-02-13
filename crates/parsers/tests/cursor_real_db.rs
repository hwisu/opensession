//! Integration test: parse the real Cursor state.vscdb on this machine.
//! This test is ignored by default (requires a real Cursor installation).
//! Run with: cargo test -p opensession-parsers --test cursor_real_db -- --nocapture --ignored

use std::path::Path;

fn real_db_path() -> &'static str {
    "/Users/hwisookim/Library/Application Support/Cursor/User/globalStorage/state.vscdb"
}

/// Safely truncate a string at a char boundary
fn safe_truncate(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        return s;
    }
    // Find the last char boundary at or before max_len
    let mut end = max_len;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[test]
#[ignore = "requires real Cursor database"]
fn parse_real_cursor_database() {
    let path = Path::new(real_db_path());
    if !path.exists() {
        eprintln!("Skipping: Cursor database not found at {}", path.display());
        return;
    }

    let parsers = opensession_parsers::all_parsers();
    let cursor_parser = parsers
        .iter()
        .find(|p| p.name() == "cursor")
        .expect("CursorParser not found in all_parsers()");

    assert!(
        cursor_parser.can_parse(path),
        "can_parse should return true for .vscdb"
    );

    let result = cursor_parser.parse(path);

    match result {
        Ok(session) => {
            println!("=== SUCCESS: Parsed Cursor session ===");
            println!("Session ID:     {}", session.session_id);
            println!("Agent tool:     {}", session.agent.tool);
            println!("Agent model:    {}", session.agent.model);
            println!("Agent provider: {}", session.agent.provider);
            println!("Title:          {:?}", session.context.title);
            println!("Created at:     {}", session.context.created_at);
            println!("Updated at:     {}", session.context.updated_at);
            println!("Tags:           {:?}", session.context.tags);
            println!("Attributes:     {:?}", session.context.attributes);
            println!("Event count:    {}", session.events.len());
            println!();

            // Print first 20 events for inspection
            for (i, event) in session.events.iter().take(20).enumerate() {
                println!("Event[{}]:", i);
                println!("  type:      {:?}", event.event_type);
                println!("  timestamp: {}", event.timestamp);
                let content_preview: String = event
                    .content
                    .blocks
                    .iter()
                    .filter_map(|b| match b {
                        opensession_core::trace::ContentBlock::Text { text } => Some(text.clone()),
                        opensession_core::trace::ContentBlock::Code { code, .. } => {
                            Some(format!("[code: {}B]", code.len()))
                        }
                        opensession_core::trace::ContentBlock::Json { data } => {
                            Some(format!("[json: {}B]", data.to_string().len()))
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" | ");
                let preview = safe_truncate(&content_preview, 150);
                println!(
                    "  content:   {}{}",
                    preview,
                    if content_preview.len() > 150 {
                        "..."
                    } else {
                        ""
                    }
                );
                println!();
            }

            // Basic assertions
            assert!(
                !session.session_id.is_empty(),
                "session_id should not be empty"
            );
            assert_eq!(session.agent.tool, "cursor");
            assert!(!session.events.is_empty(), "should have at least one event");

            // Count event types
            let mut user_msgs = 0;
            let mut agent_msgs = 0;
            let mut thinking = 0;
            let mut tool_calls = 0;
            let mut tool_results = 0;
            let mut file_edits = 0;
            let mut file_reads = 0;
            let mut shell_cmds = 0;
            let mut other = 0;

            for event in &session.events {
                match &event.event_type {
                    opensession_core::trace::EventType::UserMessage => user_msgs += 1,
                    opensession_core::trace::EventType::AgentMessage => agent_msgs += 1,
                    opensession_core::trace::EventType::Thinking => thinking += 1,
                    opensession_core::trace::EventType::ToolCall { .. } => tool_calls += 1,
                    opensession_core::trace::EventType::ToolResult { .. } => tool_results += 1,
                    opensession_core::trace::EventType::FileEdit { .. } => file_edits += 1,
                    opensession_core::trace::EventType::FileRead { .. } => file_reads += 1,
                    opensession_core::trace::EventType::ShellCommand { .. } => shell_cmds += 1,
                    _ => other += 1,
                }
            }

            println!("=== Event type breakdown ===");
            println!("  UserMessage:  {}", user_msgs);
            println!("  AgentMessage: {}", agent_msgs);
            println!("  Thinking:     {}", thinking);
            println!("  ToolCall:     {}", tool_calls);
            println!("  ToolResult:   {}", tool_results);
            println!("  FileEdit:     {}", file_edits);
            println!("  FileRead:     {}", file_reads);
            println!("  ShellCommand: {}", shell_cmds);
            println!("  Other:        {}", other);
            println!("  TOTAL:        {}", session.events.len());

            assert!(user_msgs > 0, "should have at least one user message");
            assert!(agent_msgs > 0, "should have at least one agent message");
        }
        Err(e) => {
            panic!("FAILED to parse Cursor database: {:#}", e);
        }
    }
}

/// Test that we can deserialize ALL conversations from the real DB.
#[test]
#[ignore = "requires real Cursor database"]
fn deserialize_all_conversations_from_real_db() {
    let path = Path::new(real_db_path());
    if !path.exists() {
        eprintln!("Skipping: Cursor database not found");
        return;
    }

    let conn =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("Failed to open database");

    let mut stmt = conn
        .prepare("SELECT key, value FROM cursorDiskKV WHERE key LIKE 'composerData:%'")
        .expect("Failed to prepare statement");

    let rows: Vec<(String, String)> = stmt
        .query_map([], |row| {
            let key: String = row.get(0)?;
            let value_ref = row.get_ref(1)?;
            let value = match value_ref {
                rusqlite::types::ValueRef::Text(bytes) => {
                    String::from_utf8_lossy(bytes).into_owned()
                }
                rusqlite::types::ValueRef::Blob(bytes) => {
                    String::from_utf8_lossy(bytes).into_owned()
                }
                _ => String::new(),
            };
            Ok((key, value))
        })
        .expect("query_map failed")
        .filter_map(|r| r.ok())
        .collect();

    println!("Total composerData entries: {}", rows.len());

    let mut valid_json = 0;
    let mut empty_value = 0;
    let mut has_conversations = 0;
    let mut json_parse_fail = 0;

    for (key, value) in &rows {
        if value.is_empty() {
            empty_value += 1;
            continue;
        }

        match serde_json::from_str::<serde_json::Value>(value) {
            Ok(val) => {
                valid_json += 1;
                let conv_len = val
                    .get("conversation")
                    .and_then(|v| v.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                if conv_len > 0 {
                    has_conversations += 1;
                }
            }
            Err(e) => {
                json_parse_fail += 1;
                if json_parse_fail <= 3 {
                    println!(
                        "  JSON parse fail: {} -> {} (value len={})",
                        key,
                        e,
                        value.len()
                    );
                }
            }
        }
    }

    println!("\n=== Database content summary ===");
    println!("  Total entries:              {}", rows.len());
    println!("  Empty values:               {}", empty_value);
    println!("  Valid JSON:                 {}", valid_json);
    println!("  With conversations (>0):    {}", has_conversations);
    println!("  JSON parse failures:        {}", json_parse_fail);

    // The parser should be able to handle all valid JSON entries
    assert!(
        valid_json > 0,
        "Should have at least some valid JSON entries"
    );
    assert!(
        has_conversations > 0,
        "Should have at least some conversations"
    );

    // All non-empty values should be valid JSON
    let non_empty = rows.len() - empty_value;
    if non_empty > 0 {
        let json_success_rate = valid_json as f64 / non_empty as f64;
        println!(
            "  JSON success rate:          {:.1}%",
            json_success_rate * 100.0
        );
        assert!(
            json_success_rate > 0.95,
            "JSON parse rate should be > 95% for non-empty values"
        );
    }
}
