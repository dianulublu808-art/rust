use anyhow::Result;
use async_trait::async_trait;
use grammers_client::{Client, InputMessage, types::update::Message};
use grammers_session::PeerRef;
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
    /// Получить PeerRef из текущего сообщения
    fn peer_ref(&self) -> Result<PeerRef> {
        match self.message.chat() {
            Ok(chat) => Ok(PeerRef::from(chat)),
            Err(peer_ref) => Ok(peer_ref),
        }
    }

    /// Редактировать исходное сообщение
    pub async fn edit(&self, text: impl AsRef<str>) -> Result<()> {
        let peer = self.peer_ref()?;
        self.client
            .edit_message(
                peer,
                self.message.id(),
                InputMessage::new().text(text.as_ref()),
            )
            .await
            .map_err(anyhow::Error::from)
    }

    /// Отправить новое сообщение в тот же чат
    #[allow(dead_code)]
    pub async fn reply(&self, text: impl AsRef<str>) -> Result<()> {
        let peer = self.peer_ref()?;
        self.client
            .send_message(peer, InputMessage::new().text(text.as_ref()))
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
