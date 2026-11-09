use super::{esc, ui, Command, Context, Module};
use anyhow::Result;
use async_trait::async_trait;
use std::time::Instant;
use sysinfo::System;

pub struct InfoModule;

#[async_trait]
impl Module for InfoModule {
    fn name(&self) -> &str {
        "info"
    }

    fn commands(&self) -> Vec<Command> {
        vec![
            Command::new("info", "Информация о боте и системе", ".info"),
            Command::new("alive", "Проверить, работает ли бот", ".alive"),
        ]
    }

    async fn handle(&self, cmd: &str, ctx: Context) -> Result<()> {
        match cmd {
            "alive" => alive(&ctx).await,
            _ => info_card(&ctx).await,
        }
    }
}

fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

async fn account_label(ctx: &Context) -> Result<(String, i64)> {
    let me = ctx.client.get_me().await?;
    let label = match me.username() {
        Some(u) => format!("@{u}"),
        None => esc(&me.full_name()),
    };
    Ok((label, me.bare_id()))
}

async fn alive(ctx: &Context) -> Result<()> {
    let (who, _) = account_label(ctx).await?;
    ctx.edit(format!(
        "<b>🦀 RustBot</b> <code>v{}</code>\n{}",
        version(),
        ui::quote(&[
            format!("🟢 Онлайн · работает <b>{}</b>", ui::duration(ui::process_uptime())),
            format!("👤 {who}"),
        ])
    ))
    .await
}

async fn info_card(ctx: &Context) -> Result<()> {
    let start = Instant::now();
    ctx.edit("⏳").await?;
    let ping_ms = start.elapsed().as_millis();

    let (who, me_id) = account_label(ctx).await?;

    let (os, cpu, ram_used, ram_total, sys_uptime) = tokio::task::spawn_blocking(|| {
        let mut sys = System::new_all();
        sys.refresh_all();
        let os = format!(
            "{} {}",
            System::name().unwrap_or_else(|| "Linux".into()),
            System::os_version().unwrap_or_else(|| "?".into()),
        );
        let cpu = sys
            .cpus()
            .first()
            .map(|c| c.brand().to_owned())
            .unwrap_or_else(|| "Unknown".into());
        (
            esc(&os),
            esc(&cpu),
            sys.used_memory(),
            sys.total_memory(),
            System::uptime(),
        )
    })
    .await?;

    let ram_pct = (ram_used * 100).checked_div(ram_total).unwrap_or(0);

    let head = format!("<b>🦀 RustBot</b> <code>v{}</code>", version());
    let bot = ui::card(
        "🤖",
        "Бот",
        &[
            format!("👤 {who} · <code>{me_id}</code>"),
            format!("🏓 Пинг <code>{ping_ms} мс</code>"),
            format!("⏱ Работает <b>{}</b>", ui::duration(ui::process_uptime())),
            format!("⌨️ Префикс <code>{}</code>", ctx.command_prefix()),
        ],
    );
    let sys = ui::card(
        "🖥",
        "Система",
        &[
            format!("🐧 {os}"),
            format!("⚙️ {cpu}"),
            format!(
                "🧠 {} <b>{ram_pct}%</b> · {} / {}",
                ui::bar(ram_pct),
                ui::bytes(ram_used),
                ui::bytes(ram_total)
            ),
            format!("⏱ Аптайм системы <b>{}</b>", ui::duration(sys_uptime)),
        ],
    );

    ctx.edit(format!(
        "{head}\n\n{bot}\n\n{sys}\n\n<i>Команды: <code>{}help</code></i>",
        ctx.command_prefix()
    ))
    .await
}
