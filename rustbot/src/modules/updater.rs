use super::{esc, ui, Command, Context, Module};
use anyhow::{Context as _, Result};
use async_trait::async_trait;
use std::collections::VecDeque;
use std::future::Future;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCmd;
use tokio::time::{timeout, Duration};

pub struct UpdaterModule;

/// Одновременно может идти только одно обновление.
static BUSY: AtomicBool = AtomicBool::new(false);

struct BusyGuard;
impl Drop for BusyGuard {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::SeqCst);
    }
}

#[async_trait]
impl Module for UpdaterModule {
    fn name(&self) -> &str {
        "updater"
    }

    fn commands(&self) -> Vec<Command> {
        vec![
            Command::new(
                "update",
                "Обновить из git, пересобрать и перезапуститься (check — только проверить, или имя ветки)",
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
        Duration::from_secs(300),
        TokioCmd::new(args[0])
            .args(&args[1..])
            .current_dir(cwd)
            .output(),
    )
    .await
    .with_context(|| format!("Таймаут команды {:?}", args[0]))?
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

/// Имя крейта из строки cargo вида `   Compiling grammers-client v0.7.0 (...)`.
fn parse_compiling(line: &str) -> Option<&str> {
    let mut it = line.split_whitespace();
    if it.next()? != "Compiling" {
        return None;
    }
    it.next()
}

/// Другая сборка cargo, которая держит блокировку каталога target.
async fn other_cargo_pids() -> Option<String> {
    let out = TokioCmd::new("pgrep")
        .args(["-x", "cargo"])
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let pids = String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(", ");
    (!pids.is_empty()).then_some(pids)
}

// ── сборка с прогрессом ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct Progress {
    pub elapsed: Duration,
    pub compiled: usize,
    pub current: String,
}

#[derive(Debug)]
pub struct BuildResult {
    pub success: bool,
    /// последние строки вывода (для показа ошибки)
    pub tail: String,
    pub compiled: usize,
}

/// Запускает команду сборки, читает её stderr и раз в `every` вызывает `report`.
/// При отмене задачи процесс сборки убивается (kill_on_drop).
pub async fn run_build<F, Fut>(
    mut cmd: TokioCmd,
    every: Duration,
    mut report: F,
) -> Result<BuildResult>
where
    F: FnMut(Progress) -> Fut,
    Fut: Future<Output = ()>,
{
    let mut child = cmd
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("не удалось запустить cargo (установлен ли rust?)")?;
    let stderr = child.stderr.take().context("нет stderr у cargo")?;
    let mut lines = BufReader::new(stderr).lines();

    let started = Instant::now();
    let mut tick = tokio::time::interval(every);
    tick.tick().await; // первый тик срабатывает сразу
    let mut compiled = 0usize;
    let mut current = String::new();
    let mut tail_lines: VecDeque<String> = VecDeque::with_capacity(41);

    loop {
        tokio::select! {
            line = lines.next_line() => {
                match line? {
                    Some(l) => {
                        if let Some(name) = parse_compiling(&l) {
                            compiled += 1;
                            current = name.to_owned();
                        }
                        if tail_lines.len() == 40 {
                            tail_lines.pop_front();
                        }
                        tail_lines.push_back(l);
                    }
                    None => break,
                }
            }
            _ = tick.tick() => {
                report(Progress {
                    elapsed: started.elapsed(),
                    compiled,
                    current: current.clone(),
                }).await;
            }
        }
    }

    let status = child.wait().await?;
    Ok(BuildResult {
        success: status.success(),
        tail: tail_lines.into_iter().collect::<Vec<_>>().join("\n"),
        compiled,
    })
}

fn progress_text(new_short: &str, p: &Progress) -> String {
    let mut rows = vec![
        format!("⏱ {}", ui::duration(p.elapsed.as_secs())),
        format!("📦 Скомпилировано крейтов: <b>{}</b>", p.compiled),
    ];
    if !p.current.is_empty() {
        rows.push(format!("▸ <code>{}</code>", esc(&p.current)));
    }
    format!(
        "🔨 <b>Сборка</b> <code>{new_short}</code>\n{}\n<i>Не нажимай ничего — бот перезапустится сам</i>",
        ui::quote(&rows)
    )
}

// ── команды ──────────────────────────────────────────────────────────────────

async fn cmd_update(ctx: &Context) -> Result<()> {
    if BUSY.swap(true, Ordering::SeqCst) {
        return ctx.edit("⏳ Обновление уже выполняется").await;
    }
    let _busy = BusyGuard;

    let Some(repo) = repo_dir() else {
        return ctx
            .edit(ui::err(
                "Не удалось определить папку репозитория. Запускай бота из <code>target/release/</code>",
            ))
            .await;
    };
    let rustbot = rustbot_dir().unwrap();

    let arg = ctx.args.first().map(String::as_str);
    let check_only = arg.is_some_and(|a| a.eq_ignore_ascii_case("check"));

    let current = git_out(&["git", "rev-parse", "--abbrev-ref", "HEAD"], &repo).await;
    if current.is_empty() || current == "HEAD" {
        return ctx
            .edit(ui::err(
                "Репозиторий в состоянии detached HEAD — укажи ветку: <code>.update имя</code>",
            ))
            .await;
    }
    let branch = match arg {
        Some(a) if !check_only => a.to_owned(),
        _ => current.clone(),
    };
    if !valid_branch(&branch) {
        return ctx.edit(ui::err("Недопустимое имя ветки")).await;
    }

    let old = git_out(&["git", "rev-parse", "HEAD"], &repo).await;

    ctx.edit(format!(
        "🔄 <b>Проверяю обновления</b> ветки <code>{}</code>...",
        esc(&branch)
    ))
    .await?;
    let (ok, out) = run_cmd(&["git", "fetch", "origin", &branch], &repo).await?;
    if !ok {
        return ctx
            .edit(ui::err(&format!(
                "git fetch не удался:\n<code>{}</code>",
                esc(tail(&out, 800))
            )))
            .await;
    }
    let new = git_out(&["git", "rev-parse", "FETCH_HEAD"], &repo).await;
    let same_branch = branch == current;
    let code_changes = !same_branch || old != new;

    // запущенный бинарник мог остаться от старой сборки, даже если код уже свежий
    let running = crate::version::GIT.trim_end_matches('+');
    let binary_stale = running != "unknown" && running != short(&new);

    if !code_changes && !binary_stale {
        return ctx
            .edit(format!(
                "✅ <b>Уже актуальная версия</b>\n{}",
                ui::quote(&[
                    format!("🔖 <code>{}</code> · ветка <code>{}</code>", short(&old), esc(&branch)),
                    format!("🔨 Сборка от {}", crate::version::built_at()),
                ])
            ))
            .await;
    }

    let subject = git_out(&["git", "log", "-1", "--format=%s", "FETCH_HEAD"], &repo).await;

    if check_only {
        let what = if code_changes {
            format!("<code>{}</code> → <code>{}</code>", short(&old), short(&new))
        } else {
            format!(
                "код свежий (<code>{}</code>), но запущена сборка <code>{running}</code>",
                short(&new)
            )
        };
        return ctx
            .edit(format!(
                "📦 <b>Есть что обновить</b>\n{}\nУстановить: <code>{}update</code>",
                ui::quote(&[
                    format!("🌿 {}", esc(&branch)),
                    format!("🔖 {what}"),
                    format!("💬 {}", esc(&subject)),
                ]),
                ctx.command_prefix()
            ))
            .await;
    }

    if let Some(pids) = other_cargo_pids().await {
        return ctx
            .edit(ui::err(&format!(
                "Уже идёт другая сборка cargo (pid {pids}). Дождись её или останови: <code>pkill cargo</code>"
            )))
            .await;
    }

    // применяем код (ff-only: локальные правки не затираем)
    if code_changes {
        let (ok, out) = if same_branch {
            run_cmd(&["git", "merge", "--ff-only", "FETCH_HEAD"], &repo).await?
        } else {
            run_cmd(&["git", "checkout", "-B", &branch, "FETCH_HEAD"], &repo).await?
        };
        if !ok {
            return ctx
                .edit(ui::err(&format!(
                    "Не удалось применить обновление:\n<code>{}</code>",
                    esc(tail(&out, 800))
                )))
                .await;
        }
    }

    // сборка: по умолчанию -j1, иначе Android убивает процесс по памяти
    let jobs = super::botconfig::update_jobs(&ctx.storage);
    let new_short = short(&new).to_owned();
    ctx.edit(progress_text(&new_short, &Progress::default())).await?;

    let mut cmd = TokioCmd::new("cargo");
    cmd.args(["build", "--release", "--color", "never", "-j", &jobs.to_string()])
        .current_dir(&rustbot);

    let started = Instant::now();
    let result = run_build(cmd, Duration::from_secs(20), |p| {
        let text = progress_text(&new_short, &p);
        async move {
            let _ = ctx.edit(text).await;
        }
    })
    .await;

    let rollback = |why: String| {
        let repo = repo.clone();
        let current = current.clone();
        let old = old.clone();
        async move {
            if code_changes {
                let _ = run_cmd(&["git", "checkout", "-B", &current, &old], &repo).await;
            }
            why
        }
    };

    let result = match result {
        Ok(r) => r,
        Err(e) => {
            let why = rollback(format!("{e:#}")).await;
            return ctx.edit(ui::err(&esc(&why))).await;
        }
    };

    if !result.success {
        rollback(String::new()).await;
        return ctx
            .edit(ui::err(&format!(
                "Сборка не удалась{}. Бот остался на прежней версии (<code>{}</code>).\n<code>{}</code>",
                if code_changes { ", код откатан" } else { "" },
                short(&old),
                esc(tail(&result.tail, 1500))
            )))
            .await;
    }

    // убеждаемся, что новый бинарник вообще запускается, прежде чем перезапускаться в него
    let bin = rustbot.join("target/release/rustbot");
    let check = timeout(
        Duration::from_secs(20),
        TokioCmd::new(&bin).arg("--version").output(),
    )
    .await;
    let version_line = match check {
        Ok(Ok(o)) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_owned(),
        _ => {
            rollback(String::new()).await;
            return ctx
                .edit(ui::err(
                    "Новый бинарник не запускается (<code>--version</code>). Перезапуск отменён, бот остался на прежней версии.",
                ))
                .await;
        }
    };

    let secs = started.elapsed().as_secs();
    let _ = ctx.storage.set(
        "update.notice",
        serde_json::json!({
            "from": short(&old),
            "to": short(&new),
            "secs": secs,
            "subject": subject,
            "version": version_line,
        }),
    );

    ctx.edit(format!(
        "✅ <b>Собрано</b> за {}\n{}\n🔄 Перезапускаюсь...",
        ui::duration(secs),
        ui::quote(&[
            format!("🔖 <code>{}</code> → <code>{}</code>", short(&old), short(&new)),
            format!("💬 {}", esc(&subject)),
        ])
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

    #[test]
    fn parses_cargo_lines() {
        assert_eq!(
            parse_compiling("   Compiling grammers-client v0.7.0 (https://x)"),
            Some("grammers-client")
        );
        assert_eq!(parse_compiling("   Compiling rustbot v1.0.0 (/p)"), Some("rustbot"));
        assert_eq!(parse_compiling("warning: unused"), None);
        assert_eq!(parse_compiling("    Finished `release` profile"), None);
        assert_eq!(parse_compiling(""), None);
    }

    fn sh(script: &str) -> TokioCmd {
        let mut c = TokioCmd::new("sh");
        c.arg("-c").arg(script);
        c
    }

    #[tokio::test]
    async fn build_reports_progress_and_counts_crates() {
        let reports = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Progress>::new()));
        let r2 = reports.clone();
        let res = run_build(
            sh("echo '   Compiling a v1' >&2; sleep 0.35; echo '   Compiling b v2' >&2; sleep 0.35; echo '    Finished' >&2"),
            Duration::from_millis(100),
            move |p| {
                let r2 = r2.clone();
                async move {
                    r2.lock().unwrap().push(p);
                }
            },
        )
        .await
        .unwrap();
        assert!(res.success);
        assert_eq!(res.compiled, 2);
        let reports = reports.lock().unwrap();
        assert!(!reports.is_empty(), "прогресс должен отправляться во время сборки");
        assert!(reports.iter().any(|p| p.current == "a" || p.current == "b"));
    }

    #[tokio::test]
    async fn build_failure_keeps_output_tail() {
        let res = run_build(
            sh("echo '   Compiling a v1' >&2; echo 'error: boom' >&2; exit 101"),
            Duration::from_secs(5),
            |_| async {},
        )
        .await
        .unwrap();
        assert!(!res.success);
        assert!(res.tail.contains("error: boom"));
    }

    #[tokio::test]
    async fn build_is_killed_when_cancelled() {
        let marker = std::env::temp_dir().join(format!("rb-kill-{}", std::process::id()));
        let _ = std::fs::remove_file(&marker);
        let script = format!("sleep 2; touch {}", marker.display());
        let fut = run_build(sh(&script), Duration::from_secs(5), |_| async {});
        // отменяем (как при таймауте модуля) — процесс сборки не должен дожить
        let _ = timeout(Duration::from_millis(200), fut).await;
        tokio::time::sleep(Duration::from_millis(2500)).await;
        assert!(!marker.exists(), "процесс сборки должен быть убит при отмене");
    }

    #[test]
    fn progress_text_is_html_safe() {
        let t = progress_text(
            "abc1234",
            &Progress {
                elapsed: Duration::from_secs(125),
                compiled: 7,
                current: "a<b>".into(),
            },
        );
        assert!(t.contains("2м 5с"));
        assert!(t.contains("a&lt;b&gt;"));
    }
}
