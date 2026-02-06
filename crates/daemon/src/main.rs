mod config;
mod scheduler;
mod watcher;

use anyhow::Result;
use tokio::sync::{mpsc, watch};
use tracing::{error, info};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("opensession_daemon=info".parse().unwrap())
                .add_directive(tracing::Level::WARN.into()),
        )
        .init();

    if let Err(e) = run().await {
        error!("Daemon fatal error: {:#}", e);
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    info!("opensession-daemon starting");

    let cfg = config::load_config()?;
    let watch_paths = config::resolve_watch_paths(&cfg);

    if watch_paths.is_empty() {
        info!("No session directories found to watch. The daemon will idle.");
    } else {
        info!("Watching {} directories", watch_paths.len());
    }

    // Write PID file
    write_pid_file()?;

    // Channel for file change events
    let (tx, rx) = mpsc::unbounded_channel();

    // Shutdown signal
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // Start file watcher (must keep handle alive)
    let _watcher = if !watch_paths.is_empty() {
        Some(watcher::start_watcher(&watch_paths, tx)?)
    } else {
        None
    };

    // Start scheduler in background
    let scheduler_cfg = cfg.clone();
    let scheduler_handle = tokio::spawn(async move {
        scheduler::run_scheduler(scheduler_cfg, rx, shutdown_rx).await;
    });

    // Wait for shutdown signal
    wait_for_shutdown().await;

    info!("Shutdown signal received, stopping...");
    let _ = shutdown_tx.send(true);

    // Wait for scheduler to finish
    let _ = scheduler_handle.await;

    // Clean up PID file
    cleanup_pid_file();

    info!("opensession-daemon stopped");
    Ok(())
}

/// Write PID file so the CLI can find us
fn write_pid_file() -> Result<()> {
    let path = config::pid_file_path()?;
    let dir = path.parent().unwrap();
    std::fs::create_dir_all(dir)?;
    std::fs::write(&path, std::process::id().to_string())?;
    info!("PID file written: {}", path.display());
    Ok(())
}

/// Remove PID file on clean shutdown
fn cleanup_pid_file() {
    if let Ok(path) = config::pid_file_path() {
        let _ = std::fs::remove_file(path);
    }
}

/// Wait for SIGTERM or SIGINT
async fn wait_for_shutdown() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
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
