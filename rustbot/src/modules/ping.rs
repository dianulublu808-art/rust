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
        ctx.edit("🏓").await?;
        let start = Instant::now();
        let ms = {
            ctx.edit("🏓 ...").await?;
            start.elapsed().as_millis()
        };
        ctx.edit(format!("🏓 <b>Pong!</b> <code>{ms}ms</code>"))
            .await
    }
}
