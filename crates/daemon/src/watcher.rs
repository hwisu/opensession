use anyhow::{Context, Result};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use opensession_parsers::discover;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// A file change event emitted by the watcher
#[derive(Debug, Clone)]
pub struct FileChangeEvent {
    pub path: PathBuf,
}

/// Seed startup processing by enqueueing already-existing session files
/// under the configured watch roots.
pub fn seed_existing_session_files(
    watch_roots: &[PathBuf],
    tx: &mpsc::UnboundedSender<FileChangeEvent>,
) -> usize {
    if watch_roots.is_empty() {
        return 0;
    }

    let discovered_paths = discover::discover_sessions()
        .into_iter()
        .flat_map(|location| location.paths);
    enqueue_discovered_paths(watch_roots, discovered_paths, tx)
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

fn enqueue_discovered_paths<I>(
    watch_roots: &[PathBuf],
    discovered_paths: I,
    tx: &mpsc::UnboundedSender<FileChangeEvent>,
) -> usize
where
    I: IntoIterator<Item = PathBuf>,
{
    let mut queued = 0usize;
    let mut seen = HashSet::new();

    for path in discovered_paths {
        if !is_path_under_any_watch_root(&path, watch_roots) {
            continue;
        }
        if seen.insert(path.clone()) {
            let _ = tx.send(FileChangeEvent { path });
            queued += 1;
        }
    }

    queued
}

fn is_path_under_any_watch_root(path: &Path, watch_roots: &[PathBuf]) -> bool {
    watch_roots.iter().any(|root| path.starts_with(root))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enqueue_discovered_paths_includes_only_watch_root_and_dedupes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let watch_root = temp.path().join("watch");
        let outside_root = temp.path().join("outside");
        std::fs::create_dir_all(&watch_root).expect("mkdir watch");
        std::fs::create_dir_all(&outside_root).expect("mkdir outside");

        let inside_a = watch_root.join("sessions").join("a.jsonl");
        let inside_b = watch_root.join("sessions").join("b.json");
        let outside = outside_root.join("sessions").join("c.jsonl");
        std::fs::create_dir_all(inside_a.parent().expect("inside_a parent")).expect("mkdir");
        std::fs::create_dir_all(inside_b.parent().expect("inside_b parent")).expect("mkdir");
        std::fs::create_dir_all(outside.parent().expect("outside parent")).expect("mkdir");

        let discovered = vec![
            inside_a.clone(),
            inside_b.clone(),
            inside_a.clone(), // duplicate
            outside,
        ];
        let (tx, mut rx) = mpsc::unbounded_channel();

        let queued = enqueue_discovered_paths(&[watch_root], discovered, &tx);

        let mut observed = Vec::new();
        while let Ok(event) = rx.try_recv() {
            observed.push(event.path);
        }

        assert_eq!(queued, 2);
        assert_eq!(observed.len(), 2);
        assert!(observed.contains(&inside_a));
        assert!(observed.contains(&inside_b));
    }

    #[test]
    fn is_path_under_any_watch_root_returns_false_when_unrelated() {
        let root = PathBuf::from("/tmp/watch");
        let unrelated = PathBuf::from("/tmp/other/session.jsonl");
        assert!(!is_path_under_any_watch_root(&unrelated, &[root]));
    }
}
