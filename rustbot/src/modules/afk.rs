use super::{Command, Context, Module};
use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;

pub struct AfkModule;

#[async_trait]
impl Module for AfkModule {
    fn name(&self) -> &str {
        "afk"
    }

    fn commands(&self) -> Vec<Command> {
        vec![
            Command::new(
                "afk",
                "Уйти в AFK. При упоминании автоматически ответит",
                ".afk [причина]",
            ),
            Command::new("unafk", "Вернуться из AFK", ".unafk"),
        ]
    }

    async fn handle(&self, cmd: &str, ctx: Context) -> Result<()> {
        match cmd {
            "afk" => cmd_afk(&ctx).await,
            "unafk" => cmd_unafk(&ctx).await,
            _ => Ok(()),
        }
    }
}

async fn cmd_afk(ctx: &Context) -> Result<()> {
    let reason = ctx.args_raw.trim().to_owned();
    let now = Utc::now().timestamp();

    ctx.storage.set("afk.active", true)?;
    ctx.storage.set("afk.reason", &reason)?;
    ctx.storage.set("afk.since", now)?;

    let msg = if reason.is_empty() {
        "🌙 <b>AFK включён</b>".to_owned()
    } else {
        format!(
            "🌙 <b>AFK включён</b>\n{}",
            super::ui::quote(&[format!("💬 {}", super::esc(&reason))])
        )
    };
    ctx.edit(msg).await
}

async fn cmd_unafk(ctx: &Context) -> Result<()> {
    let was_afk = ctx.storage.get::<bool>("afk.active").unwrap_or(false);
    if !was_afk {
        return ctx.edit("ℹ️ Ты и так не в AFK").await;
    }

    let since = ctx.storage.get::<i64>("afk.since").unwrap_or(0);
    let duration = if since > 0 {
        let secs = (Utc::now().timestamp() - since).max(0) as u64;
        super::ui::duration(secs)
    } else {
        String::new()
    };

    ctx.storage.del("afk.active")?;
    ctx.storage.del("afk.reason")?;
    ctx.storage.del("afk.since")?;

    let text = if duration.is_empty() {
        "☀️ <b>С возвращением!</b>".to_owned()
    } else {
        format!(
            "☀️ <b>С возвращением!</b>\n{}",
            super::ui::quote(&[format!("🌙 Был в AFK: {duration}")])
        )
    };

    ctx.edit(text).await
}
