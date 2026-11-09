use super::{Command, Context, Module};
use crate::{config::Config, storage::Storage};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

/// Тип значения настройки + правила валидации.
#[derive(Clone, Copy)]
enum Kind {
    Secs { min: u64, max: u64 },
    Lang,
}

struct Setting {
    key: &'static str,
    desc: &'static str,
    kind: Kind,
}

const SETTINGS: &[Setting] = &[
    Setting {
        key: "eval.timeout",
        desc: "Таймаут .eval, сек",
        kind: Kind::Secs { min: 1, max: 300 },
    },
    Setting {
        key: "exec.timeout",
        desc: "Таймаут .exec, сек",
        kind: Kind::Secs { min: 1, max: 600 },
    },
    Setting {
        key: "translate.lang",
        desc: "Язык перевода по умолчанию",
        kind: Kind::Lang,
    },
];

fn storage_key(key: &str) -> String {
    format!("cfg.{key}")
}

// ── Эффективные значения (используются другими модулями) ─────────────────────

pub fn eval_timeout(storage: &Storage, config: &Config) -> u64 {
    storage
        .get::<u64>("cfg.eval.timeout")
        .unwrap_or(config.eval_timeout)
}

pub fn exec_timeout(storage: &Storage, config: &Config) -> u64 {
    storage
        .get::<u64>("cfg.exec.timeout")
        .unwrap_or(config.exec_timeout)
}

pub fn translate_lang(storage: &Storage) -> String {
    storage
        .get::<String>("cfg.translate.lang")
        .unwrap_or_else(|| "ru".to_owned())
}

// ── Валидация ────────────────────────────────────────────────────────────────

fn validate(kind: Kind, raw: &str) -> Result<Value, String> {
    match kind {
        Kind::Secs { min, max } => {
            let n: u64 = raw
                .parse()
                .map_err(|_| "нужно целое число".to_owned())?;
            if n < min || n > max {
                return Err(format!("допустимо от {min} до {max}"));
            }
            Ok(Value::from(n))
        }
        Kind::Lang => {
            if raw.len() == 2 && raw.chars().all(|c| c.is_ascii_lowercase()) {
                Ok(Value::from(raw.to_owned()))
            } else {
                Err("нужен код из двух латинских букв, например ru".to_owned())
            }
        }
    }
}

fn default_secs(key: &str, config: &Config) -> u64 {
    match key {
        "eval.timeout" => config.eval_timeout,
        "exec.timeout" => config.exec_timeout,
        _ => 0,
    }
}

/// (значение, задано ли пользователем)
fn current(s: &Setting, storage: &Storage, config: &Config) -> (String, bool) {
    let key = storage_key(s.key);
    match s.kind {
        Kind::Secs { .. } => match storage.get::<u64>(&key) {
            Some(v) => (v.to_string(), true),
            None => (default_secs(s.key, config).to_string(), false),
        },
        Kind::Lang => match storage.get::<String>(&key) {
            Some(v) => (v, true),
            None => ("ru".to_owned(), false),
        },
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

// ── Модуль ───────────────────────────────────────────────────────────────────

pub struct BotConfigModule;

#[async_trait]
impl Module for BotConfigModule {
    fn name(&self) -> &str {
        "botconfig"
    }

    fn commands(&self) -> Vec<Command> {
        vec![Command::new(
            "config",
            "Настройки модулей (без аргументов — список)",
            ".config [ключ] [значение|reset]",
        )]
    }

    async fn handle(&self, _cmd: &str, ctx: Context) -> Result<()> {
        let p = ctx.command_prefix();

        let Some(key) = ctx.args.first() else {
            let mut rows = Vec::new();
            for s in SETTINGS {
                let (val, custom) = current(s, &ctx.storage, &ctx.config);
                let mark = if custom { "✏️" } else { "▫️" };
                rows.push(format!(
                    "{mark} <code>{}</code> = <b>{}</b>\n      <i>{}</i>",
                    s.key,
                    esc(&val),
                    s.desc
                ));
            }
            return ctx
                .edit(format!(
                    "{}\n\n<i>Изменить: <code>{p}config ключ значение</code> · сброс: <code>{p}config ключ reset</code></i>",
                    super::ui::card("🎛", "Настройки", &rows)
                ))
                .await;
        };

        let Some(setting) = SETTINGS.iter().find(|s| s.key.eq_ignore_ascii_case(key)) else {
            return ctx
                .edit(super::ui::err(&format!(
                    "Неизвестная настройка <code>{}</code>. Список: <code>{p}config</code>",
                    esc(key)
                )))
                .await;
        };

        let Some(value) = ctx.args.get(1) else {
            let (val, custom) = current(setting, &ctx.storage, &ctx.config);
            let note = if custom { "задано" } else { "по умолчанию" };
            return ctx
                .edit(format!(
                    "⚙️ <code>{}</code> = <code>{}</code> ({note})\n{}",
                    setting.key,
                    esc(&val),
                    setting.desc
                ))
                .await;
        };

        if value.eq_ignore_ascii_case("reset") {
            ctx.storage.del(&storage_key(setting.key))?;
            let (val, _) = current(setting, &ctx.storage, &ctx.config);
            return ctx
                .edit(format!(
                    "✅ <code>{}</code> сброшено (<code>{}</code>)",
                    setting.key,
                    esc(&val)
                ))
                .await;
        }

        match validate(setting.kind, value) {
            Ok(v) => {
                ctx.storage.set(&storage_key(setting.key), v)?;
                ctx.edit(format!(
                    "✅ <code>{}</code> = <code>{}</code>",
                    setting.key,
                    esc(value)
                ))
                .await
            }
            Err(e) => {
                ctx.edit(super::ui::err(&format!(
                    "<code>{}</code>: {e}",
                    setting.key
                )))
                .await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{validate, Kind};

    #[test]
    fn secs_range() {
        let k = Kind::Secs { min: 1, max: 300 };
        assert!(validate(k, "30").is_ok());
        assert!(validate(k, "0").is_err());
        assert!(validate(k, "301").is_err());
        assert!(validate(k, "abc").is_err());
    }

    #[test]
    fn lang_code() {
        assert!(validate(Kind::Lang, "en").is_ok());
        assert!(validate(Kind::Lang, "EN").is_err());
        assert!(validate(Kind::Lang, "eng").is_err());
    }
}
