//! Image cache rules (SPEC §10, R-IMG-*). Disk cache + LRU + prefetch window. Bodies are packet C3.
use std::collections::HashMap;
use std::hash::Hash;
use std::ops::Range;
use std::path::PathBuf;

/// FNV-1a 64-bit of the URL with any `api_key=` query parameter removed, as 16 lower-case hex chars.
pub fn cache_key(url: &str) -> String {
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;

    let (base, query) = url.split_once('?').unwrap_or((url, ""));
    let mut filtered = String::with_capacity(url.len());
    filtered.push_str(base);
    if !query.is_empty() {
        let kept: Vec<&str> = query
            .split('&')
            .filter(|seg| {
                let key = seg.split_once('=').map_or(*seg, |(k, _)| k);
                key != "api_key"
            })
            .collect();
        if !kept.is_empty() {
            filtered.push('?');
            filtered.push_str(&kept.join("&"));
        }
    }

    let mut hash = OFFSET;
    for b in filtered.bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(PRIME);
    }
    format!("{:016x}", hash)
}

/// Files `dir/<key>.img`. LRU by modification time (a hit refreshes it). `trim` evicts oldest until ≤ 90 % of max.
pub struct DiskCache {
    pub dir: PathBuf,
    pub max_bytes: u64,
}
impl DiskCache {
    fn path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.img"))
    }

    fn entries(&self) -> Vec<(PathBuf, u64, std::time::SystemTime)> {
        let mut out = Vec::new();
        let Ok(rd) = std::fs::read_dir(&self.dir) else {
            return out;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("img") {
                continue;
            }
            if let Ok(md) = entry.metadata() {
                if md.is_file() {
                    let mtime = md.modified().unwrap_or(std::time::UNIX_EPOCH);
                    out.push((path, md.len(), mtime));
                }
            }
        }
        out
    }

    pub fn new(dir: PathBuf, max_bytes: u64) -> std::io::Result<DiskCache> {
        std::fs::create_dir_all(&dir)?;
        Ok(DiskCache { dir, max_bytes })
    }

    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        let path = self.path(key);
        let data = std::fs::read(&path).ok()?;
        if let Ok(f) = std::fs::OpenOptions::new().write(true).open(&path) {
            let _ = f.set_modified(std::time::SystemTime::now());
        }
        Some(data)
    }

    /// Atomic write (temp + rename); calls `trim` when the total goes over `max_bytes`.
    pub fn put(&self, key: &str, data: &[u8]) -> std::io::Result<()> {
        let path = self.path(key);
        let tmp = self.dir.join(format!(".{key}.tmp"));
        std::fs::write(&tmp, data)?;
        std::fs::rename(&tmp, &path)?;
        if self.total_bytes() > self.max_bytes {
            self.trim();
        }
        Ok(())
    }

    pub fn total_bytes(&self) -> u64 {
        self.entries().iter().map(|(_, s, _)| *s).sum()
    }

    pub fn trim(&self) {
        let target = self.max_bytes * 9 / 10;
        let mut entries = self.entries();
        entries.sort_by_key(|(_, _, m)| *m);
        let mut total: u64 = entries.iter().map(|(_, s, _)| *s).sum();
        for (path, size, _) in entries {
            if total <= target {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                total -= size;
            }
        }
    }
}

/// R-IMG-2: rows to have images for — the visible rows plus one row ahead in the scroll direction, clamped.
pub fn wanted_rows(first_visible: usize, last_visible: usize, total_rows: usize, scrolling_down: bool) -> Range<usize> {
    let mut start = first_visible.min(total_rows);
    let mut end = (last_visible + 1).min(total_rows);
    if scrolling_down {
        end = (end + 1).min(total_rows);
    } else {
        start = start.saturating_sub(1);
    }
    start..end
}

/// Small LRU map (textures in memory). `insert` returns the values it evicted so the caller can free them.
pub struct Lru<K, V> {
    pub cap: usize,
    map: HashMap<K, (V, u64)>,
    tick: u64,
}
impl<K: Hash + Eq + Clone, V> Lru<K, V> {
    pub fn new(cap: usize) -> Self {
        Lru { cap, map: HashMap::new(), tick: 0 }
    }
    /// Marks as recently used.
    pub fn get(&mut self, k: &K) -> Option<&V> {
        self.tick += 1;
        let tick = self.tick;
        self.map.get_mut(k).map(|entry| {
            entry.1 = tick;
            &entry.0
        })
    }
    pub fn contains(&self, k: &K) -> bool {
        self.map.contains_key(k)
    }
    pub fn len(&self) -> usize {
        self.map.len()
    }
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
    pub fn insert(&mut self, k: K, v: V) -> Vec<V> {
        let mut evicted = Vec::new();
        if self.map.contains_key(&k) {
            if let Some((old, _)) = self.map.remove(&k) {
                evicted.push(old);
            }
        } else if self.map.len() >= self.cap {
            if let Some(lk) = self.map.iter().min_by_key(|(_, (_, t))| *t).map(|(k, _)| k.clone()) {
                if let Some((old, _)) = self.map.remove(&lk) {
                    evicted.push(old);
                }
            }
        }
        self.tick += 1;
        self.map.insert(k, (v, self.tick));
        evicted
    }
}
