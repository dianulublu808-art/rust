use super::{Command, Context, Module};
use anyhow::Result;
use async_trait::async_trait;
use grammers_client::InputMessage;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Mutex;
use tokio::time::{interval, Duration};

pub struct ExecModule;

#[async_trait]
impl Module for ExecModule {
    fn name(&self) -> &str {
        "exec"
    }

    fn commands(&self) -> Vec<Command> {
        vec![Command::new(
            "exec",
            "Выполнить команду в shell (с потоковым выводом)",
            ".exec <команда>",
        )]
    }

    async fn handle(&self, _cmd: &str, ctx: Context) -> Result<()> {
        if ctx.args_raw.trim().is_empty() {
            return ctx.edit("❌ Использование: <code>.exec &lt;команда&gt;</code>").await;
        }
        run_streaming(ctx).await
    }
}

async fn run_streaming(ctx: Context) -> Result<()> {
    let cmd = ctx.args_raw.trim().to_owned();
    let timeout_secs = super::botconfig::exec_timeout(&ctx.storage, &ctx.config);

    ctx.edit(format!("⏳ <code>{}</code>", super::esc(truncate(&cmd, 200))))
        .await?;

    let mut child = tokio::process::Command::new("bash")
        .arg("-c")
        .arg(&cmd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let out_buf: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let err_buf: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));

    // Читатель stdout
    let out1 = Arc::clone(&out_buf);
    let read_out = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            out1.lock().await.push_str(&line);
            out1.lock().await.push('\n');
        }
    });

    // Читатель stderr
    let err1 = Arc::clone(&err_buf);
    let read_err = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            err1.lock().await.push_str(&line);
            err1.lock().await.push('\n');
        }
    });

    let start = Instant::now();
    let mut ticker = interval(Duration::from_secs(2));
    ticker.tick().await; // пропускаем первый мгновенный тик

    let status = loop {
        tokio::select! {
            _ = ticker.tick() => {
                // Обновляем сообщение в процессе работы
                let o = out_buf.lock().await.clone();
                let e = err_buf.lock().await.clone();
                let text = fmt_running(&cmd, &o, &e, start.elapsed());
                let _ = ctx.client.edit_message(
                    ctx.peer_ref(),
                    ctx.message.id(),
                    InputMessage::new().text(&text),
                ).await;

                if start.elapsed().as_secs() >= timeout_secs {
                    let _ = child.kill().await;
                    let _ = read_out.await;
                    let _ = read_err.await;
                    return ctx.edit("⏰ Команда превысила лимит времени").await;
                }
            }
            result = child.wait() => {
                let code = result.map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
                let _ = read_out.await;
                let _ = read_err.await;
                break code;
            }
        }
    };

    let out = out_buf.lock().await.clone();
    let err = err_buf.lock().await.clone();
    let elapsed = start.elapsed();

    let body = format_output(&out, &err, status);
    let icon = if status == 0 { "✅" } else { "⚠️" };
    let header = format!(
        "{icon} <code>{cmd_short}</code> — exit {status} ({ms}ms)",
        icon = icon,
        cmd_short = super::esc(truncate(&cmd, 120)),
        status = status,
        ms = elapsed.as_millis(),
    );

    ctx.edit_code(&header, &body).await
}

fn fmt_running(cmd: &str, out: &str, err: &str, elapsed: std::time::Duration) -> String {
    let body = format_output(out, err, -1);
    format!(
        "⏳ <code>{cmd}</code> ({ms}ms)\n<pre>{body}</pre>",
        cmd = super::esc(truncate(cmd, 120)),
        ms = elapsed.as_millis(),
        body = super::esc(truncate_tail(&body, 3000)),
    )
}

fn format_output(out: &str, err: &str, _code: i32) -> String {
    match (out.trim().is_empty(), err.trim().is_empty()) {
        (true, true) => "(нет вывода)".to_owned(),
        (false, true) => truncate_tail(out.trim_end(), 3500).to_owned(),
        (true, false) => format!("stderr:\n{}", truncate_tail(err.trim_end(), 3500)),
        (false, false) => format!(
            "{}\nstderr:\n{}",
            truncate_tail(out.trim_end(), 2000),
            truncate_tail(err.trim_end(), 1000),
        ),
    }
}

fn truncate(s: &str, max: usize) -> &str {
    let s = s.trim_end();
    if s.len() <= max {
        return s;
    }
    let mut i = max;
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    &s[..i]
}

fn truncate_tail(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let start = s.len() - max;
    let mut i = start;
    while !s.is_char_boundary(i) {
        i += 1;
    }
    &s[i..]
}
