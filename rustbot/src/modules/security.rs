use super::{Command, Context, Module};
use anyhow::Result;
use async_trait::async_trait;
use grammers_client::types::{update::Message, Chat};

const BLOCKED_CHATS: &str = "security.blocked_chats";
const BLOCKED_USERS: &str = "security.blocked_users";

pub struct SecurityModule {
    me_id: i64,
}

impl SecurityModule {
    pub fn new(me_id: i64) -> Self {
        Self { me_id }
    }
}

#[async_trait]
impl Module for SecurityModule {
    fn name(&self) -> &str {
        "security"
    }

    fn commands(&self) -> Vec<Command> {
        vec![
            Command::new(
                "blacklistchat",
                "Игнорировать команды и автоответы в чате",
                ".blacklistchat [chat_id]",
            ),
            Command::new(
                "blockchat",
                "Игнорировать команды и автоответы в текущем чате",
                ".blockchat [chat_id]",
            ),
            Command::new(
                "unblacklistchat",
                "Убрать чат из блок-листа",
                ".unblacklistchat [chat_id]",
            ),
            Command::new(
                "unblockchat",
                "Убрать чат из блок-листа",
                ".unblockchat [chat_id]",
            ),
            Command::new(
                "blacklistuser",
                "Игнорировать автоответы пользователю",
                ".blacklistuser <user_id>",
            ),
            Command::new(
                "blockuser",
                "Добавить пользователя в блок-лист",
                ".blockuser <user_id>",
            ),
            Command::new(
                "unblacklistuser",
                "Убрать пользователя из блок-листа",
                ".unblacklistuser <user_id>",
            ),
            Command::new(
                "unblockuser",
                "Убрать пользователя из блок-листа",
                ".unblockuser <user_id>",
            ),
            Command::new(
                "blacklist",
                "Показать заблокированные чаты и пользователей",
                ".blacklist",
            ),
        ]
    }

    async fn handle(&self, cmd: &str, ctx: Context) -> Result<()> {
        match cmd {
            "blacklistchat" | "blockchat" => change_chat(&ctx, true).await,
            "unblacklistchat" | "unblockchat" => change_chat(&ctx, false).await,
            "blacklistuser" | "blockuser" => change_user(&ctx, self.me_id, true).await,
            "unblacklistuser" | "unblockuser" => change_user(&ctx, self.me_id, false).await,
            "blacklist" => show_blacklist(&ctx).await,
            _ => Ok(()),
        }
    }
}

pub fn is_chat_blocked(storage: &crate::storage::Storage, chat_id: i64) -> bool {
    storage
        .get::<Vec<i64>>(BLOCKED_CHATS)
        .is_some_and(|ids| ids.contains(&chat_id))
}

pub fn is_user_blocked(storage: &crate::storage::Storage, user_id: i64) -> bool {
    storage
        .get::<Vec<i64>>(BLOCKED_USERS)
        .is_some_and(|ids| ids.contains(&user_id))
}

pub fn is_message_blocked(storage: &crate::storage::Storage, message: &Message) -> bool {
    if is_chat_blocked(storage, message.chat_id().bot_api_dialog_id()) {
        return true;
    }

    message.sender().is_some_and(|sender| match sender {
        Chat::User(user) => is_user_blocked(storage, user.bare_id()),
        Chat::Group(_) | Chat::Channel(_) => false,
    })
}

async fn change_chat(ctx: &Context, block: bool) -> Result<()> {
    let chat_id =
        match ctx.args.first() {
            Some(raw) => match raw.parse::<i64>() {
                Ok(id) if id != 0 => id,
                _ => return ctx
                    .edit("❌ Укажи корректный chat_id или не передавай аргумент для текущего чата")
                    .await,
            },
            None => ctx.message.chat_id().bot_api_dialog_id(),
        };

    let changed = if block {
        ctx.storage.add_i64_to_list(BLOCKED_CHATS, chat_id)?
    } else {
        ctx.storage.remove_i64_from_list(BLOCKED_CHATS, chat_id)?
    };
    let action = if block {
        "добавлен в блок-лист"
    } else {
        "убран из блок-листа"
    };
    let status = if changed { "✅" } else { "ℹ️ Уже" };
    ctx.edit(format!("{status} Чат <code>{chat_id}</code> {action}"))
        .await
}

async fn change_user(ctx: &Context, me_id: i64, block: bool) -> Result<()> {
    let user_id = match ctx.args.first().and_then(|raw| raw.parse::<i64>().ok()) {
        Some(id) if id > 0 => id,
        _ => {
            return ctx
                .edit("Использование: <code>.blockuser &lt;положительный user_id&gt;</code>")
                .await
        }
    };
    if block && user_id == me_id {
        return ctx
            .edit("❌ Нельзя добавить собственный аккаунт в блок-лист")
            .await;
    }

    let changed = if block {
        ctx.storage.add_i64_to_list(BLOCKED_USERS, user_id)?
    } else {
        ctx.storage.remove_i64_from_list(BLOCKED_USERS, user_id)?
    };
    let action = if block {
        "добавлен в блок-лист"
    } else {
        "убран из блок-листа"
    };
    let status = if changed { "✅" } else { "ℹ️ Уже" };
    ctx.edit(format!(
        "{status} Пользователь <code>{user_id}</code> {action}"
    ))
    .await
}

async fn show_blacklist(ctx: &Context) -> Result<()> {
    let chats = ctx
        .storage
        .get::<Vec<i64>>(BLOCKED_CHATS)
        .unwrap_or_default();
    let users = ctx
        .storage
        .get::<Vec<i64>>(BLOCKED_USERS)
        .unwrap_or_default();
    let chat_list = if chats.is_empty() {
        "нет".to_owned()
    } else {
        chats
            .iter()
            .map(|id| format!("<code>{id}</code>"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let user_list = if users.is_empty() {
        "нет".to_owned()
    } else {
        users
            .iter()
            .map(|id| format!("<code>{id}</code>"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    ctx.edit(format!(
        "🛡 <b>Блок-лист</b>\nЧаты: {chat_list}\nПользователи: {user_list}"
    ))
    .await
}

#[cfg(test)]
mod tests {
    use super::{is_chat_blocked, is_user_blocked, BLOCKED_CHATS, BLOCKED_USERS};
    use crate::storage::Storage;

    #[test]
    fn blacklist_checks_persisted_ids() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::load(dir.path().join("storage.json")).unwrap();
        storage.add_i64_to_list(BLOCKED_CHATS, -100123).unwrap();
        storage.add_i64_to_list(BLOCKED_USERS, 123).unwrap();

        assert!(is_chat_blocked(&storage, -100123));
        assert!(!is_chat_blocked(&storage, -100124));
        assert!(is_user_blocked(&storage, 123));
        assert!(!is_user_blocked(&storage, 124));
    }
}
