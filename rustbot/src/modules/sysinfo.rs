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
            Command {
                name: "sysinfo",
                description: "Информация о системе (CPU, RAM, диск)",
                usage: ".sysinfo",
            },
            Command {
                name: "uptime",
                description: "Время работы системы",
                usage: ".uptime",
            },
        ]
    }

    async fn handle(&self, cmd: &str, ctx: Context) -> Result<()> {
        match cmd {
            "uptime" => handle_uptime(&ctx).await,
            _        => handle_sysinfo(&ctx).await,
        }
    }
}

async fn handle_sysinfo(ctx: &Context) -> Result<()> {
    ctx.edit("⏳ Собираю данные...").await?;

    // sysinfo блокирует — отправляем в spawn_blocking
    let text = tokio::task::spawn_blocking(|| {
        let mut sys = System::new_all();
        sys.refresh_all();

        let total_mem  = sys.total_memory();  // bytes
        let used_mem   = sys.used_memory();
        let total_swap = sys.total_swap();
        let used_swap  = sys.used_swap();

        let cpu_usage = sys.global_cpu_info().cpu_usage();
        let cpu_count = sys.cpus().len();
        let cpu_brand = sys.cpus()
            .first()
            .map(|c| c.brand().to_owned())
            .unwrap_or_else(|| "Unknown".to_owned());

        let uptime_secs = System::uptime();
        let uptime_str  = format_uptime(uptime_secs);

        let os_name    = System::name().unwrap_or_else(|| "Unknown".to_owned());
        let os_version = System::os_version().unwrap_or_else(|| "?".to_owned());
        let kernel     = System::kernel_version().unwrap_or_else(|| "?".to_owned());
        let hostname   = System::host_name().unwrap_or_else(|| "?".to_owned());

        let disks  = Disks::new_with_refreshed_list();
        let disk_info: Vec<String> = disks
            .iter()
            .map(|d| {
                let total = d.total_space();
                let avail = d.available_space();
                let used  = total - avail;
                format!(
                    "  {} `{}` — {} / {} ({}%)",
                    disk_icon(d.name().to_string_lossy().as_ref()),
                    d.name().to_string_lossy(),
                    fmt_bytes(used),
                    fmt_bytes(total),
                    if total > 0 { used * 100 / total } else { 0 },
                )
            })
            .collect();

        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        format!(
            "🖥 **Системная информация**\n\
             ━━━━━━━━━━━━━━━━━━━━\n\
             🏷 **Хост:** `{hostname}`\n\
             🖥 **ОС:** `{os_name} {os_version}` (ядро `{kernel}`)\n\
             ⏰ **Аптайм:** {uptime_str}\n\
             🕒 **Время:** `{now}`\n\
             \n\
             **CPU**\n\
             🔲 `{cpu_brand}` × {cpu_count} ядер\n\
             📊 Нагрузка: `{cpu_usage:.1}%`\n\
             \n\
             **Память**\n\
             🟢 RAM: `{used_mem_fmt}` / `{total_mem_fmt}` ({ram_pct}%)\n\
             🔵 Swap: `{used_swap_fmt}` / `{total_swap_fmt}`\n\
             \n\
             **Диски**\n\
             {disk_str}",
            hostname = hostname,
            os_name = os_name,
            os_version = os_version,
            kernel = kernel,
            uptime_str = uptime_str,
            now = now,
            cpu_brand = cpu_brand,
            cpu_count = cpu_count,
            cpu_usage = cpu_usage,
            used_mem_fmt  = fmt_bytes(used_mem),
            total_mem_fmt = fmt_bytes(total_mem),
            ram_pct = if total_mem > 0 { used_mem * 100 / total_mem } else { 0 },
            used_swap_fmt  = fmt_bytes(used_swap),
            total_swap_fmt = fmt_bytes(total_swap),
            disk_str = if disk_info.is_empty() {
                "  (нет данных)".to_owned()
            } else {
                disk_info.join("\n")
            },
        )
    })
    .await?;

    ctx.edit(&text).await
}

async fn handle_uptime(ctx: &Context) -> Result<()> {
    let secs = System::uptime();
    ctx.edit(format!("⏰ Аптайм: **{}**", format_uptime(secs))).await
}

// ─── helpers ────────────────────────────────────────────────────────────────

fn format_uptime(secs: u64) -> String {
    let d = secs / 86_400;
    let h = (secs % 86_400) / 3_600;
    let m = (secs % 3_600) / 60;
    let s = secs % 60;
    match (d, h, m) {
        (0, 0, 0) => format!("{s}s"),
        (0, 0, _) => format!("{m}m {s}s"),
        (0, _, _) => format!("{h}h {m}m {s}s"),
        _          => format!("{d}d {h}h {m}m {s}s"),
    }
}

fn fmt_bytes(b: u64) -> String {
    const KIB: u64 = 1_024;
    const MIB: u64 = KIB * 1_024;
    const GIB: u64 = MIB * 1_024;
    if b >= GIB {
        format!("{:.2} GB", b as f64 / GIB as f64)
    } else if b >= MIB {
        format!("{:.1} MB", b as f64 / MIB as f64)
    } else if b >= KIB {
        format!("{:.1} KB", b as f64 / KIB as f64)
    } else {
        format!("{b} B")
    }
}

fn disk_icon(name: &str) -> &'static str {
    if name.starts_with("sd") || name.starts_with("hd") || name.starts_with("vd") {
        "💾"
    } else if name.starts_with("nvme") || name.starts_with("ssd") {
        "⚡"
    } else {
        "💿"
    }
}
