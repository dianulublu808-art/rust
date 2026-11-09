use super::{esc, ui, Command, Context, Module, Registry};
use anyhow::Result;
use async_trait::async_trait;

pub struct HelpModule {
    registry: Registry,
}

impl HelpModule {
    pub fn new(registry: Registry) -> Self {
        Self { registry }
    }
}

/// Иконка и русское название модуля. Неизвестные (плагины) — 🧩 и своё имя.
fn module_meta(name: &str) -> (&'static str, String) {
    let (icon, title) = match name {
        "ping" => ("🏓", "Пинг"),
        "info" => ("ℹ️", "О боте"),
        "sysinfo" => ("🖥", "Система"),
        "eval" => ("🧪", "Код"),
        "exec" => ("⌨️", "Shell"),
        "utils" => ("🧰", "Утилиты"),
        "notes" => ("📝", "Заметки"),
        "afk" => ("🌙", "AFK"),
        "settings" => ("⚙️", "Префикс"),
        "botconfig" => ("🎛", "Настройки"),
        "security" => ("🛡", "Безопасность"),
        "translate" => ("🌐", "Перевод"),
        "updater" => ("🔄", "Обновление"),
        "reload" => ("🧩", "Плагины"),
        "help" => ("📖", "Справка"),
        other => return ("🧩", esc(other)),
    };
    (icon, title.to_owned())
}

type Entry = (String, Vec<Command>);

fn usage_with_prefix(c: &Command, p: char) -> String {
    let usage = c
        .usage
        .strip_prefix('.')
        .map(|tail| format!("{p}{tail}"))
        .unwrap_or_else(|| c.usage.clone());
    esc(&usage)
}

fn details_for_command(entries: &[Entry], target: &str, p: char) -> Option<String> {
    let target = target.trim_start_matches(p).trim_start_matches('.');
    for (module, cmds) in entries {
        if let Some(c) = cmds.iter().find(|c| c.name.eq_ignore_ascii_case(target)) {
            let (icon, title) = module_meta(module);
            return Some(format!(
                "<b>{icon} {p}{}</b>\n{}",
                c.name,
                ui::quote(&[
                    format!("{}", esc(&c.description)),
                    format!("💡 <code>{}</code>", usage_with_prefix(c, p)),
                    format!("🗂 {title}"),
                ])
            ));
        }
    }
    None
}

fn details_for_module(entries: &[Entry], target: &str, p: char) -> Option<String> {
    let (module, cmds) = entries
        .iter()
        .find(|(m, _)| m.eq_ignore_ascii_case(target))?;
    let (icon, title) = module_meta(module);
    let rows: Vec<String> = cmds
        .iter()
        .map(|c| format!("<code>{p}{}</code> — {}", c.name, esc(&c.description)))
        .collect();
    Some(ui::card(icon, &title, &rows))
}

fn compact(entries: &[Entry], p: char) -> String {
    let total: usize = entries.iter().map(|(_, c)| c.len()).sum();
    let mut rows = Vec::new();
    for (module, cmds) in entries {
        if cmds.is_empty() {
            continue;
        }
        let (icon, title) = module_meta(module);
        let list = cmds
            .iter()
            .map(|c| format!("<code>{p}{}</code>", c.name))
            .collect::<Vec<_>>()
            .join(" ");
        rows.push(format!("{icon} <b>{title}</b>  {list}"));
    }
    format!(
        "<b>🦀 RustBot</b> · справка <code>{total}</code>\n{}\n\n<i>Подробнее: <code>{p}help команда</code> · <code>{p}help модуль</code> · <code>{p}help all</code></i>",
        ui::quote(&rows)
    )
}

fn full(entries: &[Entry], p: char) -> String {
    const LIMIT: usize = 3700;
    let mut out = format!("<b>🦀 RustBot</b> · все команды\n");
    let mut skipped = 0usize;
    for (module, cmds) in entries {
        if cmds.is_empty() {
            continue;
        }
        let (icon, title) = module_meta(module);
        let rows: Vec<String> = cmds
            .iter()
            .map(|c| format!("<code>{p}{}</code> — {}", c.name, esc(&c.description)))
            .collect();
        let block = format!("\n<b>{icon} {title}</b>\n{}\n", ui::quote(&rows));
        if out.len() + block.len() > LIMIT {
            skipped += 1;
            continue;
        }
        out.push_str(&block);
    }
    if skipped > 0 {
        out.push_str(&format!(
            "\n<i>…и ещё модулей: {skipped} — смотри <code>{p}help модуль</code></i>"
        ));
    }
    out
}

#[async_trait]
impl Module for HelpModule {
    fn name(&self) -> &str {
        "help"
    }

    fn commands(&self) -> Vec<Command> {
        vec![Command::new(
            "help",
            "Справка: список, команда, модуль или all",
            ".help [команда|модуль|all]",
        )]
    }

    async fn handle(&self, _cmd: &str, ctx: Context) -> Result<()> {
        let entries: Vec<Entry> = self
            .registry
            .read()
            .unwrap()
            .iter()
            .map(|m| (m.name().to_owned(), m.commands()))
            .collect();
        let p = ctx.command_prefix();

        let Some(target) = ctx.args.first() else {
            return ctx.edit(compact(&entries, p)).await;
        };

        if target.eq_ignore_ascii_case("all") {
            return ctx.edit(full(&entries, p)).await;
        }

        if let Some(text) = details_for_command(&entries, target, p)
            .or_else(|| details_for_module(&entries, target, p))
        {
            return ctx.edit(text).await;
        }

        ctx.edit(ui::err(&format!(
            "«{}» не найдено. Список: <code>{p}help</code>",
            esc(target)
        )))
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<Entry> {
        vec![
            (
                "ping".into(),
                vec![Command::new("ping", "Проверка <отклика>", ".ping")],
            ),
            (
                "myplugin".into(),
                vec![Command::new("hello", "Привет", ".hello [имя]")],
            ),
        ]
    }

    #[test]
    fn compact_lists_commands_and_plugin_icon() {
        let t = compact(&sample(), '.');
        assert!(t.contains("<code>.ping</code>"));
        assert!(t.contains("🧩 <b>myplugin</b>"));
    }

    #[test]
    fn details_escape_description() {
        let t = details_for_command(&sample(), "ping", '.').unwrap();
        assert!(t.contains("Проверка &lt;отклика&gt;"));
        assert!(!t.contains("<отклика>"));
    }

    #[test]
    fn module_lookup_and_prefix_swap() {
        let t = details_for_module(&sample(), "myplugin", '!').unwrap();
        assert!(t.contains("<code>!hello</code>"));
        let c = details_for_command(&sample(), "hello", '!').unwrap();
        assert!(c.contains("!hello [имя]"));
    }

    #[test]
    fn full_stays_under_limit() {
        let many: Vec<Entry> = (0..40)
            .map(|i| {
                (
                    format!("m{i}"),
                    vec![Command::new("c", "очень длинное описание команды ".repeat(3), ".c")],
                )
            })
            .collect();
        assert!(full(&many, '.').len() < 4096);
    }

    #[test]
    fn rendered_has_no_raw_tags() {
        use grammers_client::parsers::parse_html_message;
        for html in [
            compact(&sample(), '.'),
            full(&sample(), '.'),
            details_for_module(&sample(), "ping", '.').unwrap(),
            super::ui::card("🖥", "Система", &[format!("{} <b>40%</b>", super::ui::bar(40))]),
        ] {
            let (text, entities) = parse_html_message(&html);
            assert!(!text.contains("<b>") && !text.contains("<code>") && !text.contains("blockquote"), "{text}");
            assert!(!entities.is_empty());
            println!("{text}\n-----");
        }
    }
}
