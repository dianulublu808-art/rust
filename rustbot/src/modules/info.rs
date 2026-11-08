use super::{Command, Context, Module};
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

async fn alive(ctx: &Context) -> Result<()> {
    let me = ctx.client.get_me().await?;
    let uname = me
        .username()
        .map(|u| format!("@{u}"))
        .unwrap_or_else(|| format!("id:{}", me.bare_id()));
    ctx.edit(format!(
        "✅ <b>RustBot работает</b>\n\
         👤 {uname}\n\
         🦀 Написан на Rust",
    ))
    .await
}

async fn info_card(ctx: &Context) -> Result<()> {
    let start = Instant::now();
    ctx.edit("⏳").await?;
    let ping_ms = start.elapsed().as_millis();

    let me = ctx.client.get_me().await?;
    let uname = me
        .username()
        .map(|u| format!("@{u}"))
        .unwrap_or_else(|| "(нет)".to_owned());

    let (os, cpu, ram_used, ram_total, uptime_str) = tokio::task::spawn_blocking(|| {
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
        let ram_used = sys.used_memory();
        let ram_total = sys.total_memory();

        let secs = System::uptime();
        let uptime = fmt_uptime(secs);

        (os, cpu, ram_used, ram_total, uptime)
    })
    .await?;

    let ram_pct = if ram_total > 0 {
        ram_used * 100 / ram_total
    } else {
        0
    };
    let ram_used_fmt = fmt_bytes(ram_used);
    let ram_total_fmt = fmt_bytes(ram_total);

    ctx.edit(format!(
        "🤖 <b>RustBot</b> — Telegram Userbot на Rust\n\
         ━━━━━━━━━━━━━━━━━━━━━━━━\n\
         👤 <b>Аккаунт:</b> {uname} (<code>{me_id}</code>)\n\
         🏓 <b>Пинг:</b> <code>{ping_ms}ms</code>\n\
         ⏰ <b>Аптайм:</b> {uptime_str}\n\
         \n\
         🖥 <b>Система:</b> {os}\n\
         ⚙️ <b>CPU:</b> {cpu}\n\
         🧠 <b>RAM:</b> {ram_used_fmt} / {ram_total_fmt} ({ram_pct}%)\n\
         \n\
         🦀 <b>Язык:</b> Rust {rust_ver}\n\
         📝 <b>Префикс:</b> <code>{prefix}</code>\n\
         📚 <b>Команды:</b> .help",
        uname = uname,
        me_id = me.bare_id(),
        ping_ms = ping_ms,
        uptime_str = uptime_str,
        os = os,
        cpu = cpu,
        ram_used_fmt = ram_used_fmt,
        ram_total_fmt = ram_total_fmt,
        ram_pct = ram_pct,
        rust_ver = rustc_version(),
        prefix = ctx.command_prefix(),
    ))
    .await
}

fn fmt_uptime(secs: u64) -> String {
    let d = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    match (d, h, m) {
        (0, 0, 0) => format!("{s}с"),
        (0, 0, _) => format!("{m}м {s}с"),
        (0, _, _) => format!("{h}ч {m}м"),
        _ => format!("{d}д {h}ч {m}м"),
    }
}

fn fmt_bytes(b: u64) -> String {
    const G: u64 = 1 << 30;
    const M: u64 = 1 << 20;
    const K: u64 = 1 << 10;
    if b >= G {
        format!("{:.1}GB", b as f64 / G as f64)
    } else if b >= M {
        format!("{:.0}MB", b as f64 / M as f64)
    } else if b >= K {
        format!("{:.0}KB", b as f64 / K as f64)
    } else {
        format!("{b}B")
    }
}

fn rustc_version() -> &'static str {
    // Записывается в момент компиляции через env!
    option_env!("RUSTC_VERSION").unwrap_or("stable")
}
