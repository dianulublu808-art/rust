use super::{Command, Context, Module};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

pub struct HelpModule {
    /// Список всех команд со всех модулей (собирается при построении)
    entries: Vec<(String, Vec<Command>)>,
}

impl HelpModule {
    pub fn new(modules: &[Arc<dyn Module>]) -> Self {
        let entries = modules
            .iter()
            .map(|m| (m.name().to_owned(), m.commands().to_vec()))
            .collect();
        Self { entries }
    }
}

#[async_trait]
impl Module for HelpModule {
    fn name(&self) -> &'static str { "help" }

    fn commands(&self) -> &[Command] {
        &[Command {
            name: "help",
            description: "Список всех команд",
            usage: ".help [команда]",
        }]
    }

    async fn handle(&self, _cmd: &str, ctx: Context) -> Result<()> {
        // .help <команда> — детальная справка
        if let Some(target) = ctx.args.first() {
            for (module_name, cmds) in &self.entries {
                if let Some(c) = cmds.iter().find(|c| c.name.eq_ignore_ascii_case(target)) {
                    let text = format!(
                        "📖 **{}** (модуль `{}`)\n\n{}\n\nИспользование: `{}`",
                        c.name, module_name, c.description, c.usage
                    );
                    return ctx.edit(&text).await;
                }
            }
            return ctx
                .edit(format!("❌ Команда `{target}` не найдена"))
                .await;
        }

        // .help — полный список
        let prefix = ctx.config.prefix_char();
        let mut lines = vec!["📚 **Список команд**\n".to_owned()];

        for (module_name, cmds) in &self.entries {
            if cmds.is_empty() {
                continue;
            }
            lines.push(format!("**[{module_name}]**"));
            for c in cmds {
                lines.push(format!("  `{prefix}{}` — {}", c.name, c.description));
            }
            lines.push(String::new());
        }

        ctx.edit(lines.join("\n")).await
    }
}
