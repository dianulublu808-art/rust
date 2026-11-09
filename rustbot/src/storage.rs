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
            match serde_json::from_str(&text) {
                Ok(map) => map,
                Err(e) => {
                    // не затираем повреждённый файл молча — сохраняем копию
                    let corrupt = path.with_extension("json.corrupt");
                    let _ = std::fs::copy(&path, &corrupt);
                    tracing::error!(
                        "Хранилище повреждено ({e}); копия сохранена в {:?}, начинаю с пустого",
                        corrupt
                    );
                    HashMap::new()
                }
            }
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
        // атомарная запись: tmp + rename, чтобы сбой не оставил обрезанный файл
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, json).with_context(|| format!("write storage {:?}", tmp))?;
        std::fs::rename(&tmp, &self.path)
            .with_context(|| format!("replace storage {:?}", self.path))?;
        Ok(())
    }

    /// Полностью заменить содержимое (для `.restore`).
    pub fn replace_all(&self, map: HashMap<String, Value>) -> Result<()> {
        {
            let mut g = self
                .inner
                .write()
                .map_err(|_| anyhow::anyhow!("storage lock poisoned"))?;
            *g = map;
        }
        self.flush()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
