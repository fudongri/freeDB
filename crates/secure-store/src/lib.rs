use aes::Aes128;
use anyhow::Result;
use cbc::cipher::block_padding::Pkcs7;
use cbc::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use connection_store::database_path;
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use std::{fs, path::PathBuf, sync::Arc};

const SERVICE_NAME: &str = "freedb";
const ENCRYPTION_KEY: &[u8; 16] = b"fdr6668888\x00\x00\x00\x00\x00\x00";

#[derive(Clone)]
pub struct SecureStore {
    connection: Arc<Mutex<Connection>>,
}

impl SecureStore {
    pub fn new() -> Result<Self> {
        let path = database_path()?;
        let connection = Connection::open(path)?;
        let store = Self {
            connection: Arc::new(Mutex::new(connection)),
        };
        store.init()?;
        store.migrate_from_legacy_store()?;
        store.migrate_legacy_json()?;
        Ok(store)
    }

    pub fn save_password(&self, connection_id: &str, password: &str) -> Result<()> {
        let encrypted = encrypt(password.as_bytes());
        let connection = self.connection.lock();
        connection.execute(
            "INSERT OR REPLACE INTO passwords (connection_id, encrypted_password)
             VALUES (?1, ?2)",
            params![connection_id, encrypted],
        )?;
        Ok(())
    }

    pub fn load_password(&self, connection_id: &str) -> Result<Option<String>> {
        let connection = self.connection.lock();
        let result: Option<Vec<u8>> = connection
            .query_row(
                "SELECT encrypted_password FROM passwords WHERE connection_id = ?1",
                params![connection_id],
                |row| row.get(0),
            )
            .optional()?;
        match result {
            Some(encrypted) => {
                let decrypted = decrypt(&encrypted)?;
                Ok(Some(String::from_utf8(decrypted)?))
            }
            None => Ok(None),
        }
    }

    pub fn delete_password(&self, connection_id: &str) -> Result<()> {
        let connection = self.connection.lock();
        connection.execute(
            "DELETE FROM passwords WHERE connection_id = ?1",
            params![connection_id],
        )?;
        Ok(())
    }

    /// 加载加密后的密码（不解密），用于导出
    pub fn load_encrypted_password(&self, connection_id: &str) -> Result<Option<Vec<u8>>> {
        let connection = self.connection.lock();
        let result: Option<Vec<u8>> = connection
            .query_row(
                "SELECT encrypted_password FROM passwords WHERE connection_id = ?1",
                params![connection_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(result)
    }

    /// 保存已加密的密码，用于导入
    pub fn save_encrypted_password(&self, connection_id: &str, encrypted: &[u8]) -> Result<()> {
        let connection = self.connection.lock();
        connection.execute(
            "INSERT OR REPLACE INTO passwords (connection_id, encrypted_password)
             VALUES (?1, ?2)",
            params![connection_id, encrypted],
        )?;
        Ok(())
    }

    fn init(&self) -> Result<()> {
        let connection = self.connection.lock();
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS passwords (
                connection_id TEXT PRIMARY KEY,
                encrypted_password BLOB NOT NULL
            );",
        )?;
        Ok(())
    }

    /// 从旧的 secure-store.sqlite3 迁移数据到新位置
    fn migrate_from_legacy_store(&self) -> Result<()> {
        let dir = primary_data_dir()?;
        let legacy_path = dir.join("secure-store.sqlite3");
        if !legacy_path.exists() {
            return Ok(());
        }
        let legacy_conn = Connection::open(&legacy_path)?;
        let mut stmt = legacy_conn.prepare(
            "SELECT connection_id, encrypted_password FROM passwords",
        )?;
        let passwords: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .filter_map(|r| r.ok())
            .collect();

        if passwords.is_empty() {
            let _ = fs::remove_file(&legacy_path);
            return Ok(());
        }

        let connection = self.connection.lock();
        for (id, encrypted) in passwords {
            connection.execute(
                "INSERT OR IGNORE INTO passwords (connection_id, encrypted_password)
                 VALUES (?1, ?2)",
                params![id, encrypted],
            )?;
        }
        drop(connection);
        let _ = fs::remove_file(&legacy_path);
        Ok(())
    }

    fn migrate_legacy_json(&self) -> Result<()> {
        let legacy_path = legacy_json_path()?;
        if !legacy_path.exists() {
            return Ok(());
        }
        let content = fs::read_to_string(&legacy_path)?;
        if content.trim().is_empty() {
            return Ok(());
        }
        let store: serde_json::Value = serde_json::from_str(&content)?;
        let Some(entries) = store.get("entries").and_then(|v| v.as_object()) else {
            return Ok(());
        };
        if entries.is_empty() {
            return Ok(());
        }
        let connection = self.connection.lock();
        for (id, value) in entries {
            if let Some(password) = value.as_str() {
                let encrypted = encrypt(password.as_bytes());
                connection.execute(
                    "INSERT OR IGNORE INTO passwords (connection_id, encrypted_password)
                     VALUES (?1, ?2)",
                    params![id, encrypted],
                )?;
            }
        }
        drop(connection);
        let _ = fs::remove_file(&legacy_path);
        Ok(())
    }
}

fn encrypt(data: &[u8]) -> Vec<u8> {
    let mut iv = [0u8; 16];
    getrandom::fill(&mut iv).expect("failed to generate random IV");
    let cipher = cbc::Encryptor::<Aes128>::new(ENCRYPTION_KEY.into(), &iv.into());
    let encrypted = cipher.encrypt_padded_vec_mut::<Pkcs7>(data);
    let mut result = Vec::with_capacity(16 + encrypted.len());
    result.extend_from_slice(&iv);
    result.extend_from_slice(&encrypted);
    result
}

fn decrypt(data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < 32 || data.len() % 16 != 0 {
        return Err(anyhow::anyhow!("invalid encrypted data length"));
    }
    let (iv, ciphertext) = data.split_at(16);
    let cipher = cbc::Decryptor::<Aes128>::new(ENCRYPTION_KEY.into(), iv.into());
    cipher
        .decrypt_padded_vec_mut::<Pkcs7>(ciphertext)
        .map_err(|_| anyhow::anyhow!("decryption failed"))
}

fn legacy_json_path() -> Result<PathBuf> {
    let dir = primary_data_dir()?;
    Ok(dir.join("credentials.json"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let passwords = ["", "123456", "dexintest@2024", "B5j9JulWR6in2lnjwY88nnwGc9cuhA"];
        for password in &passwords {
            let encrypted = encrypt(password.as_bytes());
            let decrypted = decrypt(&encrypted).unwrap();
            assert_eq!(*password, String::from_utf8(decrypted).unwrap());
        }
    }

    #[test]
    fn encrypt_produces_different_ciphertext_each_time() {
        let password = b"test_password";
        let enc1 = encrypt(password);
        let enc2 = encrypt(password);
        assert_ne!(enc1, enc2); // 不同 IV 产生不同密文
        assert_eq!(decrypt(&enc1).unwrap(), decrypt(&enc2).unwrap()); // 解密后结果相同
    }

    #[test]
    fn decrypt_invalid_data_returns_error() {
        assert!(decrypt(&[0u8; 16]).is_err());
        assert!(decrypt(&[0u8; 32]).is_err());
    }
}

