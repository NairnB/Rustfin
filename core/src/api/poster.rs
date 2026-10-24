//! Poster picker (PP): remote image candidates, the local poster store, the DisplayPreferences key a user's
//! personal pick lives under, and the apply/undo plans. The plans and the candidate/chip helpers are pure and
//! unit-tested (core/tests/poster_picker.rs); the `Client` methods do the Jellyfin calls.
use super::{base64_encode, image_content_type, url_encode, ApiError, Client, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The Jellyfin DisplayPreferences client id Rustfin reads and writes (PP-4).
pub const POSTER_CLIENT: &str = "rustfin";
/// PP-6: own images / backups live under this store, one file per item.
pub const POSTER_STORE: &str = "posters";

// ---------- Wire models ----------

/// One remote image candidate (`RemoteImageInfo`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct RemoteImage {
    pub provider_name: Option<String>,
    pub url: Option<String>,
    pub thumbnail_url: Option<String>,
    pub height: Option<i32>,
    pub width: Option<i32>,
    pub community_rating: Option<f64>,
    pub vote_count: Option<i32>,
    pub language: Option<String>,
    #[serde(rename = "Type")]
    pub kind: Option<String>,
    pub rating_type: Option<String>,
}

impl RemoteImage {
    /// The small URL for the candidate grid (thumbnail, else the full image).
    pub fn thumb_url(&self) -> Option<&str> {
        self.thumbnail_url.as_deref().or(self.url.as_deref()).filter(|u| !u.is_empty())
    }
    /// The full-size URL (preview panel + apply).
    pub fn full_url(&self) -> Option<&str> {
        self.url.as_deref().filter(|u| !u.is_empty())
    }
}

/// The `RemoteImageResult` wrapper the `/RemoteImages` route answers with.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct RemoteImageResult {
    pub images: Vec<RemoteImage>,
    pub total_record_count: i32,
    pub providers: Vec<String>,
}

/// PP (test-only): parse a `--poster-candidates` fixture (the `RemoteImageResult` JSON shape) into its images
/// and providers, so the picker can run without a server round-trip (image URLs are `file://` PNGs).
pub fn fixture_candidates(json: &str) -> Result<(Vec<RemoteImage>, Vec<RemoteImageProvider>)> {
    let raw: RemoteImageResult = serde_json::from_str(json).map_err(|e| ApiError::Decode(e.to_string()))?;
    let providers = raw
        .providers
        .into_iter()
        .map(|name| RemoteImageProvider { name, supported_images: vec!["Primary".to_string()] })
        .collect();
    Ok((raw.images, providers))
}

/// One remote image provider (`ImageProviderInfo`), for the provider chips.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct RemoteImageProvider {
    pub name: String,
    pub supported_images: Vec<String>,
}

/// The Jellyfin `DisplayPreferencesDto`. Every field the server returns is kept so that saving a changed
/// `CustomPrefs` never resets the user's other preferences for the item.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct DisplayPreferences {
    pub id: Option<String>,
    pub view_type: Option<String>,
    pub sort_by: Option<String>,
    pub index_by: Option<String>,
    pub remember_indexing: bool,
    pub primary_image_height: i32,
    pub primary_image_width: i32,
    pub custom_prefs: HashMap<String, Option<String>>,
    pub scroll_direction: Option<String>,
    pub show_backdrop: bool,
    pub remember_sorting: bool,
    pub sort_order: Option<String>,
    pub show_sidebar: bool,
    pub client: Option<String>,
}

// ---------- Pure helpers (unit-tested) ----------

/// The `CustomPrefs` key a user's personal poster pick is stored under (PP-4).
pub fn poster_pref_key(item_id: &str) -> String {
    format!("rf.poster.{item_id}")
}

/// The one DisplayPreferences object (per user, client "rustfin") that holds every personal poster pick, so a
/// sign-in can read them all with a single GET.
pub const POSTER_PREFS_ID: &str = "rustfin-posters";

/// Every personal pick in a `CustomPrefs` map: item id → URL.
pub fn all_poster_prefs(custom_prefs: &HashMap<String, Option<String>>) -> HashMap<String, String> {
    custom_prefs
        .iter()
        .filter_map(|(k, v)| {
            let id = k.strip_prefix("rf.poster.")?;
            let v = v.as_ref().filter(|v| !v.is_empty())?;
            Some((id.to_string(), v.clone()))
        })
        .collect()
}

/// A user's personal poster override for `item_id`, if they picked one (PP-4). The value is the URL the pref
/// stores — a remote image URL, or a `file://` path for an own image on this device.
pub fn resolve_poster(custom_prefs: &HashMap<String, Option<String>>, item_id: &str) -> Option<String> {
    custom_prefs.get(&poster_pref_key(item_id)).and_then(|v| v.clone()).filter(|v| !v.is_empty())
}

/// Sort candidates by community rating (high first), then votes, then a stable tiebreak on the provider name.
pub fn sort_candidates(mut cands: Vec<RemoteImage>) -> Vec<RemoteImage> {
    cands.sort_by(|a, b| {
        b.community_rating
            .unwrap_or(0.0)
            .partial_cmp(&a.community_rating.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.vote_count.unwrap_or(0).cmp(&a.vote_count.unwrap_or(0)))
            .then_with(|| a.provider_name.as_deref().unwrap_or("").cmp(b.provider_name.as_deref().unwrap_or("")))
    });
    cands
}

/// One picker chip (PP-2): All languages (default) · each language present · "No text" · provider · min width.
#[derive(Debug, Clone, PartialEq)]
pub enum PosterFilter {
    AllLanguages,
    Language(String),
    NoText,
    Provider(String),
    /// Only candidates at least this many pixels wide.
    MinWidth(i32),
}

impl PosterFilter {
    pub fn label(&self) -> String {
        match self {
            PosterFilter::AllLanguages => "All languages".to_string(),
            PosterFilter::Language(l) => l.clone(),
            PosterFilter::NoText => "No text".to_string(),
            PosterFilter::Provider(p) => p.clone(),
            PosterFilter::MinWidth(w) => format!("≥ {w} px"),
        }
    }

    pub fn matches(&self, c: &RemoteImage) -> bool {
        match self {
            PosterFilter::AllLanguages => true,
            PosterFilter::Language(l) => c.language.as_deref() == Some(l.as_str()),
            PosterFilter::NoText => c.language.is_none(),
            PosterFilter::Provider(p) => c.provider_name.as_deref() == Some(p.as_str()),
            PosterFilter::MinWidth(w) => c.width.is_some_and(|x| x >= *w),
        }
    }
}

/// The minimum width for the "min width" chip (PP-2). High-resolution posters for a 4K home.
pub const MIN_WIDTH_PX: i32 = 1000;

/// The chips a candidate list offers, in order (PP-2): All languages, each language, No text (when a candidate
/// has none), each provider, min width.
pub fn chips_for(cands: &[RemoteImage]) -> Vec<PosterFilter> {
    let mut out = vec![PosterFilter::AllLanguages];
    let mut langs: Vec<String> = Vec::new();
    let mut providers: Vec<String> = Vec::new();
    let mut no_text = false;
    for c in cands {
        if let Some(l) = c.language.as_deref().filter(|l| !l.is_empty()) {
            if !langs.iter().any(|x| x == l) {
                langs.push(l.to_string());
            }
        } else {
            no_text = true;
        }
        if let Some(p) = c.provider_name.as_deref().filter(|p| !p.is_empty()) {
            if !providers.iter().any(|x| x == p) {
                providers.push(p.to_string());
            }
        }
    }
    langs.sort();
    providers.sort();
    out.extend(langs.into_iter().map(PosterFilter::Language));
    if no_text {
        out.push(PosterFilter::NoText);
    }
    out.extend(providers.into_iter().map(PosterFilter::Provider));
    if cands.iter().any(|c| c.width.is_some_and(|w| w >= MIN_WIDTH_PX)) {
        out.push(PosterFilter::MinWidth(MIN_WIDTH_PX));
    }
    out
}

/// The candidates a chip keeps.
pub fn filter_candidates(cands: &[RemoteImage], f: &PosterFilter) -> Vec<RemoteImage> {
    cands.iter().filter(|c| f.matches(c)).cloned().collect()
}

/// One step of an apply/undo plan (PP-4, PP-5).
#[derive(Debug, Clone, PartialEq)]
pub enum PosterStep {
    /// Save the item's current Primary bytes into the local poster store (admin, before a change).
    BackupCurrent,
    /// `POST /Items/{id}/RemoteImages/Download` (admin).
    DownloadRemote { image_url: String },
    /// `POST /Items/{id}/Images/Primary` with these bytes, base64 (admin: own image, or undo).
    UploadPrimary { bytes: Vec<u8> },
    /// `DELETE /Items/{id}/Images/Primary` (admin undo of an apply to an item that had no poster).
    DeleteImage,
    /// Write the personal override into the user's DisplayPreferences (non-admin).
    SetPersonal { url: String },
    /// Remove the personal override (non-admin undo).
    ClearPersonal,
    /// Re-read the item and bust the cache by the new ImageTag (admin).
    Refresh,
}

/// Where a poster comes from (PP-4, PP-6).
#[derive(Debug, Clone, PartialEq)]
pub enum PosterChoice {
    /// A remote image URL (a candidate or a pasted link).
    Remote { url: String },
    /// An image the user supplied (a file or pasted image data).
    OwnImage { bytes: Vec<u8> },
}

/// The requests an **admin** apply needs, in order: back up the current Primary, write the new one, refresh.
pub fn admin_apply_plan(choice: &PosterChoice) -> Vec<PosterStep> {
    let mut v = vec![PosterStep::BackupCurrent];
    match choice {
        PosterChoice::Remote { url } => v.push(PosterStep::DownloadRemote { image_url: url.clone() }),
        PosterChoice::OwnImage { bytes } => v.push(PosterStep::UploadPrimary { bytes: bytes.clone() }),
    }
    v.push(PosterStep::Refresh);
    v
}

/// The plan a **non-admin** apply needs: only write the personal DisplayPreferences pref (`url` is a remote URL
/// or a `file://` path). Viewers never call a server image write endpoint (PP-8).
pub fn personal_apply_plan(url: &str) -> Vec<PosterStep> {
    vec![PosterStep::SetPersonal { url: url.to_string() }]
}

/// What a plan step may hand back: the bytes (and content type) of a backed-up Primary.
pub type StepOut = Option<(Vec<u8>, Option<String>)>;

/// PP-10: the one way the picker applies a choice. Picks the plan (`admin_apply_plan` for admins,
/// `personal_apply_plan` for everyone else — a viewer's choice must be a URL), runs each step through `run` in
/// order, and hands a backup to `on_backup` before the next step (a backup that cannot be saved aborts the
/// apply before anything is written). Returns the previous Primary bytes, if there were any.
pub fn run_apply(
    admin: bool,
    choice: &PosterChoice,
    mut run: impl FnMut(&PosterStep) -> Result<StepOut>,
    mut on_backup: impl FnMut(&[u8], Option<&str>) -> Result<()>,
) -> Result<Option<Vec<u8>>> {
    let plan = match (admin, choice) {
        (true, c) => admin_apply_plan(c),
        (false, PosterChoice::Remote { url }) => personal_apply_plan(url),
        (false, PosterChoice::OwnImage { .. }) => return Err(ApiError::Forbidden),
    };
    let mut previous = None;
    for step in &plan {
        if let Some((bytes, ct)) = run(step)? {
            on_backup(&bytes, ct.as_deref())?;
            previous = Some(bytes);
        }
    }
    Ok(previous)
}

/// Undo (PP-5): admins re-upload the saved previous bytes (or delete the image when the item had none);
/// non-admins just clear the pref.
pub fn undo_plan(admin: bool, previous: Option<&[u8]>) -> Vec<PosterStep> {
    if admin {
        match previous {
            Some(bytes) => vec![PosterStep::UploadPrimary { bytes: bytes.to_vec() }],
            None => vec![PosterStep::DeleteImage],
        }
    } else {
        vec![PosterStep::ClearPersonal]
    }
}

/// The poster store directory (backups + own images): `<data dir>/posters`, beside the image cache.
pub fn poster_store_dir(cache_dir: &Path) -> PathBuf {
    cache_dir.parent().map(|p| p.join(POSTER_STORE)).unwrap_or_else(|| cache_dir.join(POSTER_STORE))
}

/// True when `url` is an absolute `file://` URL whose path (with `..` resolved) lies under one of `roots`.
/// The image loader reads only such files: the poster store, and the fixture dir of `--poster-candidates`.
pub fn file_url_allowed(url: &str, roots: &[PathBuf]) -> bool {
    let Some(path) = url.strip_prefix("file://") else { return false };
    if !path.starts_with('/') && path.chars().nth(1).is_none_or(|c| c != ':') {
        return false;
    }
    let Ok(real) = Path::new(path).canonicalize() else { return false };
    roots.iter().any(|r| r.canonicalize().map(|r| real.starts_with(r)).unwrap_or(false))
}

/// Test-only fixtures: make every relative `file://x` URL in `json` absolute against `dir` (the fixture file's
/// own folder), so a tour works whatever its working directory is.
pub fn absolutize_fixture(json: &str, dir: &Path) -> String {
    let base = dir.to_string_lossy().trim_end_matches('/').to_string();
    let mut out = String::with_capacity(json.len());
    let mut rest = json;
    while let Some(i) = rest.find("\"file://") {
        let (head, tail) = rest.split_at(i + 8);
        out.push_str(head);
        if !tail.starts_with('/') {
            out.push_str(&base);
            out.push('/');
        }
        rest = tail;
    }
    out.push_str(rest);
    out
}

/// A `file://` URL for a local poster file (PP-6).
pub fn file_url(path: &Path) -> String {
    format!("file://{}", path.to_string_lossy())
}

/// True for a picture the server accepts (png / jpeg / gif / webp).
pub fn is_picture(bytes: &[u8]) -> bool {
    image_content_type(bytes).is_some()
}

/// The file extension a picture's bytes should be stored under (PP-6): the server's content type first, else
/// sniffed from the bytes, else "jpg". Keeps backups and own images with their real format.
pub fn picture_extension(content_type: Option<&str>, bytes: &[u8]) -> String {
    fn ext_of(ct: Option<&str>) -> Option<&'static str> {
        ct.and_then(|ct| match ct {
            "image/jpeg" => Some("jpg"),
            "image/png" => Some("png"),
            "image/gif" => Some("gif"),
            "image/webp" => Some("webp"),
            _ => None,
        })
    }
    ext_of(content_type).or_else(|| ext_of(image_content_type(bytes))).unwrap_or("jpg").to_string()
}

/// TheMovieDb poster widths the sized URL variant can request, in order.
const TMDB_WIDTHS: [u32; 6] = [92, 154, 185, 342, 500, 780];

/// The URL to fetch a remote poster at `width` px (PP review #2). TheMovieDb URLs (`…/t/p/<size>/…`) get their
/// sized variant (`w{width}` = the smallest standard width ≥ `width`, else left at `original`); anything else
/// is returned unchanged (the image loader downscales the full image once and caches it).
pub fn sized_poster_url(url: &str, width: u32) -> String {
    let Some(idx) = url.find("/t/p/") else { return url.to_string() };
    let prefix_len = idx + "/t/p/".len();
    let rest = &url[prefix_len..];
    let Some((size, tail)) = rest.split_once('/') else { return url.to_string() };
    if tail.is_empty() || size.is_empty() {
        return url.to_string();
    }
    match TMDB_WIDTHS.iter().copied().find(|&w| w >= width) {
        Some(w) => format!("{}w{w}/{tail}", &url[..prefix_len]),
        None => url.to_string(),
    }
}

/// Standard base64 decode (padding optional). `None` on malformed input.
pub fn decode_base64(s: &str) -> Option<Vec<u8>> {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    fn val(c: u8) -> Option<u8> {
        A.iter().position(|&x| x == c).map(|i| i as u8)
    }
    let mut out = Vec::new();
    let bytes: Vec<u8> = s.bytes().filter(|&c| c != b'=').collect();
    for chunk in bytes.chunks(4) {
        let b0 = val(*chunk.first()?)?;
        let b1 = val(*chunk.get(1)?)?;
        out.push((b0 << 2) | (b1 >> 4));
        if let Some(&c2) = chunk.get(2) {
            let b2 = val(c2)?;
            out.push((b1 << 4) | (b2 >> 2));
            if let Some(&c3) = chunk.get(3) {
                let b3 = val(c3)?;
                out.push((b2 << 6) | b3);
            }
        }
    }
    Some(out)
}

impl Client {
    /// PP-2: remote image candidates for an item's Primary, every language, up to 200.
    pub fn remote_images(&self, item_id: &str) -> Result<Vec<RemoteImage>> {
        let path = format!("/Items/{item_id}/RemoteImages?type=Primary&includeAllLanguages=true&limit=200");
        let raw: RemoteImageResult = self.get_json(&path)?;
        Ok(raw.images)
    }

    /// PP-2: the image providers the server can search, for the provider chips.
    pub fn remote_image_providers(&self, item_id: &str) -> Result<Vec<RemoteImageProvider>> {
        self.get_json(&format!("/Items/{item_id}/RemoteImages/Providers"))
    }

    /// PP-4: download a remote image into the item's Primary (admin only).
    pub fn download_remote_image(&self, item_id: &str, image_url: &str) -> Result<()> {
        self.post_empty_ok(&format!(
            "/Items/{item_id}/RemoteImages/Download?type=Primary&imageUrl={}",
            url_encode(image_url)
        ))
    }

    /// PP-4/PP-5/PP-6: upload an image as the item's Primary (admin only; base64 body, like the profile picture).
    pub fn set_item_image(&self, item_id: &str, bytes: &[u8]) -> Result<()> {
        let Some(kind) = image_content_type(bytes) else {
            return Err(ApiError::Decode("That file is not a picture (png, jpeg, gif or webp).".into()));
        };
        let mut resp = self
            .agent
            .post(&self.url(&format!("/Items/{item_id}/Images/Primary")))
            .header("Authorization", &self.auth_header())
            .header("Content-Type", kind)
            .send(base64_encode(bytes).as_bytes())
            .map_err(super::net)?;
        super::check_status(&mut resp)
    }

    /// PP-5: delete the item's Primary image (admin only; the undo of an apply to an item that had none).
    pub fn delete_item_image(&self, item_id: &str) -> Result<()> {
        self.delete_ok(&format!("/Items/{item_id}/Images/Primary"))
    }

    /// PP-4/PP-5: the item's current Primary bytes, full resolution (no `tag`).
    pub fn item_image_bytes(&self, item_id: &str) -> Result<Vec<u8>> {
        self.item_image_original(item_id).map(|(bytes, _)| bytes)
    }

    /// PP-4/PP-5/PP-6: the item's current Primary bytes plus the server's content type. The original is
    /// requested with `Accept: */*` and no `format=` query so the server never converts it (PP review #11);
    /// the content type keeps the real file extension for a backup.
    pub fn item_image_original(&self, item_id: &str) -> Result<(Vec<u8>, Option<String>)> {
        use super::check_status;
        let mut resp = self
            .agent
            .get(&format!("{}/Items/{item_id}/Images/Primary", self.base))
            .header("Authorization", &self.auth_header())
            .header("Accept", "*/*")
            .config()
            .timeout_global(Some(super::IMAGE_TIMEOUT))
            .build()
            .call()
            .map_err(super::net)?;
        check_status(&mut resp)?;
        let content_type = resp.headers().get("Content-Type").and_then(|v| v.to_str().ok()).map(str::to_string);
        let bytes = resp.body_mut().with_config().limit(20 << 20).read_to_vec().map_err(super::net)?;
        Ok((bytes, content_type))
    }

    /// PP-4: the user's DisplayPreferences for an item; a 404 means none yet (a fresh set with this id).
    pub fn display_preferences(&self, display_prefs_id: &str) -> Result<DisplayPreferences> {
        let uid = self.user_id()?;
        let path = format!("/DisplayPreferences/{display_prefs_id}?userId={uid}&client={POSTER_CLIENT}");
        match self.get_json::<DisplayPreferences>(&path) {
            Ok(p) => Ok(p),
            Err(ApiError::NotFound) => {
                Ok(DisplayPreferences { id: Some(display_prefs_id.to_string()), ..Default::default() })
            }
            Err(e) => Err(e),
        }
    }

    /// PP-4: save the user's DisplayPreferences for an item.
    pub fn save_display_preferences(&self, display_prefs_id: &str, prefs: &DisplayPreferences) -> Result<()> {
        let uid = self.user_id()?;
        let path = format!("/DisplayPreferences/{display_prefs_id}?userId={uid}&client={POSTER_CLIENT}");
        self.post_ok(&path, prefs)
    }

    /// PP-4/PP-5: set (`Some`) or clear (`None`) the user's personal poster pref for an item, keeping every
    /// other DisplayPreferences value.
    pub fn set_poster_pref(&self, display_prefs_id: &str, item_id: &str, url: Option<&str>) -> Result<()> {
        let mut prefs = self.display_preferences(display_prefs_id)?;
        let key = poster_pref_key(item_id);
        match url {
            Some(u) => {
                prefs.custom_prefs.insert(key, Some(u.to_string()));
            }
            None => {
                prefs.custom_prefs.remove(&key);
            }
        }
        self.save_display_preferences(display_prefs_id, &prefs)
    }

    /// PP-10: execute one plan step. `display_id` is the item's DisplayPreferencesId (personal steps only).
    /// Returns `Some((bytes, content_type))` for a successful backup (the item's previous Primary), `None`
    /// otherwise. A `BackupCurrent` that 404s is *not* an error: the item simply had no poster yet (PP review
    /// #5). A `Refresh` error is swallowed here (PP review #9): the change already happened.
    pub fn run_poster_step(
        &self,
        item_id: &str,
        display_id: Option<&str>,
        step: &PosterStep,
    ) -> Result<Option<(Vec<u8>, Option<String>)>> {
        match step {
            PosterStep::BackupCurrent => match self.item_image_original(item_id) {
                Ok((bytes, ct)) => Ok(Some((bytes, ct))),
                Err(ApiError::NotFound) => Ok(None),
                Err(e) => Err(e),
            },
            PosterStep::DownloadRemote { image_url } => {
                self.download_remote_image(item_id, image_url)?;
                Ok(None)
            }
            PosterStep::UploadPrimary { bytes } => {
                self.set_item_image(item_id, bytes)?;
                Ok(None)
            }
            PosterStep::DeleteImage => {
                self.delete_item_image(item_id)?;
                Ok(None)
            }
            PosterStep::SetPersonal { url } => {
                self.set_poster_pref(display_id.unwrap_or(""), item_id, Some(url))?;
                Ok(None)
            }
            PosterStep::ClearPersonal => {
                self.set_poster_pref(display_id.unwrap_or(""), item_id, None)?;
                Ok(None)
            }
            PosterStep::Refresh => {
                let _ = self.item(item_id);
                Ok(None)
            }
        }
    }
}
