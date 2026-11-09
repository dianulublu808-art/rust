use super::{Command, Context, Module};
use anyhow::Result;
use async_trait::async_trait;
use std::time::Instant;

pub struct PingModule;

#[async_trait]
impl Module for PingModule {
    fn name(&self) -> &str {
        "ping"
    }

    fn commands(&self) -> Vec<Command> {
        vec![Command::new("ping", "Проверка времени отклика", ".ping")]
    }

    async fn handle(&self, _cmd: &str, ctx: Context) -> Result<()> {
        let start = Instant::now();
        ctx.edit("🏓").await?;
        let ms = start.elapsed().as_millis();
        let quality = match ms {
            0..=150 => "🟢",
            151..=500 => "🟡",
            _ => "🔴",
        };
        ctx.edit(format!("<b>🏓 Pong</b>  {quality} <code>{ms} мс</code>"))
            .await
    }
}
