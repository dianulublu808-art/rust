use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::io::Write;
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Простое JSON-хранилище «ключ → значение» с авто-сохранением.
#[derive(Clone)]
pub struct Storage {
    inner: Arc<RwLock<HashMap<String, Value>>>,
    path: PathBuf,
    /// Сериализует запись на диск: без него параллельные flush() пишут в один
    /// и тот же tmp-файл и могут испортить storage.json.
    flush_lock: Arc<Mutex<()>>,
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
            flush_lock: Arc::new(Mutex::new(())),
        })
    }

    pub fn get<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Option<T> {
        let g = self.read_guard();
        g.get(key)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    pub fn set<T: Serialize>(&self, key: &str, value: T) -> Result<()> {
        let v = serde_json::to_value(value)?;
        {
            let mut g = self.write_guard();
            g.insert(key.to_owned(), v);
        }
        self.flush()
    }

    pub fn del(&self, key: &str) -> Result<()> {
        {
            let mut g = self.write_guard();
            g.remove(key);
        }
        self.flush()
    }

    pub fn add_i64_to_list(&self, key: &str, value: i64) -> Result<bool> {
        let changed = {
            let mut map = self.write_guard();
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
            let mut map = self.write_guard();
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
        let g = self.read_guard();
        g.keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect()
    }

    /// Паника в чужом потоке не должна навсегда ломать хранилище.
    fn read_guard(&self) -> RwLockReadGuard<'_, HashMap<String, Value>> {
        self.inner.read().unwrap_or_else(|e| e.into_inner())
    }

    fn write_guard(&self) -> RwLockWriteGuard<'_, HashMap<String, Value>> {
        self.inner.write().unwrap_or_else(|e| e.into_inner())
    }

    fn flush(&self) -> Result<()> {
        // Снимок берём под flush_lock: так порядок записей совпадает с порядком
        // изменений и более старый снимок не затрёт более новый.
        let _guard = self.flush_lock.lock().unwrap_or_else(|e| e.into_inner());
        let json = {
            let g = self.read_guard();
            serde_json::to_vec_pretty(&*g)?
        };
        // атомарная запись: tmp + fsync + rename (переживает убийство процесса)
        let tmp = self.path.with_extension("json.tmp");
        {
            let mut f = std::fs::File::create(&tmp)
                .with_context(|| format!("create {:?}", tmp))?;
            f.write_all(&json)
                .with_context(|| format!("write storage {:?}", tmp))?;
            f.sync_all().with_context(|| format!("fsync {:?}", tmp))?;
        }
        std::fs::rename(&tmp, &self.path)
            .with_context(|| format!("replace storage {:?}", self.path))?;
        Ok(())
    }

    /// Полностью заменить содержимое (для `.restore`).
    pub fn replace_all(&self, map: HashMap<String, Value>) -> Result<()> {
        {
            let mut g = self.write_guard();
            *g = map;
        }
        self.flush()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_writes_keep_file_valid_and_complete() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("storage.json");
        let st = Storage::load(&path).unwrap();

        let threads: Vec<_> = (0..8)
            .map(|t| {
                let st = st.clone();
                std::thread::spawn(move || {
                    for i in 0..40 {
                        st.set(&format!("k.{t}.{i}"), i).unwrap();
                    }
                })
            })
            .collect();
        for th in threads {
            th.join().unwrap();
        }

        // перечитываем с диска: файл валиден и содержит все 320 ключей
        let reloaded = Storage::load(&path).unwrap();
        assert_eq!(reloaded.keys_with_prefix("k.").len(), 320);
        assert!(!path.with_extension("json.corrupt").exists());
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn corrupt_file_is_preserved_not_silently_wiped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("storage.json");
        std::fs::write(&path, "{ not json").unwrap();
        let st = Storage::load(&path).unwrap();
        assert!(st.keys_with_prefix("").is_empty());
        let copy = std::fs::read_to_string(path.with_extension("json.corrupt")).unwrap();
        assert_eq!(copy, "{ not json");
    }

    #[test]
    fn survives_poisoned_lock() {
        let dir = tempfile::tempdir().unwrap();
        let st = Storage::load(dir.path().join("s.json")).unwrap();
        let st2 = st.clone();
        let _ = std::thread::spawn(move || {
            let _g = st2.write_guard();
            panic!("boom");
        })
        .join();
        st.set("a", 1).unwrap();
        assert_eq!(st.get::<i32>("a"), Some(1));
    }
}
