mod app_config;
mod error;
mod routes;
mod startup;
mod storage;

pub use app_config::AppConfig;
pub use startup::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    startup::run().await
}
