use crate::{
    config::Config,
    modules::{security, Context, Module, Registry},
    storage::Storage,
};
use anyhow::{anyhow, bail, Result};
use grammers_client::{types::update::Message, Client, InputMessage, Update};
use grammers_session::PeerRef;
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, warn};

/// Предел времени одного вызова модуля (`.update` со сборкой — самый долгий).
const MODULE_TIMEOUT: Duration = Duration::from_secs(2400);

pub struct Dispatcher {
    modules: Registry,
    config: Arc<Config>,
    storage: Arc<Storage>,
    me_id: i64,
    me_username: Option<String>,
}

impl Dispatcher {
    pub fn new(
        config: Arc<Config>,
        storage: Arc<Storage>,
        me_id: i64,
        me_username: Option<String>,
        modules: Registry,
    ) -> Result<Self> {
        {
            let g = modules.read().unwrap();
            check_duplicates(&g)?;
        }
        Ok(Self {
            modules,
            config,
            storage,
            me_id,
            me_username,
        })
    }

    pub async fn handle(&self, client: Client, update: Update) -> Result<()> {
        let msg = match update {
            Update::NewMessage(m) => m,
            Update::MessageEdited(m) => m,
            _ => return Ok(()),
        };

        if msg.outgoing() {
            self.handle_outgoing(client, msg).await
        } else {
            self.handle_incoming(&client, &msg).await;
            Ok(())
        }
    }

    // ── Входящие ─────────────────────────────────────────────────────────────

    async fn handle_incoming(&self, client: &Client, msg: &Message) {
        if security::is_message_blocked(&self.storage, msg) {
            return;
        }
        self.try_afk_reply(client, msg).await;
    }

    async fn try_afk_reply(&self, client: &Client, msg: &Message) {
        let active = self.storage.get::<bool>("afk.active").unwrap_or(false);
        if !active {
            return;
        }

        let text = msg.text();

        let mentions = if let Some(ref u) = self.me_username {
            text.contains(&format!("@{u}"))
        } else {
            false
        };
        let is_reply_to_me = self.msg_is_reply_to_me(msg);

        if !mentions && !is_reply_to_me {
            return;
        }

        // Не отвечать самому себе
        if let Some(s) = msg.sender() {
            use grammers_client::types::Chat;
            if let Chat::User(u) = s {
                if u.bare_id() == self.me_id {
                    return;
                }
            }
        }

        let reason = self.storage.get::<String>("afk.reason").unwrap_or_default();
        let since = self.storage.get::<i64>("afk.since").unwrap_or(0);

        let since_str = if since > 0 {
            chrono::DateTime::from_timestamp(since, 0)
                .map(|d| format!(" (с {})", d.format("%H:%M")))
                .unwrap_or_default()
        } else {
            String::new()
        };

        let body = if reason.is_empty() {
            format!("🌙 AFK{since_str}")
        } else {
            format!("🌙 AFK{since_str}\n💬 {reason}")
        };

        let peer = match msg.chat() {
            Ok(chat) => PeerRef::from(chat),
            Err(pr) => pr,
        };

        let _ = client
            .send_message(
                peer,
                InputMessage::new().text(&body).reply_to(Some(msg.id())),
            )
            .await;
    }

    fn msg_is_reply_to_me(&self, msg: &Message) -> bool {
        // Telegram не даёт sender_id ответа без доп. запроса,
        // поэтому пока полагаемся только на mentions.
        // Можно расширить позже через get_messages_by_id.
        let _ = msg;
        false
    }

    // ── Исходящие ─────────────────────────────────────────────────────────────

    async fn handle_outgoing(&self, client: Client, msg: Message) -> Result<()> {
        let text = msg.text().to_owned();

        // Любое исходящее НЕ-команда сбрасывает AFK
        let prefix = self.command_prefix();
        let parsed = parse_command(&text, prefix);
        if self.storage.get::<bool>("afk.active").unwrap_or(false) && parsed.is_none() {
            let _ = self.storage.del("afk.active");
            let _ = self.storage.del("afk.reason");
            let _ = self.storage.del("afk.since");
        }

        let Some((cmd, args_raw)) = parsed else {
            return Ok(());
        };
        let args: Vec<String> = args_raw.split_whitespace().map(str::to_owned).collect();

        let chat_id = msg.chat_id().bot_api_dialog_id();
        crate::term::seen(chat_id, &text);

        if security::is_chat_blocked(&self.storage, chat_id) && !is_recovery_command(&cmd) {
            crate::term::skipped("чат в блок-листе — команда проигнорирована (.unblockchat)");
            return Ok(());
        }

        let handler: Option<Arc<dyn Module>> = {
            let g = self.modules.read().unwrap();
            let found = g
                .iter()
                .find(|m| {
                    m.commands()
                        .iter()
                        .any(|c| c.name.eq_ignore_ascii_case(&cmd))
                })
                .cloned();
            found
        };

        match handler {
            None => {
                warn!("Неизвестная команда: '{cmd}'");
                Ok(())
            }
            Some(module) => {
                let notify_peer = match msg.chat() {
                    Ok(chat) => PeerRef::from(chat),
                    Err(pr) => pr,
                };
                let notify_id = msg.id();
                let notify_client = client.clone();

                let ctx = Context {
                    client,
                    message: msg,
                    args,
                    args_raw,
                    config: Arc::clone(&self.config),
                    storage: Arc::clone(&self.storage),
                };
                let started = std::time::Instant::now();
                let name = module.name().to_owned();
                let cmd_owned = cmd.clone();
                let result =
                    guarded(async move { module.handle(&cmd_owned, ctx).await }, MODULE_TIMEOUT)
                        .await;
                let ms = started.elapsed().as_millis();
                let label = format!("{prefix}{cmd}");
                match result {
                    Ok(()) => crate::term::command_ok(&label, ms, chat_id),
                    Err(e) => {
                        let msg_text = format!("{e:#}");
                        crate::term::command_err(&label, &name, &msg_text);
                        // пользователь должен увидеть сбой в чате, а не молчание
                        let text = crate::modules::ui::err(&format!(
                            "<code>{}</code> ({}): {}",
                            crate::modules::esc(&label),
                            crate::modules::esc(&name),
                            crate::modules::esc(&msg_text)
                        ));
                        let _ = notify_client
                            .edit_message(notify_peer, notify_id, InputMessage::new().html(text))
                            .await;
                    }
                }
                Ok(())
            }
        }
    }

    /// (модулей, команд) — для стартового блока.
    pub fn stats(&self) -> (usize, usize) {
        let g = self.modules.read().unwrap();
        (g.len(), g.iter().map(|m| m.commands().len()).sum())
    }

    fn command_prefix(&self) -> char {
        self.storage
            .get::<String>("settings.prefix")
            .and_then(|prefix| prefix.chars().next())
            .unwrap_or_else(|| self.config.prefix_char())
    }
}

fn is_recovery_command(cmd: &str) -> bool {
    matches!(
        cmd,
        "blacklist" | "unblacklistchat" | "unblockchat" | "unblacklistuser" | "unblockuser"
    )
}

pub fn check_duplicates(modules: &[Arc<dyn Module>]) -> Result<()> {
    let mut command_owners = HashMap::new();
    for m in modules {
        for c in m.commands() {
            let key = c.name.to_ascii_lowercase();
            if let Some(previous) = command_owners.insert(key.clone(), m.name().to_owned()) {
                bail!(
                    "command '{}' is registered by both '{}' and '{}'",
                    key,
                    previous,
                    m.name()
                );
            }
            debug!("cmd '{}' ← {}", c.name, m.name());
        }
    }
    Ok(())
}

/// Разбор `<prefix>команда аргументы`.
///
/// Команда — слово из букв/цифр/`_`, начинающееся с буквы, и идёт сразу за
/// префиксом: `...`, `. привет`, `.5 яблок` командами не считаются.
pub fn parse_command(text: &str, prefix: char) -> Option<(String, String)> {
    let rest = text.strip_prefix(prefix)?;
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let (cmd, tail) = rest.split_at(end);
    let mut chars = cmd.chars();
    let first = chars.next()?;
    if !first.is_alphabetic() || !chars.all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    Some((cmd.to_lowercase(), tail.trim_start().to_owned()))
}

/// Запускает обработчик в отдельной задаче: паника не роняет диспетчер,
/// зависший модуль обрывается по таймауту.
async fn guarded<F>(fut: F, limit: Duration) -> Result<()>
where
    F: Future<Output = Result<()>> + Send + 'static,
{
    let mut task = tokio::spawn(fut);
    match tokio::time::timeout(limit, &mut task).await {
        Ok(Ok(res)) => res,
        Ok(Err(join)) if join.is_panic() => Err(anyhow!("внутренняя ошибка модуля (panic)")),
        Ok(Err(join)) => Err(anyhow!("задача модуля прервана: {join}")),
        Err(_) => {
            task.abort();
            Err(anyhow!("превышено время выполнения ({} с)", limit.as_secs()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_commands() {
        assert_eq!(parse_command(".ping", '.'), Some(("ping".into(), "".into())));
        assert_eq!(
            parse_command(".Tr ru hello world", '.'),
            Some(("tr".into(), "ru hello world".into()))
        );
        assert_eq!(
            parse_command(".exec ls\nls -la", '.'),
            Some(("exec".into(), "ls\nls -la".into()))
        );
        assert_eq!(
            parse_command(".eval\nfn main(){}", '.'),
            Some(("eval".into(), "fn main(){}".into()))
        );
        assert_eq!(parse_command("!help", '!'), Some(("help".into(), "".into())));
    }

    #[test]
    fn ignores_non_commands() {
        assert_eq!(parse_command("hello", '.'), None);
        assert_eq!(parse_command(".", '.'), None);
        assert_eq!(parse_command("...", '.'), None);
        assert_eq!(parse_command(". привет", '.'), None);
        assert_eq!(parse_command(".5 яблок", '.'), None);
        assert_eq!(parse_command(".a-b", '.'), None);
    }

    #[test]
    fn supports_unicode_commands() {
        assert_eq!(parse_command(".Пинг х", '.'), Some(("пинг".into(), "х".into())));
    }

    #[tokio::test]
    async fn guarded_passes_result_through() {
        assert!(guarded(async { Ok(()) }, Duration::from_secs(1)).await.is_ok());
        let err = guarded(async { Err(anyhow!("boom")) }, Duration::from_secs(1))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("boom"));
    }

    #[tokio::test]
    async fn guarded_reports_panic_instead_of_dying() {
        let err = guarded(
            async {
                if true {
                    panic!("module bug");
                }
                Ok(())
            },
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("panic"));
    }

    #[tokio::test]
    async fn guarded_aborts_hung_module() {
        let err = guarded(
            async {
                tokio::time::sleep(Duration::from_secs(30)).await;
                Ok(())
            },
            Duration::from_millis(50),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("превышено время"));
    }
}
