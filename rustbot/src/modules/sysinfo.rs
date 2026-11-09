use super::{esc, ui, Command, Context, Module};
use anyhow::Result;
use async_trait::async_trait;
use chrono::Local;
use sysinfo::{Disks, System};

pub struct SysinfoModule;

#[async_trait]
impl Module for SysinfoModule {
    fn name(&self) -> &str {
        "sysinfo"
    }

    fn commands(&self) -> Vec<Command> {
        vec![
            Command::new("sysinfo", "CPU, RAM, диск", ".sysinfo"),
            Command::new("uptime", "Аптайм системы", ".uptime"),
        ]
    }

    async fn handle(&self, cmd: &str, ctx: Context) -> Result<()> {
        if cmd == "uptime" {
            let sys_up = System::uptime();
            return ctx
                .edit(ui::card(
                    "⏱",
                    "Аптайм",
                    &[
                        format!("🖥 Система <b>{}</b>", ui::duration(sys_up)),
                        format!("🦀 Бот <b>{}</b>", ui::duration(ui::process_uptime())),
                    ],
                ))
                .await;
        }

        ctx.edit("⏳ Собираю данные...").await?;

        let text = tokio::task::spawn_blocking(|| {
            let mut sys = System::new_all();
            sys.refresh_all();
            // для корректной загрузки CPU нужен второй замер
            std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
            sys.refresh_cpu();

            let total_mem = sys.total_memory();
            let used_mem = sys.used_memory();
            let total_swap = sys.total_swap();
            let used_swap = sys.used_swap();
            let cpu_usage = sys.global_cpu_info().cpu_usage() as u64;
            let cpu_count = sys.cpus().len();
            let cpu_brand = sys
                .cpus()
                .first()
                .map(|c| c.brand().to_owned())
                .unwrap_or_else(|| "Unknown".into());

            let os = format!(
                "{} {}",
                System::name().unwrap_or_else(|| "?".into()),
                System::os_version().unwrap_or_else(|| "?".into())
            );
            let kernel = System::kernel_version().unwrap_or_else(|| "?".into());
            let hostname = System::host_name().unwrap_or_else(|| "?".into());
            let now = Local::now().format("%d.%m.%Y %H:%M:%S").to_string();

            let pct = |used: u64, total: u64| (used * 100).checked_div(total).unwrap_or(0);

            let head = format!("<b>🖥 {}</b>", esc(&hostname));
            let about = ui::quote(&[
                format!("🐧 {} · ядро {}", esc(&os), esc(&kernel)),
                format!("⚙️ {} × {cpu_count}", esc(&cpu_brand)),
                format!("⏱ Аптайм <b>{}</b>", ui::duration(System::uptime())),
                format!("🕒 {now}"),
            ]);

            let mut res = vec![
                format!(
                    "<b>CPU</b>   {} <b>{cpu_usage}%</b>",
                    ui::bar(cpu_usage)
                ),
                format!(
                    "<b>RAM</b>   {} <b>{}%</b> · {} / {}",
                    ui::bar(pct(used_mem, total_mem)),
                    pct(used_mem, total_mem),
                    ui::bytes(used_mem),
                    ui::bytes(total_mem)
                ),
            ];
            if total_swap > 0 {
                res.push(format!(
                    "<b>SWAP</b>  {} <b>{}%</b> · {} / {}",
                    ui::bar(pct(used_swap, total_swap)),
                    pct(used_swap, total_swap),
                    ui::bytes(used_swap),
                    ui::bytes(total_swap)
                ));
            }
            let resources = ui::card("📊", "Ресурсы", &res);

            let disks = Disks::new_with_refreshed_list();
            let mut seen = std::collections::HashSet::new();
            let mut disk_rows: Vec<String> = Vec::new();
            for d in disks.iter() {
                let total = d.total_space();
                if total == 0 {
                    continue;
                }
                let mount = d.mount_point().to_string_lossy().to_string();
                if !seen.insert(mount.clone()) {
                    continue;
                }
                let used = total.saturating_sub(d.available_space());
                let p = pct(used, total);
                disk_rows.push(format!(
                    "<code>{}</code>\n{} <b>{p}%</b> · {} / {}",
                    esc(&mount),
                    ui::bar(p),
                    ui::bytes(used),
                    ui::bytes(total)
                ));
                if disk_rows.len() >= 4 {
                    break;
                }
            }
            let disks_block = if disk_rows.is_empty() {
                String::new()
            } else {
                format!("\n\n{}", ui::card("💾", "Диски", &disk_rows))
            };

            format!("{head}\n{about}\n\n{resources}{disks_block}")
        })
        .await?;

        ctx.edit(text).await
    }
}
