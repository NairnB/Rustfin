//! Saved servers and sessions (SPEC §6, R-SET-*). Stored as `settings.json` through `Storage`.
//! Signatures fixed by the lead; bodies are packet C2.
use crate::api::Session;
use crate::platform::Storage;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const SETTINGS_KEY: &str = "settings.json";

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct ServerEntry {
    /// Jellyfin server Id (from /System/Info/Public).
    pub id: String,
    pub name: String,
    /// Normalised base address.
    pub address: String,
    pub session: Option<Session>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Random v4-style 32-hex id, created once (R-API-1).
    pub device_id: String,
    pub servers: Vec<ServerEntry>,
    /// `id` of the server used last.
    pub last_server: Option<String>,
}

impl Settings {
    /// Reads settings; missing or corrupt file → fresh settings with a new `device_id` (and saves nothing).
    pub fn load(store: &dyn Storage) -> Settings {
        match store.read(SETTINGS_KEY) {
            Some(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|_| Settings::fresh()),
            None => Settings::fresh(),
        }
    }
    pub fn save(&self, store: &dyn Storage) -> std::io::Result<()> {
        let bytes = serde_json::to_vec(self).map_err(io_err)?;
        store.write(SETTINGS_KEY, &bytes)
    }
    /// Insert or update by `id` (keeps list order; new ones go to the end) and set `last_server`.
    pub fn remember(&mut self, entry: ServerEntry) {
        self.last_server = Some(entry.id.clone());
        match self.servers.iter_mut().find(|e| e.id == entry.id) {
            Some(slot) => *slot = entry,
            None => self.servers.push(entry),
        }
    }
    pub fn last(&self) -> Option<&ServerEntry> {
        let id = self.last_server.as_deref()?;
        self.servers.iter().find(|e| e.id == id)
    }
    /// R-SET-4: drop the session of server `id` (keeps the server).
    pub fn forget_session(&mut self, id: &str) {
        if let Some(e) = self.servers.iter_mut().find(|e| e.id == id) {
            e.session = None;
        }
    }
}

impl Settings {
    fn fresh() -> Settings {
        Settings { device_id: new_device_id(), ..Default::default() }
    }
}

fn io_err(e: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
}

/// 32 lower-case hex chars from OS randomness (fall back to time + address entropy if unavailable).
pub fn new_device_id() -> String {
    let mut buf = [0u8; 16];
    if getrandom::fill(&mut buf).is_err() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let mut seed = nanos
            ^ COUNTER.fetch_add(1, Ordering::Relaxed).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (std::process::id() as u64).wrapping_mul(0x0001_0000_0000_0001);
        for b in buf.iter_mut() {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            *b = (seed & 0xff) as u8;
        }
    }
    let mut out = String::with_capacity(32);
    for b in buf {
        out.push(char::from_digit((b >> 4) as u32, 16).unwrap());
        out.push(char::from_digit((b & 0x0f) as u32, 16).unwrap());
    }
    out
}

/// Files under one directory: `dir/<key>`, cache in `dir/cache`. Atomic writes (temp file + rename).
pub struct FileStorage {
    pub dir: PathBuf,
}
impl FileStorage {
    pub fn new(dir: PathBuf) -> std::io::Result<FileStorage> {
        std::fs::create_dir_all(&dir)?;
        Ok(FileStorage { dir })
    }
}
impl Storage for FileStorage {
    fn read(&self, key: &str) -> Option<Vec<u8>> {
        std::fs::read(self.dir.join(key)).ok()
    }
    fn write(&self, key: &str, data: &[u8]) -> std::io::Result<()> {
        let path = self.dir.join(key);
        let tmp = self.dir.join(format!("{key}.tmp"));
        std::fs::write(&tmp, data)?;
        std::fs::rename(&tmp, &path)
    }
    fn cache_dir(&self) -> PathBuf {
        let dir = self.dir.join("cache");
        let _ = std::fs::create_dir_all(&dir);
        dir
    }
}

/// In-memory storage for tests (cache dir = a temp dir made on first use).
#[derive(Default)]
pub struct MemStorage {
    pub files: std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>,
}
impl Storage for MemStorage {
    fn read(&self, key: &str) -> Option<Vec<u8>> {
        self.files.lock().unwrap().get(key).cloned()
    }
    fn write(&self, key: &str, data: &[u8]) -> std::io::Result<()> {
        self.files.lock().unwrap().insert(key.to_string(), data.to_vec());
        Ok(())
    }
    fn cache_dir(&self) -> PathBuf {
        std::env::temp_dir().join("jf-memstorage-cache")
    }
}
