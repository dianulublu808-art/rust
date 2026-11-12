use super::{esc, Command, Context, Module};
use anyhow::{Context as _, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use tokio::process::Command as TokioCmd;
use tokio::time::{timeout, Duration};

pub struct UpdaterModule;

#[async_trait]
impl Module for UpdaterModule {
    fn name(&self) -> &str {
        "updater"
    }

    fn commands(&self) -> Vec<Command> {
        vec![
            Command::new(
                "update",
                "Обновить из git и пересобрать (check — только проверить, или имя ветки)",
                ".update [check|ветка]",
            ),
            Command::new("restart", "Перезапустить бота", ".restart"),
        ]
    }

    async fn handle(&self, cmd: &str, ctx: Context) -> Result<()> {
        match cmd {
            "update" => cmd_update(&ctx).await,
            "restart" => cmd_restart(&ctx).await,
            _ => Ok(()),
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
        Duration::from_secs(1800),
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
    let combined = if stderr.is_empty() {
        stdout
    } else {
        format!("{stdout}{stderr}")
    };
    Ok((out.status.success(), combined))
}

async fn git_out(args: &[&str], repo: &PathBuf) -> String {
    match run_cmd(args, repo).await {
        Ok((true, out)) => out.trim().to_owned(),
        _ => String::new(),
    }
}

fn valid_branch(b: &str) -> bool {
    !b.is_empty()
        && b.len() <= 100
        && !b.starts_with('-')
        && !b.contains("..")
        && b.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-'))
}

fn short(hash: &str) -> &str {
    &hash[..hash.len().min(7)]
}

fn tail(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut start = s.len() - max;
    while !s.is_char_boundary(start) {
        start += 1;
    }
    &s[start..]
}

// ── команды ──────────────────────────────────────────────────────────────────

async fn cmd_update(ctx: &Context) -> Result<()> {
    let Some(repo) = repo_dir() else {
        return ctx
            .edit(
                "❌ Не удалось определить директорию репозитория.\n\
                 Запускай собранный бинарник из <code>target/release/</code>",
            )
            .await;
    };
    let rustbot = rustbot_dir().unwrap();

    let arg = ctx.args.first().map(String::as_str);
    let check_only = arg.is_some_and(|a| a.eq_ignore_ascii_case("check"));

    let current = git_out(&["git", "rev-parse", "--abbrev-ref", "HEAD"], &repo).await;
    if current.is_empty() || current == "HEAD" {
        return ctx
            .edit("❌ Репозиторий в состоянии detached HEAD — укажи ветку: <code>.update имя</code>")
            .await;
    }
    let branch = match arg {
        Some(a) if !check_only => a.to_owned(),
        _ => current.clone(),
    };
    if !valid_branch(&branch) {
        return ctx.edit("❌ Недопустимое имя ветки").await;
    }

    let old = git_out(&["git", "rev-parse", "HEAD"], &repo).await;

    // 1. fetch
    ctx.edit(format!(
        "🔄 <b>Шаг 1/3:</b> git fetch <code>{}</code>...",
        esc(&branch)
    ))
    .await?;
    let (ok, out) = run_cmd(&["git", "fetch", "origin", &branch], &repo).await?;
    if !ok {
        return ctx
            .edit_code("❌ git fetch завершился с ошибкой:", tail(&out, 1000))
            .await;
    }
    let new = git_out(&["git", "rev-parse", "FETCH_HEAD"], &repo).await;
    let same_branch = branch == current;

    if same_branch && old == new {
        let running = crate::version::GIT.trim_end_matches('+');
        let stale = if running != short(&old) && running != "unknown" {
            format!(
                "\n⚠️ Запущена сборка <code>{running}</code>, а в репозитории <code>{}</code> — пересобери и перезапусти: <code>cargo build --release</code>",
                short(&old)
            )
        } else {
            String::new()
        };
        return ctx
            .edit(format!(
                "✅ <b>Уже актуальная версия</b> (<code>{}</code>, ветка <code>{}</code>){stale}",
                short(&old),
                esc(&branch)
            ))
            .await;
    }

    let subject = git_out(&["git", "log", "-1", "--format=%s", "FETCH_HEAD"], &repo).await;

    if check_only {
        return ctx
            .edit(format!(
                "📦 <b>Есть обновление</b> ветки <code>{}</code>\n<code>{}</code> → <code>{}</code>\n{}\nУстановить: <code>{}update</code>",
                esc(&branch),
                short(&old),
                short(&new),
                esc(&subject),
                ctx.command_prefix()
            ))
            .await;
    }

    // 2. применяем (ff-only: локальные правки не затираем)
    let (ok, out) = if same_branch {
        run_cmd(&["git", "merge", "--ff-only", "FETCH_HEAD"], &repo).await?
    } else {
        run_cmd(&["git", "checkout", "-B", &branch, "FETCH_HEAD"], &repo).await?
    };
    if !ok {
        return ctx
            .edit_code("❌ Не удалось применить обновление:", tail(&out, 1000))
            .await;
    }

    // 3. сборка
    ctx.edit(format!(
        "🔨 <b>Шаг 2/3:</b> сборка <code>{}</code> (может занять много минут)...",
        short(&new)
    ))
    .await?;
    let (ok, out) = run_cmd(&["cargo", "build", "--release"], &rustbot).await?;

    if !ok {
        // откат кода к прежней версии; запущенный бинарник не тронут
        let _ = run_cmd(&["git", "checkout", "-B", &current, &old], &repo).await;
        return ctx
            .edit_code(
                &format!(
                    "❌ Ошибка компиляции, код откатан на <code>{}</code>:",
                    short(&old)
                ),
                tail(&out, 2000),
            )
            .await;
    }

    ctx.edit(format!(
        "✅ <b>Шаг 3/3:</b> собрано <code>{}</code> — {}\nПерезапускаю...",
        short(&new),
        esc(&subject)
    ))
    .await?;
    do_restart();
}

async fn cmd_restart(ctx: &Context) -> Result<()> {
    ctx.edit("🔄 <b>Перезапускаю...</b>").await?;
    do_restart();
}

/// Пауза перед аварийным перезапуском: чем чаще перезапуски подряд, тем дольше
/// ждём (до 5 минут), чтобы не крутиться в цикле. Если бот проработал 10+ минут,
/// счётчик сбрасывается. Возвращает (новый счётчик, пауза).
pub fn restart_backoff(prev_count: u32, uptime_secs: u64) -> (u32, Duration) {
    let count = if uptime_secs >= 600 { 0 } else { prev_count };
    let delay = (count as u64 * 15).min(300);
    (count + 1, Duration::from_secs(delay))
}

/// Перезапуск после сбоя (например, потери связи): с ожиданием и счётчиком
/// подряд идущих перезапусков в переменной окружения.
pub fn restart_after_failure(reason: &str) -> ! {
    let prev: u32 = std::env::var("RUSTBOT_RESTARTS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let (count, delay) = restart_backoff(prev, super::ui::process_uptime());
    eprintln!(
        "перезапуск #{count}: {reason}{}",
        if delay.is_zero() {
            String::new()
        } else {
            format!(" (жду {} с)", delay.as_secs())
        }
    );
    std::env::set_var("RUSTBOT_RESTARTS", count.to_string());
    std::thread::sleep(delay);
    do_restart()
}

fn do_restart() -> ! {
    use std::os::unix::process::CommandExt;
    let args: Vec<String> = std::env::args().collect();
    let err = std::process::Command::new(&args[0]).args(&args[1..]).exec();
    // exec() никогда не возвращается, если успешен
    eprintln!("exec failed: {err}");
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_and_caps() {
        assert_eq!(restart_backoff(0, 5), (1, Duration::from_secs(0)));
        assert_eq!(restart_backoff(1, 5), (2, Duration::from_secs(15)));
        assert_eq!(restart_backoff(3, 5), (4, Duration::from_secs(45)));
        assert_eq!(restart_backoff(50, 5), (51, Duration::from_secs(300)));
    }

    #[test]
    fn backoff_resets_after_healthy_run() {
        assert_eq!(restart_backoff(7, 601), (1, Duration::from_secs(0)));
    }

    #[test]
    fn branch_validation() {
        assert!(valid_branch("reload-and-config"));
        assert!(valid_branch("feature/x_1.2"));
        assert!(!valid_branch("-rf"));
        assert!(!valid_branch("a..b"));
        assert!(!valid_branch("a b"));
        assert!(!valid_branch("a;rm"));
        assert!(!valid_branch(""));
    }
}
