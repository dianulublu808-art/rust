use crate::{
    config::Config,
    modules::{Context, Module},
    storage::Storage,
};
use anyhow::Result;
use grammers_client::{Client, InputMessage, Update, types::update::Message};
use grammers_session::PeerRef;
use std::sync::Arc;
use tracing::{debug, error, warn};

pub struct Dispatcher {
    modules:      Vec<Arc<dyn Module>>,
    config:       Arc<Config>,
    storage:      Arc<Storage>,
    me_id:        i64,
    me_username:  Option<String>,
}

impl Dispatcher {
    pub fn new(
        config: Arc<Config>,
        storage: Arc<Storage>,
        me_id: i64,
        me_username: Option<String>,
        modules: Vec<Arc<dyn Module>>,
    ) -> Self {
        for m in &modules {
            for c in m.commands() {
                debug!("cmd '{}' ← {}", c.name, m.name());
            }
        }
        Self { modules, config, storage, me_id, me_username }
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
        self.try_afk_reply(client, msg).await;
    }

    async fn try_afk_reply(&self, client: &Client, msg: &Message) {
        let active = self.storage.get::<bool>("afk.active").unwrap_or(false);
        if !active { return; }

        let text = msg.text();

        let mentions = if let Some(ref u) = self.me_username {
            text.contains(&format!("@{u}"))
        } else {
            false
        };
        let is_reply_to_me = self.msg_is_reply_to_me(msg);

        if !mentions && !is_reply_to_me { return; }

        // Не отвечать самому себе
        if let Some(s) = msg.sender() {
            use grammers_client::types::Chat;
            if let Chat::User(u) = s {
                if u.bare_id() == self.me_id { return; }
            }
        }

        let reason = self.storage.get::<String>("afk.reason").unwrap_or_default();
        let since  = self.storage.get::<i64>("afk.since").unwrap_or(0);

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
            Err(pr)  => pr,
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
        let prefix = self.config.prefix_char();
        if self.storage.get::<bool>("afk.active").unwrap_or(false)
            && !text.starts_with(prefix)
        {
            let _ = self.storage.del("afk.active");
            let _ = self.storage.del("afk.reason");
            let _ = self.storage.del("afk.since");
        }

        if !text.starts_with(prefix) { return Ok(()); }

        let rest = text[prefix.len_utf8()..].trim_start();
        if rest.is_empty() { return Ok(()); }

        let mut parts = rest.splitn(2, char::is_whitespace);
        let cmd = match parts.next() {
            Some(c) if !c.is_empty() => c.to_lowercase(),
            _ => return Ok(()),
        };
        let args_raw  = parts.next().unwrap_or("").to_owned();
        let args: Vec<String> = args_raw.split_whitespace().map(str::to_owned).collect();

        let handler = self.modules.iter().find(|m| {
            m.commands().iter().any(|c| c.name.eq_ignore_ascii_case(&cmd))
        });

        match handler {
            None => { warn!("Неизвестная команда: '{cmd}'"); Ok(()) }
            Some(module) => {
                let ctx = Context {
                    client,
                    message: msg,
                    args,
                    args_raw,
                    config:  Arc::clone(&self.config),
                    storage: Arc::clone(&self.storage),
                };
                if let Err(e) = module.handle(&cmd, ctx).await {
                    error!(module = module.name(), cmd = %cmd, "Ошибка модуля: {e:#}");
                }
                Ok(())
            }
        }
    }
}
