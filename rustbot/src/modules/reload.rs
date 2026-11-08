use super::{Command, Context, Module, Registry};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

/// `.reload` — перечитать каталог плагинов без перезапуска бота.
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
}

#[async_trait]
impl Module for ReloadModule {
    fn name(&self) -> &str {
        "reload"
    }

    fn commands(&self) -> Vec<Command> {
        vec![Command::new(
            "reload",
            "Перезагрузить WASM-плагины без перезапуска",
            ".reload",
        )]
    }

    async fn handle(&self, _cmd: &str, ctx: Context) -> Result<()> {
        let new_plugins = match crate::plugins::load_plugins(&self.plugin_dir) {
            Ok(p) => p,
            Err(e) => {
                return ctx
                    .edit(format!("❌ Ошибка загрузки плагинов:\n<code>{e:#}</code>"))
                    .await;
            }
        };
        let count = new_plugins.len();

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

        match result {
            Ok(()) => {
                ctx.edit(format!("🔄 Плагины перезагружены: <b>{count}</b>"))
                    .await
            }
            Err(e) => {
                ctx.edit(format!("❌ Конфликт команд, старые плагины оставлены:\n<code>{e:#}</code>"))
                    .await
            }
        }
    }
}
