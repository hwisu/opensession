use anyhow::Result;
use opensession_parsers::discover::discover_sessions;

/// List all locally discovered AI sessions
pub fn run_discover() -> Result<()> {
    let locations = discover_sessions();

    if locations.is_empty() {
        println!("No AI sessions found on this machine.");
        println!();
        println!("Supported tools: Claude Code, OpenCode, Goose, Aider, Cursor");
        println!("Sessions are searched in their default locations.");
        return Ok(());
    }

    let mut total_files = 0usize;

    for loc in &locations {
        println!("[{}] {} session file(s)", loc.tool, loc.paths.len());
        for path in &loc.paths {
            let metadata = std::fs::metadata(path).ok();
            let modified = metadata
                .and_then(|m| m.modified().ok())
                .map(|t| {
                    let dt: chrono::DateTime<chrono::Utc> = t.into();
                    dt.format("%Y-%m-%d %H:%M:%S UTC").to_string()
                })
                .unwrap_or_else(|| "unknown".to_string());

            let size = std::fs::metadata(path)
                .map(|m| format_size(m.len()))
                .unwrap_or_else(|_| "?".to_string());

            println!("  {} ({}, {})", path.display(), size, modified);
            total_files += 1;
        }
        println!();
    }

    println!(
        "Total: {} tool(s), {} session file(s)",
        locations.len(),
        total_files
    );

    Ok(())
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
