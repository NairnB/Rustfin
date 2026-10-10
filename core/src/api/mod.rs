//! Jellyfin HTTP client (SPEC §4, rules R-API-*). Blocking calls: run them on the worker pool, never on the UI thread.
//! Signatures are fixed by the lead (tests in `core/tests/` depend on them); bodies are packet C1.
pub mod models;

pub use models::*;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
    "PrimaryImageAspectRatio,ProductionYear,RunTimeTicks,UserData,SeriesName,ParentIndexNumber,IndexNumber,ImageTags";
/// Fields requested for an item page (R-API-18).
const ITEM_FIELDS: &str = "Overview,Genres,People,MediaStreams,MediaSources";

/// Routes that have a 10.9+ form and a legacy form (R-API-4). Fallback choice is remembered per client.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Route {
    Views,
    Resume,
    Latest,
    Item,
    Played,
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
    let mut v = serde_json::to_value(r).unwrap_or(serde_json::Value::Null);
    if let serde_json::Value::Object(map) = &mut v {
        map.insert("PlayMethod".into(), serde_json::Value::String("DirectPlay".into()));
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
    /// R-API-20
    pub fn search(&self, term: &str) -> Result<Vec<Item>> {
        let uid = self.user_id()?;
        let term = url_encode(term);
        let path = format!(
            "/Items?userId={uid}&searchTerm={term}&Recursive=true&IncludeItemTypes=Movie,Series,Episode&Limit=60&Fields={CARD_FIELDS}"
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
    /// GET raw bytes (images), IMAGE_TIMEOUT, 20 MB cap.
    pub fn get_bytes(&self, url: &str) -> Result<Vec<u8>> {
        let mut resp = self
            .agent
            .get(url)
            .header("Authorization", &self.auth_header())
            .config()
            .timeout_global(Some(IMAGE_TIMEOUT))
            .build()
            .call()
            .map_err(net)?;
        check_status(&mut resp)?;
        resp.body_mut().with_config().limit(20 << 20).read_to_vec().map_err(net)
    }
}
