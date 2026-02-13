use anyhow::Result;
use opensession_local_db::git::extract_git_context;
use opensession_local_db::LocalDb;
use opensession_parsers::discover::discover_sessions;
use opensession_parsers::{all_parsers, SessionParser};
use std::path::Path;

/// Run the index command: discover all local sessions and build/update the local DB index.
pub fn run_index() -> Result<()> {
    let db = LocalDb::open()?;
    let parsers = all_parsers();
    let locations = discover_sessions();

    if locations.is_empty() {
        println!("No AI sessions found on this machine.");
        return Ok(());
    }

    let mut indexed = 0usize;
    let mut skipped = 0usize;
    let mut errors = 0usize;

    for loc in &locations {
        for path in &loc.paths {
            match index_one_file(&db, &parsers, path) {
                Ok(true) => indexed += 1,
                Ok(false) => skipped += 1,
                Err(e) => {
                    eprintln!("  Error indexing {}: {:#}", path.display(), e);
                    errors += 1;
                }
            }
        }
    }

    println!(
        "Indexed: {indexed} | Skipped: {skipped} | Errors: {errors} | Total in DB: {}",
        db.session_count().unwrap_or(0)
    );

    Ok(())
}

/// Index a single session file. Returns Ok(true) if indexed, Ok(false) if skipped.
fn index_one_file(db: &LocalDb, parsers: &[Box<dyn SessionParser>], path: &Path) -> Result<bool> {
    let parser = match parsers.iter().find(|p| p.can_parse(path)) {
        Some(p) => p,
        None => return Ok(false),
    };

    let session = parser.parse(path)?;
    let path_str = path.to_string_lossy().to_string();

    let cwd = session
        .context
        .attributes
        .get("cwd")
        .or_else(|| session.context.attributes.get("working_directory"))
        .and_then(|v| v.as_str().map(String::from));
    let git = cwd.as_deref().map(extract_git_context).unwrap_or_default();

    db.upsert_local_session(&session, &path_str, &git)?;
    Ok(true)
}
