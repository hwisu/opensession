//! Integration tests that run against real local session files.
//! These are ignored by default — run with: cargo test -p opensession-parsers -- real --ignored

#[test]
#[ignore = "requires real Codex session files"]
fn parse_real_codex_session() {
    let codex = opensession_parsers::all_parsers()
        .into_iter()
        .find(|p| p.name() == "codex")
        .unwrap();

    let paths = opensession_parsers::discover::discover_for_tool("codex");
    assert!(!paths.is_empty(), "No Codex session files found");

    for path in &paths {
        assert!(
            codex.can_parse(path),
            "can_parse failed for {}",
            path.display()
        );
        let session = codex
            .parse(path)
            .unwrap_or_else(|_| panic!("Failed to parse {}", path.display()));

        println!(
            "Codex session: id={} title={:?} events={} model={}",
            session.session_id,
            session.context.title,
            session.events.len(),
            session.agent.model,
        );

        // Sessions should have at least a session_id
        assert!(!session.session_id.is_empty());
    }
}

#[test]
#[ignore = "requires real Gemini session files"]
fn parse_real_gemini_session() {
    let gemini = opensession_parsers::all_parsers()
        .into_iter()
        .find(|p| p.name() == "gemini")
        .unwrap();

    let paths = opensession_parsers::discover::discover_for_tool("gemini");
    assert!(!paths.is_empty(), "No Gemini session files found");

    for path in &paths {
        assert!(
            gemini.can_parse(path),
            "can_parse failed for {}",
            path.display()
        );
        let session = gemini
            .parse(path)
            .unwrap_or_else(|_| panic!("Failed to parse {}", path.display()));

        println!(
            "Gemini session: id={} title={:?} events={} model={}",
            session.session_id,
            session.context.title,
            session.events.len(),
            session.agent.model,
        );

        assert!(!session.session_id.is_empty());
    }
}

#[test]
#[ignore = "requires real OpenCode session files"]
fn parse_real_opencode_session() {
    let opencode = opensession_parsers::all_parsers()
        .into_iter()
        .find(|p| p.name() == "opencode")
        .unwrap();

    let paths = opensession_parsers::discover::discover_for_tool("opencode");
    assert!(!paths.is_empty(), "No OpenCode session files found");

    for path in &paths {
        assert!(
            opencode.can_parse(path),
            "can_parse failed for {}",
            path.display()
        );
        let session = opencode
            .parse(path)
            .unwrap_or_else(|_| panic!("Failed to parse {}", path.display()));

        println!(
            "OpenCode session: id={} title={:?} events={} model={}",
            session.session_id,
            session.context.title,
            session.events.len(),
            session.agent.model,
        );

        assert!(!session.session_id.is_empty());
    }
}
