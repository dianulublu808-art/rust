use super::{Command, Context, Module};
use anyhow::Result;
use async_trait::async_trait;

/// Вспомогательные команды: id, me, delete
pub struct RawModule;

#[async_trait]
impl Module for RawModule {
    fn name(&self) -> &'static str { "utils" }

    fn commands(&self) -> &[Command] {
        &[
            Command {
                name: "id",
                description: "Показать ID текущего чата и отправителя",
                usage: ".id",
            },
            Command {
                name: "me",
                description: "Показать информацию о себе",
                usage: ".me",
            },
            Command {
                name: "d",
                description: "Удалить это сообщение (или ответ)",
                usage: ".d",
            },
            Command {
                name: "cat",
                description: "Показать текст ответного сообщения",
                usage: ".cat (в ответ на сообщение)",
            },
        ]
    }

    async fn handle(&self, cmd: &str, ctx: Context) -> Result<()> {
        match cmd {
            "id"  => handle_id(&ctx).await,
            "me"  => handle_me(&ctx).await,
            "d"   => handle_delete(&ctx).await,
            "cat" => handle_cat(&ctx).await,
            _     => Ok(()),
        }
    }
}

async fn handle_id(ctx: &Context) -> Result<()> {
    use grammers_client::types::Chat;

    let chat = ctx.message.chat();
    let chat_id = match &chat {
        Chat::User(u)    => format!("User `{}`", u.id()),
        Chat::Group(g)   => format!("Group `{}`", g.id()),
        Chat::Channel(c) => format!("Channel `{}`", c.id()),
    };

    let sender_part = ctx
        .message
        .sender()
        .map(|s| match s {
            Chat::User(u) => format!("\n👤 **Отправитель:** `{}`", u.id()),
            _             => String::new(),
        })
        .unwrap_or_default();

    ctx.edit(format!("🆔 **Чат:** {chat_id}{sender_part}")).await
}

async fn handle_me(ctx: &Context) -> Result<()> {
    let me = ctx.client.get_me().await?;
    let username = me
        .username()
        .map(|u| format!("@{u}"))
        .unwrap_or_else(|| "(нет)".to_owned());

    ctx.edit(format!(
        "👤 **Ты**\n\
         🆔 ID: `{}`\n\
         📛 Имя: `{}`\n\
         🔖 Username: {}",
        me.id(),
        me.full_name(),
        username,
    ))
    .await
}

async fn handle_delete(ctx: &Context) -> Result<()> {
    // Пытаемся удалить сообщение, на которое ответили
    if let Some(reply) = ctx.message.reply_to_message_id() {
        ctx.client
            .delete_messages(ctx.message.chat(), &[reply])
            .await?;
    }
    // Удаляем само сообщение с командой
    ctx.client
        .delete_messages(ctx.message.chat(), &[ctx.message.id()])
        .await?;
    Ok(())
}

async fn handle_cat(ctx: &Context) -> Result<()> {
    let reply_id = match ctx.message.reply_to_message_id() {
        Some(id) => id,
        None => return ctx.edit("❌ Ответь на сообщение командой `.cat`").await,
    };

    // Получаем сообщение по ID
    let mut messages = ctx
        .client
        .get_messages_by_id(ctx.message.chat(), &[reply_id])
        .await?;

    let text = messages
        .pop()
        .flatten()
        .map(|m| m.text().to_owned())
        .unwrap_or_else(|| "(пусто или медиа)".to_owned());

    ctx.edit(format!("📄 **Текст сообщения:**\n{text}")).await
}
