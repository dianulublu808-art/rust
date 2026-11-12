//! Информация о сборке (см. build.rs).

/// Короткий хеш коммита; `+` в конце — в рабочей папке были правки.
pub const GIT: &str = env!("RUSTBOT_GIT");

/// "1.0.0"
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Время сборки, unix-секунды.
pub fn build_ts() -> i64 {
    env!("RUSTBOT_BUILD_TS").parse().unwrap_or(0)
}

/// "v1.0.0 · d758cae"
pub fn short() -> String {
    format!("v{VERSION} · {GIT}")
}

/// "03.11 14:25" — когда собран бинарник (локальное время).
pub fn built_at() -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_opt(build_ts(), 0)
        .single()
        .map(|t| t.format("%d.%m %H:%M").to_string())
        .unwrap_or_else(|| "?".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_is_embedded() {
        assert!(!GIT.is_empty());
        assert!(short().contains(VERSION));
        assert!(build_ts() > 1_700_000_000);
        assert_ne!(built_at(), "?");
    }
}
