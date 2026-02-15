/// Integration test: parse a real Claude Code team session with subagents.
/// Run with: cargo test -p opensession-parsers --test claude_code_team -- --ignored --nocapture
use opensession_parsers::claude_code::ClaudeCodeParser;
use opensession_parsers::SessionParser;
use std::path::Path;

#[test]
#[ignore] // requires real session file
fn test_parse_team_session_with_subagents() {
    let parser = ClaudeCodeParser;
    let path = Path::new("/Users/hwisookim/.claude/projects/-Users-hwisookim-opensession/b7655a2e-2241-46cb-8f31-d116dbdfcbce.jsonl");

    if !path.exists() {
        eprintln!("Skipping: test file not found");
        return;
    }

    let session = parser.parse(path).expect("Failed to parse team session");

    println!("Session ID: {}", session.session_id);
    println!("Total events: {}", session.events.len());
    println!("Stats: {:?}", session.stats);

    // Count events by task_id
    let mut task_events: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut parent_count = 0usize;
    for event in &session.events {
        if let Some(ref tid) = event.task_id {
            *task_events.entry(tid.clone()).or_default() += 1;
        } else {
            parent_count += 1;
        }
    }

    println!("\nParent events (no task_id): {}", parent_count);
    println!("Subagent tasks: {}", task_events.len());
    for (tid, count) in &task_events {
        let start = session.events.iter().find(|e| {
            e.task_id.as_deref() == Some(tid.as_str())
                && matches!(e.event_type, opensession_core::EventType::TaskStart { .. })
        });
        let title = if let Some(e) = start {
            if let opensession_core::EventType::TaskStart { ref title } = e.event_type {
                title.clone().unwrap_or_default()
            } else {
                String::new()
            }
        } else {
            String::new()
        };
        println!("  {} ({} events) - {}", tid, count, title);
    }

    // Verify subagent events were merged
    assert!(
        !task_events.is_empty(),
        "Expected subagent tasks but found none"
    );
    assert!(
        session.stats.task_count > 0,
        "Expected task_count > 0 in stats"
    );
    // The parent session had ~200+ assistant entries, subagents add more
    assert!(
        session.events.len() > 300,
        "Expected merged events > 300, got {}",
        session.events.len()
    );
}
