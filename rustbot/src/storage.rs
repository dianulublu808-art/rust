use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

/// Простое JSON-хранилище «ключ → значение» с авто-сохранением.
#[derive(Clone)]
pub struct Storage {
    inner: Arc<RwLock<HashMap<String, Value>>>,
    path: PathBuf,
}

impl Storage {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let map: HashMap<String, Value> = if path.exists() {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("read storage {:?}", path))?;
            serde_json::from_str(&text).unwrap_or_default()
        } else {
            HashMap::new()
        };
        Ok(Self {
            inner: Arc::new(RwLock::new(map)),
            path,
        })
    }

    pub fn get<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Option<T> {
        let g = self.inner.read().ok()?;
        g.get(key)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    pub fn set<T: Serialize>(&self, key: &str, value: T) -> Result<()> {
        let v = serde_json::to_value(value)?;
        {
            let mut g = self.inner.write().map_err(|_| anyhow::anyhow!("lock"))?;
            g.insert(key.to_owned(), v);
        }
        self.flush()
    }

    pub fn del(&self, key: &str) -> Result<()> {
        {
            let mut g = self.inner.write().map_err(|_| anyhow::anyhow!("lock"))?;
            g.remove(key);
        }
        self.flush()
    }

    pub fn add_i64_to_list(&self, key: &str, value: i64) -> Result<bool> {
        let changed = {
            let mut map = self
                .inner
                .write()
                .map_err(|_| anyhow::anyhow!("storage lock poisoned"))?;
            let list = map
                .entry(key.to_owned())
                .or_insert_with(|| Value::Array(Vec::new()));
            let values = list
                .as_array_mut()
                .ok_or_else(|| anyhow::anyhow!("storage key '{key}' is not a list"))?;
            if values.iter().any(|item| item.as_i64() == Some(value)) {
                false
            } else {
                values.push(Value::from(value));
                true
            }
        };
        if changed {
            self.flush()?;
        }
        Ok(changed)
    }

    pub fn remove_i64_from_list(&self, key: &str, value: i64) -> Result<bool> {
        let changed = {
            let mut map = self
                .inner
                .write()
                .map_err(|_| anyhow::anyhow!("storage lock poisoned"))?;
            let Some(list) = map.get_mut(key) else {
                return Ok(false);
            };
            let values = list
                .as_array_mut()
                .ok_or_else(|| anyhow::anyhow!("storage key '{key}' is not a list"))?;
            let before = values.len();
            values.retain(|item| item.as_i64() != Some(value));
            values.len() != before
        };
        if changed {
            self.flush()?;
        }
        Ok(changed)
    }

    pub fn keys_with_prefix(&self, prefix: &str) -> Vec<String> {
        let g = self.inner.read().unwrap_or_else(|e| e.into_inner());
        g.keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect()
    }

    fn flush(&self) -> Result<()> {
        let g = self.inner.read().map_err(|_| anyhow::anyhow!("lock"))?;
        let json = serde_json::to_string_pretty(&*g)?;
        std::fs::write(&self.path, json)
            .with_context(|| format!("write storage {:?}", self.path))?;
        Ok(())
    }
}
