use super::{Command, Context, Module};
use anyhow::{Context as _, Result};
use async_trait::async_trait;
use std::io::Write;
use std::process::Stdio;
use tempfile::NamedTempFile;
use tokio::process::Command as TokioCmd;
use tokio::time::{timeout, Duration};

pub struct EvalModule;

fn wrap_code(code: &str) -> String {
    let code = code
        .trim()
        .trim_start_matches("```rust")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    if code.contains("fn main") {
        code.to_owned()
    } else {
        format!("#![allow(unused)]\nfn main() {{\n{code}\n}}")
    }
}

#[async_trait]
impl Module for EvalModule {
    fn name(&self) -> &str {
        "eval"
    }

    fn commands(&self) -> Vec<Command> {
        vec![
            Command::new(
                "eval",
                "Скомпилировать и запустить Rust-код",
                ".eval println!(\"Hello!\");",
            ),
            Command::new("e", "Псевдоним .eval", ".e <код>"),
        ]
    }

    async fn handle(&self, _cmd: &str, ctx: Context) -> Result<()> {
        let raw = if ctx.args_raw.trim().is_empty() {
            // Попробуем текст ответного сообщения
            match ctx.message.reply_to_message_id() {
                Some(rid) => {
                    let msgs = ctx
                        .client
                        .get_messages_by_id(ctx.peer_ref(), &[rid])
                        .await?;
                    msgs.into_iter()
                        .flatten()
                        .next()
                        .map(|m| m.text().to_owned())
                        .unwrap_or_default()
                }
                None => String::new(),
            }
        } else {
            ctx.args_raw.clone()
        };

        if raw.trim().is_empty() {
            return ctx
                .edit("❌ Использование: `.eval <код>` или ответом на сообщение с кодом")
                .await;
        }

        let code = wrap_code(&raw);
        let secs = super::botconfig::eval_timeout(&ctx.storage, &ctx.config);

        ctx.edit("⏳ Компилирую...").await?;

        let mut src = NamedTempFile::new().context("tempfile")?;
        src.write_all(code.as_bytes()).context("write src")?;
        let src_path = src.path().to_path_buf();

        let bin = NamedTempFile::new().context("tempfile bin")?;
        let bin_path = bin.path().to_path_buf();
        drop(bin);

        // Компиляция
        let compile = timeout(
            Duration::from_secs(secs),
            TokioCmd::new("rustc")
                .args([
                    "--edition",
                    "2021",
                    "-o",
                    bin_path.to_str().unwrap(),
                    src_path.to_str().unwrap(),
                ])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output(),
        )
        .await;

        let co = match compile {
            Err(_) => return ctx.edit("⏰ Компиляция превысила лимит").await,
            Ok(Err(e)) => return ctx.edit(format!("❌ rustc не найден: {e}")).await,
            Ok(Ok(o)) => o,
        };

        if !co.status.success() {
            let s = String::from_utf8_lossy(&co.stderr);
            return ctx.edit_code("❌ Ошибка компиляции:", &s).await;
        }

        // Запуск
        ctx.edit("⏳ Выполняю...").await?;

        let run = timeout(
            Duration::from_secs(secs),
            TokioCmd::new(&bin_path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output(),
        )
        .await;

        let _ = std::fs::remove_file(&bin_path);

        let ro = match run {
            Err(_) => return ctx.edit("⏰ Выполнение превысило лимит").await,
            Ok(Err(e)) => return ctx.edit(format!("❌ Не удалось запустить: {e}")).await,
            Ok(Ok(o)) => o,
        };

        let stdout = String::from_utf8_lossy(&ro.stdout);
        let stderr = String::from_utf8_lossy(&ro.stderr);
        let code = ro.status.code().unwrap_or(-1);

        let output = match (stdout.trim().is_empty(), stderr.trim().is_empty()) {
            (true, true) => format!("(нет вывода, exit {code})"),
            (false, true) => stdout.trim_end().to_owned(),
            (true, false) => format!("stderr:\n{}", stderr.trim_end()),
            (false, false) => format!("{}\nstderr:\n{}", stdout.trim_end(), stderr.trim_end()),
        };

        let icon = if ro.status.success() { "✅" } else { "⚠️" };
        ctx.edit_code(&format!("{icon} exit {code}:"), &output)
            .await
    }
}
