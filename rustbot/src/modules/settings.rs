use super::{Command, Context, Module};
use anyhow::Result;
use async_trait::async_trait;

pub struct SettingsModule;

#[async_trait]
impl Module for SettingsModule {
    fn name(&self) -> &str {
        "settings"
    }

    fn commands(&self) -> Vec<Command> {
        vec![
            Command::new(
                "setprefix",
                "Изменить префикс команд",
                ".setprefix <символ|reset>",
            ),
            Command::new("prefix", "Показать текущий префикс", ".prefix"),
        ]
    }

    async fn handle(&self, cmd: &str, ctx: Context) -> Result<()> {
        match cmd {
            "prefix" => {
                ctx.edit(format!(
                    "Текущий префикс команд: <code>{}</code>",
                    ctx.command_prefix()
                ))
                .await
            }
            "setprefix" => set_prefix(&ctx).await,
            _ => Ok(()),
        }
    }
}

async fn set_prefix(ctx: &Context) -> Result<()> {
    let requested = ctx.args_raw.trim();
    if requested.eq_ignore_ascii_case("reset") {
        ctx.storage.del("settings.prefix")?;
        return ctx
            .edit(format!(
                "✅ Префикс сброшен. Сейчас используется <code>{}</code>",
                ctx.config.prefix_char()
            ))
            .await;
    }

    let mut chars = requested.chars();
    let Some(prefix) = chars.next() else {
        return ctx
            .edit("Использование: <code>.setprefix &lt;один символ|reset&gt;</code>")
            .await;
    };
    if chars.next().is_some() || !valid_prefix(prefix) {
        return ctx
            .edit("❌ Укажи ровно один знак пунктуации, например <code>!</code>")
            .await;
    }

    ctx.storage.set("settings.prefix", prefix.to_string())?;
    ctx.edit(format!(
        "✅ Префикс команд изменён на <code>{prefix}</code>"
    ))
    .await
}

fn valid_prefix(prefix: char) -> bool {
    !prefix.is_whitespace()
        && !prefix.is_control()
        && !prefix.is_alphanumeric()
        && !matches!(prefix, '<' | '>' | '&' | '"' | '\'')
}

#[cfg(test)]
mod tests {
    use super::valid_prefix;

    #[test]
    fn accepts_punctuation_prefixes() {
        assert!(valid_prefix('.'));
        assert!(valid_prefix('!'));
        assert!(valid_prefix('🦀'));
    }

    #[test]
    fn rejects_whitespace_letters_and_html_specials() {
        assert!(!valid_prefix(' '));
        assert!(!valid_prefix('a'));
        assert!(!valid_prefix('<'));
        assert!(!valid_prefix('&'));
    }
}
