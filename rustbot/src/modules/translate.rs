use super::{Command, Context, Module};
use anyhow::{Context as _, Result};
use async_trait::async_trait;

pub struct TranslateModule;

#[async_trait]
impl Module for TranslateModule {
    fn name(&self) -> &str {
        "translate"
    }

    fn commands(&self) -> Vec<Command> {
        vec![Command::new(
            "tr",
            "Перевести текст. .tr ru <текст> или ответом",
            ".tr [lang] [текст]",
        )]
    }

    async fn handle(&self, _cmd: &str, ctx: Context) -> Result<()> {
        cmd_translate(&ctx).await
    }
}

async fn cmd_translate(ctx: &Context) -> Result<()> {
    // Разбираем аргументы: .tr [lang] [текст]
    let (lang, text) = parse_args(ctx).await?;

    if text.trim().is_empty() {
        return ctx
            .edit("❌ Использование: <code>.tr ru текст</code> или ответом на сообщение")
            .await;
    }

    ctx.edit("🌐 Перевожу...").await?;

    let translated = translate_text(&text, &lang)
        .await
        .with_context(|| "Ошибка перевода")?;

    ctx.edit(format!("🌐 <b>Перевод → {lang}</b>\n{}", super::esc(&translated)))
        .await
}

async fn parse_args(ctx: &Context) -> Result<(String, String)> {
    let default_lang = super::botconfig::translate_lang(&ctx.storage);

    if ctx.args.is_empty() {
        // Только ответ
        let text = get_reply_text(ctx).await?;
        return Ok((default_lang, text));
    }

    let first = &ctx.args[0];

    // Определяем, является ли первый аргумент языком (2 буквы или xx|yy)
    let is_lang = first.len() == 2 || (first.len() == 5 && first.chars().nth(2) == Some('|'));

    if is_lang {
        let lang = parse_lang(first);
        let rest = ctx.args[1..].join(" ");
        let text = if rest.trim().is_empty() {
            get_reply_text(ctx).await?
        } else {
            rest
        };
        Ok((lang, text))
    } else {
        // Весь args_raw — текст, язык по умолчанию
        let text = if ctx.args_raw.trim().is_empty() {
            get_reply_text(ctx).await?
        } else {
            ctx.args_raw.clone()
        };
        Ok((default_lang, text))
    }
}

fn parse_lang(s: &str) -> String {
    // "en|ru" → возвращаем "ru" (целевой язык)
    if let Some(pos) = s.find('|') {
        s[pos + 1..].to_owned()
    } else {
        s.to_owned()
    }
}

async fn get_reply_text(ctx: &Context) -> Result<String> {
    match ctx.message.reply_to_message_id() {
        Some(rid) => {
            let msgs = ctx
                .client
                .get_messages_by_id(ctx.peer_ref(), &[rid])
                .await?;
            Ok(msgs
                .into_iter()
                .flatten()
                .next()
                .map(|m| m.text().to_owned())
                .unwrap_or_default())
        }
        None => Ok(String::new()),
    }
}

/// Переводит текст через неофициальный Google Translate API.
async fn translate_text(text: &str, target: &str) -> Result<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let url = format!(
        "https://translate.googleapis.com/translate_a/single\
         ?client=gtx&sl=auto&tl={target}&dt=t&q={q}",
        target = urlencoding(target),
        q = urlencoding(text),
    );

    let resp: serde_json::Value = client
        .get(&url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await?
        .json()
        .await?;

    // Ответ: [[[translated, original, ...], ...], ...]
    let mut result = String::new();
    if let Some(arr) = resp[0].as_array() {
        for chunk in arr {
            if let Some(s) = chunk[0].as_str() {
                result.push_str(s);
            }
        }
    }

    if result.is_empty() {
        anyhow::bail!("Пустой ответ от сервера перевода");
    }

    Ok(result)
}

fn urlencoding(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if c.is_ascii_alphanumeric() || "-._~".contains(c) {
                vec![c]
            } else {
                let mut bytes = [0u8; 4];
                let encoded: String = c
                    .encode_utf8(&mut bytes)
                    .bytes()
                    .map(|b| format!("%{b:02X}"))
                    .collect::<Vec<_>>()
                    .concat()
                    .chars()
                    .collect();
                encoded.chars().collect()
            }
        })
        .collect()
}
