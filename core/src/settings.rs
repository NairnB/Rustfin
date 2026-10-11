//! Saved servers and sessions (SPEC §6, R-SET-*). Stored as `settings.json` through `Storage`.
//! Signatures fixed by the lead; bodies are packet C2.
use crate::api::{ExtraFilters, Filters, LibraryQuery, SegmentKind, Session, SortBy};
use crate::platform::{Storage, SubStyle};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

pub const SETTINGS_KEY: &str = "settings.json";

/// A Home row the user can reorder and hide (G12, P-HOME-5). "Latest" expands to one row per video library.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum HomeSection {
    Continue,
    NextUp,
    Favourites,
    Latest,
    #[default]
    Libraries,
}

impl HomeSection {
    pub const ALL: [HomeSection; 5] = [
        HomeSection::Continue,
        HomeSection::NextUp,
        HomeSection::Favourites,
        HomeSection::Latest,
        HomeSection::Libraries,
    ];

    pub fn label(self) -> &'static str {
        match self {
            HomeSection::Continue => "Continue watching",
            HomeSection::NextUp => "Next up",
            HomeSection::Favourites => "Favourites",
            HomeSection::Latest => "Latest per library",
            HomeSection::Libraries => "Libraries",
        }
    }

    /// The default order: Continue, Next up, Favourites, Latest, Libraries (P-USER-3 / P-HOME-5).
    pub fn default_order() -> Vec<HomeSection> {
        HomeSection::ALL.to_vec()
    }
}

/// How dates are shown (P-USER-2). Stored in settings; applied to premiere/birth dates.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DateFormat {
    /// "2026-10-10"
    #[default]
    Iso,
    /// "10/10/2026"
    Us,
    /// "10 Oct 2026"
    Long,
}

impl DateFormat {
    pub const ALL: [DateFormat; 3] = [DateFormat::Iso, DateFormat::Us, DateFormat::Long];

    pub fn label(self) -> &'static str {
        match self {
            DateFormat::Iso => "2026-10-10",
            DateFormat::Us => "10/10/2026",
            DateFormat::Long => "10 Oct 2026",
        }
    }

    /// Formats an ISO date (the leading `YYYY-MM-DD` part) in this style; anything else is left as-is.
    pub fn format(self, iso: &str) -> String {
        let Some((y, m, d)) = parse_iso_date(iso) else {
            return iso.to_string();
        };
        match self {
            DateFormat::Iso => format!("{y:04}-{m:02}-{d:02}"),
            DateFormat::Us => format!("{d:02}/{m:02}/{y:04}"),
            DateFormat::Long => format!("{d:02} {} {y:04}", month_name(m)),
        }
    }
}

/// The first `YYYY-MM-DD` in `s`, as (y, m, d), or None if the string does not start that way.
fn parse_iso_date(s: &str) -> Option<(i32, u32, u32)> {
    let s = s.trim();
    if s.len() < 10 {
        return None;
    }
    let b = s.as_bytes();
    let d = |i: usize| b[i].is_ascii_digit().then(|| (b[i] - b'0') as u32);
    if b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let y = (d(0)? as i32) * 1000 + (d(1)? as i32) * 100 + (d(2)? as i32) * 10 + d(3)? as i32;
    let m = d(5)? * 10 + d(6)?;
    let dd = d(8)? * 10 + d(9)?;
    Some((y, m, dd))
}

fn month_name(m: u32) -> &'static str {
    match m {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => "",
    }
}

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

/// A user who signed in on this device before (D10 / R9): the session kept so the Switch user picker can sign
/// them straight back in. Newest first in `Settings::saved_users`.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct SavedUser {
    /// Server id the session belongs to.
    pub server_id: String,
    pub session: Session,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Random v4-style 32-hex id, created once (R-API-1).
    pub device_id: String,
    pub servers: Vec<ServerEntry>,
    /// `id` of the server used last.
    pub last_server: Option<String>,
    /// TMDB API key typed by the user (R-F5-2); never in the code or the repo.
    pub tmdb_key: String,
    /// User subtitle preferences (P-USER-5).
    pub subtitles: SubPrefs,
    /// Per-library sort + filters (P-LIB-2..9), remembered across sessions. Keyed by library id.
    pub library_view: HashMap<String, LibraryView>,
    // ---- G12 user settings (P-USER-* / P-HOME-5) ----
    /// F3's switch: show the release year on cards and item pages.
    pub show_release_year: bool,
    /// F3's switch: show the story year on cards and item pages.
    pub show_story_year: bool,
    /// P-USER-2: how dates are shown.
    pub date_format: DateFormat,
    /// P-USER-2: draw backdrops on item / show pages.
    pub backdrops: bool,
    /// P-USER-4: preferred audio language (display name; empty = any).
    pub preferred_audio_language: String,
    /// P-USER-4: max streaming bitrate in kbps (0 = auto); used by G6.
    pub max_streaming_bitrate: u32,
    /// P-USER-4: automatically play the next episode after the up-next countdown.
    pub auto_play_next: bool,
    /// P-USER-4: OSD skip-back length (seconds).
    pub skip_back_seconds: u32,
    /// P-USER-4: OSD skip-forward length (seconds).
    pub skip_forward_seconds: u32,
    /// P-HOME-5: Home sections in display order (Favourites is G3's row).
    pub home_order: Vec<HomeSection>,
    /// P-HOME-5: Home sections the user hid.
    pub home_hidden: Vec<HomeSection>,
    /// G9 / P-TV-7: list missing (and not yet aired) episodes on a show page. Off by default.
    pub show_missing_episodes: bool,
    /// G10 / P-PLAY-11: segment kinds skipped automatically during playback (default none).
    pub auto_skip: Vec<SegmentKind>,
    /// D10 / R9: users who signed in here before, newest first (all servers).
    pub saved_users: Vec<SavedUser>,
}

/// When to show subtitles (P-USER-5). `Default` = the server's default for the item.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SubMode {
    #[default]
    Default,
    Always,
    OnlyForced,
    None,
}

/// User subtitle preferences (P-USER-5): mode, preferred language (a Jellyfin 3-letter code, empty = any)
/// and appearance.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(default)]
pub struct SubPrefs {
    pub mode: SubMode,
    pub language: String,
    pub style: SubStyle,
}

/// G11 / P-LIB-14: how a library's items are drawn.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ViewMode {
    #[default]
    Posters,
    Thumbs,
    /// P-LIB-14: wide strips (Banner image, else Thumb, else Backdrop).
    Banners,
    List,
}

impl ViewMode {
    pub const ALL: [ViewMode; 4] = [ViewMode::Posters, ViewMode::Thumbs, ViewMode::Banners, ViewMode::List];
    /// Menu label.
    pub fn label(self) -> &'static str {
        match self {
            ViewMode::Posters => "Posters",
            ViewMode::Thumbs => "Thumbs",
            ViewMode::Banners => "Banners",
            ViewMode::List => "List",
        }
    }
}

/// The remembered sort + filters + view mode for one library (P-LIB-2..14).
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
#[serde(default)]
pub struct LibraryView {
    pub sort: SortBy,
    pub descending: bool,
    pub filters: Filters,
    pub extra: ExtraFilters,
    pub mode: ViewMode,
}

impl LibraryView {
    /// The `LibraryQuery` this view describes.
    pub fn query(&self) -> LibraryQuery {
        LibraryQuery { sort: self.sort, descending: self.descending, filters: self.filters, extra: self.extra.clone() }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            device_id: String::new(),
            servers: vec![],
            last_server: None,
            tmdb_key: String::new(),
            subtitles: SubPrefs::default(),
            library_view: HashMap::new(),
            show_release_year: true,
            show_story_year: true,
            date_format: DateFormat::Iso,
            backdrops: true,
            preferred_audio_language: String::new(),
            max_streaming_bitrate: 0,
            auto_play_next: true,
            skip_back_seconds: 10,
            skip_forward_seconds: 30,
            home_order: HomeSection::default_order(),
            home_hidden: vec![],
            show_missing_episodes: false,
            auto_skip: vec![],
            saved_users: vec![],
        }
    }
}

impl Settings {
    /// P-HOME-5: a Home section is shown when it is not in the hidden list.
    pub fn home_visible(&self, section: HomeSection) -> bool {
        !self.home_hidden.contains(&section)
    }
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
    /// P-AUTH-5: remember a fresh sign-in. `keep` false stores the server only — the session lives in the
    /// client in memory, so the next start asks for the password again.
    pub fn remember_signed_in(&mut self, mut entry: ServerEntry, keep: bool) {
        if !keep {
            entry.session = None;
        } else if let Some(session) = entry.session.clone().filter(|_| !entry.id.is_empty()) {
            self.save_user(&entry.id, session);
        }
        self.remember(entry);
    }
    /// D10: keep (or refresh) a user's session for the Switch user picker; they go to the front.
    pub fn save_user(&mut self, server_id: &str, session: Session) {
        self.saved_users.retain(|u| !(u.server_id == server_id && u.session.user_id == session.user_id));
        self.saved_users.insert(0, SavedUser { server_id: server_id.to_string(), session });
    }
    /// D10: the sessions saved for one server, newest first.
    pub fn saved_for(&self, server_id: &str) -> Vec<Session> {
        self.saved_users.iter().filter(|u| u.server_id == server_id).map(|u| u.session.clone()).collect()
    }
    /// D10: drop one saved user (signed out for good, or their token no longer works).
    pub fn forget_saved_user(&mut self, server_id: &str, user_id: &str) {
        self.saved_users.retain(|u| !(u.server_id == server_id && u.session.user_id == user_id));
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

#[cfg(test)]
mod saved_user_tests {
    use super::*;

    fn session(id: &str, name: &str) -> Session {
        Session { user_id: id.into(), user_name: name.into(), token: format!("tok-{id}") }
    }

    fn entry(session: Option<Session>) -> ServerEntry {
        ServerEntry { id: "srv".into(), name: "Den".into(), address: "http://x:8096".into(), session }
    }

    #[test]
    fn sign_ins_are_saved_newest_first_without_duplicates() {
        let mut s = Settings::default();
        s.remember_signed_in(entry(Some(session("1", "viewer"))), true);
        s.remember_signed_in(entry(Some(session("2", "tester"))), true);
        s.remember_signed_in(entry(Some(session("1", "viewer"))), true);
        let names: Vec<_> = s.saved_for("srv").into_iter().map(|u| u.user_name).collect();
        assert_eq!(names, ["viewer", "tester"]);
    }

    #[test]
    fn not_remembered_is_not_saved() {
        let mut s = Settings::default();
        s.remember_signed_in(entry(Some(session("1", "viewer"))), false);
        assert!(s.saved_for("srv").is_empty());
    }

    #[test]
    fn saved_users_are_per_server_and_can_be_forgotten() {
        let mut s = Settings::default();
        s.save_user("a", session("1", "viewer"));
        s.save_user("b", session("2", "tester"));
        assert_eq!(s.saved_for("a").len(), 1);
        s.forget_saved_user("a", "1");
        assert!(s.saved_for("a").is_empty());
        assert_eq!(s.saved_for("b").len(), 1);
    }

    #[test]
    fn old_settings_files_load_without_saved_users() {
        let s: Settings = serde_json::from_str(r#"{"device_id":"x"}"#).unwrap();
        assert!(s.saved_users.is_empty());
    }
}
