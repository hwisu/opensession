use anyhow::Result;
use opensession_core::session::{is_auxiliary_session, working_directory};
use opensession_local_db::git::extract_git_context;
use opensession_local_db::LocalDb;
use opensession_parsers::discover::discover_sessions;
use std::path::Path;

/// Run the index command: discover all local sessions and build/update the local DB index.
pub fn run_index() -> Result<()> {
    let db = LocalDb::open()?;
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
            match index_one_file(&db, path) {
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
fn index_one_file(db: &LocalDb, path: &Path) -> Result<bool> {
    let session = match opensession_parsers::parse_with_default_parsers(path)? {
        Some(session) => session,
        None => return Ok(false),
    };
    if is_auxiliary_session(&session) {
        return Ok(false);
    }
    let path_str = path.to_string_lossy().to_string();

    let git = working_directory(&session)
        .map(extract_git_context)
        .unwrap_or_default();

    db.upsert_local_session(&session, &path_str, &git)?;
    Ok(true)
}
