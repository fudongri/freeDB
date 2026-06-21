use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, path::PathBuf};

const SERVICE_NAME: &str = "freedb";

#[derive(Clone, Default)]
pub struct SecureStore;

impl SecureStore {
    pub fn save_password(&self, connection_id: &str, password: &str) -> Result<()> {
        let path = store_path()?;
        let mut store = read_store(&path)?;
        store.entries.insert(connection_id.to_string(), password.to_string());
        write_store(&path, &store)
    }

    pub fn load_password(&self, connection_id: &str) -> Result<Option<String>> {
        let path = store_path()?;
        let store = read_store(&path)?;
        Ok(store.entries.get(connection_id).cloned())
    }

    pub fn delete_password(&self, connection_id: &str) -> Result<()> {
        let path = store_path()?;
        if !path.exists() {
            return Ok(());
        }
        let mut store = read_store(&path)?;
        store.entries.remove(connection_id);
        write_store(&path, &store)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct PasswordStore {
    entries: HashMap<String, String>,
}

fn store_path() -> Result<PathBuf> {
    let dir = primary_data_dir()?;
    let path = dir.join("credentials.json");
    migrate_legacy_if_needed(&path)?;
    Ok(path)
}

fn primary_data_dir() -> Result<PathBuf> {
    for dir in candidate_data_dirs() {
        if ensure_dir_writable(&dir).is_ok() {
            return Ok(dir);
        }
    }
    Err(anyhow::anyhow!("unable to create secure store directory"))
}

fn candidate_data_dirs() -> Vec<PathBuf> {
    [
        dirs::data_local_dir().map(|p| p.join(SERVICE_NAME)),
        std::env::current_dir().ok().map(|p| p.join(format!(".{}-data", SERVICE_NAME))),
        dirs::home_dir().map(|p| p.join(format!(".{}", SERVICE_NAME))),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn ensure_dir_writable(dir: &PathBuf) -> Result<()> {
    fs::create_dir_all(dir)?;
    let probe = dir.join(".write-test");
    fs::write(&probe, b"ok")?;
    fs::remove_file(probe)?;
    Ok(())
}

fn read_store(path: &PathBuf) -> Result<PasswordStore> {
    if !path.exists() {
        return Ok(PasswordStore::default());
    }
    let content = fs::read_to_string(path)?;
    if content.trim().is_empty() {
        return Ok(PasswordStore::default());
    }
    serde_json::from_str(&content).context("failed to parse credential store")
}

fn write_store(path: &PathBuf, store: &PasswordStore) -> Result<()> {
    let content = serde_json::to_string_pretty(store)?;
    fs::write(path, content).context("failed to write credential store")
}

fn legacy_dirs(target: &PathBuf) -> Vec<PathBuf> {
    let mut candidate = candidate_data_dirs()
        .into_iter()
        .filter(|d| d.join("credentials.json") != *target)
        .collect::<Vec<_>>();

    // 从旧 uudb 目录迁移
    let uudb_legacy: Vec<PathBuf> = [
        dirs::data_local_dir().map(|p| p.join("uudb")),
        std::env::current_dir().ok().map(|p| p.join(".uudb-data")),
        dirs::home_dir().map(|p| p.join(".uudb")),
    ]
    .into_iter()
    .flatten()
    .collect();
    candidate.extend(uudb_legacy);

    candidate
}

fn migrate_legacy_if_needed(target: &PathBuf) -> Result<()> {
    if count_entries(target)? > 0 {
        return Ok(());
    }
    for legacy_dir in legacy_dirs(target) {
        let legacy = legacy_dir.join("credentials.json");
        if legacy == *target || !legacy.exists() {
            continue;
        }
        if count_entries(&legacy)? == 0 {
            continue;
        }
        fs::copy(&legacy, target)?;
        return Ok(());
    }
    Ok(())
}

fn count_entries(path: &PathBuf) -> Result<usize> {
    if !path.exists() {
        return Ok(0);
    }
    Ok(read_store(path)?.entries.len())
}
