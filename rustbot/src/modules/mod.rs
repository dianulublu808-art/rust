use anyhow::Result;
use async_trait::async_trait;
use grammers_client::{Client, message::InputMessage};
use grammers_client::update::Message;
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
    pub usage: &'static str,
}

/// Контекст, передаваемый каждому обработчику
pub struct Context {
    pub client: Client,
    pub message: Message,
    pub args: Vec<String>,
    pub args_raw: String,
    pub config: Arc<Config>,
}

impl Context {
    /// Редактировать исходное сообщение
    pub async fn edit(&self, text: impl AsRef<str>) -> Result<()> {
        let peer_ref = self
            .message
            .peer_ref()
            .await
            .ok_or_else(|| anyhow::anyhow!("Не удалось получить peer ref для редактирования"))?;
        self.client
            .edit_message(
                peer_ref,
                self.message.id(),
                InputMessage::new().text(text.as_ref()),
            )
            .await
            .map_err(anyhow::Error::from)
    }

    /// Отправить новое сообщение в тот же чат
    pub async fn reply(&self, text: impl AsRef<str>) -> Result<()> {
        let peer_ref = self
            .message
            .peer_ref()
            .await
            .ok_or_else(|| anyhow::anyhow!("Не удалось получить peer ref для ответа"))?;
        self.client
            .send_message(peer_ref, InputMessage::new().text(text.as_ref()))
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
