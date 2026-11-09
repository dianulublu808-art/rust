//! Единый стиль оформления ответов: карточки, цитаты, индикаторы.

use std::sync::OnceLock;
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();

/// Вызвать один раз при старте — от этого момента считается аптайм бота.
pub fn init() {
    let _ = START.get_or_init(Instant::now);
}

/// Аптайм процесса бота в секундах.
pub fn process_uptime() -> u64 {
    START.get().map(|s| s.elapsed().as_secs()).unwrap_or(0)
}

/// Индикатор из 10 делений: `▰▰▰▱▱▱▱▱▱▱`.
pub fn bar(pct: u64) -> String {
    let pct = pct.min(100);
    let filled = ((pct + 5) / 10) as usize;
    format!("{}{}", "▰".repeat(filled), "▱".repeat(10 - filled))
}

/// Размер в человекочитаемом виде.
pub fn bytes(b: u64) -> String {
    const K: f64 = 1024.0;
    let f = b as f64;
    if f >= K * K * K * K {
        format!("{:.1} ТБ", f / (K * K * K * K))
    } else if f >= K * K * K {
        format!("{:.1} ГБ", f / (K * K * K))
    } else if f >= K * K {
        format!("{:.0} МБ", f / (K * K))
    } else if f >= K {
        format!("{:.0} КБ", f / K)
    } else {
        format!("{b} Б")
    }
}

/// Длительность: `3д 4ч`, `2ч 15м`, `5м 7с`, `12с`.
pub fn duration(secs: u64) -> String {
    let d = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if d > 0 {
        format!("{d}д {h}ч")
    } else if h > 0 {
        format!("{h}ч {m}м")
    } else if m > 0 {
        format!("{m}м {s}с")
    } else {
        format!("{s}с")
    }
}

/// Блок-цитата из строк.
pub fn quote(rows: &[String]) -> String {
    format!("<blockquote>{}</blockquote>", rows.join("\n"))
}

/// Сворачиваемая цитата (для длинных текстов).
pub fn quote_expandable(text: &str) -> String {
    format!("<blockquote expandable>{text}</blockquote>")
}

/// Карточка: жирный заголовок с иконкой и цитата со строками.
pub fn card(icon: &str, title: &str, rows: &[String]) -> String {
    format!("<b>{icon} {title}</b>\n{}", quote(rows))
}

/// Успех.
pub fn ok(text: &str) -> String {
    format!("✅ {text}")
}

/// Ошибка (text уже должен быть экранирован).
pub fn err(text: &str) -> String {
    format!("⛔ <b>Ошибка</b>\n{}", quote(&[text.to_owned()]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_scale() {
        assert_eq!(bar(0), "▱▱▱▱▱▱▱▱▱▱");
        assert_eq!(bar(50), "▰▰▰▰▰▱▱▱▱▱");
        assert_eq!(bar(100), "▰▰▰▰▰▰▰▰▰▰");
        assert_eq!(bar(250), "▰▰▰▰▰▰▰▰▰▰");
    }

    #[test]
    fn duration_format() {
        assert_eq!(duration(5), "5с");
        assert_eq!(duration(125), "2м 5с");
        assert_eq!(duration(3 * 3600 + 120), "3ч 2м");
        assert_eq!(duration(2 * 86400 + 5 * 3600), "2д 5ч");
    }

    #[test]
    fn bytes_format() {
        assert_eq!(bytes(512), "512 Б");
        assert_eq!(bytes(2048), "2 КБ");
        assert_eq!(bytes(3 * 1024 * 1024 * 1024), "3.0 ГБ");
    }
}
