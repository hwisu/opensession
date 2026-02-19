use anyhow::{Context, Result};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// A file change event emitted by the watcher
#[derive(Debug, Clone)]
pub struct FileChangeEvent {
    pub path: PathBuf,
}

/// Start watching the given directories, sending file change events to the channel.
/// Returns the watcher handle (must be kept alive).
pub fn start_watcher(
    paths: &[PathBuf],
    tx: mpsc::UnboundedSender<FileChangeEvent>,
) -> Result<RecommendedWatcher> {
    let tx_clone = tx.clone();

    let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        match res {
            Ok(event) => {
                let should_emit = matches!(
                    event.kind,
                    notify::EventKind::Create(_) | notify::EventKind::Modify(_)
                );

                if should_emit {
                    for path in event.paths {
                        // Only care about session-like files
                        if is_session_file(&path) {
                            debug!("File change detected: {}", path.display());
                            let _ = tx_clone.send(FileChangeEvent { path });
                        }
                    }
                }
            }
            Err(e) => {
                error!("Watcher error: {}", e);
            }
        }
    })
    .context("Failed to create file watcher")?;

    for path in paths {
        info!("Watching directory: {}", path.display());
        if let Err(e) = watcher.watch(path, RecursiveMode::Recursive) {
            warn!("Failed to watch {}: {}", path.display(), e);
        }
    }

    Ok(watcher)
}

/// Check if a file looks like a session file we care about
fn is_session_file(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

    matches!(ext, "jsonl" | "json" | "db")
}
