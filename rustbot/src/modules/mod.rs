use crate::{config::Config, storage::Storage};
use anyhow::Result;
use async_trait::async_trait;
use grammers_client::{types::update::Message, Client, InputMessage};
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
    pub name: String,
    pub description: String,
    pub usage: String,
}

impl Command {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        usage: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            usage: usage.into(),
        }
    }
}

// ── Контекст ─────────────────────────────────────────────────────────────────

pub struct Context {
    pub client: Client,
    pub message: Message,
    pub args: Vec<String>,
    pub args_raw: String,
    pub config: Arc<Config>,
    pub storage: Arc<Storage>,
}

impl Context {
    pub fn peer_ref(&self) -> PeerRef {
        match self.message.chat() {
            Ok(chat) => PeerRef::from(chat),
            Err(pr) => pr,
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
        let body = truncate_utf8(body, 3500);
        self.edit(format!("{header}\n```\n{body}\n```")).await
    }
}

fn truncate_utf8(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

// ── Трейт модуля ─────────────────────────────────────────────────────────────

#[async_trait]
pub trait Module: Send + Sync {
    fn name(&self) -> &str;
    fn commands(&self) -> Vec<Command>;
    async fn handle(&self, cmd: &str, ctx: Context) -> Result<()>;
}
