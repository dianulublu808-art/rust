use super::{Command, Context, Module};
use anyhow::Result;
use async_trait::async_trait;
use std::time::Instant;

pub struct PingModule;

#[async_trait]
impl Module for PingModule {
    fn name(&self) -> &'static str { "ping" }

    fn commands(&self) -> &[Command] {
        &[Command {
            name: "ping",
            description: "Проверка времени отклика",
            usage: ".ping",
        }]
    }

    async fn handle(&self, _cmd: &str, ctx: Context) -> Result<()> {
        ctx.edit("🏓 Measuring...").await?;
        let start = Instant::now();
        ctx.edit("🏓 Pong!").await?;
        let ms = start.elapsed().as_millis();
        ctx.edit(format!("🏓 Pong! `{ms}ms`")).await
    }
}
