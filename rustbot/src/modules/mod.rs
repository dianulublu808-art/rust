use crate::{config::Config, storage::Storage};
use anyhow::Result;
use async_trait::async_trait;
use grammers_client::{types::update::Message, Client, InputMessage};
use grammers_session::PeerRef;
use std::sync::{Arc, RwLock};

/// Общий изменяемый список модулей (нужен для горячей перезагрузки плагинов).
pub type Registry = Arc<RwLock<Vec<Arc<dyn Module>>>>;

pub mod afk;
pub mod botconfig;
pub mod eval;
pub mod exec;
pub mod help;
pub mod info;
pub mod notes;
pub mod ping;
pub mod raw;
pub mod reload;
pub mod security;
pub mod settings;
pub mod sysinfo;
pub mod translate;
pub mod ui;
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

    pub fn command_prefix(&self) -> char {
        self.storage
            .get::<String>("settings.prefix")
            .and_then(|prefix| prefix.chars().next())
            .unwrap_or_else(|| self.config.prefix_char())
    }

    /// Редактировать исходное сообщение.
    pub async fn edit(&self, text: impl AsRef<str>) -> Result<()> {
        self.client
            .edit_message(
                self.peer_ref(),
                self.message.id(),
                InputMessage::new().html(text.as_ref()),
            )
            .await
            .map_err(anyhow::Error::from)
    }

    /// Отправить новое сообщение в тот же чат.
    #[allow(dead_code)]
    pub async fn reply(&self, text: impl AsRef<str>) -> Result<()> {
        self.client
            .send_message(self.peer_ref(), InputMessage::new().html(text.as_ref()))
            .await
            .map_err(anyhow::Error::from)?;
        Ok(())
    }

    /// Редактировать, обернув вывод в code-блок.
    pub async fn edit_code(&self, header: &str, body: &str) -> Result<()> {
        let body = truncate_utf8(body, 3500);
        self.edit(format!("{header}\n<pre>{}</pre>", esc(body))).await
    }
}

/// Экранирование текста для HTML-разметки Telegram.
pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
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

#[cfg(test)]
mod html_tests {
    use super::esc;
    use grammers_client::parsers::parse_html_message;

    #[test]
    fn html_escape_roundtrip() {
        let body = esc("a < b && c > d\nline2");
        let (text, entities) = parse_html_message(&format!("<b>x</b>\n<pre>{body}</pre>"));
        assert_eq!(text, "x\na < b && c > d\nline2");
        assert_eq!(entities.len(), 2);
    }

    #[test]
    fn html_plain_text_untouched() {
        let (text, entities) = parse_html_message(&esc("<script>1 & 2</script>"));
        assert_eq!(text, "<script>1 & 2</script>");
        assert!(entities.is_empty());
    }
}
