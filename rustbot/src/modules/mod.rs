use anyhow::Result;
use async_trait::async_trait;
use grammers_client::{Client, types::Message};
use std::sync::Arc;
use crate::config::Config;

pub mod ping;
pub mod help;
pub mod eval;
pub mod exec;
pub mod sysinfo;
pub mod raw;

/// Описание одной команды модуля
#[derive(Debug, Clone)]
pub struct Command {
    pub name: &'static str,
    pub description: &'static str,
    pub usage: &'static str, // пример: ".ping" или ".eval <код>"
}

/// Контекст, передаваемый каждому обработчику
pub struct Context {
    pub client: Client,
    pub message: Message,
    /// Аргументы после команды (split по пробелу)
    pub args: Vec<String>,
    /// Полный текст после команды (включая пробелы)
    pub args_raw: String,
    pub config: Arc<Config>,
}

impl Context {
    /// Редактировать исходное сообщение
    pub async fn edit(&self, text: impl AsRef<str>) -> Result<()> {
        use grammers_client::InputMessage;
        self.client
            .edit_message(
                self.message.chat(),
                self.message.id(),
                InputMessage::text(text.as_ref()),
            )
            .await
            .map_err(anyhow::Error::from)
    }

    /// Отправить новое сообщение в тот же чат
    pub async fn reply(&self, text: impl AsRef<str>) -> Result<()> {
        use grammers_client::InputMessage;
        self.client
            .send_message(self.message.chat(), InputMessage::text(text.as_ref()))
            .await
            .map_err(anyhow::Error::from)?;
        Ok(())
    }

    /// Редактировать с форматированием моноширинного вывода
    pub async fn edit_code(&self, header: &str, body: &str) -> Result<()> {
        let text = format!("{header}\n```\n{body}\n```");
        self.edit(&text).await
    }
}

#[async_trait]
pub trait Module: Send + Sync {
    fn name(&self) -> &'static str;
    fn commands(&self) -> &[Command];
    async fn handle(&self, cmd: &str, ctx: Context) -> Result<()>;
}
