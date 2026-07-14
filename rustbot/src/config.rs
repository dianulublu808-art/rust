use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub api_id: i32,
    pub api_hash: String,
    pub phone: String,
    pub session_path: String,
    /// Символ-префикс команд (например ".")
    pub prefix: String,
    /// Telegram ID владельца (необязательно)
    pub owner_id: Option<i64>,
    /// Таймаут выполнения .eval в секундах
    #[serde(default = "default_eval_timeout")]
    pub eval_timeout: u64,
    /// Таймаут выполнения .exec в секундах
    #[serde(default = "default_exec_timeout")]
    pub exec_timeout: u64,
}

fn default_eval_timeout() -> u64 { 15 }
fn default_exec_timeout() -> u64 { 10 }

impl Config {
    pub fn load(path: &str) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Не удалось прочитать конфиг '{path}'"))?;
        toml::from_str(&content)
            .with_context(|| "Не удалось распарсить config.toml")
    }

    /// Возвращает первый символ префикса
    pub fn prefix_char(&self) -> char {
        self.prefix.chars().next().unwrap_or('.')
    }
}
