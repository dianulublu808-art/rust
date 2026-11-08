use super::{Command, Context, Module, Registry};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

/// `.reload` — перечитать каталог плагинов без перезапуска бота,
/// `.plugins` — показать загруженные плагины.
pub struct ReloadModule {
    registry: Registry,
    plugin_dir: String,
    /// Сколько первых модулей в реестре — встроенные (всё после них — плагины).
    core_count: usize,
}

impl ReloadModule {
    pub fn new(registry: Registry, plugin_dir: String, core_count: usize) -> Self {
        Self {
            registry,
            plugin_dir,
            core_count,
        }
    }

    fn plugin_names(&self) -> Vec<String> {
        let g = self.registry.read().unwrap();
        let names: Vec<String> = g
            .iter()
            .skip(self.core_count)
            .map(|m| m.name().to_owned())
            .collect();
        names
    }

    async fn cmd_plugins(&self, ctx: &Context) -> Result<()> {
        let plugins: Vec<(String, Vec<Command>)> = {
            let g = self.registry.read().unwrap();
            let list: Vec<(String, Vec<Command>)> = g
                .iter()
                .skip(self.core_count)
                .map(|m| (m.name().to_owned(), m.commands()))
                .collect();
            list
        };

        if plugins.is_empty() {
            return ctx
                .edit(format!(
                    "🧩 Плагинов нет. Положи манифест и .wasm в <code>{}</code> и вызови <code>{}reload</code>",
                    self.plugin_dir,
                    ctx.command_prefix()
                ))
                .await;
        }

        let p = ctx.command_prefix();
        let mut lines = vec![format!("🧩 <b>Плагины</b> ({})\n", plugins.len())];
        for (name, cmds) in plugins {
            let list = cmds
                .iter()
                .map(|c| format!("<code>{p}{}</code>", c.name))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("• <b>{name}</b>: {list}"));
        }
        ctx.edit(lines.join("\n")).await
    }

    async fn cmd_reload(&self, ctx: &Context) -> Result<()> {
        let old_names = self.plugin_names();

        let new_plugins = match crate::plugins::load_plugins(&self.plugin_dir) {
            Ok(p) => p,
            Err(e) => {
                return ctx
                    .edit(format!("❌ Ошибка загрузки плагинов:\n<code>{e:#}</code>"))
                    .await;
            }
        };
        let new_names: Vec<String> = new_plugins.iter().map(|m| m.name().to_owned()).collect();

        let result: Result<()> = {
            let mut g = self.registry.write().unwrap();
            let keep = self.core_count.min(g.len());
            let mut candidate: Vec<Arc<dyn Module>> = g[..keep].to_vec();
            candidate.extend(new_plugins);
            let checked = crate::dispatcher::check_duplicates(&candidate);
            if checked.is_ok() {
                *g = candidate;
            }
            checked
        };

        if let Err(e) = result {
            return ctx
                .edit(format!(
                    "❌ Конфликт команд, старые плагины оставлены:\n<code>{e:#}</code>"
                ))
                .await;
        }

        let added: Vec<&String> = new_names.iter().filter(|n| !old_names.contains(n)).collect();
        let removed: Vec<&String> = old_names.iter().filter(|n| !new_names.contains(n)).collect();

        let mut text = format!("🔄 Плагины перезагружены: <b>{}</b>", new_names.len());
        if !added.is_empty() {
            text.push_str(&format!(
                "\n➕ {}",
                added.iter().map(|n| n.as_str()).collect::<Vec<_>>().join(", ")
            ));
        }
        if !removed.is_empty() {
            text.push_str(&format!(
                "\n➖ {}",
                removed.iter().map(|n| n.as_str()).collect::<Vec<_>>().join(", ")
            ));
        }
        ctx.edit(text).await
    }
}

#[async_trait]
impl Module for ReloadModule {
    fn name(&self) -> &str {
        "reload"
    }

    fn commands(&self) -> Vec<Command> {
        vec![
            Command::new(
                "reload",
                "Перезагрузить WASM-плагины без перезапуска",
                ".reload",
            ),
            Command::new("plugins", "Список загруженных плагинов", ".plugins"),
        ]
    }

    async fn handle(&self, cmd: &str, ctx: Context) -> Result<()> {
        match cmd {
            "plugins" => self.cmd_plugins(&ctx).await,
            _ => self.cmd_reload(&ctx).await,
        }
    }
}
