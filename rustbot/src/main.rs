mod bot;
mod config;
mod dispatcher;
mod error;
mod modules;
mod plugins;
mod storage;
mod term;

use anyhow::Result;
use config::Config;
use std::sync::Arc;
use tracing::error;

#[tokio::main]
async fn main() -> Result<()> {
    // место паники в логе — полезно для диагностики, если что-то упадёт
    if std::env::var_os("RUST_BACKTRACE").is_none() {
        std::env::set_var("RUST_BACKTRACE", "1");
    }
    term::init_logging();
    term::banner();

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
