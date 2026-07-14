mod bot;
mod config;
mod dispatcher;
mod error;
mod modules;

use anyhow::Result;
use config::Config;
use std::sync::Arc;
use tracing::error;
use tracing_subscriber::{fmt, EnvFilter};

#[tokio::main]
async fn main() -> Result<()> {
    // Инициализация логгера
    // Уровень по умолчанию: INFO. Переопределяется через RUST_LOG=rustbot=debug
    fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("rustbot=info,warn")),
        )
        .with_target(false)
        .with_thread_ids(false)
        .compact()
        .init();

    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config.toml".to_owned());

    let config = match Config::load(&config_path) {
        Ok(c) => Arc::new(c),
        Err(e) => {
            error!("Не удалось загрузить конфиг: {e:#}");
            error!("Скопируй config.toml.example в config.toml и заполни поля.");
            std::process::exit(1);
        }
    };

    if let Err(e) = bot::run(config).await {
        error!("Критическая ошибка: {e:#}");
        std::process::exit(1);
    }

    Ok(())
}
