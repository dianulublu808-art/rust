use super::{Command, Context, Module};
use anyhow::{Context as _, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use tokio::process::Command as TokioCmd;
use tokio::time::{timeout, Duration};

pub struct UpdaterModule;

#[async_trait]
impl Module for UpdaterModule {
    fn name(&self) -> &'static str { "updater" }

    fn commands(&self) -> &[Command] {
        &[
            Command {
                name:        "update",
                description: "Скачать обновления из git и пересобрать бота",
                usage:       ".update",
            },
            Command {
                name:        "restart",
                description: "Перезапустить бота",
                usage:       ".restart",
            },
        ]
    }

    async fn handle(&self, cmd: &str, ctx: Context) -> Result<()> {
        match cmd {
            "update"  => cmd_update(&ctx).await,
            "restart" => cmd_restart(&ctx).await,
            _         => Ok(()),
        }
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// target/release/rustbot → ../../.. = rustbot dir
fn rustbot_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    // target/release/rustbot → target/release → target → rustbot
    exe.ancestors().nth(3).map(PathBuf::from)
}

fn repo_dir() -> Option<PathBuf> {
    rustbot_dir()?.parent().map(PathBuf::from)
}

async fn run_cmd(args: &[&str], cwd: &PathBuf) -> Result<(bool, String)> {
    let out = timeout(
        Duration::from_secs(600),
        TokioCmd::new(args[0])
            .args(&args[1..])
            .current_dir(cwd)
            .output(),
    )
    .await
    .with_context(|| "Таймаут команды")?
    .with_context(|| format!("Не удалось запустить {:?}", args[0]))?;

    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let combined = if stderr.is_empty() { stdout } else { format!("{stdout}{stderr}") };
    Ok((out.status.success(), combined))
}

// ── команды ──────────────────────────────────────────────────────────────────

async fn cmd_update(ctx: &Context) -> Result<()> {
    let repo = match repo_dir() {
        Some(d) => d,
        None    => {
            return ctx.edit("❌ Не удалось определить директорию репозитория.\n\
                              Убедись, что запускаешь собранный бинарник из `target/release/`").await;
        }
    };

    let rustbot = rustbot_dir().unwrap();

    // 1. git pull
    ctx.edit("🔄 <b>Шаг 1/3:</b> git pull...").await?;
    let (ok, out) = run_cmd(&["git", "pull", "origin", "rustbot"], &repo).await?;
    if !ok {
        let snippet = &out[..out.len().min(1000)];
        return ctx.edit_code("❌ git pull завершился с ошибкой:", snippet).await;
    }

    if out.contains("Already up to date") || out.contains("уже обновлён") {
        return ctx.edit("✅ <b>Уже актуальная версия</b>, обновлений нет.").await;
    }

    // 2. cargo build --release
    ctx.edit("🔨 <b>Шаг 2/3:</b> Сборка (может занять несколько минут)...").await?;
    let (ok, out) = run_cmd(
        &["cargo", "build", "--release"],
        &rustbot,
    ).await?;

    if !ok {
        let snippet = &out[..out.len().min(2000)];
        return ctx.edit_code("❌ Ошибка компиляции:", snippet).await;
    }

    // 3. Restart
    ctx.edit("✅ <b>Шаг 3/3:</b> Собрано! Перезапускаю...").await?;
    do_restart();
}

async fn cmd_restart(ctx: &Context) -> Result<()> {
    ctx.edit("🔄 <b>Перезапускаю...</b>").await?;
    do_restart();
}

fn do_restart() -> ! {
    use std::os::unix::process::CommandExt;
    let args: Vec<String> = std::env::args().collect();
    let err = std::process::Command::new(&args[0])
        .args(&args[1..])
        .exec();
    // exec() никогда не возвращается, если успешен
    eprintln!("exec failed: {err}");
    std::process::exit(1);
}
