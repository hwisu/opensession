use opensession_local_db::LocalDb;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tracing::{debug, error, info, warn};

use crate::config::{DaemonConfig, PublishMode};
use crate::repo_registry::RepoRegistry;
use crate::watcher::FileChangeEvent;

use super::config_resolution::{
    resolve_git_retention_schedule, resolve_lifecycle_schedule, resolve_publish_mode,
    should_auto_upload,
};
use super::git_retention::run_git_retention_once;
use super::lifecycle::{run_lifecycle_cleanup_on_start, run_lifecycle_cleanup_once};
use super::pipeline::process_file;

pub async fn run_scheduler(
    config: DaemonConfig,
    mut rx: mpsc::UnboundedReceiver<FileChangeEvent>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
    db: std::sync::Arc<LocalDb>,
) {
    let debounce_duration = Duration::from_secs(config.daemon.debounce_secs);

    let effective_mode = resolve_publish_mode(&config.daemon);
    let mut repo_registry = match RepoRegistry::load_default() {
        Ok(registry) => registry,
        Err(error) => {
            warn!("failed to load repo registry: {error}");
            RepoRegistry::default()
        }
    };

    run_lifecycle_cleanup_on_start(&config, &db, &repo_registry);

    let mut pending: HashMap<PathBuf, Instant> = HashMap::new();

    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let retention_schedule = resolve_git_retention_schedule(&config);
    let mut next_retention_run = retention_schedule.map(|(_, interval)| Instant::now() + interval);
    let lifecycle_interval = resolve_lifecycle_schedule(&config);
    let mut next_lifecycle_run = lifecycle_interval.map(|interval| Instant::now() + interval);

    loop {
        tokio::select! {
            Some(event) = rx.recv() => {
                debug!("Scheduling: {:?}", event.path.display());
                pending.insert(event.path, Instant::now());
            }
            _ = tick.tick() => {
                let now = Instant::now();
                let effective_debounce = match effective_mode {
                    PublishMode::Realtime => Duration::from_millis(config.daemon.realtime_debounce_ms),
                    _ => debounce_duration,
                };

                let ready: Vec<PathBuf> = pending
                    .iter()
                    .filter(|(_, last_change)| now.duration_since(**last_change) >= effective_debounce)
                    .map(|(path, _)| path.clone())
                    .collect();

                for path in ready {
                    pending.remove(&path);
                    if matches!(effective_mode, PublishMode::Manual) {
                        debug!(
                            "Manual mode, indexing locally without auto-publish: {}",
                            path.display()
                        );
                    }
                    if let Err(error) = process_file(
                        &path,
                        &config,
                        &db,
                        &mut repo_registry,
                        should_auto_upload(&effective_mode),
                    )
                    .await
                    {
                        error!("Failed to process {}: {:#}", path.display(), error);
                    }
                }

                maybe_run_retention_cycle(now, retention_schedule, &mut next_retention_run, &repo_registry);
                maybe_run_lifecycle_cycle(now, lifecycle_interval, &mut next_lifecycle_run, &config, &db, &repo_registry);
            }
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    info!("Scheduler shutting down");
                    break;
                }
            }
        }
    }
}

fn maybe_run_retention_cycle(
    now: Instant,
    retention_schedule: Option<(u32, Duration)>,
    next_retention_run: &mut Option<Instant>,
    repo_registry: &RepoRegistry,
) {
    if let (Some((keep_days, interval)), Some(next_at)) = (retention_schedule, *next_retention_run)
    {
        if now >= next_at {
            if let Err(error) = run_git_retention_once(repo_registry, keep_days) {
                warn!("Git retention scan failed: {error}");
            }
            *next_retention_run = Some(now + interval);
        }
    }
}

fn maybe_run_lifecycle_cycle(
    now: Instant,
    lifecycle_interval: Option<Duration>,
    next_lifecycle_run: &mut Option<Instant>,
    config: &DaemonConfig,
    db: &LocalDb,
    repo_registry: &RepoRegistry,
) {
    if let (Some(interval), Some(next_at)) = (lifecycle_interval, *next_lifecycle_run) {
        if now >= next_at {
            if let Err(error) = run_lifecycle_cleanup_once(config, db, repo_registry) {
                warn!("Lifecycle cleanup failed: {error}");
            }
            *next_lifecycle_run = Some(now + interval);
        }
    }
}
