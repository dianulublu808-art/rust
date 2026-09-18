use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub api_id: i32,
    pub api_hash: String,
    pub phone: String,
    pub session_path: String,
    pub prefix: String,
    #[allow(dead_code)]
    pub owner_id: Option<i64>,
    #[serde(default = "default_storage_path")]
    pub storage_path: String,
    #[serde(default = "default_eval_timeout")]
    pub eval_timeout: u64,
    #[serde(default = "default_exec_timeout")]
    pub exec_timeout: u64,
}

fn default_storage_path() -> String { "storage.json".to_owned() }
fn default_eval_timeout() -> u64 { 30 }
fn default_exec_timeout() -> u64 { 60 }

impl Config {
    pub fn load(path: &str) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Не удалось прочитать конфиг '{path}'"))?;
        toml::from_str(&content)
            .with_context(|| "Не удалось распарсить config.toml")
    }

    pub fn prefix_char(&self) -> char {
        self.prefix.chars().next().unwrap_or('.')
    }
}
