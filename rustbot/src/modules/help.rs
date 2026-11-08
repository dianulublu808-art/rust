use super::{Command, Context, Module};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

pub struct HelpModule {
    entries: Vec<(String, Vec<Command>)>,
}

impl HelpModule {
    pub fn new(modules: &[Arc<dyn Module>]) -> Self {
        let entries = modules
            .iter()
            .map(|m| (m.name().to_owned(), m.commands()))
            .collect();
        Self { entries }
    }
}

#[async_trait]
impl Module for HelpModule {
    fn name(&self) -> &str {
        "help"
    }

    fn commands(&self) -> Vec<Command> {
        vec![Command::new(
            "help",
            "Список всех команд или справка по команде",
            ".help [команда]",
        )]
    }

    async fn handle(&self, _cmd: &str, ctx: Context) -> Result<()> {
        if let Some(target) = ctx.args.first() {
            // .help <команда>
            for (module, cmds) in &self.entries {
                if let Some(c) = cmds.iter().find(|c| c.name.eq_ignore_ascii_case(target)) {
                    return ctx
                        .edit(format!(
                            "📖 <b>{}</b> — <i>{}</i>\n\
                         🗂 Модуль: <code>{module}</code>\n\
                         💡 Использование: <code>{}</code>",
                            c.name, c.description, c.usage,
                        ))
                        .await;
                }
            }
            return ctx
                .edit(format!("❌ Команда <code>{target}</code> не найдена"))
                .await;
        }

        // .help — полный список
        let p = ctx.config.prefix_char();
        let mut lines = vec![format!(
            "📚 <b>RustBot</b> — команды (префикс: <code>{p}</code>)\n"
        )];

        for (module, cmds) in &self.entries {
            if cmds.is_empty() {
                continue;
            }
            lines.push(format!("<b>[{module}]</b>"));
            for c in cmds {
                lines.push(format!("  <code>{p}{}</code> — {}", c.name, c.description));
            }
            lines.push(String::new());
        }

        ctx.edit(lines.join("\n")).await
    }
}
