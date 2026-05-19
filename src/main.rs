#![windows_subsystem = "windows"]

mod app;
mod config;
mod desktop;
mod events;
mod hotkeys;
mod startup;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    if let Err(err) = app::run() {
        tracing::error!("vdesk failed: {err:#}");
        std::process::exit(1);
    }
}
