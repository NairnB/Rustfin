//! F5 filmography from TMDB (SPEC R-F5-1/2). Blocking client + pure helpers. Bodies: packet F5a.
//! The API key comes from the user's settings only; never in the code or the repo.
use crate::api::{Item, ItemKind};
use crate::images::cache_key;
use std::cmp::Ordering;
use std::path::PathBuf;
use std::time::Duration;

/// Cache lifetime (R-F5-2): a cached response is reused only while it is younger than 7 days.
const CACHE_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Movie,
    Tv,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dept {
    Acting,
    Crew,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Credit {
    pub tmdb_id: u64,
    pub kind: MediaKind,
    pub dept: Dept,
    /// movie `title` or tv `name`
    pub title: String,
    /// from `release_date` / `first_air_date` (YYYY-…)
    pub year: Option<i32>,
    /// `character` for acting, `job` for crew
    pub role: String,
    pub overview: String,
    pub poster_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TmdbError {
    NoKey,
    BadKey,
    Network(String),
    Decode(String),
}

pub struct Tmdb {
    pub key: String,
    /// `https://api.themoviedb.org/3` in the app; a local stub in tests.
    pub base: String,
    /// Responses cached here as files (7 days).
    pub cache_dir: Option<PathBuf>,
}

impl Tmdb {
    pub fn new(key: &str, cache_dir: Option<PathBuf>) -> Tmdb {
        Tmdb { key: key.trim().to_string(), base: "https://api.themoviedb.org/3".into(), cache_dir }
    }

    /// Cache file for `url`; named from the URL with `api_key=` removed, so the key never touches the
    /// file system. Stored under `<cache_dir>/tmdb/<key>.json`.
    fn cache_path(&self, url: &str) -> Option<PathBuf> {
        let dir = self.cache_dir.as_ref()?;
        Some(dir.join("tmdb").join(format!("{}.json", cache_key(url))))
    }

    /// A cached body that is still within `CACHE_MAX_AGE`, if any.
    fn cache_read(&self, url: &str) -> Option<String> {
        let path = self.cache_path(url)?;
        let md = std::fs::metadata(&path).ok()?;
        let age = md.modified().ok()?.elapsed().ok()?;
        if age > CACHE_MAX_AGE {
            return None;
        }
        std::fs::read_to_string(&path).ok()
    }

    /// Store the raw response body only (never the URL, so never the key).
    fn cache_write(&self, url: &str, body: &str) {
        let Some(path) = self.cache_path(url) else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&path, body);
    }

    /// `GET {base}{path}` (path includes its query string) → the raw body. Empty key → NoKey; HTTP 401 →
    /// BadKey. Served from the disk cache when fresh.
    fn get_body(&self, path: &str) -> Result<String, TmdbError> {
        if self.key.is_empty() {
            return Err(TmdbError::NoKey);
        }
        let url = format!("{}{}", self.base, path);
        if let Some(body) = self.cache_read(&url) {
            return Ok(body);
        }
        let mut resp = ureq::get(&url).call().map_err(|e| match e {
            ureq::Error::StatusCode(401) => TmdbError::BadKey,
            other => TmdbError::Network(other.to_string()),
        })?;
        let body = resp.body_mut().read_to_string().map_err(|e| TmdbError::Decode(e.to_string()))?;
        if self.cache_dir.is_some() {
            self.cache_write(&url, &body);
        }
        Ok(body)
    }

    /// `GET {base}/search/person?query=<name>&api_key=<key>` → id of the first result. Empty key → NoKey;
    /// HTTP 401 → BadKey.
    pub fn find_person(&self, name: &str) -> Result<Option<u64>, TmdbError> {
        let body = self.get_body(&format!("/search/person?query={}&api_key={}", url_encode(name), self.key))?;
        let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| TmdbError::Decode(e.to_string()))?;
        Ok(v.get("results")
            .and_then(|r| r.as_array())
            .and_then(|a| a.first())
            .and_then(|r| r.get("id"))
            .and_then(|i| i.as_u64()))
    }

    /// `GET {base}/person/{id}/combined_credits?api_key=<key>` → cast as Acting, crew as Crew.
    pub fn credits(&self, person_id: u64) -> Result<Vec<Credit>, TmdbError> {
        let body = self.get_body(&format!("/person/{person_id}/combined_credits?api_key={}", self.key))?;
        let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| TmdbError::Decode(e.to_string()))?;
        let mut out = Vec::new();
        for (dept, key) in [(Dept::Acting, "cast"), (Dept::Crew, "crew")] {
            if let Some(arr) = v.get(key).and_then(|x| x.as_array()) {
                out.extend(arr.iter().map(|e| parse_credit(e, dept)));
            }
        }
        Ok(out)
    }

    /// Poster URL at w342: `https://image.tmdb.org/t/p/w342{poster_path}`.
    pub fn poster_url(poster_path: &str) -> String {
        format!("https://image.tmdb.org/t/p/w342{poster_path}")
    }
}

/// Percent-encode a query value (RFC 3986 unreserved left as-is).
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// One `cast`/`crew` entry → a `Credit` (movie `title`/`release_date`, tv `name`/`first_air_date`).
fn parse_credit(v: &serde_json::Value, dept: Dept) -> Credit {
    let kind = match v.get("media_type").and_then(|x| x.as_str()) {
        Some("tv") => MediaKind::Tv,
        _ => MediaKind::Movie,
    };
    let (title_key, date_key) = match kind {
        MediaKind::Tv => ("name", "first_air_date"),
        MediaKind::Movie => ("title", "release_date"),
    };
    let title = v.get(title_key).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let year = v.get(date_key).and_then(|x| x.as_str()).and_then(|d| d.get(..4)).and_then(|y| y.parse::<i32>().ok());
    let role_key = match dept {
        Dept::Acting => "character",
        Dept::Crew => "job",
    };
    Credit {
        tmdb_id: v.get("id").and_then(|x| x.as_u64()).unwrap_or(0),
        kind,
        dept,
        title,
        year,
        role: v.get(role_key).and_then(|x| x.as_str()).unwrap_or("").to_string(),
        overview: v.get("overview").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        poster_path: v.get("poster_path").and_then(|x| x.as_str()).map(str::to_string),
    }
}

/// Acting and crew lists, each newest first (year descending, no year last, then title A–Z); a title that appears
/// twice in one list (e.g. two crew jobs) is kept once with the roles joined by ", ".
pub fn split_sorted(credits: &[Credit]) -> (Vec<Credit>, Vec<Credit>) {
    let acting = merge(credits.iter().filter(|c| c.dept == Dept::Acting).cloned().collect());
    let crew = merge(credits.iter().filter(|c| c.dept == Dept::Crew).cloned().collect());
    (sort_newest(acting), sort_newest(crew))
}

/// Merge duplicate titles in a list (in first-seen order), joining roles with ", ".
fn merge(list: Vec<Credit>) -> Vec<Credit> {
    let mut out: Vec<Credit> = Vec::new();
    for c in list {
        if let Some(existing) = out.iter_mut().find(|m| m.title == c.title) {
            existing.role.push_str(", ");
            existing.role.push_str(&c.role);
        } else {
            out.push(c);
        }
    }
    out
}

/// Newest first: year descending, no year last, then title A–Z.
fn sort_newest(mut list: Vec<Credit>) -> Vec<Credit> {
    list.sort_by(|a, b| match (a.year, b.year) {
        (Some(ay), Some(by)) => by.cmp(&ay).then_with(|| title_cmp(&a.title, &b.title)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => title_cmp(&a.title, &b.title),
    });
    list
}

fn title_cmp(a: &str, b: &str) -> Ordering {
    a.to_lowercase().cmp(&b.to_lowercase())
}

/// The server item for a credit: same title (case-insensitive, ignoring punctuation and spaces) and same year
/// (or either year missing), Movie↔Movie / Tv↔Series. None if not on the server.
pub fn on_server<'a>(credit: &Credit, server: &'a [Item]) -> Option<&'a Item> {
    let want_kind = match credit.kind {
        MediaKind::Movie => ItemKind::Movie,
        MediaKind::Tv => ItemKind::Series,
    };
    server.iter().find(|item| {
        item.kind == want_kind
            && norm(&item.name) == norm(&credit.title)
            && match (credit.year, item.production_year) {
                (Some(c), Some(i)) => c == i,
                _ => true,
            }
    })
}

/// Lower-cased, punctuation and spaces removed.
fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}
