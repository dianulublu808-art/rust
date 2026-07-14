use super::{Command, Context, Module};
use anyhow::Result;
use async_trait::async_trait;
use grammers_client::peer::Peer;

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
    let chat_id = match ctx.message.peer() {
        Some(Peer::User(u))    => format!("User `{}`", u.raw.id()),
        Some(Peer::Group(g))   => format!("Group `{}`", g.id()),
        Some(Peer::Channel(c)) => format!("Channel `{}`", c.id()),
        None                   => "Unknown".to_owned(),
    };

    let sender_part = match ctx.message.sender() {
        Some(Peer::User(u)) => format!("\n👤 **Отправитель:** `{}`", u.raw.id()),
        _                   => String::new(),
    };

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
        me.raw.id(),
        me.full_name(),
        username,
    ))
    .await
}

async fn handle_delete(ctx: &Context) -> Result<()> {
    let peer_ref = ctx
        .message
        .peer_ref()
        .await
        .ok_or_else(|| anyhow::anyhow!("cannot get peer ref"))?;

    if let Some(reply) = ctx.message.reply_to_message_id() {
        ctx.client
            .delete_messages(peer_ref.clone(), &[reply])
            .await?;
    }
    ctx.client
        .delete_messages(peer_ref, &[ctx.message.id()])
        .await?;
    Ok(())
}

async fn handle_cat(ctx: &Context) -> Result<()> {
    let reply_id = match ctx.message.reply_to_message_id() {
        Some(id) => id,
        None => return ctx.edit("❌ Ответь на сообщение командой `.cat`").await,
    };

    let peer_ref = ctx
        .message
        .peer_ref()
        .await
        .ok_or_else(|| anyhow::anyhow!("cannot get peer ref"))?;

    let mut messages = ctx
        .client
        .get_messages_by_id(peer_ref, &[reply_id])
        .await?;

    let text = messages
        .pop()
        .flatten()
        .map(|m| m.text().to_owned())
        .unwrap_or_else(|| "(пусто или медиа)".to_owned());

    ctx.edit(format!("📄 **Текст сообщения:**\n{text}")).await
}
