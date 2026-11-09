use super::{Command, Context, Module};
use anyhow::Result;
use async_trait::async_trait;
use grammers_client::types::Chat;

pub struct RawModule;

#[async_trait]
impl Module for RawModule {
    fn name(&self) -> &str {
        "utils"
    }

    fn commands(&self) -> Vec<Command> {
        vec![
            Command::new("id", "ID чата и отправителя", ".id"),
            Command::new("me", "Информация о себе", ".me"),
            Command::new("d", "Удалить сообщение (+ ответ)", ".d"),
            Command::new("cat", "Показать текст ответа", ".cat"),
        ]
    }

    async fn handle(&self, cmd: &str, ctx: Context) -> Result<()> {
        match cmd {
            "id" => handle_id(&ctx).await,
            "me" => handle_me(&ctx).await,
            "d" => handle_delete(&ctx).await,
            "cat" => handle_cat(&ctx).await,
            _ => Ok(()),
        }
    }
}

fn chat_label(chat: &Chat) -> String {
    match chat {
        Chat::User(u) => format!("👤 Личный чат <code>{}</code>", u.bare_id()),
        Chat::Group(g) => format!("👥 Группа <code>{}</code>", g.id()),
        Chat::Channel(c) => format!("📢 Канал <code>{}</code>", c.bare_id()),
    }
}

async fn handle_id(ctx: &Context) -> Result<()> {
    let chat_part = match ctx.message.chat() {
        Ok(chat) => chat_label(chat),
        Err(pr) => format!("💬 Чат <code>{}</code>", pr.id),
    };

    let mut rows = vec![chat_part];
    if let Some(Chat::User(u)) = ctx.message.sender() {
        rows.push(format!("🙋 Отправитель <code>{}</code>", u.bare_id()));
    }

    ctx.edit(super::ui::card("🆔", "Идентификаторы", &rows)).await
}

async fn handle_me(ctx: &Context) -> Result<()> {
    let me = ctx.client.get_me().await?;
    let uname = me
        .username()
        .map(|u| format!("@{u}"))
        .unwrap_or_else(|| "нет".to_owned());

    ctx.edit(super::ui::card(
        "👤",
        "Профиль",
        &[
            format!("📛 {}", super::esc(&me.full_name())),
            format!("🔖 {uname}"),
            format!("🆔 <code>{}</code>", me.bare_id()),
        ],
    ))
    .await
}

async fn handle_delete(ctx: &Context) -> Result<()> {
    let peer = ctx.peer_ref();
    if let Some(reply_id) = ctx.message.reply_to_message_id() {
        ctx.client
            .delete_messages(peer, &[reply_id])
            .await?;
    }
    ctx.client
        .delete_messages(peer, &[ctx.message.id()])
        .await?;
    Ok(())
}

async fn handle_cat(ctx: &Context) -> Result<()> {
    let reply_id = match ctx.message.reply_to_message_id() {
        Some(id) => id,
        None => return ctx.edit("❌ Ответь на сообщение командой <code>.cat</code>").await,
    };

    let msgs = ctx
        .client
        .get_messages_by_id(ctx.peer_ref(), &[reply_id])
        .await?;
    let text = msgs
        .into_iter()
        .flatten()
        .next()
        .map(|m| m.text().to_owned())
        .unwrap_or_else(|| "(пусто или медиа)".to_owned());

    ctx.edit(format!("📄 {}", super::esc(&text))).await
}
