use crate::{config::Config, storage::Storage};
use anyhow::Result;
use async_trait::async_trait;
use grammers_client::{types::update::Message, Client, InputMessage, InvocationError};
use std::future::Future;
use std::time::Duration;
use tracing::warn;
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

    /// Редактировать исходное сообщение (с повтором при FLOOD_WAIT).
    pub async fn edit(&self, text: impl AsRef<str>) -> Result<()> {
        let text = text.as_ref();
        let res = with_flood_retry(|| {
            self.client.edit_message(
                self.peer_ref(),
                self.message.id(),
                InputMessage::new().html(text),
            )
        })
        .await;
        match res {
            Ok(_) => Ok(()),
            // повторное редактирование тем же текстом — не ошибка
            Err(e) if e.is("MESSAGE_NOT_MODIFIED") => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// Отправить новое сообщение в тот же чат.
    #[allow(dead_code)]
    pub async fn reply(&self, text: impl AsRef<str>) -> Result<()> {
        let text = text.as_ref();
        with_flood_retry(|| {
            self.client
                .send_message(self.peer_ref(), InputMessage::new().html(text))
        })
        .await?;
        Ok(())
    }

    /// Редактировать, обернув вывод в code-блок.
    pub async fn edit_code(&self, header: &str, body: &str) -> Result<()> {
        let body = truncate_utf8(body, 3500);
        self.edit(format!("{header}\n<pre>{}</pre>", esc(body))).await
    }
}

/// Предельное ожидание, на которое мы согласны подождать при FLOOD_WAIT.
const MAX_FLOOD_WAIT_SECS: u32 = 60;

/// Повторяет запрос, если Telegram ответил FLOOD_WAIT_X (до 2 раз и не дольше
/// `MAX_FLOOD_WAIT_SECS`); остальные ошибки возвращает как есть.
pub async fn with_flood_retry<T, F, Fut>(mut op: F) -> Result<T, InvocationError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, InvocationError>>,
{
    let mut attempts = 0u32;
    loop {
        match op().await {
            Err(InvocationError::Rpc(rpc))
                if rpc.name == "FLOOD_WAIT"
                    && attempts < 2
                    && rpc.value.unwrap_or(1) <= MAX_FLOOD_WAIT_SECS =>
            {
                attempts += 1;
                let secs = rpc.value.unwrap_or(1);
                warn!("FLOOD_WAIT {secs}с — жду и повторяю ({attempts}/2)");
                tokio::time::sleep(Duration::from_secs(secs as u64) + Duration::from_millis(500))
                    .await;
            }
            other => return other,
        }
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

#[cfg(test)]
mod flood_tests {
    use super::*;
    use grammers_mtsender::RpcError;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn rpc(name: &str, value: Option<u32>) -> InvocationError {
        InvocationError::Rpc(RpcError {
            code: 420,
            name: name.to_owned(),
            value,
            caused_by: None,
        })
    }

    #[tokio::test]
    async fn retries_flood_wait_then_succeeds() {
        let calls = AtomicU32::new(0);
        let res = with_flood_retry(|| {
            let n = calls.fetch_add(1, Ordering::SeqCst);
            async move {
                if n == 0 {
                    Err(rpc("FLOOD_WAIT", Some(0)))
                } else {
                    Ok(42)
                }
            }
        })
        .await;
        assert_eq!(res.unwrap(), 42);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn gives_up_after_two_retries() {
        let calls = AtomicU32::new(0);
        let res: Result<(), _> = with_flood_retry(|| {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Err(rpc("FLOOD_WAIT", Some(0))) }
        })
        .await;
        assert!(res.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn does_not_wait_for_huge_flood_or_other_errors() {
        let calls = AtomicU32::new(0);
        let res: Result<(), _> = with_flood_retry(|| {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Err(rpc("FLOOD_WAIT", Some(3600))) }
        })
        .await;
        assert!(res.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let calls = AtomicU32::new(0);
        let res: Result<(), _> = with_flood_retry(|| {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Err(rpc("MESSAGE_ID_INVALID", None)) }
        })
        .await;
        assert!(res.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
