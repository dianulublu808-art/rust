use super::{Command, Context, Module};
use anyhow::Result;
use async_trait::async_trait;
use chrono::Local;
use sysinfo::{Disks, System};

pub struct SysinfoModule;

#[async_trait]
impl Module for SysinfoModule {
    fn name(&self) -> &'static str { "sysinfo" }

    fn commands(&self) -> &[Command] {
        &[
            Command { name: "sysinfo", description: "CPU, RAM, диск", usage: ".sysinfo" },
            Command { name: "uptime",  description: "Аптайм системы",  usage: ".uptime"  },
        ]
    }

    async fn handle(&self, cmd: &str, ctx: Context) -> Result<()> {
        if cmd == "uptime" {
            let secs = System::uptime();
            return ctx.edit(format!("⏰ <b>Аптайм:</b> {}", fmt_uptime(secs))).await;
        }

        ctx.edit("⏳ Собираю данные...").await?;

        let text = tokio::task::spawn_blocking(|| {
            let mut sys = System::new_all();
            sys.refresh_all();

            let total_mem  = sys.total_memory();
            let used_mem   = sys.used_memory();
            let total_swap = sys.total_swap();
            let used_swap  = sys.used_swap();
            let cpu_usage  = sys.global_cpu_info().cpu_usage();
            let cpu_count  = sys.cpus().len();
            let cpu_brand  = sys.cpus().first()
                .map(|c| c.brand().to_owned())
                .unwrap_or_else(|| "Unknown".into());

            let os_name    = System::name().unwrap_or_else(|| "?".into());
            let os_version = System::os_version().unwrap_or_else(|| "?".into());
            let kernel     = System::kernel_version().unwrap_or_else(|| "?".into());
            let hostname   = System::host_name().unwrap_or_else(|| "?".into());
            let uptime     = fmt_uptime(System::uptime());
            let now        = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

            let disks = Disks::new_with_refreshed_list();
            let disk_lines: Vec<String> = disks.iter().map(|d| {
                let total = d.total_space();
                let avail = d.available_space();
                let used  = total.saturating_sub(avail);
                let pct   = if total > 0 { used * 100 / total } else { 0 };
                format!(
                    "  💾 <code>{}</code> — {} / {} ({}%)",
                    d.name().to_string_lossy(),
                    fmt_bytes(used),
                    fmt_bytes(total),
                    pct,
                )
            }).collect();
            let ram_used_fmt = fmt_bytes(used_mem);
            let ram_total_fmt = fmt_bytes(total_mem);
            let ram_pct = if total_mem > 0 { used_mem * 100 / total_mem } else { 0 };
            let swap_used_fmt = fmt_bytes(used_swap);
            let swap_total_fmt = fmt_bytes(total_swap);
            let disk_text = if disk_lines.is_empty() {
                "  (нет данных)".to_owned()
            } else {
                disk_lines.join("\n")
            };

            format!(
                "🖥 <b>Системная информация</b>\n\
                 ━━━━━━━━━━━━━━━━━━━━\n\
                 🏷 <b>Хост:</b> <code>{hostname}</code>\n\
                 🖥 <b>ОС:</b> {os_name} {os_version} (ядро {kernel})\n\
                 ⏰ <b>Аптайм:</b> {uptime}\n\
                 🕒 <b>Время:</b> <code>{now}</code>\n\
                 \n\
                 ⚙️ <b>CPU:</b> {cpu_brand} × {cpu_count}\n\
                 📊 <b>Нагрузка:</b> <code>{cpu_usage:.1}%</code>\n\
                 \n\
                 🧠 <b>RAM:</b> {ram_used_fmt} / {ram_total_fmt} ({ram_pct}%)\n\
                 🔵 <b>Swap:</b> {swap_used_fmt} / {swap_total_fmt}\n\
                 \n\
                 💾 <b>Диски:</b>\n{disk_text}",
                hostname = hostname,
                os_name = os_name, os_version = os_version, kernel = kernel,
                uptime = uptime, now = now,
                cpu_brand = cpu_brand, cpu_count = cpu_count, cpu_usage = cpu_usage,
                ram_used_fmt = ram_used_fmt,
                ram_total_fmt = ram_total_fmt,
                ram_pct = ram_pct,
                swap_used_fmt = swap_used_fmt,
                swap_total_fmt = swap_total_fmt,
                disk_text = disk_text,
            )
        }).await?;

        ctx.edit(&text).await
    }
}

fn fmt_uptime(secs: u64) -> String {
    let d = secs / 86400; let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60; let s = secs % 60;
    match (d, h, m) {
        (0, 0, 0) => format!("{s}с"),
        (0, 0, _) => format!("{m}м {s}с"),
        (0, _, _) => format!("{h}ч {m}м {s}с"),
        _          => format!("{d}д {h}ч {m}м {s}с"),
    }
}

fn fmt_bytes(b: u64) -> String {
    const G: u64 = 1<<30; const M: u64 = 1<<20; const K: u64 = 1<<10;
    if b >= G { format!("{:.2}GB", b as f64/G as f64) }
    else if b >= M { format!("{:.1}MB", b as f64/M as f64) }
    else if b >= K { format!("{:.0}KB", b as f64/K as f64) }
    else { format!("{b}B") }
}
