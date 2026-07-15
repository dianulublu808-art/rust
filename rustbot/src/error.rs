use thiserror::Error;

#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum BotError {
    #[error("Config error: {0}")]
    Config(String),

    #[error("Authorization failed: {0}")]
    Auth(String),

    #[error("Module '{module}' failed on command '{cmd}': {source:#}")]
    Module {
        module: &'static str,
        cmd: String,
        #[source]
        source: anyhow::Error,
    },

    #[error("Telegram API error: {0}")]
    Api(#[from] grammers_client::InvocationError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
