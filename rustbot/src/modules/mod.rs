use crate::{config::Config, storage::Storage};
use anyhow::Result;
use async_trait::async_trait;
use grammers_client::{Client, InputMessage, types::update::Message};
use grammers_session::PeerRef;
use std::sync::Arc;

pub mod afk;
pub mod eval;
pub mod exec;
pub mod help;
pub mod info;
pub mod notes;
pub mod ping;
pub mod raw;
pub mod sysinfo;
pub mod translate;
pub mod updater;

// ── Описание команды ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Command {
    pub name:        &'static str,
    pub description: &'static str,
    pub usage:       &'static str,
}

// ── Контекст ─────────────────────────────────────────────────────────────────

pub struct Context {
    pub client:   Client,
    pub message:  Message,
    pub args:     Vec<String>,
    pub args_raw: String,
    pub config:   Arc<Config>,
    pub storage:  Arc<Storage>,
}

impl Context {
    pub fn peer_ref(&self) -> PeerRef {
        match self.message.chat() {
            Ok(chat) => PeerRef::from(chat),
            Err(pr)  => pr,
        }
    }

    /// Редактировать исходное сообщение.
    pub async fn edit(&self, text: impl AsRef<str>) -> Result<()> {
        self.client
            .edit_message(
                self.peer_ref(),
                self.message.id(),
                InputMessage::new().text(text.as_ref()),
            )
            .await
            .map_err(anyhow::Error::from)
    }

    /// Отправить новое сообщение в тот же чат.
    #[allow(dead_code)]
    pub async fn reply(&self, text: impl AsRef<str>) -> Result<()> {
        self.client
            .send_message(self.peer_ref(), InputMessage::new().text(text.as_ref()))
            .await
            .map_err(anyhow::Error::from)?;
        Ok(())
    }

    /// Редактировать, обернув вывод в code-блок.
    pub async fn edit_code(&self, header: &str, body: &str) -> Result<()> {
        let body = if body.len() > 3500 { &body[..3500] } else { body };
        self.edit(format!("{header}\n```\n{body}\n```")).await
    }
}

// ── Трейт модуля ─────────────────────────────────────────────────────────────

#[async_trait]
pub trait Module: Send + Sync {
    fn name(&self)              -> &'static str;
    fn commands(&self)          -> &[Command];
    async fn handle(&self, cmd: &str, ctx: Context) -> Result<()>;
}
