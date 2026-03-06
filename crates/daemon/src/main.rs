mod cli;
mod config;
mod entrypoint;
mod health;
pub mod hooks;
mod repo_registry;
mod runtime;
mod scheduler;
mod watcher;

#[tokio::main]
async fn main() {
    entrypoint::run_process().await;
}
