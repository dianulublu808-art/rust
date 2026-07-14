use super::{Command, Context, Module};
use anyhow::Result;
use async_trait::async_trait;
use std::process::Stdio;
use tokio::process::Command as TokioCmd;
use tokio::time::{timeout, Duration};

pub struct ExecModule;

#[async_trait]
impl Module for ExecModule {
    fn name(&self) -> &'static str { "exec" }

    fn commands(&self) -> &[Command] {
        &[Command {
            name: "exec",
            description: "Выполнить команду в shell (bash)",
            usage: ".exec ls -la",
        }]
    }

    async fn handle(&self, _cmd: &str, ctx: Context) -> Result<()> {
        if ctx.args_raw.trim().is_empty() {
            return ctx.edit("❌ Использование: `.exec <команда>`").await;
        }

        let shell_cmd = ctx.args_raw.trim().to_owned();
        let secs = ctx.config.exec_timeout;

        ctx.edit("⏳ Выполняю...").await?;

        let result = timeout(
            Duration::from_secs(secs),
            TokioCmd::new("bash")
                .arg("-c")
                .arg(&shell_cmd)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output(),
        )
        .await;

        match result {
            Err(_) => ctx.edit("⏰ Команда превысила лимит времени").await,
            Ok(Err(e)) => ctx.edit(format!("❌ Не удалось запустить: {e}")).await,
            Ok(Ok(out)) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                let code   = out.status.code().unwrap_or(-1);

                let body = match (stdout.trim().is_empty(), stderr.trim().is_empty()) {
                    (true,  true)  => format!("(нет вывода, код: {code})"),
                    (false, true)  => truncate_str(stdout.trim_end(), 3500).to_owned(),
                    (true,  false) => format!("stderr:\n{}", truncate_str(stderr.trim_end(), 3500)),
                    (false, false) => format!(
                        "{}\n\nstderr:\n{}",
                        truncate_str(stdout.trim_end(), 2000),
                        truncate_str(stderr.trim_end(), 1000),
                    ),
                };

                let icon = if out.status.success() { "✅" } else { "⚠️" };
                ctx.edit_code(
                    &format!("{icon} `{shell_cmd}` (exit {code}):"),
                    &body,
                )
                .await
            }
        }
    }
}

fn truncate_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].trim_end()
}
