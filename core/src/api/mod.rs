//! Jellyfin HTTP client (SPEC §4, rules R-API-*). Blocking calls: run them on the worker pool, never on the UI thread.
//! Signatures are fixed by the lead (tests in `core/tests/` depend on them); bodies are packet C1.
pub mod admin;
pub mod gen_admin;
pub mod item_edit;
pub mod models;
pub mod poster;
pub mod trickplay;

pub use models::*;
pub use trickplay::{TrickplayCell, TrickplayInfo};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;
// TV web route: sync XHR stands in for ureq
#[cfg(target_arch = "wasm32")]
use crate::webhttp as ureq;

/// R-API-5
#[derive(Debug, Clone, PartialEq)]
pub enum ApiError {
    Network(String),
    Unauthorized,
    NotFound,
    Server(u16, String),
    Decode(String),
    /// R-API-3: server older than 10.8 (carries the version string).
    TooOld(String),
    BadAddress(String),
    /// R-TAG-6: the server refused a change (HTTP 403): only admins may edit.
    Forbidden,
    /// A local (disk) failure, e.g. saving a poster backup before an apply.
    Io(String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::Network(e) => write!(f, "Can't reach the server ({e})"),
            ApiError::Unauthorized => f.write_str("Signed out by the server"),
            ApiError::NotFound => f.write_str("Not found on the server"),
            ApiError::Server(c, m) => write!(f, "Server error {c}: {m}"),
            ApiError::Decode(e) => write!(f, "Unexpected reply from the server ({e})"),
            ApiError::TooOld(v) => write!(f, "Server version {v} is too old (needs 10.8 or newer)"),
            ApiError::BadAddress(a) => write!(f, "\"{a}\" is not a server address"),
            ApiError::Forbidden => f.write_str("Only a server admin can change this"),
            ApiError::Io(e) => f.write_str(e),
        }
    }
}
impl std::error::Error for ApiError {}

pub type Result<T> = std::result::Result<T, ApiError>;

pub const TICKS_PER_SEC: f64 = 10_000_000.0;

/// R-API-32
pub fn ticks_to_secs(t: i64) -> f64 {
    t as f64 / TICKS_PER_SEC
}
/// R-API-32 (rounded to the nearest tick)
pub fn secs_to_ticks(s: f64) -> i64 {
    (s * TICKS_PER_SEC).round() as i64
}

/// G10 / P-PLAY-11: what a media segment marks (10.10 `MediaSegmentType`).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SegmentKind {
    Intro,
    Outro,
    Recap,
    Preview,
    Commercial,
    Unknown,
}

impl SegmentKind {
    /// The kinds a user can pick for auto-skip, in settings order.
    pub const SKIPPABLE: [SegmentKind; 5] =
        [SegmentKind::Intro, SegmentKind::Recap, SegmentKind::Outro, SegmentKind::Preview, SegmentKind::Commercial];

    pub fn from_server(s: &str) -> SegmentKind {
        match s {
            "Intro" => SegmentKind::Intro,
            "Outro" => SegmentKind::Outro,
            "Recap" => SegmentKind::Recap,
            "Preview" => SegmentKind::Preview,
            "Commercial" => SegmentKind::Commercial,
            _ => SegmentKind::Unknown,
        }
    }

    /// Settings label ("Skip intros automatically" style noun).
    pub fn label(self) -> &'static str {
        match self {
            SegmentKind::Intro => "intros",
            SegmentKind::Outro => "credits",
            SegmentKind::Recap => "recaps",
            SegmentKind::Preview => "previews",
            SegmentKind::Commercial => "commercials",
            SegmentKind::Unknown => "other segments",
        }
    }

    /// The player button text while inside a segment of this kind; `None` for `Unknown` (nothing to offer).
    pub fn button_label(self) -> Option<&'static str> {
        match self {
            SegmentKind::Intro => Some("Skip intro"),
            SegmentKind::Recap => Some("Skip recap"),
            SegmentKind::Outro => Some("Skip credits"),
            SegmentKind::Preview => Some("Skip preview"),
            SegmentKind::Commercial => Some("Skip commercial"),
            SegmentKind::Unknown => None,
        }
    }
}

/// G10 / P-PLAY-11: one media segment, in seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub kind: SegmentKind,
    pub start: f64,
    pub end: f64,
}

/// R-API-2: trim, add `http://` when no scheme, drop trailing `/`, keep any path base.
/// Empty or containing spaces → `BadAddress`. Port probing (`:8096`) is `Client::connect`'s job, not this.
pub fn normalise_address(input: &str) -> Result<String> {
    let s = input.trim();
    if s.is_empty() || s.chars().any(char::is_whitespace) {
        return Err(ApiError::BadAddress(s.to_string()));
    }
    let (scheme, rest) = match s.split_once("://") {
        Some((scheme, rest)) => (scheme.to_ascii_lowercase(), rest),
        None => ("http".to_string(), s),
    };
    if scheme != "http" && scheme != "https" {
        return Err(ApiError::BadAddress(s.to_string()));
    }
    let rest = rest.trim_end_matches('/');
    Ok(format!("{scheme}://{rest}"))
}

/// R-API-3: true for 10.8.0 and newer; unparsable versions are accepted (fail later, not here).
pub fn version_supported(version: &str) -> bool {
    let mut parts = version.split('.');
    let (Some(major), Some(minor)) = (parts.next(), parts.next()) else {
        return true;
    };
    let (Ok(major), Ok(minor)) = (major.parse::<u32>(), minor.parse::<u32>()) else {
        return true;
    };
    (major, minor) >= (10, 8)
}

/// R-API-31: card width × scale rounded **up** to a multiple of 40, at least 40.
pub fn image_width_bucket(card_w: f32, scale: f32) -> u32 {
    let px = (card_w * scale).ceil().max(40.0);
    (px / 40.0).ceil() as u32 * 40
}

pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
pub const IMAGE_TIMEOUT: Duration = Duration::from_secs(15);

/// Fields requested for card lists (SPEC §4 line 92).
const CARD_FIELDS: &str =
    "PrimaryImageAspectRatio,ProductionYear,RunTimeTicks,UserData,SeriesName,ParentIndexNumber,IndexNumber,ImageTags,Tags,Genres";
/// Fields requested for an item page (R-API-18).
const ITEM_FIELDS: &str =
    "Overview,Genres,People,MediaStreams,MediaSources,Tags,Taglines,Studios,ProviderIds,Chapters,DateCreated,RemoteTrailers";

/// G8: one media source with the technical fields of the media info panel.
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct SourceInfo {
    pub id: String,
    pub name: Option<String>,
    pub container: Option<String>,
    pub path: Option<String>,
    pub size: Option<i64>,
    pub bitrate: Option<i64>,
    pub media_streams: Vec<StreamInfo>,
}

/// G8: one stream of a `SourceInfo`.
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct StreamInfo {
    pub index: i32,
    #[serde(rename = "Type")]
    pub kind: StreamKind,
    pub codec: Option<String>,
    pub language: Option<String>,
    pub display_title: Option<String>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub bit_rate: Option<i64>,
    pub channels: Option<i32>,
}

/// Routes that have a 10.9+ form and a legacy form (R-API-4). Fallback choice is remembered per client.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Route {
    Views,
    Resume,
    Latest,
    Item,
    Played,
    Favorite,
    MediaInfo,
}

fn net(e: ureq::Error) -> ApiError {
    ApiError::Network(e.to_string())
}

fn check_status(resp: &mut ureq::http::Response<ureq::Body>) -> Result<()> {
    let code = resp.status().as_u16();
    if code < 400 {
        return Ok(());
    }
    match code {
        401 => Err(ApiError::Unauthorized),
        403 => Err(ApiError::Forbidden),
        404 => Err(ApiError::NotFound),
        _ => {
            let body = resp.body_mut().read_to_string().unwrap_or_default();
            let body: String = body.chars().take(200).collect();
            Err(ApiError::Server(code, body))
        }
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

/// P-LIB-9..12: append the query's `Filters=` list and the G11 extra filter params to an `/Items` path.
pub fn push_filters(path: &mut String, q: &LibraryQuery) {
    let mut filters: Vec<&str> = Vec::new();
    if q.filters.unplayed {
        filters.push("IsUnplayed");
    }
    if q.filters.played {
        filters.push("IsPlayed");
    }
    if q.filters.resumable {
        filters.push("IsResumable");
    }
    if q.filters.favourites {
        filters.push("IsFavorite");
    }
    if !filters.is_empty() {
        path.push_str(&format!("&Filters={}", filters.join(",")));
    }
    let x = &q.extra;
    let pipe = |v: &[String]| v.iter().map(|s| url_encode(s)).collect::<Vec<_>>().join("%7C");
    if !x.genres.is_empty() {
        path.push_str(&format!("&Genres={}", pipe(&x.genres)));
    }
    if !x.ratings.is_empty() {
        path.push_str(&format!("&OfficialRatings={}", pipe(&x.ratings)));
    }
    if !x.tags.is_empty() {
        path.push_str(&format!("&Tags={}", pipe(&x.tags)));
    }
    let years = x.years();
    if !years.is_empty() {
        path.push_str(&format!("&Years={}", years.iter().map(|y| y.to_string()).collect::<Vec<_>>().join(",")));
    }
    if let Some((id, _)) = &x.studio {
        path.push_str(&format!("&StudioIds={id}"));
    }
    for (on, param) in [
        (x.has_subtitles, "HasSubtitles"),
        (x.has_trailer, "HasTrailer"),
        (x.has_special_features, "HasSpecialFeature"),
        (x.has_theme_song, "HasThemeSong"),
        (x.has_theme_video, "HasThemeVideo"),
        (x.hd, "IsHD"),
        (x.uhd, "Is4K"),
        (x.three_d, "Is3D"),
    ] {
        if on {
            path.push_str(&format!("&{param}=true"));
        }
    }
}

/// R-API-2 probe target: `http` + no explicit port → same authority on `:8096` (keeps any path base).
fn probe_8096(base: &str) -> Option<String> {
    let rest = base.strip_prefix("http://")?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    if authority.is_empty() || authority.contains(':') {
        return None;
    }
    Some(format!("http://{authority}:8096{path}"))
}

/// R-API-22: the report body with `PlayMethod` and `CanSeek` folded in.
fn report_body(r: &ProgressReport) -> serde_json::Value {
    report_body_as(r, "DirectPlay")
}

/// G6: as `report_body` with the given `PlayMethod`.
fn report_body_as(r: &ProgressReport, method: &str) -> serde_json::Value {
    let mut v = serde_json::to_value(r).unwrap_or(serde_json::Value::Null);
    if let serde_json::Value::Object(map) = &mut v {
        map.insert("PlayMethod".into(), serde_json::Value::String(method.into()));
        map.insert("CanSeek".into(), serde_json::Value::Bool(true));
    }
    v
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct AuthResult {
    access_token: String,
    user: AuthUser,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct AuthUser {
    id: String,
    name: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct QcConnect {
    authenticated: bool,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct UserDto {
    policy: UserPolicy,
    configuration: UserConfig,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct UserConfig {
    remember_subtitle_selections: bool,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct UserPolicy {
    is_administrator: bool,
}

/// P-LIB-2..8: how a library page is sorted. The first nine are server-side (Jellyfin `SortBy`); the last two
/// (`StoryOrder`, `MyOrder`) are client-side sorts over all loaded items (R-F3-2, R-F4-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SortBy {
    #[default]
    Name,
    DateAdded,
    PremiereDate,
    CommunityRating,
    CriticRating,
    DatePlayed,
    PlayCount,
    Runtime,
    Random,
    /// P-LIB-7: server `OfficialRating`.
    ParentalRating,
    StoryOrder,
    MyOrder,
}

impl SortBy {
    /// Jellyfin `SortBy` query value; `None` for the two client-side sorts (the caller sorts locally).
    pub fn server_value(self) -> Option<&'static str> {
        match self {
            SortBy::Name => Some("SortName"),
            SortBy::DateAdded => Some("DateCreated"),
            SortBy::PremiereDate => Some("PremiereDate"),
            SortBy::CommunityRating => Some("CommunityRating"),
            SortBy::CriticRating => Some("CriticRating"),
            SortBy::DatePlayed => Some("DatePlayed"),
            SortBy::PlayCount => Some("PlayCount"),
            SortBy::Runtime => Some("Runtime"),
            SortBy::Random => Some("Random"),
            SortBy::ParentalRating => Some("OfficialRating"),
            SortBy::StoryOrder | SortBy::MyOrder => None,
        }
    }
    /// Menu label (British English, short).
    pub fn label(self) -> &'static str {
        match self {
            SortBy::Name => "Name",
            SortBy::DateAdded => "Date added",
            SortBy::PremiereDate => "Premiere date",
            SortBy::CommunityRating => "Community rating",
            SortBy::CriticRating => "Critic rating",
            SortBy::DatePlayed => "Date played",
            SortBy::PlayCount => "Play count",
            SortBy::Runtime => "Runtime",
            SortBy::Random => "Random",
            SortBy::ParentalRating => "Parental rating",
            SortBy::StoryOrder => "Story order",
            SortBy::MyOrder => "My order",
        }
    }
    /// Client-side over all loaded items (no server sort).
    pub fn is_local(self) -> bool {
        matches!(self, SortBy::StoryOrder | SortBy::MyOrder)
    }
}

/// P-LIB-9: the basic library filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Filters {
    pub unplayed: bool,
    pub played: bool,
    pub resumable: bool,
    pub favourites: bool,
}

/// G11 / P-LIB-10..12: the "Filters…" dialog choices — genres, years, parental ratings, tags, feature flags,
/// video quality, and one studio (picked from the Studios tab). Empty = no filter.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ExtraFilters {
    pub genres: Vec<String>,
    pub ratings: Vec<String>,
    pub tags: Vec<String>,
    pub year_from: Option<u32>,
    pub year_to: Option<u32>,
    pub has_subtitles: bool,
    pub has_trailer: bool,
    pub has_special_features: bool,
    pub has_theme_song: bool,
    pub has_theme_video: bool,
    pub hd: bool,
    pub uhd: bool,
    pub three_d: bool,
    /// (studio id, name) chosen on the Studios tab.
    pub studio: Option<(String, String)>,
}

impl ExtraFilters {
    pub fn is_empty(&self) -> bool {
        *self == ExtraFilters::default()
    }
    /// The years the range covers (`from`..=`to`; one bound alone is that single year up to the other's default).
    pub fn years(&self) -> Vec<u32> {
        match (self.year_from, self.year_to) {
            (None, None) => vec![],
            (Some(a), None) => (a..=a.max(a + 100).min(2100)).collect(),
            (None, Some(b)) => (b.saturating_sub(100).max(1850)..=b).collect(),
            (Some(a), Some(b)) => (a.min(b)..=a.max(b)).take(300).collect(),
        }
    }
    /// A short description for the "Filtered by" line.
    pub fn summary(&self) -> String {
        let mut p: Vec<String> = Vec::new();
        if let Some((_, n)) = &self.studio {
            p.push(n.clone());
        }
        p.extend(self.genres.iter().cloned());
        p.extend(self.ratings.iter().cloned());
        p.extend(self.tags.iter().cloned());
        match (self.year_from, self.year_to) {
            (Some(a), Some(b)) => p.push(format!("{a}–{b}")),
            (Some(a), None) => p.push(format!("from {a}")),
            (None, Some(b)) => p.push(format!("to {b}")),
            _ => {}
        }
        for (on, label) in [
            (self.has_subtitles, "subtitles"),
            (self.has_trailer, "trailer"),
            (self.has_special_features, "special features"),
            (self.has_theme_song, "theme song"),
            (self.has_theme_video, "theme video"),
            (self.hd, "HD"),
            (self.uhd, "4K"),
            (self.three_d, "3D"),
        ] {
            if on {
                p.push(label.to_string());
            }
        }
        p.join(" · ")
    }
}

/// P-LIB-2..9: the sort + filters for a library page (remembered per library in settings).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LibraryQuery {
    pub sort: SortBy,
    pub descending: bool,
    pub filters: Filters,
    #[serde(default)]
    pub extra: ExtraFilters,
}

/// G11 / P-LIB-10: what the "Filters…" dialog can offer for a library (`/Items/Filters`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct FilterOptions {
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub official_ratings: Vec<String>,
    pub years: Vec<u32>,
}

/// G11 / P-LIB-16: one row of movie suggestions.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Recommendation {
    pub recommendation_type: String,
    pub baseline_item_name: Option<String>,
    pub items: Vec<Item>,
}

impl Recommendation {
    /// jellyfin-web's wording for the row heading.
    pub fn title(&self) -> String {
        let base = self.baseline_item_name.clone().unwrap_or_default();
        match self.recommendation_type.as_str() {
            "SimilarToRecentlyPlayed" => format!("Because you watched {base}"),
            "SimilarToLikedItem" => format!("Because you liked {base}"),
            "HasDirectorFromRecentlyPlayed" | "HasLikedDirector" => format!("Directed by {base}"),
            "HasActorFromRecentlyPlayed" | "HasLikedActor" => format!("Starring {base}"),
            _ => "Suggested for you".to_string(),
        }
    }
}

/// One server connection (+ session once signed in). Cheap to clone; share across worker threads.
#[derive(Clone)]
pub struct Client {
    /// Normalised base address, e.g. `http://10.0.0.20:8096`.
    pub base: String,
    pub device: DeviceInfo,
    pub session: Option<Session>,
    agent: ureq::Agent,
    /// Routes remembered as legacy after a 404 on the new form (R-API-4); shared by clones.
    fallback: Arc<Mutex<HashSet<Route>>>,
}

impl Client {
    /// No network. `base` must already be normalised.
    pub fn new(base: &str, device: DeviceInfo) -> Client {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(REQUEST_TIMEOUT))
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .build()
            .into();
        Client {
            base: base.trim_end_matches('/').to_string(),
            device,
            session: None,
            agent,
            fallback: Arc::new(Mutex::new(HashSet::new())),
        }
    }
    pub fn with_session(base: &str, device: DeviceInfo, session: Session) -> Client {
        let mut c = Client::new(base, device);
        c.session = Some(session);
        c
    }

    /// R-API-1: the full `Authorization` header value.
    pub fn auth_header(&self) -> String {
        let name = self.device.name.replace('"', "'").replace(',', " ");
        let id = self.device.id.replace('"', "'").replace(',', " ");
        let mut h = format!(
            "MediaBrowser Client=\"Rustfin\", Device=\"{name}\", DeviceId=\"{id}\", Version=\"{}\"",
            self.device.version
        );
        if let Some(s) = &self.session {
            h.push_str(&format!(", Token=\"{}\"", s.token));
        }
        h
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    fn user_id(&self) -> Result<&str> {
        self.session.as_ref().map(|s| s.user_id.as_str()).ok_or(ApiError::Unauthorized)
    }

    fn uses_legacy(&self, route: Route) -> bool {
        self.fallback.lock().unwrap().contains(&route)
    }
    fn remember_legacy(&self, route: Route) {
        self.fallback.lock().unwrap().insert(route);
    }

    fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let mut resp =
            self.agent.get(&self.url(path)).header("Authorization", &self.auth_header()).call().map_err(net)?;
        check_status(&mut resp)?;
        resp.body_mut().read_json().map_err(|e| ApiError::Decode(e.to_string()))
    }
    fn post_json<T: DeserializeOwned>(&self, path: &str, body: &impl Serialize) -> Result<T> {
        let mut resp = self
            .agent
            .post(&self.url(path))
            .header("Authorization", &self.auth_header())
            .send_json(body)
            .map_err(net)?;
        check_status(&mut resp)?;
        resp.body_mut().read_json().map_err(|e| ApiError::Decode(e.to_string()))
    }
    fn get_ok(&self, path: &str) -> Result<()> {
        let mut resp =
            self.agent.get(&self.url(path)).header("Authorization", &self.auth_header()).call().map_err(net)?;
        check_status(&mut resp)
    }
    fn post_ok(&self, path: &str, body: &impl Serialize) -> Result<()> {
        let mut resp = self
            .agent
            .post(&self.url(path))
            .header("Authorization", &self.auth_header())
            .send_json(body)
            .map_err(net)?;
        check_status(&mut resp)
    }
    fn post_empty_ok(&self, path: &str) -> Result<()> {
        let mut resp =
            self.agent.post(&self.url(path)).header("Authorization", &self.auth_header()).send_empty().map_err(net)?;
        check_status(&mut resp)
    }
    fn delete_ok(&self, path: &str) -> Result<()> {
        let mut resp =
            self.agent.delete(&self.url(path)).header("Authorization", &self.auth_header()).call().map_err(net)?;
        check_status(&mut resp)
    }

    /// R-API-4: try the new route, on 404 fall back to the legacy route and remember the choice.
    fn get_with_fallback<T: DeserializeOwned>(&self, route: Route, new: &str, legacy: &str) -> Result<T> {
        if self.uses_legacy(route) {
            return self.get_json(legacy);
        }
        match self.get_json(new) {
            Ok(v) => Ok(v),
            Err(ApiError::NotFound) => {
                self.remember_legacy(route);
                self.get_json(legacy)
            }
            Err(e) => Err(e),
        }
    }

    /// R-API-2/3: normalise, `GET /System/Info/Public`; if that fails with a network error and the address has no
    /// explicit port and is `http`, retry once on `:8096`. Rejects versions < 10.8 with `TooOld`.
    pub fn connect(address: &str, device: DeviceInfo) -> Result<(Client, PublicInfo)> {
        let base = normalise_address(address)?;
        let client = Client::new(&base, device.clone());
        match client.public_info() {
            Ok(info) => {
                if !version_supported(&info.version) {
                    return Err(ApiError::TooOld(info.version));
                }
                Ok((client, info))
            }
            Err(ApiError::Network(err)) => {
                let Some(probed) = probe_8096(&base) else {
                    return Err(ApiError::Network(err));
                };
                let client = Client::new(&probed, device);
                let info = client.public_info()?;
                if !version_supported(&info.version) {
                    return Err(ApiError::TooOld(info.version));
                }
                Ok((client, info))
            }
            Err(e) => Err(e),
        }
    }
    pub fn public_info(&self) -> Result<PublicInfo> {
        self.get_json("/System/Info/Public")
    }
    /// R-API-12 (an empty list is normal).
    pub fn public_users(&self) -> Result<Vec<PublicUser>> {
        self.get_json("/Users/Public")
    }
    /// R-API-10: sets `self.session` and returns it. Wrong password → `Unauthorized`.
    pub fn sign_in(&mut self, user: &str, password: &str) -> Result<Session> {
        let body = serde_json::json!({ "Username": user, "Pw": password });
        let resp: AuthResult = self.post_json("/Users/AuthenticateByName", &body)?;
        let session = Session { user_id: resp.user.id, user_name: resp.user.name, token: resp.access_token };
        self.session = Some(session.clone());
        Ok(session)
    }
    /// R-API-11
    pub fn quick_connect_enabled(&self) -> Result<bool> {
        self.get_json("/QuickConnect/Enabled")
    }
    pub fn quick_connect_initiate(&self) -> Result<QuickConnectState> {
        let mut resp = self
            .agent
            .post(&self.url("/QuickConnect/Initiate"))
            .header("Authorization", &self.auth_header())
            .send_empty()
            .map_err(net)?;
        check_status(&mut resp)?;
        resp.body_mut().read_json().map_err(|e| ApiError::Decode(e.to_string()))
    }
    /// One poll: true once the code was approved on another device.
    pub fn quick_connect_poll(&self, secret: &str) -> Result<bool> {
        let path = format!("/QuickConnect/Connect?secret={}", url_encode(secret));
        let qc: QcConnect = self.get_json(&path)?;
        Ok(qc.authenticated)
    }
    /// After an approved poll: authenticates, sets and returns the session.
    pub fn quick_connect_finish(&mut self, secret: &str) -> Result<Session> {
        let body = serde_json::json!({ "Secret": secret });
        let resp: AuthResult = self.post_json("/Users/AuthenticateWithQuickConnect", &body)?;
        let session = Session { user_id: resp.user.id, user_name: resp.user.name, token: resp.access_token };
        self.session = Some(session.clone());
        Ok(session)
    }
    /// R-SET-2: checks the stored token is still good.
    pub fn verify_session(&self) -> Result<()> {
        match self.get_ok("/Users/Me") {
            Ok(()) => Ok(()),
            Err(ApiError::NotFound) => {
                let uid = self.user_id()?;
                self.get_ok(&format!("/Users/{uid}"))
            }
            Err(e) => Err(e),
        }
    }
    /// R-API-24, then clears `self.session` (even if the call fails).
    pub fn sign_out(&mut self) -> Result<()> {
        let result = self.post_empty_ok("/Sessions/Logout");
        self.session = None;
        result
    }

    /// R-API-13: the user's libraries (CollectionFolder items with `collection_type`).
    pub fn views(&self) -> Result<Vec<Item>> {
        let uid = self.user_id()?;
        let page: Page =
            self.get_with_fallback(Route::Views, &format!("/UserViews?userId={uid}"), &format!("/Users/{uid}/Views"))?;
        Ok(page.items)
    }
    /// R-API-14
    pub fn resume(&self, limit: usize) -> Result<Vec<Item>> {
        let uid = self.user_id()?;
        let new = format!("/UserItems/Resume?userId={uid}&Limit={limit}&MediaTypes=Video&Fields={CARD_FIELDS}");
        let legacy = format!("/Users/{uid}/Items/Resume?Limit={limit}&MediaTypes=Video&Fields={CARD_FIELDS}");
        let page: Page = self.get_with_fallback(Route::Resume, &new, &legacy)?;
        Ok(page.items)
    }
    /// R-API-15
    pub fn next_up(&self, limit: usize) -> Result<Vec<Item>> {
        let uid = self.user_id()?;
        let path = format!("/Shows/NextUp?userId={uid}&Limit={limit}&Fields={CARD_FIELDS}");
        let page: Page = self.get_json(&path)?;
        Ok(page.items)
    }
    /// R-API-16 (returns a bare array, not a paged object).
    pub fn latest(&self, parent_id: &str, limit: usize) -> Result<Vec<Item>> {
        let uid = self.user_id()?;
        let new = format!("/Items/Latest?userId={uid}&parentId={parent_id}&Limit={limit}&Fields={CARD_FIELDS}");
        let legacy = format!("/Users/{uid}/Items/Latest?parentId={parent_id}&Limit={limit}&Fields={CARD_FIELDS}");
        self.get_with_fallback(Route::Latest, &new, &legacy)
    }
    /// R-API-17: Movies and Series directly under a library, sorted by name.
    pub fn library_page(&self, parent_id: &str, start: usize, limit: usize) -> Result<Page> {
        let uid = self.user_id()?;
        let path = format!(
            "/Items?userId={uid}&ParentId={parent_id}&Recursive=true&IncludeItemTypes=Movie,Series&SortBy=SortName&SortOrder=Ascending&StartIndex={start}&Limit={limit}&Fields={CARD_FIELDS}"
        );
        self.get_json(&path)
    }
    /// R-API-18 (with media sources/streams).
    pub fn item(&self, id: &str) -> Result<Item> {
        let uid = self.user_id()?;
        let new = format!("/Items/{id}?userId={uid}&Fields={ITEM_FIELDS}");
        let legacy = format!("/Users/{uid}/Items/{id}?Fields={ITEM_FIELDS}");
        self.get_with_fallback(Route::Item, &new, &legacy)
    }
    /// R-API-19
    pub fn seasons(&self, series_id: &str) -> Result<Vec<Item>> {
        let uid = self.user_id()?;
        let path = format!("/Shows/{series_id}/Seasons?userId={uid}");
        let page: Page = self.get_json(&path)?;
        Ok(page.items)
    }
    /// R-API-19: all episodes of the series, or of one season.
    pub fn episodes(&self, series_id: &str, season_id: Option<&str>) -> Result<Vec<Item>> {
        let uid = self.user_id()?;
        let path = match season_id {
            Some(sid) => format!("/Shows/{series_id}/Episodes?userId={uid}&seasonId={sid}&Fields=Overview"),
            None => format!("/Shows/{series_id}/Episodes?userId={uid}&Fields=Overview"),
        };
        let page: Page = self.get_json(&path)?;
        Ok(page.items)
    }
    /// R-PLAY-4: the episode after `episode_id` in the series order, if any.
    pub fn next_episode(&self, series_id: &str, episode_id: &str) -> Result<Option<Item>> {
        let uid = self.user_id()?;
        let path = format!("/Shows/{series_id}/Episodes?userId={uid}&startItemId={episode_id}&Limit=2");
        let page: Page = self.get_json(&path)?;
        Ok(page.items.into_iter().nth(1))
    }
    /// P-PLAY-16 / P-TV-9: the episode before `episode_id` in the series order, if any.
    pub fn previous_episode(&self, series_id: &str, episode_id: &str) -> Result<Option<Item>> {
        let uid = self.user_id()?;
        // `adjacentTo` + `Limit=1` returns the episode before `episode_id`, or `episode_id` itself when there is none.
        let path = format!("/Shows/{series_id}/Episodes?userId={uid}&adjacentTo={episode_id}&Limit=1");
        let page: Page = self.get_json(&path)?;
        Ok(page.items.into_iter().next().filter(|i| i.id != episode_id))
    }
    /// R-API-20
    pub fn search(&self, term: &str) -> Result<Vec<Item>> {
        let uid = self.user_id()?;
        let term = url_encode(term);
        let path = format!(
            "/Items?userId={uid}&searchTerm={term}&Recursive=true&IncludeItemTypes=Movie,Series,Episode,BoxSet&Limit=60&Fields={CARD_FIELDS}"
        );
        let page: Page = self.get_json(&path)?;
        Ok(page.items)
    }
    /// R-API-21
    pub fn playback_info(&self, id: &str) -> Result<PlaybackInfo> {
        let uid = self.user_id()?;
        let path = format!("/Items/{id}/PlaybackInfo?userId={uid}");
        self.post_json(&path, &serde_json::json!({}))
    }
    /// R-API-22
    pub fn report_start(&self, r: &ProgressReport) -> Result<()> {
        self.post_ok("/Sessions/Playing", &report_body(r))
    }
    pub fn report_progress(&self, r: &ProgressReport) -> Result<()> {
        self.post_ok("/Sessions/Playing/Progress", &report_body(r))
    }
    pub fn report_stopped(&self, r: &ProgressReport) -> Result<()> {
        self.post_ok("/Sessions/Playing/Stopped", &report_body(r))
    }
    /// R-API-23
    pub fn set_played(&self, id: &str, played: bool) -> Result<()> {
        let uid = self.user_id()?;
        let new = format!("/UserPlayedItems/{id}?userId={uid}");
        let legacy = format!("/Users/{uid}/PlayedItems/{id}");
        if self.uses_legacy(Route::Played) {
            return self.mutate_ok(&legacy, played);
        }
        match self.mutate_ok(&new, played) {
            Ok(()) => Ok(()),
            Err(ApiError::NotFound) => {
                self.remember_legacy(Route::Played);
                self.mutate_ok(&legacy, played)
            }
            Err(e) => Err(e),
        }
    }

    fn mutate_ok(&self, path: &str, post: bool) -> Result<()> {
        if post {
            self.post_empty_ok(path)
        } else {
            self.delete_ok(path)
        }
    }

    /// R-API-30
    pub fn stream_url(&self, item_id: &str, media_source_id: &str) -> String {
        let token = self.session.as_ref().map(|s| s.token.as_str()).unwrap_or("");
        format!("{}/Videos/{item_id}/stream?static=true&mediaSourceId={media_source_id}&api_key={token}", self.base)
    }
    /// R-API-30: only for external subtitle streams (None otherwise or for unknown codecs).
    pub fn subtitle_url(&self, item_id: &str, media_source_id: &str, s: &MediaStream) -> Option<String> {
        if !s.is_external || s.kind != StreamKind::Subtitle {
            return None;
        }
        let ext = match s.codec.as_deref() {
            Some("subrip") => "srt",
            Some("ass") | Some("ssa") => "ass",
            Some("webvtt") => "vtt",
            _ => return None,
        };
        let token = self.session.as_ref().map(|s| s.token.as_str()).unwrap_or("");
        Some(format!(
            "{}/Videos/{item_id}/{media_source_id}/Subtitles/{}/0/Stream.{ext}?api_key={token}",
            self.base, s.index
        ))
    }
    /// R-API-31 incl. the episode → series fallback. `width_px` is already bucketed. None when there is no image.
    /// No token in image URLs (images are public on 10.10); the client sends the header anyway.
    pub fn image_url(&self, item: &Item, kind: ImageKind, width_px: u32) -> Option<String> {
        let (id, tag) = match kind {
            ImageKind::Primary => {
                if let Some(t) = item.image_tags.get("Primary") {
                    (item.id.clone(), t.clone())
                } else {
                    (item.series_id.clone()?, item.series_primary_image_tag.clone()?)
                }
            }
            ImageKind::Backdrop => {
                if let Some(t) = item.backdrop_image_tags.first() {
                    (item.id.clone(), t.clone())
                } else {
                    (item.parent_backdrop_item_id.clone()?, item.parent_backdrop_image_tags.first()?.clone())
                }
            }
            ImageKind::Thumb => (item.id.clone(), item.image_tags.get("Thumb")?.clone()),
        };
        let name = match kind {
            ImageKind::Primary => "Primary",
            ImageKind::Backdrop => "Backdrop",
            ImageKind::Thumb => "Thumb",
        };
        Some(format!("{}/Items/{id}/Images/{name}?maxWidth={width_px}&quality=90&tag={tag}", self.base))
    }
    /// P-LIB-14: the banner-view image — the item's Banner, else Thumb, else Backdrop (own or parent's).
    pub fn banner_image_url(&self, item: &Item, width_px: u32) -> Option<String> {
        let (id, tag, name) = if let Some(t) = item.image_tags.get("Banner") {
            (item.id.clone(), t.clone(), "Banner")
        } else if let Some(t) = item.image_tags.get("Thumb") {
            (item.id.clone(), t.clone(), "Thumb")
        } else if let Some(t) = item.backdrop_image_tags.first() {
            (item.id.clone(), t.clone(), "Backdrop")
        } else {
            (item.parent_backdrop_item_id.clone()?, item.parent_backdrop_image_tags.first()?.clone(), "Backdrop")
        };
        Some(format!("{}/Items/{id}/Images/{name}?maxWidth={width_px}&quality=90&tag={tag}", self.base))
    }
    /// GET raw bytes (images), IMAGE_TIMEOUT, 20 MB cap. The `Authorization` header is only sent when the URL
    /// is on this server's origin (scheme + host + port): remote candidate thumbs and saved poster picks live on
    /// tmdb / fanart, and the Jellyfin token must never reach a third party.
    pub fn get_bytes(&self, url: &str) -> Result<Vec<u8>> {
        let builder = self.agent.get(url).config().timeout_global(Some(IMAGE_TIMEOUT)).build();
        let builder =
            if is_server_url(&self.base, url) { builder.header("Authorization", &self.auth_header()) } else { builder };
        let mut resp = builder.call().map_err(net)?;
        check_status(&mut resp)?;
        resp.body_mut().with_config().limit(20 << 20).read_to_vec().map_err(net)
    }

    /// R-TAG-6: true when the signed-in user is a server administrator (`/Users/Me` → Policy.IsAdministrator).
    pub fn is_admin(&self) -> Result<bool> {
        match self.get_json::<UserDto>("/Users/Me") {
            Ok(u) => Ok(u.policy.is_administrator),
            Err(ApiError::NotFound) => {
                let uid = self.user_id()?;
                let u: UserDto = self.get_json(&format!("/Users/{uid}"))?;
                Ok(u.policy.is_administrator)
            }
            Err(e) => Err(e),
        }
    }

    /// FIX-4: the signed-in user's "Remember subtitle selections" (`/Users/Me` → Configuration.RememberSubtitleSelections).
    pub fn remember_subtitle_selections(&self) -> Result<bool> {
        match self.get_json::<UserDto>("/Users/Me") {
            Ok(u) => Ok(u.configuration.remember_subtitle_selections),
            Err(ApiError::NotFound) => {
                let uid = self.user_id()?;
                let u: UserDto = self.get_json(&format!("/Users/{uid}"))?;
                Ok(u.configuration.remember_subtitle_selections)
            }
            Err(e) => Err(e),
        }
    }

    /// R-TAG-6: re-read the full item, replace its tag list with `edit(current tags)`, post it back; every other
    /// field unchanged. Returns the new tag list. Non-admin → `ApiError::Forbidden`.
    pub fn update_tags(&self, item_id: &str, edit: impl FnOnce(&[String]) -> Vec<String>) -> Result<Vec<String>> {
        let uid = self.user_id()?;
        let new = format!("/Items/{item_id}?userId={uid}&Fields={ITEM_FIELDS}");
        let legacy = format!("/Users/{uid}/Items/{item_id}?Fields={ITEM_FIELDS}");
        let mut value: serde_json::Value = self.get_with_fallback(Route::Item, &new, &legacy)?;
        let tags: Vec<String> = match value.get("Tags") {
            Some(serde_json::Value::Array(arr)) => arr.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
            _ => Vec::new(),
        };
        let new_tags = edit(&tags);
        let tags_json: Vec<serde_json::Value> = new_tags.iter().map(|t| serde_json::Value::String(t.clone())).collect();
        if let serde_json::Value::Object(map) = &mut value {
            map.insert("Tags".into(), serde_json::Value::Array(tags_json));
        }
        self.post_ok(&format!("/Items/{item_id}"), &value)?;
        Ok(new_tags)
    }

    /// F5b / P-PPL-1: a person item (`/Items/{id}` with Overview, PremiereDate…; legacy route on 404 like `item`).
    pub fn person(&self, person_id: &str) -> Result<Item> {
        let uid = self.user_id()?;
        let new = format!("/Items/{person_id}?userId={uid}&Fields=Overview,PremiereDate");
        let legacy = format!("/Users/{uid}/Items/{person_id}?Fields=Overview,PremiereDate");
        self.get_with_fallback(Route::Item, &new, &legacy)
    }

    /// F5b / P-PPL-2: films and shows on the server with this person (`/Items?PersonIds=…&Recursive=true&
    /// IncludeItemTypes=Movie,Series`, card fields), sorted by name.
    pub fn person_items(&self, person_id: &str) -> Result<Vec<Item>> {
        let uid = self.user_id()?;
        let path = format!(
            "/Items?userId={uid}&PersonIds={person_id}&Recursive=true&IncludeItemTypes=Movie,Series&SortBy=SortName&SortOrder=Ascending&Fields={CARD_FIELDS}"
        );
        let page: Page = self.get_json(&path)?;
        Ok(page.items)
    }

    /// G12 / P-USER-1: change the signed-in user's password (`POST /Users/{id}/Password`). Wrong current
    /// password → `Unauthorized`.
    pub fn change_password(&self, user_id: &str, current: &str, new: &str) -> Result<()> {
        let body = serde_json::json!({ "Id": user_id, "CurrentPw": current, "NewPw": new });
        self.post_ok(&format!("/Users/{user_id}/Password"), &body)
    }

    /// G12 / P-AUTH-9, P-USER-7: approve a Quick Connect code shown on another device
    /// (`POST /QuickConnect/Authorize?code=`). An unknown/expired code → `NotFound`.
    pub fn quick_connect_authorize(&self, code: &str) -> Result<()> {
        self.post_empty_ok(&format!("/QuickConnect/Authorize?code={}", url_encode(code)))
    }

    /// G12 / P-USER-1: the signed-in user's profile picture URL (public, no token needed).
    pub fn user_image_url(&self, user_id: &str, width_px: u32) -> String {
        format!("{}/Users/{user_id}/Images/Primary?maxWidth={width_px}&quality=90", self.base)
    }

    /// P-LIB-2..9: a library page with a server-side sort and the basic filters (`IsUnplayed` / `IsPlayed` /
    /// `IsResumable` / `IsFavorite`), plus the G11 extra filters. `StoryOrder` / `MyOrder` have no server sort: the
    /// items come back in name order and the caller sorts them locally over all loaded pages.
    pub fn library_query(&self, parent_id: &str, start: usize, limit: usize, q: &LibraryQuery) -> Result<Page> {
        self.library_query_types(parent_id, start, limit, q, "Movie,Series")
    }

    /// G11 / P-LIB-21: `library_query` for another set of `IncludeItemTypes` (e.g. `"Episode"`).
    pub fn library_query_types(
        &self,
        parent_id: &str,
        start: usize,
        limit: usize,
        q: &LibraryQuery,
        types: &str,
    ) -> Result<Page> {
        let uid = self.user_id()?;
        let mut path = format!(
            "/Items?userId={uid}&ParentId={parent_id}&Recursive=true&IncludeItemTypes={types}&StartIndex={start}&Limit={limit}&Fields={CARD_FIELDS}"
        );
        let sort = q.sort.server_value().unwrap_or("SortName");
        let order = if q.descending && !q.sort.is_local() { "Descending" } else { "Ascending" };
        path.push_str(&format!("&SortBy={sort}&SortOrder={order}"));
        push_filters(&mut path, q);
        self.get_json(&path)
    }

    /// G3 / P-ITEM-7: mark an item as a favourite (`favorite` true → POST) or unmark it (DELETE), with the
    /// R-API-4 legacy fallback like `set_played`.
    pub fn set_favorite(&self, id: &str, favorite: bool) -> Result<()> {
        let uid = self.user_id()?;
        let new = format!("/UserFavoriteItems/{id}?userId={uid}");
        let legacy = format!("/Users/{uid}/FavoriteItems/{id}");
        if self.uses_legacy(Route::Favorite) {
            return self.mutate_ok(&legacy, favorite);
        }
        match self.mutate_ok(&new, favorite) {
            Ok(()) => Ok(()),
            Err(ApiError::NotFound) => {
                self.remember_legacy(Route::Favorite);
                self.mutate_ok(&legacy, favorite)
            }
            Err(e) => Err(e),
        }
    }

    /// G3 / P-COLL-6: the user's favourite items of the given `IncludeItemTypes` (comma-separated, e.g.
    /// `"Movie,Series,Episode"`), recursive across all libraries.
    pub fn favourites(&self, include_item_types: &str) -> Result<Vec<Item>> {
        let uid = self.user_id()?;
        let path = format!(
            "/Items?userId={uid}&Filters=IsFavorite&Recursive=true&IncludeItemTypes={include_item_types}&Fields={CARD_FIELDS}"
        );
        let page: Page = self.get_json(&path)?;
        Ok(page.items)
    }

    /// G3 / P-COLL-6: the user's favourite people (`/Persons?isFavorite=true`).
    pub fn favourite_people(&self) -> Result<Vec<Item>> {
        let uid = self.user_id()?;
        let path = format!("/Persons?userId={uid}&isFavorite=true&Fields={CARD_FIELDS}");
        let page: Page = self.get_json(&path)?;
        Ok(page.items)
    }
    /// G9 / P-TV-7: all episodes of the series; with `include_missing` the server's missing / not-yet-aired
    /// placeholder episodes come too, otherwise they are filtered out (`isMissing=false`).
    pub fn episodes_opts(&self, series_id: &str, include_missing: bool) -> Result<Vec<Item>> {
        let uid = self.user_id()?;
        let mut path = format!("/Shows/{series_id}/Episodes?userId={uid}&Fields=Overview,PremiereDate");
        if !include_missing {
            path.push_str("&isMissing=false");
        }
        let page: Page = self.get_json(&path)?;
        Ok(page.items)
    }

    /// G9 / P-LIB-23: up to `limit` playable items (movies and episodes) under a library, for Play all
    /// (`shuffle` false: the library's sort, series by name then episode order) or Shuffle (server random).
    pub fn library_playable(
        &self,
        parent_id: &str,
        limit: usize,
        q: &LibraryQuery,
        shuffle: bool,
    ) -> Result<Vec<Item>> {
        let uid = self.user_id()?;
        let (sort, order) = if shuffle {
            ("Random", "Ascending")
        } else {
            let order = if q.descending && !q.sort.is_local() { "Descending" } else { "Ascending" };
            match q.sort.server_value() {
                Some(v) if v != "SortName" => (v, order),
                _ => ("SeriesSortName,ParentIndexNumber,IndexNumber,SortName", order),
            }
        };
        let mut path = format!(
            "/Items?userId={uid}&ParentId={parent_id}&Recursive=true&IncludeItemTypes=Movie,Episode&Limit={limit}&Fields={CARD_FIELDS}&SortBy={sort}&SortOrder={order}"
        );
        push_filters(&mut path, q);
        let page: Page = self.get_json(&path)?;
        Ok(page.items)
    }

    /// G10 / P-PLAY-11: the media segments (intro, recap, credits …) of an item, from `GET /MediaSegments/{id}`
    /// (Jellyfin 10.10). A server without the route (404) or an item with none gives an empty list.
    pub fn media_segments(&self, item_id: &str) -> Result<Vec<Segment>> {
        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct Raw {
            #[serde(rename = "Type", default)]
            kind: String,
            #[serde(default)]
            start_ticks: i64,
            #[serde(default)]
            end_ticks: i64,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct Resp {
            #[serde(default)]
            items: Vec<Raw>,
        }
        match self.get_json::<Resp>(&format!("/MediaSegments/{item_id}")) {
            Ok(r) => Ok(r
                .items
                .into_iter()
                .map(|s| Segment {
                    kind: SegmentKind::from_server(&s.kind),
                    start: ticks_to_secs(s.start_ticks),
                    end: ticks_to_secs(s.end_ticks),
                })
                .filter(|s| s.end > s.start)
                .collect()),
            Err(ApiError::NotFound) => Ok(vec![]),
            Err(e) => Err(e),
        }
    }
    /// G8 / P-ITEM-8: the local trailers of an item (`GET /Items/{id}/LocalTrailers`), playable like any video.
    pub fn local_trailers(&self, id: &str) -> Result<Vec<Item>> {
        let uid = self.user_id()?;
        self.get_json(&format!("/Items/{id}/LocalTrailers?userId={uid}"))
    }

    /// G8 / P-ITEM-15: the special features (extras) of an item (`GET /Items/{id}/SpecialFeatures`).
    pub fn special_features(&self, id: &str) -> Result<Vec<Item>> {
        let uid = self.user_id()?;
        self.get_json(&format!("/Items/{id}/SpecialFeatures?userId={uid}"))
    }

    /// G8 / P-ITEM-14: items like this one (`GET /Items/{id}/Similar`).
    pub fn similar(&self, id: &str, limit: usize) -> Result<Vec<Item>> {
        let uid = self.user_id()?;
        let page: Page =
            self.get_json(&format!("/Items/{id}/Similar?userId={uid}&limit={limit}&Fields={CARD_FIELDS}"))?;
        Ok(page.items)
    }

    /// G8 / P-ITEM-18 + P-ITEM-11: every media source of an item with the technical fields `models.rs` does not
    /// keep (name, size, bitrate, path, resolution), for the media info panel and the version picker labels.
    pub fn media_info(&self, id: &str) -> Result<Vec<SourceInfo>> {
        #[derive(Deserialize, Default)]
        #[serde(rename_all = "PascalCase", default)]
        struct Raw {
            media_sources: Vec<SourceInfo>,
        }
        let uid = self.user_id()?;
        let fields = "MediaSources,MediaStreams,Path";
        let new = format!("/Items/{id}?userId={uid}&Fields={fields}");
        let legacy = format!("/Users/{uid}/Items/{id}?Fields={fields}");
        let raw: Raw = self.get_with_fallback(Route::MediaInfo, &new, &legacy)?;
        Ok(raw.media_sources)
    }

    // ---------- G7: collections and playlists ----------

    /// G7 / P-LIB-17 + P-COLL-1: every collection (BoxSet) on the server the user can see, by name.
    pub fn collections(&self) -> Result<Vec<Item>> {
        let uid = self.user_id()?;
        let path = format!(
            "/Items?userId={uid}&IncludeItemTypes=BoxSet&Recursive=true&SortBy=SortName&SortOrder=Ascending&Fields={CARD_FIELDS}"
        );
        let page: Page = self.get_json(&path)?;
        Ok(page.items)
    }

    /// G7 / P-COLL-1: the items inside a collection (`/Items?ParentId=<boxset>`), in the server's own order.
    pub fn collection_items(&self, collection_id: &str) -> Result<Vec<Item>> {
        let uid = self.user_id()?;
        let path = format!("/Items?userId={uid}&ParentId={collection_id}&Fields={CARD_FIELDS}");
        let page: Page = self.get_json(&path)?;
        Ok(page.items)
    }

    /// G7 / P-COLL-2: create a collection holding `ids`; returns the new collection id. Admin only (403 → `Forbidden`).
    pub fn create_collection(&self, name: &str, ids: &[String]) -> Result<String> {
        let path = format!("/Collections?Name={}&Ids={}", url_encode(name), ids.join(","));
        let made: Created = self.post_empty_json(&path)?;
        Ok(made.id)
    }

    /// G7 / P-COLL-2: add items to a collection. Admin only.
    pub fn add_to_collection(&self, collection_id: &str, ids: &[String]) -> Result<()> {
        self.post_empty_ok(&format!("/Collections/{collection_id}/Items?Ids={}", ids.join(",")))
    }

    /// G7 / P-COLL-3: take items out of a collection (the items themselves stay on the server). Admin only.
    pub fn remove_from_collection(&self, collection_id: &str, ids: &[String]) -> Result<()> {
        self.delete_ok(&format!("/Collections/{collection_id}/Items?Ids={}", ids.join(",")))
    }

    /// G7 / P-COLL-3: the collections that currently contain `item_id` (checks each collection's children).
    pub fn collections_containing(&self, item_id: &str) -> Result<Vec<Item>> {
        let mut found = Vec::new();
        for c in self.collections()? {
            if self.collection_items(&c.id)?.iter().any(|i| i.id == item_id) {
                found.push(c);
            }
        }
        Ok(found)
    }

    /// G7 / P-COLL-4: the signed-in user's playlists, by name.
    pub fn playlists(&self) -> Result<Vec<Item>> {
        let uid = self.user_id()?;
        let path = format!(
            "/Items?userId={uid}&IncludeItemTypes=Playlist&Recursive=true&SortBy=SortName&SortOrder=Ascending&Fields={CARD_FIELDS}"
        );
        let page: Page = self.get_json(&path)?;
        // FIX-4 PL4: each playlist id once (first-seen order), for the Playlists page and the Add-to picker alike
        let mut seen = std::collections::HashSet::new();
        Ok(page.items.into_iter().filter(|i| seen.insert(i.id.clone())).collect())
    }

    /// G7 / P-COLL-5: create a video playlist holding `ids`; returns the new playlist id.
    pub fn create_playlist(&self, name: &str, ids: &[String]) -> Result<String> {
        let uid = self.user_id()?;
        let body = serde_json::json!({ "Name": name, "Ids": ids, "UserId": uid, "MediaType": "Video" });
        let mut resp = self
            .agent
            .post(&self.url("/Playlists"))
            .header("Authorization", &self.auth_header())
            .send_json(&body)
            .map_err(net)?;
        check_status(&mut resp)?;
        let made: Created = resp.body_mut().read_json().map_err(|e| ApiError::Decode(e.to_string()))?;
        Ok(made.id)
    }

    /// G7 / P-COLL-4: a playlist's entries in order. Each carries the `PlaylistItemId` the server needs to remove or
    /// move it (the plain `Item` has no field for it, so it is read from the raw reply here).
    pub fn playlist_items(&self, playlist_id: &str) -> Result<Vec<PlaylistEntry>> {
        let uid = self.user_id()?;
        let path = format!("/Playlists/{playlist_id}/Items?userId={uid}&Fields={CARD_FIELDS}");
        let raw: RawEntries = self.get_json(&path)?;
        let mut out = Vec::with_capacity(raw.items.len());
        for v in raw.items {
            let entry_id = v.get("PlaylistItemId").and_then(|x| x.as_str()).unwrap_or_default().to_string();
            let item: Item = serde_json::from_value(v).map_err(|e| ApiError::Decode(e.to_string()))?;
            out.push(PlaylistEntry { entry_id, item });
        }
        Ok(out)
    }

    /// FIX-3 #9: the first `limit` items of a playlist, in order, with their image tags — what a playlist tile's
    /// mosaic is drawn from.
    pub fn playlist_posters(&self, playlist_id: &str, limit: usize) -> Result<Vec<Item>> {
        let uid = self.user_id()?;
        let path = format!("/Playlists/{playlist_id}/Items?userId={uid}&Limit={limit}&Fields={CARD_FIELDS}");
        let page: Page = self.get_json(&path)?;
        Ok(page.items)
    }

    /// G7 / P-COLL-5: add items to the end of a playlist.
    pub fn add_to_playlist(&self, playlist_id: &str, ids: &[String]) -> Result<()> {
        let uid = self.user_id()?;
        self.post_empty_ok(&format!("/Playlists/{playlist_id}/Items?userId={uid}&Ids={}", ids.join(",")))
    }

    /// G7 / P-COLL-5: remove entries (their `PlaylistItemId`s, not item ids) from a playlist.
    pub fn remove_from_playlist(&self, playlist_id: &str, entry_ids: &[String]) -> Result<()> {
        self.delete_ok(&format!("/Playlists/{playlist_id}/Items?EntryIds={}", entry_ids.join(",")))
    }

    /// G7 / P-COLL-5: move the entry at `from` to position `to` (0-based) of a playlist whose entries are
    /// `entry_ids` (their `PlaylistItemId`s, in the current order). Jellyfin 10.10.7's Move call never lands an
    /// item at position 0 and overshoots downwards by one, so `move_plan` works out calls that land exactly.
    pub fn move_playlist_item(&self, playlist_id: &str, entry_ids: &[String], from: usize, to: usize) -> Result<()> {
        for (entry, new_index) in move_plan(entry_ids.len(), from, to) {
            self.post_empty_ok(&format!("/Playlists/{playlist_id}/Items/{}/Move/{new_index}", entry_ids[entry]))?;
        }
        Ok(())
    }

    /// G7 / P-COLL-5: delete a playlist, or a collection the user made, with `DELETE /Items/{id}`. Only ever call
    /// it on a playlist or collection: it is the server's generic item delete.
    pub fn delete_item(&self, id: &str) -> Result<()> {
        self.delete_ok(&format!("/Items/{id}"))
    }

    /// G6 / P-PLAY-13: PlaybackInfo with a device profile. `max_kbps` = 0 and `force_transcode` false is the plain
    /// request (direct play, as `playback_info`). A cap makes the server answer with a `TranscodingUrl` when the source
    /// is above it; `force_transcode` turns direct play off (used when mpv could not open the direct stream).
    pub fn playback_plan(&self, id: &str, max_kbps: u32, force_transcode: bool) -> Result<PlaybackPlan> {
        let uid = self.user_id()?;
        let path = format!("/Items/{id}/PlaybackInfo?userId={uid}");
        let raw: serde_json::Value = self.post_json(&path, &transcode_body(max_kbps, force_transcode))?;
        plan_from_json(&raw, &self.base, self.session.as_ref().map(|s| s.token.as_str()).unwrap_or(""))
    }
    /// G6 / P-PLAY-13: the report calls with an explicit `PlayMethod` ("DirectPlay" or "Transcode").
    pub fn report_start_as(&self, r: &ProgressReport, method: &str) -> Result<()> {
        self.post_ok("/Sessions/Playing", &report_body_as(r, method))
    }
    pub fn report_progress_as(&self, r: &ProgressReport, method: &str) -> Result<()> {
        self.post_ok("/Sessions/Playing/Progress", &report_body_as(r, method))
    }
    pub fn report_stopped_as(&self, r: &ProgressReport, method: &str) -> Result<()> {
        self.post_ok("/Sessions/Playing/Stopped", &report_body_as(r, method))
    }

    fn post_empty_json<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let mut resp =
            self.agent.post(&self.url(path)).header("Authorization", &self.auth_header()).send_empty().map_err(net)?;
        check_status(&mut resp)?;
        resp.body_mut().read_json().map_err(|e| ApiError::Decode(e.to_string()))
    }

    // ---------- G13: sign-in extras and search extras ----------

    /// G13 / P-AUTH-10: the server's branding (`GET /Branding/Configuration`, no sign-in needed): the login disclaimer.
    pub fn branding(&self) -> Result<Branding> {
        self.get_json("/Branding/Configuration")
    }

    /// G13 / P-AUTH-6: ask the server to start a password reset for `user` (`POST /Users/ForgotPassword`).
    pub fn forgot_password(&self, user: &str) -> Result<ForgotPassword> {
        self.post_json("/Users/ForgotPassword", &serde_json::json!({ "EnteredUsername": user }))
    }

    /// G13 / P-AUTH-6: redeem the PIN from the server's PIN file (`POST /Users/ForgotPassword/Pin`).
    pub fn forgot_password_pin(&self, pin: &str) -> Result<PinRedeem> {
        self.post_json("/Users/ForgotPassword/Pin", &serde_json::json!({ "Pin": pin }))
    }

    /// G13 / P-SRCH-2: people whose name matches `term` (`/Persons?searchTerm=`).
    pub fn search_people(&self, term: &str) -> Result<Vec<Item>> {
        let uid = self.user_id()?;
        let path = format!("/Persons?userId={uid}&searchTerm={}&Limit=20&Fields={CARD_FIELDS}", url_encode(term));
        let page: Page = self.get_json(&path)?;
        Ok(page.items)
    }

    /// G13 / P-SRCH-3: what to show before typing: the server's suggestions (`/Items/Suggestions`), or when it has
    /// none the newest films and shows.
    pub fn suggestions(&self, limit: usize) -> Result<Vec<Item>> {
        let uid = self.user_id()?;
        let path = format!(
            "/Items/Suggestions?userId={uid}&mediaType=Video&type=Movie,Series&limit={limit}&Fields={CARD_FIELDS}"
        );
        let page: Page = self.get_json(&path)?;
        if !page.items.is_empty() {
            return Ok(page.items);
        }
        let path = format!(
            "/Items?userId={uid}&Recursive=true&IncludeItemTypes=Movie,Series&SortBy=DateCreated&SortOrder=Descending&Limit={limit}&Fields={CARD_FIELDS}"
        );
        let page: Page = self.get_json(&path)?;
        Ok(page.items)
    }

    /// G11 / P-LIB-18: the genres in a library (`/Genres?ParentId=`), by name. `types` is `IncludeItemTypes`.
    pub fn genres(&self, parent_id: &str, types: &str) -> Result<Vec<Item>> {
        let uid = self.user_id()?;
        let path = format!(
            "/Genres?userId={uid}&ParentId={parent_id}&IncludeItemTypes={types}&SortBy=SortName&SortOrder=Ascending"
        );
        let page: Page = self.get_json(&path)?;
        Ok(page.items)
    }

    /// G11 / P-LIB-20: the studios / networks in a library (`/Studios?ParentId=`), by name.
    pub fn studios(&self, parent_id: &str, types: &str) -> Result<Vec<Item>> {
        let uid = self.user_id()?;
        let path = format!(
            "/Studios?userId={uid}&ParentId={parent_id}&IncludeItemTypes={types}&SortBy=SortName&SortOrder=Ascending"
        );
        let page: Page = self.get_json(&path)?;
        Ok(page.items)
    }

    /// G11 / P-LIB-19: episodes yet to air for the series in a library (`/Shows/Upcoming?parentId=`).
    pub fn upcoming(&self, parent_id: &str) -> Result<Vec<Item>> {
        let uid = self.user_id()?;
        let path = format!("/Shows/Upcoming?userId={uid}&parentId={parent_id}&Limit=60&Fields={CARD_FIELDS}");
        let page: Page = self.get_json(&path)?;
        Ok(page.items)
    }

    /// G11 / P-LIB-16: movie suggestion rows for a library (`/Movies/Recommendations?parentId=`).
    pub fn recommendations(&self, parent_id: &str) -> Result<Vec<Recommendation>> {
        let uid = self.user_id()?;
        let path = format!(
            "/Movies/Recommendations?userId={uid}&parentId={parent_id}&categoryLimit=6&itemLimit=12&Fields={CARD_FIELDS}"
        );
        self.get_json(&path)
    }

    /// G11 / P-LIB-10: the genres, tags, parental ratings and years a library's items use (`/Items/Filters`),
    /// to fill the "Filters…" dialog.
    pub fn filter_options(&self, parent_id: &str, types: &str) -> Result<FilterOptions> {
        let uid = self.user_id()?;
        let path = format!("/Items/Filters?userId={uid}&parentId={parent_id}&includeItemTypes={types}");
        self.get_json(&path)
    }

    /// RR5 / P-ITEM-3: every film and show of a genre, library-wide, by name (`/Items?Genres=`).
    pub fn items_by_genre(&self, genre: &str) -> Result<Vec<Item>> {
        let uid = self.user_id()?;
        let path = format!(
            "/Items?userId={uid}&Genres={}&Recursive=true&IncludeItemTypes=Movie,Series&SortBy=SortName&SortOrder=Ascending&Fields={CARD_FIELDS}",
            url_encode(genre)
        );
        let page: Page = self.get_json(&path)?;
        Ok(page.items)
    }

    /// RR5 / P-ITEM-3: every film and show of a studio, library-wide, by name (`/Items?StudioIds=`).
    pub fn items_by_studio(&self, studio_id: &str) -> Result<Vec<Item>> {
        let uid = self.user_id()?;
        let path = format!(
            "/Items?userId={uid}&StudioIds={studio_id}&Recursive=true&IncludeItemTypes=Movie,Series&SortBy=SortName&SortOrder=Ascending&Fields={CARD_FIELDS}"
        );
        let page: Page = self.get_json(&path)?;
        Ok(page.items)
    }

    /// RR5 / P-ITEM-21: the playable items (films, episodes) under `items`: a film or episode stands for itself,
    /// anything else (series, season, collection, folder) is replaced by the films / episodes below it.
    pub fn playable_under(&self, items: &[Item]) -> Result<Vec<Item>> {
        let uid = self.user_id()?;
        let mut out = vec![];
        for it in items {
            if matches!(it.kind, ItemKind::Movie | ItemKind::Episode) {
                out.push(it.clone());
            } else if matches!(it.kind, ItemKind::Series | ItemKind::Season | ItemKind::BoxSet | ItemKind::Folder) {
                let path = format!(
                    "/Items?userId={uid}&ParentId={}&Recursive=true&IncludeItemTypes=Movie,Episode&SortBy=SeriesSortName,ParentIndexNumber,IndexNumber,SortName&Fields={CARD_FIELDS}",
                    it.id
                );
                let page: Page = self.get_json(&path)?;
                out.extend(page.items);
            }
        }
        Ok(out)
    }

    /// RR5 / P-TV-2: the server's next-up episode of one series (`/Shows/NextUp?seriesId=`); `None` when it has none.
    pub fn series_next_up(&self, series_id: &str) -> Result<Option<Item>> {
        let uid = self.user_id()?;
        let path = format!("/Shows/NextUp?userId={uid}&seriesId={series_id}&Limit=1&Fields={CARD_FIELDS}");
        let page: Page = self.get_json(&path)?;
        Ok(page.items.into_iter().next())
    }

    /// RR8 / P-USER-4: `next_up` with the web's two options — `rewatching` also lists episodes the user has started
    /// rewatching, `cutoff_days` > 0 drops shows not watched for that many days.
    pub fn next_up_with(&self, limit: usize, rewatching: bool, cutoff_days: u32) -> Result<Vec<Item>> {
        let uid = self.user_id()?;
        let path = next_up_path(uid, limit, rewatching, cutoff_days, now_unix_secs());
        let page: Page = self.get_json(&path)?;
        Ok(page.items)
    }

    /// RR8 / P-USER-4: `playback_plan` that can ask for fMP4 HLS segments instead of MPEG-TS.
    pub fn playback_plan_with(
        &self,
        id: &str,
        max_kbps: u32,
        force_transcode: bool,
        fmp4: bool,
    ) -> Result<PlaybackPlan> {
        self.playback_plan_for(id, max_kbps, force_transcode, fmp4, None, None)
    }

    /// C3-SUBS / R-SUBS-1: `playback_plan_with` naming the chosen audio and subtitle stream (`subtitle` -1 = none),
    /// so a transcode burns in the chosen subtitle and not the source's default.
    pub fn playback_plan_for(
        &self,
        id: &str,
        max_kbps: u32,
        force_transcode: bool,
        fmp4: bool,
        audio: Option<i32>,
        subtitle: Option<i32>,
    ) -> Result<PlaybackPlan> {
        let uid = self.user_id()?;
        let path = playback_info_path(uid, id, audio, subtitle);
        let raw: serde_json::Value = self.post_json(&path, &transcode_body_with(max_kbps, force_transcode, fmp4))?;
        plan_from_json(&raw, &self.base, self.session.as_ref().map(|s| s.token.as_str()).unwrap_or(""))
    }

    /// RR8 / P-USER-1: set the user's profile picture (`POST /Users/{id}/Images/Primary`, base64 body with the
    /// image's content type). Anything that is not a png / jpeg / gif / webp is refused before it is sent.
    pub fn set_user_image(&self, user_id: &str, bytes: &[u8]) -> Result<()> {
        let Some(kind) = image_content_type(bytes) else {
            return Err(ApiError::Decode("That file is not a picture (png, jpeg, gif or webp).".into()));
        };
        let mut resp = self
            .agent
            .post(&self.url(&format!("/Users/{user_id}/Images/Primary")))
            .header("Authorization", &self.auth_header())
            .header("Content-Type", kind)
            .send(base64_encode(bytes).as_bytes())
            .map_err(net)?;
        check_status(&mut resp)
    }

    /// RR8 / P-USER-1: remove the user's profile picture (`DELETE /Users/{id}/Images/Primary`).
    pub fn delete_user_image(&self, user_id: &str) -> Result<()> {
        self.delete_ok(&format!("/Users/{user_id}/Images/Primary"))
    }

    /// RR8 / P-USER-2: up to `limit` random movies and shows that have a backdrop, for the screensaver.
    pub fn backdrop_items(&self, limit: usize) -> Result<Vec<Item>> {
        let uid = self.user_id()?;
        let path = format!(
            "/Items?userId={uid}&Recursive=true&IncludeItemTypes=Movie,Series&ImageTypes=Backdrop&SortBy=Random&Limit={limit}&Fields={CARD_FIELDS}"
        );
        let page: Page = self.get_json(&path)?;
        Ok(page.items)
    }

    /// FIX-2 #9: the playlists this user may add to — `playlists()` also returns other users' public ones, which the
    /// server refuses to change. One `/Playlists/{id}/Users/{userId}` per playlist: kept when it answers `CanEdit`
    /// (the owner is listed too); 404 / 403 / no `CanEdit` → left out.
    pub fn editable_playlists(&self) -> Result<Vec<Item>> {
        let uid = self.user_id()?.to_string();
        let mut out = vec![];
        for p in self.playlists()? {
            #[derive(serde::Deserialize, Default)]
            #[serde(rename_all = "PascalCase", default)]
            struct Perm {
                can_edit: bool,
            }
            match self.get_json::<Perm>(&format!("/Playlists/{}/Users/{uid}", p.id)) {
                Ok(perm) if perm.can_edit => out.push(p),
                Ok(_) | Err(ApiError::NotFound) | Err(ApiError::Server(..)) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(out)
    }

    /// D2 (Apple TV details hero): the item's title logo image, when it has one (no request; `width_px` bucketed).
    /// FIX-1 #12: always PNG — a re-encoded JPEG loses the transparency and paints a black box behind the title.
    pub fn logo_url(&self, item: &Item, width_px: u32) -> Option<String> {
        let tag = item.image_tags.get("Logo")?;
        Some(format!("{}/Items/{}/Images/Logo?maxWidth={width_px}&format=png&tag={tag}", self.base, item.id))
    }

    /// RR6 / P-ITEM-16: the other parts of a multi-part film (`GET /Videos/{id}/AdditionalParts`), in order.
    pub fn additional_parts(&self, id: &str) -> Result<Vec<Item>> {
        let uid = self.user_id()?;
        let page: Page = self.get_json(&format!("/Videos/{id}/AdditionalParts?userId={uid}"))?;
        Ok(page.items)
    }

    /// RR6 / P-ITEM-17: the still of one chapter (`/Items/{id}/Images/Chapter/{index}`); `tag` is the chapter's
    /// `ImageTag`, so the URL changes when the picture does. `width_px` is already bucketed.
    pub fn chapter_image_url(&self, item_id: &str, index: usize, tag: &str, width_px: u32) -> String {
        format!("{}/Items/{item_id}/Images/Chapter/{index}?maxWidth={width_px}&quality=90&tag={tag}", self.base)
    }

    /// RR6 / P-PPL-1: a person's `(PremiereDate, EndDate)` = born, died (ISO strings as the server sends them).
    /// `Item` has no end date, so this reads the two fields from the raw answer.
    pub fn person_dates(&self, person_id: &str) -> Result<(Option<String>, Option<String>)> {
        let uid = self.user_id()?;
        let v: serde_json::Value = self.get_json(&format!("/Items/{person_id}?userId={uid}"))?;
        let field = |k: &str| v.get(k).and_then(|d| d.as_str()).map(str::to_string).filter(|d| !d.is_empty());
        Ok((field("PremiereDate"), field("EndDate")))
    }
    /// FIX-1 #13: the second search when no title matches — films and shows whose plot, genres, tags or people
    /// mention `term` (Jellyfin's `searchTerm` only looks at names). Fetches the film/show list with plots and
    /// matches here (fine at home-library sizes). Also returns the most common genres, for the empty page.
    pub fn search_text(&self, term: &str) -> Result<(Vec<Item>, Vec<String>)> {
        let uid = self.user_id()?;
        let path = format!(
            "/Items?userId={uid}&Recursive=true&IncludeItemTypes=Movie,Series&Limit=3000&Fields={CARD_FIELDS},Overview,People"
        );
        let page: Page = self.get_json(&path)?;
        let genres = top_genres(&page.items, 8);
        Ok((text_matches(page.items, term), genres))
    }
}

/// RR8 / P-USER-4: the `/Shows/NextUp` path. `now_secs` is the Unix time used for the cutoff date.
pub fn next_up_path(uid: &str, limit: usize, rewatching: bool, cutoff_days: u32, now_secs: i64) -> String {
    let mut path = format!("/Shows/NextUp?userId={uid}&Limit={limit}&Fields={CARD_FIELDS}");
    if rewatching {
        path.push_str("&enableRewatching=true");
    }
    if cutoff_days > 0 {
        path.push_str(&format!("&nextUpDateCutoff={}", date_days_ago(now_secs, cutoff_days)));
    }
    path
}

// crate::time: std's clock panics in the TV web build (wasm32)
fn now_unix_secs() -> i64 {
    crate::time::SystemTime::now().duration_since(crate::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// RR8: the UTC date `days` before Unix time `now_secs`, as `YYYY-MM-DD`.
pub fn date_days_ago(now_secs: i64, days: u32) -> String {
    let z = now_secs.div_euclid(86_400) - i64::from(days) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// RR8: standard base64 with padding.
pub fn base64_encode(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n =
            (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        out.push(A[(n >> 18) as usize & 63] as char);
        out.push(A[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { A[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { A[n as usize & 63] as char } else { '=' });
    }
    out
}

/// RR8: the content type of a picture from its first bytes (png, jpeg, gif, webp); None for anything else.
pub fn image_content_type(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if b.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if b.len() >= 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// The origin (`scheme://host:port`) of an absolute http(s) URL, lower-cased with the default port made
/// explicit (http → 80, https → 443). `None` for anything that is not an absolute http(s) URL (a `file://`
/// path, a relative path, …). Used to keep the Jellyfin token on the server's own origin only.
pub fn origin(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    let scheme = if lower.starts_with("http://") {
        "http"
    } else if lower.starts_with("https://") {
        "https"
    } else {
        return None;
    };
    let hostport = &lower[scheme.len() + 3..];
    let hostport = hostport.split(['/', '?', '#']).next()?;
    let (host, port) = match hostport.rsplit_once(':') {
        // `host:port`; an IPv6 literal (`[::1]:8096`) has no bare `:` after the first split and stays whole.
        Some((h, p)) if !h.contains(':') => (h, p),
        _ => (hostport, ""),
    };
    let port = if port.is_empty() {
        if scheme == "http" {
            "80"
        } else {
            "443"
        }
    } else {
        port
    };
    Some(format!("{scheme}://{host}:{port}"))
}

/// True when `url` shares `base`'s origin (scheme + host + port) — the only URLs the client may send the
/// Jellyfin token to.
pub fn is_server_url(base: &str, url: &str) -> bool {
    match (origin(base), origin(url)) {
        (Some(b), Some(u)) => b == u,
        _ => false,
    }
}

/// G7: the server calls `(index into the current entries, newIndex to send)` that move the entry at `from` to `to`
/// in a playlist of `n` entries. Measured on Jellyfin 10.10.7: an upward Move lands at `max(newIndex, 1)`, a downward
/// Move lands at `newIndex + 1` (capped at the end), and nothing can be moved to position 0. So: downward moves send
/// `to - 1` (or `n - 1` for the end), upward moves send `to`, an adjacent swap moves the lower entry up, and the two
/// cases that need position 0 move every entry to the end in the wanted order (`newIndex = n - 1` always lands last).
pub fn move_plan(n: usize, from: usize, to: usize) -> Vec<(usize, usize)> {
    if from == to || from >= n || to >= n {
        return vec![];
    }
    if to > from {
        if to == n - 1 {
            return vec![(from, n - 1)];
        }
        if to > from + 1 {
            return vec![(from, to - 1)];
        }
        if from >= 1 {
            return vec![(to, from)];
        }
    } else if to >= 1 {
        return vec![(from, to)];
    }
    let mut order: Vec<usize> = (0..n).collect();
    let moved = order.remove(from);
    order.insert(to, moved);
    order.into_iter().map(|e| (e, n - 1)).collect()
}

/// One playlist row: the item plus the server's `PlaylistItemId` (G7).
#[derive(Debug, Clone, PartialEq)]
pub struct PlaylistEntry {
    pub entry_id: String,
    pub item: Item,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct Created {
    id: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct RawEntries {
    items: Vec<serde_json::Value>,
}

/// G13 / P-AUTH-10: `/Branding/Configuration`.
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct Branding {
    pub login_disclaimer: Option<String>,
}

/// G13 / P-AUTH-6: what the server will do for a forgotten password.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ForgotAction {
    /// A PIN file was written on the server.
    Pin,
    /// Ask an administrator.
    #[default]
    ContactAdmin,
    /// Only works from the server's own network.
    InNetworkRequired,
}

impl<'de> Deserialize<'de> for ForgotAction {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.as_str() {
            "PinCode" => ForgotAction::Pin,
            "InNetworkRequired" => ForgotAction::InNetworkRequired,
            _ => ForgotAction::ContactAdmin,
        })
    }
}

/// G13 / P-AUTH-6: the answer to `POST /Users/ForgotPassword`.
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct ForgotPassword {
    pub action: ForgotAction,
    pub pin_file: Option<String>,
    pub pin_expiration_date: Option<String>,
}

impl ForgotPassword {
    /// The server's answer in plain words for the screen.
    pub fn message(&self) -> String {
        match self.action {
            ForgotAction::Pin => {
                let file = self.pin_file.as_deref().unwrap_or("a file");
                format!("A PIN file was written on the server: {file}. Open it there and type the PIN below.")
            }
            ForgotAction::InNetworkRequired => {
                "Passwords can only be reset from the server's own network. Ask an administrator.".into()
            }
            ForgotAction::ContactAdmin => "Ask an administrator to reset your password.".into(),
        }
    }
}

/// G13 / P-AUTH-6: the answer to `POST /Users/ForgotPassword/Pin`.
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct PinRedeem {
    pub success: bool,
    pub users_reset: Vec<String>,
}

/// G6 / P-PLAY-13: PlaybackInfo plus what the transcoding path needs. `transcoding_url` is absolute (base + the
/// server's relative url, with the token added when the server left it out); `None` = play the source directly.
/// `source_bitrate` is the first source's `Bitrate` in bits per second.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlaybackPlan {
    pub info: PlaybackInfo,
    pub transcoding_url: Option<String>,
    pub source_bitrate: Option<i64>,
}

/// G6: the `POST /Items/{id}/PlaybackInfo` body. No cap and no force = `{}` (the server's default profile).
/// Otherwise a simple profile: direct play mkv/mp4 with h264/hevc + aac/ac3/opus, transcode to HLS ts h264 + aac.
/// `force_transcode` with no cap uses 20 Mbit/s.
pub fn transcode_body(max_kbps: u32, force_transcode: bool) -> serde_json::Value {
    transcode_body_with(max_kbps, force_transcode, false)
}

/// RR8 / P-USER-4: `transcode_body`; with `fmp4` the HLS transcode uses fMP4 segments (container `mp4`) instead of ts.
pub fn transcode_body_with(max_kbps: u32, force_transcode: bool, fmp4: bool) -> serde_json::Value {
    if max_kbps == 0 && !force_transcode {
        return serde_json::json!({});
    }
    let bps = i64::from(if max_kbps == 0 { 20_000 } else { max_kbps }) * 1000;
    serde_json::json!({
        "MaxStreamingBitrate": bps,
        "EnableDirectPlay": !force_transcode,
        "EnableDirectStream": !force_transcode,
        "EnableTranscoding": true,
        "AutoOpenLiveStream": false,
        "DeviceProfile": {
            "Name": "Rustfin",
            "MaxStreamingBitrate": bps,
            "DirectPlayProfiles": [
                { "Type": "Video", "Container": "mkv,mp4", "VideoCodec": "h264,hevc", "AudioCodec": "aac,ac3,opus" }
            ],
            "TranscodingProfiles": [
                {
                    "Type": "Video", "Container": if fmp4 { "mp4" } else { "ts" }, "Protocol": "hls", "Context": "Streaming",
                    "VideoCodec": "h264", "AudioCodec": "aac", "MaxAudioChannels": "2",
                    "BreakOnNonKeyFrames": true
                }
            ],
            "CodecProfiles": [],
            "SubtitleProfiles": [
                { "Format": "srt", "Method": "External" },
                { "Format": "ass", "Method": "External" },
                { "Format": "vtt", "Method": "External" }
            ]
        }
    })
}

impl Client {
    /// RR4 / P-PLAY-21: the item's trickplay resolution to draw seek-bar thumbnails from (`/Items/{id}?Fields=Trickplay`).
    /// None when the item has none (or the server does not know trickplay).
    pub fn trickplay_info(&self, item_id: &str) -> Result<Option<TrickplayInfo>> {
        let uid = self.user_id()?;
        let raw: serde_json::Value = self.get_json(&format!("/Items/{item_id}?userId={uid}&Fields=Trickplay"))?;
        Ok(raw.get("Trickplay").and_then(trickplay::pick))
    }

    /// RR4 / P-PLAY-21: URL of tile sheet `sheet` at `width` px per thumbnail (fetched by the image loader).
    pub fn trickplay_sheet_url(&self, item_id: &str, width: u32, sheet: u32) -> String {
        format!("{}/Videos/{item_id}/Trickplay/{width}/{sheet}.jpg?MediaSourceId={item_id}", self.base)
    }
}

/// C3-SUBS / R-SUBS-1: the `POST /Items/{id}/PlaybackInfo` path, with the chosen audio / subtitle stream indexes.
pub fn playback_info_path(uid: &str, id: &str, audio: Option<i32>, subtitle: Option<i32>) -> String {
    let mut path = format!("/Items/{id}/PlaybackInfo?userId={uid}");
    if let Some(a) = audio {
        path.push_str(&format!("&AudioStreamIndex={a}"));
    }
    if let Some(s) = subtitle {
        path.push_str(&format!("&SubtitleStreamIndex={s}"));
    }
    path
}

/// C3-SUBS / R-SUBS-2: `url` with `AudioStreamIndex` / `SubtitleStreamIndex` set to the given values (each replaced
/// when the server's url already carries it, so never twice); `None` leaves that parameter as it is. `encode` also
/// sets `SubtitleMethod=Encode` (R-SUBS-5: burn the chosen embedded subtitle in).
pub fn with_stream_indexes(url: &str, audio: Option<i32>, subtitle: Option<i32>, encode: bool) -> String {
    let (head, query) = url.split_once('?').unwrap_or((url, ""));
    let mut parts: Vec<String> = query.split('&').filter(|p| !p.is_empty()).map(String::from).collect();
    let method = encode.then_some("Encode");
    for (key, val) in [("AudioStreamIndex", audio), ("SubtitleStreamIndex", subtitle)]
        .map(|(k, v)| (k, v.map(|v| v.to_string())))
        .into_iter()
        .chain([("SubtitleMethod", method.map(String::from))])
    {
        let Some(v) = val else { continue };
        let pre = format!("{key}=");
        let mut seen = false;
        parts.retain_mut(|p| {
            if !p.starts_with(&pre) {
                return true;
            }
            if seen {
                return false;
            }
            seen = true;
            *p = format!("{pre}{v}");
            true
        });
        if !seen {
            parts.push(format!("{pre}{v}"));
        }
    }
    if parts.is_empty() {
        head.to_string()
    } else {
        format!("{head}?{}", parts.join("&"))
    }
}

/// G6: read a PlaybackInfo answer into a `PlaybackPlan`.
pub fn plan_from_json(raw: &serde_json::Value, base: &str, token: &str) -> Result<PlaybackPlan> {
    let info: PlaybackInfo = serde_json::from_value(raw.clone()).map_err(|e| ApiError::Decode(e.to_string()))?;
    let first = raw.get("MediaSources").and_then(|m| m.get(0));
    let source_bitrate = first.and_then(|m| m.get("Bitrate")).and_then(|b| b.as_i64());
    let transcoding_url =
        first.and_then(|m| m.get("TranscodingUrl")).and_then(|u| u.as_str()).filter(|u| !u.is_empty()).map(|u| {
            let mut url = if u.starts_with("http") { u.to_string() } else { format!("{base}{u}") };
            if !url.contains("api_key=") && !url.contains("ApiKey=") {
                url.push(if url.contains('?') { '&' } else { '?' });
                url.push_str(&format!("api_key={token}"));
            }
            url
        });
    Ok(PlaybackPlan { info, transcoding_url, source_bitrate })
}

/// FIX-1 #13: items whose plot, genres, tags or people mention every word of `term` (case-insensitive), at most 30.
pub fn text_matches(items: Vec<Item>, term: &str) -> Vec<Item> {
    let words: Vec<String> = term.split_whitespace().map(|w| w.to_lowercase()).collect();
    if words.is_empty() {
        return vec![];
    }
    items
        .into_iter()
        .filter(|it| {
            let mut hay = it.overview.clone().unwrap_or_default().to_lowercase();
            for s in it.genres.iter().chain(it.tags.iter()).chain(it.people.iter().map(|p| &p.name)) {
                hay.push(' ');
                hay.push_str(&s.to_lowercase());
            }
            words.iter().all(|w| hay.contains(w.as_str()))
        })
        .take(30)
        .collect()
}

/// FIX-1 #13: the `n` most common genres in `items`, most common first.
pub fn top_genres(items: &[Item], n: usize) -> Vec<String> {
    let mut count: HashMap<&str, usize> = HashMap::new();
    for g in items.iter().flat_map(|i| i.genres.iter()) {
        *count.entry(g.as_str()).or_default() += 1;
    }
    let mut v: Vec<(&str, usize)> = count.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    v.into_iter().take(n).map(|(g, _)| g.to_string()).collect()
}
