use thiserror::Error;

#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum BotError {
    #[error("Config: {0}")]
    Config(String),
    #[error("Auth: {0}")]
    Auth(String),
    #[error("API: {0}")]
    Api(#[from] grammers_client::InvocationError),
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
}
