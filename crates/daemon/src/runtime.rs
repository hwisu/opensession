use anyhow::Result;
use opensession_local_db::LocalDb;
use std::sync::Arc;
use tokio::sync::{mpsc, watch};
use tracing::info;

use crate::{config, health, scheduler, watcher};

pub(crate) async fn run() -> Result<()> {
    info!("opensession-daemon starting");

    let cfg = config::load_config()?;
    let watch_paths = config::resolve_watch_paths(&cfg);

    if watch_paths.is_empty() {
        info!("No session directories found to watch. The daemon will idle.");
    } else {
        info!("Watching {} directories", watch_paths.len());
    }

    let db = Arc::new(LocalDb::open()?);
    info!("Local DB opened");

    write_pid_file()?;

    let (tx, rx) = mpsc::unbounded_channel();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let _watcher = start_watcher_pipeline(&watch_paths, &tx)?;

    let scheduler_cfg = cfg.clone();
    let scheduler_shutdown = shutdown_rx.clone();
    let scheduler_db = Arc::clone(&db);
    let scheduler_handle = tokio::spawn(async move {
        scheduler::run_scheduler(scheduler_cfg, rx, scheduler_shutdown, scheduler_db).await;
    });

    let health_shutdown = shutdown_rx.clone();
    let health_handle = tokio::spawn(health::run_health_check(
        cfg.server.url.clone(),
        cfg.server.api_key.clone(),
        watch_paths.clone(),
        cfg.daemon.health_check_interval_secs,
        health_shutdown,
    ));

    wait_for_shutdown().await;

    info!("Shutdown signal received, stopping...");
    let _ = shutdown_tx.send(true);

    let _ = scheduler_handle.await;
    let _ = health_handle.await;

    cleanup_pid_file();

    info!("opensession-daemon stopped");
    Ok(())
}

fn start_watcher_pipeline(
    watch_paths: &[std::path::PathBuf],
    tx: &mpsc::UnboundedSender<watcher::FileChangeEvent>,
) -> Result<Option<notify::RecommendedWatcher>> {
    if watch_paths.is_empty() {
        return Ok(None);
    }

    let watcher_handle = watcher::start_watcher(watch_paths, tx.clone())?;
    let seeded = watcher::seed_existing_session_files(watch_paths, tx);
    if seeded > 0 {
        info!(
            "Queued {} existing session files for startup backfill",
            seeded
        );
    }
    Ok(Some(watcher_handle))
}

fn write_pid_file() -> Result<()> {
    let path = config::pid_file_path()?;
    let dir = path.parent().expect("pid file path should have parent");
    std::fs::create_dir_all(dir)?;
    std::fs::write(&path, std::process::id().to_string())?;
    info!("PID file written: {}", path.display());
    Ok(())
}

fn cleanup_pid_file() {
    if let Ok(path) = config::pid_file_path() {
        let _ = std::fs::remove_file(path);
    }
}

async fn wait_for_shutdown() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut sigterm = signal(SignalKind::terminate()).expect("Failed to register SIGTERM");
        let mut sigint = signal(SignalKind::interrupt()).expect("Failed to register SIGINT");
        tokio::select! {
            _ = sigterm.recv() => info!("Received SIGTERM"),
            _ = sigint.recv() => info!("Received SIGINT"),
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to register Ctrl+C handler");
        info!("Received Ctrl+C");
    }
}

#[cfg(test)]
mod tests {
    use super::start_watcher_pipeline;
    use tokio::sync::mpsc;

    #[test]
    fn watcher_pipeline_skips_empty_watch_paths() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let watcher = start_watcher_pipeline(&[], &tx).expect("empty watcher pipeline");
        assert!(watcher.is_none());
    }
}
