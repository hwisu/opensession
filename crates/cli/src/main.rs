mod cat_cmd;
mod cleanup_cmd;
mod cli_args;
mod config_cmd;
mod docs_cmd;
mod doctor_cmd;
mod entrypoint;
mod handoff_v1;
mod hooks;
mod inspect;
mod open_target;
mod parse_cmd;
mod register;
mod review;
mod runtime_settings;
mod setup_cmd;
mod share;
mod summary_cmd;
mod url_opener;
mod user_guidance;
mod view;

#[tokio::main]
async fn main() {
    entrypoint::run_process().await;
}
