use super::{Command, Context, Module};
use anyhow::Result;
use async_trait::async_trait;

pub struct NotesModule;

const PREFIX: &str = "note.";

#[async_trait]
impl Module for NotesModule {
    fn name(&self) -> &'static str { "notes" }

    fn commands(&self) -> &[Command] {
        &[
            Command {
                name:        "save",
                description: "Сохранить заметку: .save <имя> <текст> (или в ответ)",
                usage:       ".save <имя> [текст]",
            },
            Command {
                name:        "get",
                description: "Получить заметку",
                usage:       ".get <имя>",
            },
            Command {
                name:        "notes",
                description: "Список всех заметок",
                usage:       ".notes",
            },
            Command {
                name:        "delnote",
                description: "Удалить заметку",
                usage:       ".delnote <имя>",
            },
        ]
    }

    async fn handle(&self, cmd: &str, ctx: Context) -> Result<()> {
        match cmd {
            "save"    => cmd_save(&ctx).await,
            "get"     => cmd_get(&ctx).await,
            "notes"   => cmd_list(&ctx).await,
            "delnote" => cmd_del(&ctx).await,
            _         => Ok(()),
        }
    }
}

async fn cmd_save(ctx: &Context) -> Result<()> {
    let name = match ctx.args.first() {
        Some(n) => n.clone(),
        None    => return ctx.edit("❌ Использование: `.save <имя> <текст>`").await,
    };

    // Текст: либо из аргументов, либо из ответного сообщения
    let text = if ctx.args.len() > 1 {
        ctx.args[1..].join(" ")
    } else {
        // Пробуем получить текст ответа
        match ctx.message.reply_to_message_id() {
            Some(rid) => {
                let msgs = ctx.client.get_messages_by_id(ctx.peer_ref(), &[rid]).await?;
                msgs.into_iter().flatten().next()
                    .map(|m| m.text().to_owned())
                    .unwrap_or_default()
            }
            None => return ctx.edit("❌ Укажи текст или ответь на сообщение").await,
        }
    };

    if text.trim().is_empty() {
        return ctx.edit("❌ Пустой текст").await;
    }

    let key = format!("{PREFIX}{name}");
    ctx.storage.set(&key, &text)?;
    ctx.edit(format!("✅ Заметка <code>{name}</code> сохранена")).await
}

async fn cmd_get(ctx: &Context) -> Result<()> {
    let name = match ctx.args.first() {
        Some(n) => n,
        None    => return ctx.edit("❌ Использование: `.get <имя>`").await,
    };

    let key = format!("{PREFIX}{name}");
    match ctx.storage.get::<String>(&key) {
        Some(text) => ctx.edit(format!("📝 <b>{name}</b>\n{text}")).await,
        None       => ctx.edit(format!("❌ Заметка <code>{name}</code> не найдена")).await,
    }
}

async fn cmd_list(ctx: &Context) -> Result<()> {
    let mut keys = ctx.storage.keys_with_prefix(PREFIX);
    keys.sort();

    if keys.is_empty() {
        return ctx.edit("📭 Заметок нет. Сохрани: `.save <имя> <текст>`").await;
    }

    let list: Vec<String> = keys.iter()
        .map(|k| format!("▫️ <code>{}</code>", &k[PREFIX.len()..]))
        .collect();

    ctx.edit(format!(
        "📚 <b>Заметки</b> ({}):\n{}",
        keys.len(),
        list.join("\n")
    )).await
}

async fn cmd_del(ctx: &Context) -> Result<()> {
    let name = match ctx.args.first() {
        Some(n) => n.clone(),
        None    => return ctx.edit("❌ Использование: `.delnote <имя>`").await,
    };

    let key = format!("{PREFIX}{name}");
    if ctx.storage.get::<String>(&key).is_none() {
        return ctx.edit(format!("❌ Заметка <code>{name}</code> не найдена")).await;
    }

    ctx.storage.del(&key)?;
    ctx.edit(format!("🗑 Заметка <code>{name}</code> удалена")).await
}
