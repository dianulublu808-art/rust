//! Красивый вывод в терминал: баннер, цветные логи, информационный блок.

use std::fmt;
use std::io::IsTerminal;
use std::sync::OnceLock;
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::{
    fmt::{
        format::{FormatEvent, FormatFields, Writer},
        FmtContext,
    },
    registry::LookupSpan,
    EnvFilter,
};

/// Цвета включены, только если вывод — терминал, не задан NO_COLOR и TERM != dumb.
pub fn color_enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        std::io::stdout().is_terminal()
            && std::env::var_os("NO_COLOR").is_none()
            && std::env::var("TERM").map(|t| t != "dumb").unwrap_or(true)
    })
}

/// Обернуть текст в SGR-код (`"1;92"`, `"38;5;208"`), если цвета включены.
pub fn paint(code: &str, text: impl fmt::Display) -> String {
    if color_enabled() {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

pub fn dim(text: impl fmt::Display) -> String {
    paint("90", text)
}

// ── логи ─────────────────────────────────────────────────────────────────────

struct Pretty;

impl<S, N> FormatEvent<S, N> for Pretty
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let color = writer.has_ansi_escapes();
        let (sym, code) = match *event.metadata().level() {
            Level::ERROR => ("✖", "1;91"),
            Level::WARN => ("▲", "1;93"),
            Level::INFO => ("●", "96"),
            _ => ("·", "90"),
        };
        let time = chrono::Local::now().format("%H:%M:%S");
        if color {
            write!(writer, "\x1b[90m{time}\x1b[0m \x1b[{code}m{sym}\x1b[0m ")?;
        } else {
            write!(writer, "{time} {sym} ")?;
        }
        ctx.field_format().format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

pub fn init_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("rustbot=info,warn")),
        )
        .with_ansi(color_enabled())
        .event_format(Pretty)
        .init();
}

// ── трассировка команд ───────────────────────────────────────────────────────
//
// tracing экранирует ANSI-коды внутри сообщений, поэтому цветные строки
// о выполнении команд печатаем напрямую (в том же стиле, что и логи).

fn stamp() -> String {
    dim(chrono::Local::now().format("%H:%M:%S"))
}

pub fn command_ok(label: &str, ms: u128, chat: i64) {
    println!(
        "{} {} {}  {}  {}",
        stamp(),
        paint("1;92", "✓"),
        paint("1", label),
        dim(format!("{ms} мс")),
        dim(format!("чат {chat}"))
    );
}

/// Увидели ваше исходящее сообщение с префиксом — до выполнения команды.
pub fn seen(chat: i64, text: &str) {
    let short: String = text.chars().take(40).collect();
    println!("{} {} {}  {}", stamp(), dim("↗"), short.replace('\n', " "), dim(format!("чат {chat}")));
}

pub fn skipped(why: &str) {
    println!("{} {} {}", stamp(), paint("93", "○"), why);
}

pub fn command_err(label: &str, module: &str, err: &str) {
    println!(
        "{} {} {}  {}: {}",
        stamp(),
        paint("1;91", "✗"),
        paint("1", label),
        module,
        paint("91", err)
    );
}

// ── баннер ───────────────────────────────────────────────────────────────────

const LOGO: [&str; 3] = [
    "╦═╗╦ ╦╔═╗╔╦╗╔╗ ╔═╗╔╦╗",
    "╠╦╝║ ║╚═╗ ║ ╠╩╗║ ║ ║ ",
    "╩╚═╚═╝╚═╝ ╩ ╚═╝╚═╝ ╩ ",
];

/// Градиент "ржавчины" по колонкам логотипа (256-цветная палитра).
const GRADIENT: [u8; 7] = [214, 208, 208, 202, 202, 166, 130];

pub fn banner() {
    println!();
    for line in LOGO {
        let mut out = String::from("  ");
        for (i, ch) in line.chars().enumerate() {
            if ch == ' ' || !color_enabled() {
                out.push(ch);
            } else {
                let c = GRADIENT[(i / 3).min(GRADIENT.len() - 1)];
                out.push_str(&format!("\x1b[1;38;5;{c}m{ch}\x1b[0m"));
            }
        }
        println!("{out}");
    }
    println!(
        "  {}",
        dim(format!(
            "Telegram userbot · {} · собран {}",
            crate::version::short(),
            crate::version::built_at()
        ))
    );
    println!();
}

/// Блок со строками «название — значение».
pub fn info_block(title: &str, rows: &[(&str, String)], footer: &str) {
    println!("  {} {}", paint("38;5;208", "┌"), paint("1", title));
    for (label, value) in rows {
        let pad = 9usize.saturating_sub(label.chars().count());
        println!(
            "  {} {}{}  {}",
            paint("38;5;208", "│"),
            dim(label),
            " ".repeat(pad),
            value
        );
    }
    println!("  {} {}", paint("38;5;208", "└"), footer);
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logo_is_aligned() {
        let w = LOGO[0].chars().count();
        assert!(LOGO.iter().all(|l| l.chars().count() == w));
    }

    #[test]
    fn paint_is_plain_without_tty() {
        // в тестах stdout не терминал
        assert_eq!(paint("1;91", "x"), "x");
    }
}

#[cfg(test)]
mod demo {
    #[test]
    #[ignore]
    fn demo_output() {
        super::banner();
        super::info_block(
            "Сессия",
            &[
                ("Аккаунт", super::paint("1", "マークシャ (@user)")),
                ("ID", "6150422667".into()),
                ("Префикс", super::paint("1;38;5;208", ".")),
                ("Модули", "17 · команд 41".into()),
                ("Диалоги", "143 в кэше".into()),
            ],
            &format!("{} {}", super::paint("1;92", "онлайн"), super::dim("· жду команды · Ctrl+C — выход")),
        );
        super::init_logging();
        super::command_ok(".ping", 83, 6150422667);
        super::command_err(".update", "updater", "git fetch failed");
        tracing::warn!("cannot getChannelDifference for PeerId(-1002155298579)");
        tracing::error!("Критическая ошибка: пример");
    }
}
