use super::{Command, Context, Module};
use anyhow::{Context as _, Result};
use async_trait::async_trait;
use std::io::Write;
use std::process::Stdio;
use tempfile::NamedTempFile;
use tokio::process::Command as TokioCmd;
use tokio::time::{timeout, Duration};

pub struct EvalModule;

/// Оборачивает сниппет в `fn main()` если его нет
fn wrap_code(code: &str) -> String {
    let trimmed = code.trim();
    if trimmed.contains("fn main") {
        trimmed.to_owned()
    } else {
        // Убираем markdown-блок если пользователь его добавил
        let code = trimmed
            .trim_start_matches("```rust")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();
        format!(
            "#![allow(unused)]\nfn main() {{\n{code}\n}}"
        )
    }
}

#[async_trait]
impl Module for EvalModule {
    fn name(&self) -> &'static str { "eval" }

    fn commands(&self) -> &[Command] {
        &[Command {
            name: "eval",
            description: "Скомпилировать и запустить Rust-код",
            usage: ".eval println!(\"Hello!\");",
        }]
    }

    async fn handle(&self, _cmd: &str, ctx: Context) -> Result<()> {
        if ctx.args_raw.trim().is_empty() {
            return ctx.edit("❌ Использование: `.eval <код>`").await;
        }

        let code = wrap_code(&ctx.args_raw);
        let secs = ctx.config.eval_timeout;

        ctx.edit("⏳ Компилирую...").await?;

        // Временный файл с исходником
        let mut src_file = NamedTempFile::new().context("tempfile create")?;
        src_file
            .write_all(code.as_bytes())
            .context("tempfile write")?;
        let src_path = src_file.path().to_path_buf();

        // Временный файл под бинарник
        let bin_file = NamedTempFile::new().context("tempfile bin create")?;
        let bin_path = bin_file.path().to_path_buf();
        // Явно закрываем, чтобы rustc мог в него записать (на Windows)
        drop(bin_file);

        // --- Компиляция ---
        let compile_result = timeout(
            Duration::from_secs(secs),
            TokioCmd::new("rustc")
                .args([
                    "--edition", "2021",
                    "-o", bin_path.to_str().unwrap(),
                    src_path.to_str().unwrap(),
                ])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output(),
        )
        .await;

        let compile_out = match compile_result {
            Err(_) => return ctx.edit("⏰ Компиляция превысила лимит времени").await,
            Ok(Err(e)) => return ctx.edit(format!("❌ Не удалось запустить rustc: {e}")).await,
            Ok(Ok(o)) => o,
        };

        if !compile_out.status.success() {
            let stderr = String::from_utf8_lossy(&compile_out.stderr);
            let truncated = truncate(&stderr, 3000);
            return ctx
                .edit_code("❌ **Ошибка компиляции:**", truncated)
                .await;
        }

        // --- Запуск ---
        ctx.edit("⏳ Выполняю...").await?;

        let run_result = timeout(
            Duration::from_secs(secs),
            TokioCmd::new(&bin_path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output(),
        )
        .await;

        // Удаляем бинарник
        let _ = std::fs::remove_file(&bin_path);

        let run_out = match run_result {
            Err(_) => return ctx.edit("⏰ Программа превысила лимит времени").await,
            Ok(Err(e)) => return ctx.edit(format!("❌ Не удалось запустить: {e}")).await,
            Ok(Ok(o)) => o,
        };

        let stdout = String::from_utf8_lossy(&run_out.stdout);
        let stderr = String::from_utf8_lossy(&run_out.stderr);
        let exit_code = run_out.status.code().unwrap_or(-1);

        let output = if stdout.is_empty() && stderr.is_empty() {
            format!("(нет вывода, код выхода: {exit_code})")
        } else {
            let mut parts = Vec::new();
            if !stdout.is_empty() {
                parts.push(format!("stdout:\n{}", truncate(&stdout, 1500)));
            }
            if !stderr.is_empty() {
                parts.push(format!("stderr:\n{}", truncate(&stderr, 1000)));
            }
            parts.join("\n\n")
        };

        let status_icon = if run_out.status.success() { "✅" } else { "⚠️" };
        ctx.edit_code(
            &format!("{status_icon} **Результат** (exit {exit_code}):"),
            &output,
        )
        .await
    }
}

fn truncate(s: &str, max_bytes: usize) -> &str {
    let s = s.trim_end();
    if s.len() <= max_bytes {
        return s;
    }
    // Откатываемся до ближайшей границы UTF-8 символа
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].trim_end()
}
