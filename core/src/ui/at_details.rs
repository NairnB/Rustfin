//! D2: the Apple TV look for the details pages (film + show) — docs/DESIGN-appletv.md §4–5.
//! Full-bleed hero backdrop fading into the page, title logo (text if none), metadata line, stream badges,
//! white Play pill + round icon buttons, the More/Less overview, link chips; below: season picker, rows of
//! 16:9 episode cards, Up Next, round cast photos, related posters and the Information section.
//! Pure drawing parts over plain data (textures, strings); item.rs and show_page.rs arrange them.
//! Every value comes from `Theme`.
use super::widgets::{self, Icon};
use crate::api::{image_width_bucket, ImageKind, Item, MediaStream, PersonRef, SourceInfo, StreamInfo, StreamKind};
use crate::app::Cx;
use crate::theme::{Theme, Weight};
use egui::{pos2, vec2, Align2, Color32, Id, Mesh, Pos2, Rect, Response, Sense, Stroke, TextureId, Ui, Vec2};

// ---------- 1. Data: metadata line, badges, information columns (pure, unit-tested) ----------

/// Apple's long runtime: 40 s → "1 min", 2400 s → "40 min", 5520 s → "1 hr 32 min", 7200 s → "2 hr".
pub fn runtime_long(secs: f64) -> String {
    let m = (secs / 60.0).round().max(1.0) as i64;
    match (m / 60, m % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} hr"),
        (h, m) => format!("{h} hr {m} min"),
    }
}

/// The hero metadata line: year line · rating · runtime · first two genres (parts that exist only).
/// `years` is the year line the caller built with the Settings switches (`story::year_line`).
pub fn meta_parts(item: &Item, years: &str) -> Vec<String> {
    let mut v = vec![];
    if !years.is_empty() {
        v.push(years.to_string());
    }
    if let Some(r) = item.official_rating.as_deref().filter(|r| !r.trim().is_empty()) {
        v.push(r.trim().to_string());
    }
    if let Some(t) = item.run_time_ticks.filter(|&t| t > 0) {
        v.push(runtime_long(crate::api::ticks_to_secs(t)));
    }
    let genres: Vec<&str> = item.genres.iter().map(|g| g.trim()).filter(|g| !g.is_empty()).take(2).collect();
    if !genres.is_empty() {
        v.push(genres.join(", "));
    }
    v
}

/// A small outlined badge after the metadata line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Badge {
    FourK,
    Hd,
    DolbyVision,
    Hdr,
    DolbyAtmos,
    Dolby,
    Surround7,
    Surround5,
    Cc,
    Sdh,
    Ad,
}

impl Badge {
    pub fn label(self) -> &'static str {
        match self {
            Badge::FourK => "4K",
            Badge::Hd => "HD",
            Badge::DolbyVision => "Dolby Vision",
            Badge::Hdr => "HDR",
            Badge::DolbyAtmos => "Dolby Atmos",
            Badge::Dolby => "Dolby",
            Badge::Surround7 => "7.1",
            Badge::Surround5 => "5.1",
            Badge::Cc => "CC",
            Badge::Sdh => "SDH",
            Badge::Ad => "AD",
        }
    }
}

/// What the badges need from a stream, from either the item's `MediaStream`s or the media-info `StreamInfo`s.
/// `codec` and `title` are lower case; `title` joins the display title and the track title.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StreamFacts {
    pub kind: StreamKind,
    pub codec: String,
    pub title: String,
    pub width: i32,
    pub height: i32,
    pub channels: i32,
}

impl From<&MediaStream> for StreamFacts {
    fn from(s: &MediaStream) -> Self {
        let title = [s.display_title.as_deref(), s.title.as_deref()].iter().flatten().copied().collect::<Vec<_>>();
        StreamFacts {
            kind: s.kind.clone(),
            codec: s.codec.clone().unwrap_or_default().to_lowercase(),
            title: title.join(" ").to_lowercase(),
            ..Default::default()
        }
    }
}

impl From<&StreamInfo> for StreamFacts {
    fn from(s: &StreamInfo) -> Self {
        StreamFacts {
            kind: s.kind.clone(),
            codec: s.codec.clone().unwrap_or_default().to_lowercase(),
            title: s.display_title.clone().unwrap_or_default().to_lowercase(),
            width: s.width.unwrap_or(0),
            height: s.height.unwrap_or(0),
            channels: s.channels.unwrap_or(0),
        }
    }
}

fn has_word(hay: &str, word: &str) -> bool {
    hay.split(|c: char| !c.is_ascii_alphanumeric() && c != '.').any(|w| w == word)
}

/// Badges in Apple's order: picture (4K / HD), range (Dolby Vision / HDR), sound (Dolby Atmos / Dolby,
/// 7.1 / 5.1), then accessibility (SDH or CC, AD). Each at most once.
pub fn badges(streams: &[StreamFacts]) -> Vec<Badge> {
    let video: Vec<&StreamFacts> = streams.iter().filter(|s| s.kind == StreamKind::Video).collect();
    let audio: Vec<&StreamFacts> = streams.iter().filter(|s| s.kind == StreamKind::Audio).collect();
    let subs: Vec<&StreamFacts> = streams.iter().filter(|s| s.kind == StreamKind::Subtitle).collect();
    let mut v = vec![];
    let uhd = video
        .iter()
        .any(|s| s.width >= 3200 || s.height >= 2000 || has_word(&s.title, "4k") || has_word(&s.title, "2160p"));
    let hd = video
        .iter()
        .any(|s| s.width >= 1260 || s.height >= 700 || has_word(&s.title, "1080p") || has_word(&s.title, "720p"));
    if uhd {
        v.push(Badge::FourK);
    } else if hd {
        v.push(Badge::Hd);
    }
    if video.iter().any(|s| s.title.contains("dolby vision") || has_word(&s.title, "dovi") || has_word(&s.title, "dv"))
    {
        v.push(Badge::DolbyVision);
    } else if video.iter().any(|s| s.title.contains("hdr") || has_word(&s.title, "hlg")) {
        v.push(Badge::Hdr);
    }
    if audio.iter().any(|s| s.title.contains("atmos")) {
        v.push(Badge::DolbyAtmos);
    } else if audio.iter().any(|s| matches!(s.codec.as_str(), "ac3" | "eac3" | "truehd") || s.title.contains("dolby")) {
        v.push(Badge::Dolby);
    }
    if audio.iter().any(|s| s.channels >= 8 || has_word(&s.title, "7.1")) {
        v.push(Badge::Surround7);
    } else if audio.iter().any(|s| s.channels >= 6 || has_word(&s.title, "5.1")) {
        v.push(Badge::Surround5);
    }
    if subs.iter().any(|s| has_word(&s.title, "sdh") || s.title.contains("hearing")) {
        v.push(Badge::Sdh);
    } else if !subs.is_empty() {
        v.push(Badge::Cc);
    }
    if audio.iter().any(|s| s.title.contains("descri")) {
        v.push(Badge::Ad);
    }
    v
}

/// One column of the Information section: a heading and its lines.
#[derive(Debug, Clone, PartialEq)]
pub struct InfoColumn {
    pub heading: String,
    pub lines: Vec<String>,
}

fn stream_name(s: &StreamInfo, fallback: &str) -> String {
    s.display_title
        .clone()
        .filter(|t| !t.is_empty())
        .or_else(|| s.language.clone().filter(|l| !l.is_empty()))
        .unwrap_or_else(|| format!("{fallback} {}", s.index))
}

/// "2018-05-04T00:00:00Z" → "4 May 2018" (British order).
pub fn release_date(iso: &str) -> Option<String> {
    let (y, rest) = iso.get(..10)?.split_once('-')?;
    let (m, d) = rest.split_once('-')?;
    let months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let m = months.get(m.parse::<usize>().ok()?.checked_sub(1)?)?;
    Some(format!("{} {m} {y}", d.parse::<u32>().ok()?))
}

/// The "Information" section in Apple's spirit, one labelled column per fact: Studio, Released, Run Time, Rated,
/// Director, Writers, then Audio, Subtitles and Format from the media info. Empty columns are left out.
/// `admin` adds the file path (as the old Media info did).
pub fn info_columns(item: &Item, src: Option<&SourceInfo>, admin: bool) -> Vec<InfoColumn> {
    let mut cols = vec![];
    let mut push = |heading: &str, lines: Vec<String>| {
        if !lines.is_empty() {
            cols.push(InfoColumn { heading: heading.to_string(), lines });
        }
    };
    if let Some(st) = item.studios.first().map(|s| s.name.trim()).filter(|s| !s.is_empty()) {
        push("Studio", vec![st.to_string()]);
    }
    if let Some(d) = item.premiere_date.as_deref().and_then(release_date) {
        push("Released", vec![d]);
    } else if let Some(y) = item.production_year {
        push("Released", vec![y.to_string()]);
    }
    if let Some(t) = item.run_time_ticks.filter(|&t| t > 0) {
        push("Run Time", vec![runtime_long(crate::api::ticks_to_secs(t))]);
    }
    if let Some(r) = item.official_rating.as_deref().filter(|r| !r.is_empty()) {
        push("Rated", vec![r.to_string()]);
    }
    let names = |kind: &str| -> Vec<String> {
        item.people.iter().filter(|p| p.kind == kind).map(|p| p.name.clone()).take(4).collect()
    };
    push("Director", names("Director"));
    push("Writers", names("Writer"));
    if let Some(src) = src {
        let of = |k: StreamKind| src.media_streams.iter().filter(move |s| s.kind == k);
        push("Audio", of(StreamKind::Audio).map(|s| stream_name(s, "Track")).collect());
        push("Subtitles", of(StreamKind::Subtitle).map(|s| stream_name(s, "Subtitle")).collect());
        let mut format = vec![];
        for s in of(StreamKind::Video) {
            let mut p = vec![];
            if let (Some(w), Some(h)) = (s.width, s.height) {
                p.push(format!("{w}×{h}"));
            }
            if let Some(c) = s.codec.as_deref().filter(|c| !c.is_empty()) {
                p.push(c.to_uppercase());
            }
            if !p.is_empty() {
                format.push(p.join(" · "));
            }
        }
        if let Some(c) = src.container.as_deref().filter(|c| !c.is_empty()) {
            format.push(c.to_uppercase());
        }
        if let Some(b) = src.bitrate {
            format.push(super::item::format_bitrate(b));
        }
        if let Some(sz) = src.size {
            format.push(super::item::format_size(sz));
        }
        if admin {
            if let Some(p) = src.path.as_deref().filter(|p| !p.is_empty()) {
                format.push(p.to_string());
            }
        }
        push("Format", format);
    }
    cols
}

// ---------- 2. Textures for the parts (through the image pipeline) ----------

/// The loaded texture of `item`'s `kind` image at width `w` (None while loading or when it has none).
pub fn tex(cx: &mut Cx, ui: &Ui, item: &Item, kind: ImageKind, w: f32) -> Option<TextureId> {
    // PP-4: a personal poster pick overrides the item's Primary.
    if kind == ImageKind::Primary {
        if let Some(url) = cx.poster_url(item) {
            return cx.images.get(&url);
        }
    }
    let url = cx.client.as_ref()?.image_url(item, kind, image_width_bucket(w, ui.ctx().pixels_per_point()))?;
    cx.images.get(&url)
}

/// The title logo texture (None while loading or when the item has no logo). While the logo's final texture is not
/// ready (its pass has not run, or a padded logo's wider fetch is on the way) `hero_title` is told to hold: the box is
/// reserved but nothing is drawn, so the logo never appears small and then grows.
pub fn logo(cx: &mut Cx, ui: &Ui, item: &Item) -> Option<TextureId> {
    let ppp = ui.ctx().pixels_per_point();
    let hint_key = Id::new(("at-logo-w", &item.id));
    let url = cx.client.as_ref()?.logo_url(item, image_width_bucket(cx.theme.hero_logo_w, ppp))?;
    let ntex = cx.images.get(&url)?;
    let ninfo = logo_pass(cx, ui, &url, ntex, hint_key);
    // a heavily padded logo is fetched wider (once its bounds are known) so its visible part stays sharp
    let hint: f32 = ui.data_mut(|d| d.get_temp(hint_key)).unwrap_or(1.0);
    let wide = if ninfo.loaded && hint > 1.0 {
        let wurl = cx.client.as_ref()?.logo_url(item, image_width_bucket(cx.theme.hero_logo_w * hint, ppp))?;
        match cx.images.get(&wurl) {
            Some(wtex) => Some((wtex, logo_pass(cx, ui, &wurl, wtex, hint_key))),
            // the wide image failed to load or decode: keep the cropped narrow logo now, not after the wait
            None if !cx.images.usable(&wurl) => Some((ntex, LogoInfo { loaded: true, ..LogoInfo::default() })),
            None => None,
        }
    } else {
        None
    };
    let wait_key = Id::new(("at-logo-wait", &item.id));
    let now = ui.input(|i| i.time);
    // when this wait began (a gap of over a second between frames means a fresh visit)
    let (since, last): (f64, f64) = ui.data_mut(|d| d.get_temp(wait_key)).unwrap_or((now, now));
    let since = if now - last > 1.0 { now } else { since };
    ui.data_mut(|d| d.insert_temp(wait_key, (since, now)));
    let (tex, draw) = logo_choice(Some((ntex, ninfo)), hint, wide, (now - since) as f32)?;
    ui.data_mut(|d| d.insert_temp(Id::new(("at-logo-hold", tex)), !draw));
    if !draw {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
    }
    Some(tex)
}

/// How long a padded logo's wider texture is waited for before the cropped narrow one is drawn instead (the wide fetch
/// may have failed).
pub const LOGO_WIDE_WAIT: f32 = 6.0;

/// Which texture the title uses and whether to draw it yet. `narrow` is the normal-width logo with its pass result,
/// `wide` the re-fetched wider one (only for `hint` > 1). Nothing is drawn until the final texture's bounds are known;
/// if the wide one cannot be used (its pass failed, or it took longer than `LOGO_WIDE_WAIT`) the cropped narrow one is.
pub fn logo_choice<T: Copy>(
    narrow: Option<(T, LogoInfo)>,
    hint: f32,
    wide: Option<(T, LogoInfo)>,
    waited_s: f32,
) -> Option<(T, bool)> {
    let (nt, ni) = narrow?;
    if !ni.loaded {
        return Some((nt, false));
    }
    if hint <= 1.0 {
        return Some((nt, true));
    }
    match wide {
        Some((wt, wi)) if wi.loaded && wi.ok => Some((wt, true)),
        Some((_, wi)) if wi.loaded => Some((nt, true)),
        _ => Some((nt, waited_s > LOGO_WIDE_WAIT)),
    }
}

/// FIX-2 #14 / FIX-3: once per logo texture, find out (off the UI thread) whether it is too dark for the dark hero,
/// where its visible pixels are, and (dark ones) build a glyph-shaped halo texture. Returns what is known so far.
fn logo_pass(cx: &mut Cx, ui: &Ui, url: &str, tex: TextureId, hint_key: Id) -> LogoInfo {
    let key = Id::new(("at-logo-info", tex));
    if let Some(info) = ui.data_mut(|d| d.get_temp::<LogoInfo>(key)) {
        return info;
    }
    ui.data_mut(|d| d.insert_temp(key, LogoInfo::default()));
    if let Some(c) = cx.client.clone() {
        let (ctx, url) = (ui.ctx().clone(), url.to_string());
        std::thread::spawn(move || {
            let bytes = c.get_bytes(&url);
            let mut info = bytes.as_ref().map(|b| logo_info(b)).unwrap_or_default();
            info.ok &= bytes.is_ok();
            if let Some(img) = bytes.as_ref().ok().filter(|_| info.dark).and_then(|b| logo_halo_image(b, info.bounds)) {
                let h = ctx.load_texture("logo-halo", img, egui::TextureOptions::LINEAR);
                ctx.data_mut(|d| d.insert_temp(Id::new(("at-logo-halo", tex)), h));
            }
            info.loaded = true;
            ctx.data_mut(|d| {
                if info.bounds.width() < 0.5 && d.get_temp::<f32>(hint_key).is_none() {
                    d.insert_temp(hint_key, (1.0 / info.bounds.width()).min(LOGO_MAX_FETCH_SCALE));
                }
                d.insert_temp(key, info)
            });
            ctx.request_repaint();
        });
    }
    LogoInfo::default()
}

/// Most a padded logo's fetch width is widened by (visible part → title width).
const LOGO_MAX_FETCH_SCALE: f32 = 4.0;

/// What one pass over a logo's pixels finds: whether it is too dark for the hero, and its visible bounds as a UV rect.
/// `ok` = the bytes were fetched and decoded (`logo_info` sets it; false for the default).
/// `loaded` is false until the pass has run (the logo is then not drawn, so the title never jumps from uncropped).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LogoInfo {
    pub dark: bool,
    pub bounds: Rect,
    pub loaded: bool,
    /// The bytes were fetched (false when the fetch failed, so a wider texture's whole-image default is not trusted).
    pub ok: bool,
}

impl Default for LogoInfo {
    fn default() -> Self {
        LogoInfo { dark: false, bounds: Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), loaded: false, ok: false }
    }
}

/// FIX-2 #14: a logo is too dark for the hero when its visible pixels average below a quarter of full brightness
/// (black-on-transparent logos vanish against the dark backdrop). Undecodable or empty → not dark.
pub fn logo_is_dark(bytes: &[u8]) -> bool {
    logo_info(bytes).dark
}

/// FIX-3 L1: the dark verdict and the bounds of the pixels with alpha above ~10 %, from one pass. Undecodable or
/// fully transparent → not dark, whole image.
pub fn logo_info(bytes: &[u8]) -> LogoInfo {
    let Ok(img) = image::load_from_memory(bytes) else { return LogoInfo::default() };
    let img = img.to_rgba8();
    let (w, h) = (img.width(), img.height());
    let (mut sum, mut weight) = (0.0_f64, 0.0_f64);
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for (x, y, p) in img.enumerate_pixels() {
        let a = p[3] as f64 / 255.0;
        if a < 0.1 {
            continue;
        }
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x + 1);
        y1 = y1.max(y + 1);
        let lum = (0.2126 * p[0] as f64 + 0.7152 * p[1] as f64 + 0.0722 * p[2] as f64) / 255.0;
        sum += lum * a;
        weight += a;
    }
    if weight <= 0.0 {
        return LogoInfo::default();
    }
    let (fw, fh) = (w as f32, h as f32);
    LogoInfo {
        dark: sum / weight < 0.25,
        bounds: Rect::from_min_max(pos2(x0 as f32 / fw, y0 as f32 / fh), pos2(x1 as f32 / fw, y1 as f32 / fh)),
        loaded: false,
        ok: true,
    }
}

/// Largest up-scale of a logo's own pixels (physical, not points): a heavily padded logo is not blown up past this.
pub const LOGO_MAX_UPSCALE: f32 = 2.0;

/// FIX-3 L1/L3: the on-screen size of the logo's visible part (`uv` of a `tex`-pixel texture) fitted into `boxsz`,
/// aspect kept, up-scaled at most `LOGO_MAX_UPSCALE` physical pixels per texture pixel (`ppp` points → pixels).
/// Padding never shrinks the glyphs.
pub fn logo_fit(tex: Vec2, uv: Rect, boxsz: Vec2, ppp: f32) -> Vec2 {
    let vis = vec2(tex.x * uv.width(), tex.y * uv.height());
    if vis.x <= 0.0 || vis.y <= 0.0 {
        return boxsz;
    }
    vis * (boxsz.x / vis.x).min(boxsz.y / vis.y).min(LOGO_MAX_UPSCALE / ppp.max(0.1))
}

/// FIX-3 L2: the halo texture reaches this many texture pixels past the glyph box (≤ 12 px on screen at the 2× cap).
pub const LOGO_HALO_PAD: u32 = 6;
/// FIX-3 L2: peak alpha of the halo, 63/255 = 24.7 % (≤ 25 %).
pub const LOGO_HALO_ALPHA: u8 = 63;

/// FIX-3 L2: a white, blurred copy of a dark logo's own alpha (cropped to `bounds`, padded by `LOGO_HALO_PAD`), peak
/// alpha `LOGO_HALO_ALPHA`. Drawn under the logo it follows the glyph shapes — no plate.
pub fn logo_halo_image(bytes: &[u8], bounds: Rect) -> Option<egui::ColorImage> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (w, h) = (img.width() as f32, img.height() as f32);
    let x0 = (bounds.min.x * w).round() as u32;
    let y0 = (bounds.min.y * h).round() as u32;
    let cw = ((bounds.max.x * w).round() as u32).saturating_sub(x0);
    let ch = ((bounds.max.y * h).round() as u32).saturating_sub(y0);
    if cw == 0 || ch == 0 {
        return None;
    }
    let pad = LOGO_HALO_PAD;
    let (ow, oh) = ((cw + 2 * pad) as usize, (ch + 2 * pad) as usize);
    let mut a = vec![0.0_f32; ow * oh];
    for y in 0..ch {
        for x in 0..cw {
            a[(y + pad) as usize * ow + (x + pad) as usize] = img.get_pixel(x0 + x, y0 + y)[3] as f32 / 255.0;
        }
    }
    // two box-blur passes (radius pad/2 each) ≈ a soft falloff that dies before the edge of the padded image
    let r = (pad / 2) as i32;
    for _ in 0..2 {
        for horizontal in [true, false] {
            let src = a.clone();
            for y in 0..oh as i32 {
                for x in 0..ow as i32 {
                    let mut sum = 0.0;
                    for d in -r..=r {
                        let (sx, sy) = if horizontal { (x + d, y) } else { (x, y + d) };
                        if sx >= 0 && sy >= 0 && (sx as usize) < ow && (sy as usize) < oh {
                            sum += src[sy as usize * ow + sx as usize];
                        }
                    }
                    a[y as usize * ow + x as usize] = sum / (2 * r + 1) as f32;
                }
            }
        }
    }
    // spread the blur's reach so the glyph edge itself is at about half the peak, then cap at the peak alpha
    let pixels = a
        .iter()
        .map(|v| Color32::from_white_alpha(((v * 2.0).min(1.0) * LOGO_HALO_ALPHA as f32).round() as u8))
        .collect();
    Some(egui::ColorImage { size: [ow, oh], source_size: vec2(ow as f32, oh as f32), pixels })
}

/// Where `hero_title` puts a logo, relative to the bottom-left of its `boxsz` box: the glyph rect, the UV crop and (dark
/// logos) the halo rect. One place, so the tests check what is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LogoLayout {
    pub rect: Rect,
    pub uv: Rect,
    pub halo: Option<Rect>,
}

pub fn logo_layout(tex: Vec2, info: &LogoInfo, boxsz: Vec2, ppp: f32) -> LogoLayout {
    let size = logo_fit(tex, info.bounds, boxsz, ppp);
    let rect = Rect::from_min_size(pos2(0.0, boxsz.y - size.y), size);
    let vis_w = tex.x * info.bounds.width();
    let halo = (info.dark && vis_w > 0.0).then(|| rect.expand(LOGO_HALO_PAD as f32 * size.x / vis_w));
    LogoLayout { rect, uv: info.bounds, halo }
}

/// A cast member's photo texture.
pub fn person_tex(cx: &mut Cx, ui: &Ui, p: &PersonRef) -> Option<TextureId> {
    let tag = p.primary_image_tag.clone()?;
    let it = Item { id: p.id.clone(), image_tags: [("Primary".to_string(), tag)].into(), ..Default::default() };
    tex(cx, ui, &it, ImageKind::Primary, cx.theme.person_size)
}

/// The role under a cast photo: the character for actors, else the job ("Director").
pub fn role_of(p: &PersonRef) -> String {
    p.role.clone().filter(|r| !r.trim().is_empty()).unwrap_or_else(|| p.kind.clone())
}

// ---------- 2b. Painting helpers ----------

/// Pixel size of a loaded texture (for cover-cropping and fitting logos).
pub fn tex_size(ui: &Ui, tex: TextureId) -> Option<Vec2> {
    let m = ui.ctx().tex_manager();
    let m = m.read();
    m.meta(tex).map(|m| vec2(m.size[0] as f32, m.size[1] as f32))
}

/// UV rect that fills `target` with an image of `img` size without stretching (centre crop; `y_bias` 0 = keep
/// the top, 0.5 = centre).
pub fn cover_uv(img: Vec2, target: Vec2, y_bias: f32) -> Rect {
    if img.x <= 0.0 || img.y <= 0.0 || target.x <= 0.0 || target.y <= 0.0 {
        return Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    }
    let (ia, ta) = (img.x / img.y, target.x / target.y);
    if ia > ta {
        let w = ta / ia;
        let x0 = (1.0 - w) / 2.0;
        Rect::from_min_max(pos2(x0, 0.0), pos2(x0 + w, 1.0))
    } else {
        let h = ia / ta;
        let y0 = (1.0 - h) * y_bias.clamp(0.0, 1.0);
        Rect::from_min_max(pos2(0.0, y0), pos2(1.0, y0 + h))
    }
}

/// Image filling `rect` (cover crop) with rounded corners.
fn paint_cover(ui: &Ui, painter: &egui::Painter, tex: TextureId, rect: Rect, radius: f32, y_bias: f32) {
    let uv = cover_uv(tex_size(ui, tex).unwrap_or(rect.size()), rect.size(), y_bias);
    let mut shape = egui::epaint::RectShape::filled(rect, radius, Color32::WHITE);
    shape.brush = Some(std::sync::Arc::new(egui::epaint::Brush { fill_texture_id: tex, uv }));
    painter.add(shape);
}

/// A gradient over `rect` through `stops` (position 0..1 along the axis, colour), vertical or horizontal.
fn gradient(painter: &egui::Painter, rect: Rect, stops: &[(f32, Color32)], vertical: bool) {
    let mut mesh = Mesh::default();
    for (i, &(p, c)) in stops.iter().enumerate() {
        let (a, b) = if vertical {
            let y = rect.top() + rect.height() * p;
            (pos2(rect.left(), y), pos2(rect.right(), y))
        } else {
            let x = rect.left() + rect.width() * p;
            (pos2(x, rect.top()), pos2(x, rect.bottom()))
        };
        mesh.colored_vertex(a, c);
        mesh.colored_vertex(b, c);
        if i > 0 {
            let k = (i as u32) * 2;
            mesh.add_triangle(k - 2, k - 1, k);
            mesh.add_triangle(k - 1, k + 1, k);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// `bg` at an eased alpha for a smooth fade (premultiplied: `gamma_multiply` keeps it correct).
fn fade_stops(c: Color32, from: f32, to: f32) -> Vec<(f32, Color32)> {
    (0..=8)
        .map(|i| {
            let p = i as f32 / 8.0;
            let a = from + (to - from) * (p * p * (3.0 - 2.0 * p));
            (p, c.gamma_multiply(a))
        })
        .collect()
}

/// The light sheen across the top of a focused piece of art.
fn sheen(painter: &egui::Painter, t: &Theme, rect: Rect, a: f32) {
    if a <= 0.01 {
        return;
    }
    let top = Rect::from_min_size(rect.min, vec2(rect.width(), rect.height() / 3.0));
    gradient(painter, top, &[(0.0, t.sheen.gamma_multiply(a)), (1.0, Color32::TRANSPARENT)], true);
}

/// Artwork that lifts when focused: the shared focus look for every card on the details page.
/// Draws a skeleton (with `fallback` initials) while there is no texture, the progress bar inset along the
/// bottom, and a dim veil for unavailable things. Returns the drawn (lifted) rect.
#[allow(clippy::too_many_arguments)]
fn art(
    ui: &Ui,
    t: &Theme,
    rect: Rect,
    tex: Option<TextureId>,
    fallback: &str,
    a: f32,
    progress: Option<f32>,
    dim: bool,
) -> Rect {
    let r = widgets::lift(rect, t.focus_scale, a);
    let p = ui.painter();
    let tv = super::parts::is_tv(ui.ctx());
    let st = poster_focus_style(tv, a > 0.01);
    if a > 0.01 {
        p.add(t.shadow(a).as_shape(r, t.card_radius));
        // FIX-3 #3: on TV the same glow + white ring as `parts::card`; desktop keeps shadow + sheen only.
        if st.glow {
            super::parts::card_glow(ui, t, r, a);
        }
        if st.ring {
            super::parts::card_ring(ui, t, r, a);
        }
    }
    match tex {
        Some(tex) => paint_cover(ui, p, tex, r, t.card_radius, 0.5),
        None => {
            p.rect_filled(r, t.card_radius, t.skeleton);
            if !fallback.is_empty() {
                p.text(r.center(), Align2::CENTER_CENTER, fallback, t.font(t.heading_size), t.muted);
            }
        }
    }
    if dim {
        p.rect_filled(r, t.card_radius, t.dim_scrim);
    }
    if let Some(f) = progress {
        let inset = t.gap / 2.0;
        let bar = Rect::from_min_size(
            pos2(r.left() + inset, r.bottom() - inset - t.progress_h),
            vec2(r.width() - 2.0 * inset, t.progress_h),
        );
        p.rect_filled(bar, t.progress_h / 2.0, t.progress_track);
        let mut done = bar;
        done.set_right(bar.left() + bar.width() * f.clamp(0.0, 1.0));
        p.rect_filled(done, t.progress_h / 2.0, t.progress);
    }
    if st.sheen {
        sheen(p, t, r, a);
    }
    r
}

fn initials(name: &str) -> String {
    name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect()
}

/// Laid-out text cut to `max_rows` rows (… at the cut), wrapped at `w`.
fn text_job(text: &str, font: egui::FontId, color: Color32, w: f32, max_rows: usize) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_string(),
        egui::TextFormat { font_id: font, color, ..Default::default() },
    );
    job.wrap =
        egui::text::TextWrapping { max_width: w, max_rows, break_anywhere: false, overflow_character: Some('…') };
    job
}

/// Paints text at `at` (top-left), returns its size.
fn text_at(ui: &Ui, at: Pos2, job: egui::text::LayoutJob) -> Vec2 {
    let g = ui.fonts(|f| f.layout_job(job));
    let size = g.size();
    ui.painter().galley(at, g, Color32::PLACEHOLDER);
    size
}

fn line_h(size: f32) -> f32 {
    size * 1.3
}

/// True when the focused cell of the control `id` differs from last frame's (so a scroll is wanted now, and the
/// mouse wheel is not fought on the frames after).
pub fn focus_moved(ui: &Ui, id: Id, focus: Option<usize>) -> bool {
    let key = id.with("last-focus");
    let last = ui.data_mut(|d| d.get_temp::<Option<usize>>(key));
    ui.data_mut(|d| d.insert_temp(key, focus));
    focus.is_some() && last != Some(focus)
}

/// The content area including its side margins (what "full bleed" means beside the sidebar), as D1's hero does.
fn bleed_rect(ui: &Ui, t: &Theme) -> Rect {
    let m = ui.max_rect();
    Rect::from_x_y_ranges((m.left() - t.margin)..=(m.right() + t.margin), m.y_range())
}

// ---------- 3. Hero ----------

/// The hero's art: backdrop and (optional) logo textures, both already loaded by the caller.
#[derive(Clone, Copy, Default)]
pub struct HeroArt {
    pub backdrop: Option<TextureId>,
    pub logo: Option<TextureId>,
}

/// Full-bleed backdrop `height` high at the top of the page (past the panel's side margins), darkened on the
/// left behind the text and faded into `bg` at the bottom. No backdrop (or backdrops off): a quiet surface
/// gradient. Returns the content rect (inside the margins) to lay the hero text into.
pub fn hero(ui: &mut Ui, t: &Theme, art: HeroArt, height: f32) -> Rect {
    let (alloc, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let screen = bleed_rect(ui, t);
    let full = Rect::from_min_max(pos2(screen.left(), alloc.top()), pos2(screen.right(), alloc.bottom()));
    let clip =
        Rect::from_min_max(pos2(screen.left(), ui.clip_rect().top()), pos2(screen.right(), ui.clip_rect().bottom()));
    let p = ui.painter().with_clip_rect(clip);
    match art.backdrop {
        Some(tex) => paint_cover(ui, &p, tex, full, 0.0, 0.3),
        None => gradient(&p, full, &[(0.0, t.surface_hi), (1.0, t.bg)], true),
    }
    // left: darken behind the text column
    let left = Rect::from_min_max(full.min, pos2(full.left() + full.width() * 0.7, full.bottom()));
    gradient(&p, left, &fade_stops(t.bg, 0.85, 0.0), false);
    // top: a light veil so the window edge never glares
    let top = Rect::from_min_size(full.min, vec2(full.width(), t.hero_fade_h / 2.0));
    gradient(&p, top, &fade_stops(t.bg, 0.45, 0.0), true);
    // bottom: fade into the page
    let fade_h = t.hero_fade_h.min(full.height());
    let bottom = Rect::from_min_max(pos2(full.left(), full.bottom() - fade_h), full.max);
    gradient(&p, bottom, &fade_stops(t.bg, 0.0, 1.0), true);
    alloc
}

/// The title: the logo fitted into `hero_logo_w × hero_logo_h` (kept aspect, anchored bottom-left), or the
/// name in `hero_title_size` bold (two lines at most). Allocates its height in `ui`.
pub fn hero_title(ui: &mut Ui, t: &Theme, name: &str, logo: Option<TextureId>) {
    if let Some(tex) = logo {
        let boxsz = vec2(t.hero_logo_w, t.hero_logo_h);
        let s = tex_size(ui, tex).unwrap_or(boxsz);
        let info = ui.data_mut(|d| d.get_temp::<LogoInfo>(Id::new(("at-logo-info", tex)))).unwrap_or_default();
        // the final box is reserved from the start, so the page below never moves when the crop arrives
        let (rect, _) = ui.allocate_exact_size(boxsz, Sense::hover());
        let hold: bool = ui.data_mut(|d| d.get_temp(Id::new(("at-logo-hold", tex)))).unwrap_or(false);
        if !info.loaded || hold {
            // not ready to draw yet: the text title stands in the reserved box (never a blank one)
            let job = text_job(name, t.font_w(t.hero_title_size, Weight::Bold), t.text, t.hero_text_w, 2);
            let g = ui.fonts(|f| f.layout_job(job));
            ui.painter().galley(pos2(rect.left(), (rect.bottom() - g.size().y).max(rect.top())), g, t.text);
            return;
        }
        let l = logo_layout(s, &info, boxsz, ui.ctx().pixels_per_point());
        let r = l.rect.translate(rect.left_top().to_vec2());
        // FIX-2 #14 / FIX-3 L2: a dark logo gets a soft light halo shaped like its glyphs so it reads on the dark hero
        if let Some(h) = l.halo {
            let halo: Option<egui::TextureHandle> = ui.data_mut(|d| d.get_temp(Id::new(("at-logo-halo", tex))));
            if let Some(halo) = halo {
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                ui.painter().image(halo.id(), h.translate(rect.left_top().to_vec2()), uv, Color32::WHITE);
            }
        }
        ui.painter().image(tex, r, l.uv, Color32::WHITE);
        return;
    }
    let job = text_job(name, t.font_w(t.hero_title_size, Weight::Bold), t.text, t.hero_text_w, 2);
    let g = ui.fonts(|f| f.layout_job(job));
    let (rect, _) = ui.allocate_exact_size(g.size(), Sense::hover());
    ui.painter().galley(rect.min, g, t.text);
}

/// D7: a content-sized hero height for `hero_column(id)`: its measured column plus room above, between `hero_h` and
/// `backdrop_h` (no tall empty band over short pages).
pub fn hero_fit_h(ui: &Ui, t: &Theme, id: &str) -> f32 {
    let last_h: f32 = ui.data_mut(|d| d.get_temp(Id::new(("at-hero-col", id)))).unwrap_or(t.hero_h);
    (last_h + t.gap + 2.0 * t.row_gap).clamp(t.hero_h, t.backdrop_h)
}

/// The hero's text column, laid out top-down but sitting on the hero's bottom edge (its height is measured the
/// frame before). When it is taller than the hero (an expanded overview) the page grows below the hero instead of
/// the text running off the top.
pub fn hero_column<R>(ui: &mut Ui, t: &Theme, rect: Rect, id: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let key = Id::new(("at-hero-col", id));
    let last_h: f32 = ui.data_mut(|d| d.get_temp(key)).unwrap_or(0.0);
    let top = (rect.bottom() - t.gap - last_h).max(rect.top() + t.row_gap);
    let max = Rect::from_min_size(pos2(rect.left(), top), vec2(rect.width(), ui.ctx().screen_rect().height() * 2.0));
    let mut col = ui.new_child(egui::UiBuilder::new().max_rect(max));
    col.spacing_mut().item_spacing.y = 0.0;
    let r = add(&mut col);
    let h = col.min_rect().height();
    if (h - last_h).abs() > 0.5 {
        ui.data_mut(|d| d.insert_temp(key, h));
        ui.ctx().request_repaint();
    }
    let over = col.min_rect().bottom() + t.gap - rect.bottom();
    if over > 0.0 {
        ui.add_space(over);
    }
    r
}

/// The small line above the title ("Night Shift Stories", "Season 1, Episode 2"): semibold, muted.
pub fn kicker(ui: &mut Ui, t: &Theme, text: &str) {
    let job = text_job(text, t.font_w(t.body_size, Weight::SemiBold), t.muted, t.hero_text_w, 1);
    let g = ui.fonts(|f| f.layout_job(job));
    let (rect, _) = ui.allocate_exact_size(g.size(), Sense::hover());
    ui.painter().galley(rect.min, g, t.muted);
}

/// The metadata line ("2018 · PG · 1 hr 32 min · Drama, Thriller") followed by the badges, one row.
pub fn meta_row(ui: &mut Ui, t: &Theme, parts: &[String], ratings: (Option<f32>, Option<f32>), badges: &[Badge]) {
    let (community, critic) = ratings;
    if parts.is_empty() && badges.is_empty() && community.is_none() && critic.is_none() {
        return;
    }
    let font = t.font_w(t.body_size, Weight::Medium);
    let mut text = parts.join("  ·  ");
    if (community.is_some() || critic.is_some()) && !text.is_empty() {
        text.push_str("  ·  ");
    }
    let g = ui.fonts(|f| f.layout_no_wrap(text, font.clone(), t.muted));
    let h = g.size().y.max(t.badge_h);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    let cy = rect.center().y;
    let mut x = rect.left() + g.size().x;
    ui.painter().galley(pos2(rect.left(), cy - g.size().y / 2.0), g, t.muted);
    // P-ITEM-1: community rating with a painted star, critic rating as a whole percent
    let rating = |label: String, star: bool, x: &mut f32| {
        if star {
            super::parts::star(ui, t, pos2(*x + t.body_size / 2.0, cy), t.body_size * 0.9);
            *x += t.body_size + t.gap / 8.0;
        }
        let g = ui.fonts(|f| f.layout_no_wrap(label, font.clone(), t.muted));
        let w = g.size().x;
        ui.painter().galley(pos2(*x, cy - g.size().y / 2.0), g, t.muted);
        *x += w;
    };
    if let Some(r) = community {
        rating(format!("{r:.1}"), true, &mut x);
    }
    if let Some(c) = critic {
        let sep = if community.is_some() { "  ·  " } else { "" };
        rating(format!("{sep}{c:.0}%"), false, &mut x);
    }
    x += t.gap / 2.0;
    // the same badge look as Home's hero (D1)
    let labels: Vec<&str> = badges.iter().map(|b| b.label()).collect();
    super::parts::badges(ui, t, pos2(x, cy), &labels);
}

/// Number of rows `text` takes at `width` in the overview font (to decide whether More/Less is needed).
pub fn overview_rows(ui: &Ui, t: &Theme, text: &str, width: f32) -> usize {
    let job = text_job(text, t.font(t.body_size), t.text, width, usize::MAX);
    ui.fonts(|f| f.layout_job(job)).rows.len()
}

/// The overview: `lines` rows (… at the cut) or all of it when `open`, in `text` at 85 % so it sits under
/// the title. Width `hero_text_w`.
pub fn overview(ui: &mut Ui, t: &Theme, text: &str, open: bool, lines: usize) {
    let rows = if open { usize::MAX } else { lines };
    let job = text_job(text, t.font(t.body_size), t.text.gamma_multiply(0.85), t.hero_text_w, rows);
    let g = ui.fonts(|f| f.layout_job(job));
    let (rect, _) = ui.allocate_exact_size(g.size(), Sense::hover());
    ui.painter().galley(rect.min, g, t.text);
}

/// A text-only button ("More", "Less"): semibold small caps-like label; focused → a white plate.
pub fn text_button(ui: &mut Ui, t: &Theme, label: &str, focused: bool) -> Response {
    let font = t.font_w(t.small_size, Weight::Bold);
    let g = ui.fonts(|f| f.layout_no_wrap(label.to_uppercase(), font, Color32::PLACEHOLDER));
    let pad = t.chip_pad / 2.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 2.0 * pad, t.badge_h + pad / 2.0), Sense::click());
    let a = widgets::focus_amount(ui, t, resp.id, focused);
    let ink = widgets::plate(ui, t, rect, rect.height() / 2.0, Color32::TRANSPARENT, a, t.pill_focus_scale);
    ui.painter().galley(rect.center() - g.size() / 2.0, g, ink);
    resp
}

// ---------- 4. Action row: Play pill, labelled pills, round icon buttons ----------

/// Icons for the round buttons. Drawn with the painter (no icon font, no Apple artwork).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundIcon {
    Add,
    Watched,
    Favourite,
    More,
    Shuffle,
    Restart,
    Trailer,
    Previous,
    Next,
    /// Two ticks: the whole series watched.
    WatchedAll,
}

/// One button in the action row.
#[derive(Debug, Clone, PartialEq)]
pub enum ActionBtn {
    /// The white Play / Resume pill.
    Play(String),
    /// A glass pill with a label (Audio: English, Subtitles: Off …).
    Pill(String),
    /// A glass pill that opens a menu: label + chevron (Season 2 ⌄).
    Menu(String),
    /// A round icon button; `on` shows the set state (watched, favourite), `caption` names it under focus.
    Round { icon: RoundIcon, on: bool, caption: String },
}

fn round_icon(ui: &Ui, t: &Theme, icon: RoundIcon, on: bool, c: Pos2, size: f32, ink: Color32) {
    let p = ui.painter();
    let h = size / 2.0;
    let st = Stroke::new(t.icon_stroke, ink);
    match icon {
        RoundIcon::Add => widgets::icon(ui, t, Icon::Plus, c, size, ink),
        RoundIcon::More => widgets::icon(ui, t, Icon::More, c, size, ink),
        RoundIcon::Watched => {
            if on {
                p.circle_filled(c, h * 1.15, ink);
                let bg = if ink == t.text { t.bg } else { t.focus_fill };
                widgets::icon(ui, t, Icon::Tick, c, size * 0.6, bg);
            } else {
                p.circle_stroke(c, h * 1.15, st);
                widgets::icon(ui, t, Icon::Tick, c, size * 0.6, ink);
            }
        }
        RoundIcon::Favourite => {
            // two lobes and a point: a heart polygon
            let pts: Vec<Pos2> = (0..=40)
                .map(|i| {
                    let a = std::f32::consts::PI + i as f32 / 40.0 * std::f32::consts::TAU;
                    let x = 16.0 * a.sin().powi(3);
                    let y = 13.0 * a.cos() - 5.0 * (2.0 * a).cos() - 2.0 * (3.0 * a).cos() - (4.0 * a).cos();
                    pos2(c.x + x / 17.0 * h, c.y - y / 17.0 * h + h * 0.1)
                })
                .collect();
            if on {
                // each half (tip → top dip) is convex
                for half in [&pts[..=20], &pts[20..]] {
                    let mut v = half.to_vec();
                    v.push(pos2(c.x, v[0].y.min(v[v.len() - 1].y)));
                    p.add(egui::Shape::convex_polygon(v, ink, Stroke::NONE));
                }
            }
            p.add(egui::Shape::line(pts, st));
        }
        RoundIcon::Shuffle => {
            let (l, r) = (c.x - h, c.x + h);
            let (top, bot) = (c.y - h * 0.6, c.y + h * 0.6);
            p.add(egui::Shape::line(
                vec![pos2(l, bot), pos2(c.x - h * 0.2, bot), pos2(c.x + h * 0.2, top), pos2(r, top)],
                st,
            ));
            p.add(egui::Shape::line(
                vec![pos2(l, top), pos2(c.x - h * 0.2, top), pos2(c.x + h * 0.2, bot), pos2(r, bot)],
                st,
            ));
            for y in [top, bot] {
                let ah = h * 0.35;
                p.add(egui::Shape::line(vec![pos2(r - ah, y - ah), pos2(r, y), pos2(r - ah, y + ah)], st));
            }
        }
        RoundIcon::Restart => {
            let pts: Vec<Pos2> = (0..=24)
                .map(|i| {
                    let a = (-60.0 + 300.0 * i as f32 / 24.0).to_radians();
                    pos2(c.x + h * 0.85 * a.cos(), c.y + h * 0.85 * a.sin())
                })
                .collect();
            let end = pts[0];
            p.add(egui::Shape::line(pts, st));
            let ah = h * 0.4;
            p.add(egui::Shape::line(
                vec![pos2(end.x - ah, end.y - ah * 0.2), end, pos2(end.x + ah * 0.1, end.y + ah)],
                st,
            ));
        }
        RoundIcon::WatchedAll => {
            let ink2 = if on { ink } else { ink.gamma_multiply(0.8) };
            widgets::icon(ui, t, Icon::Tick, pos2(c.x - h * 0.3, c.y), size * 0.6, ink2);
            widgets::icon(ui, t, Icon::Tick, pos2(c.x + h * 0.35, c.y), size * 0.6, ink2);
            if on {
                p.circle_stroke(c, h * 1.15, st);
            }
        }
        RoundIcon::Previous | RoundIcon::Next => {
            // a play triangle against a bar: ⏮ / ⏭
            let k = if icon == RoundIcon::Next { 1.0 } else { -1.0 };
            let mut tri = vec![
                pos2(c.x - k * h * 0.6, c.y - h * 0.7),
                pos2(c.x + k * h * 0.5, c.y),
                pos2(c.x - k * h * 0.6, c.y + h * 0.7),
            ];
            if k < 0.0 {
                tri.reverse(); // keep the winding egui expects
            }
            p.add(egui::Shape::convex_polygon(tri, ink, Stroke::NONE));
            let x = c.x + k * h * 0.7;
            p.line_segment([pos2(x, c.y - h * 0.7), pos2(x, c.y + h * 0.7)], Stroke::new(t.icon_stroke * 1.2, ink));
        }
        RoundIcon::Trailer => {
            let r = Rect::from_center_size(c, vec2(size * 1.1, size * 0.8));
            p.rect_stroke(r, t.icon_stroke * 1.5, st, egui::StrokeKind::Middle);
            widgets::icon(ui, t, Icon::Play, c, size * 0.4, ink);
        }
    }
}

/// A round glass button with a painter icon; while focused, its caption fades in under it.
/// FIX-2 #10: with `labels` (desktop) the caption always shows under the button, muted until focused; without
/// (TV) it shows on focus only.
pub fn round_button(
    ui: &mut Ui,
    t: &Theme,
    icon: RoundIcon,
    on: bool,
    caption: &str,
    focused: bool,
    labels: bool,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(t.round_button, t.round_button), Sense::click());
    let a = widgets::focus_amount(ui, t, resp.id, focused);
    let rest = if resp.hovered() { t.glass_hi } else { t.glass };
    let ink = widgets::plate(ui, t, rect, t.round_button / 2.0, rest, a, t.pill_focus_scale);
    round_icon(ui, t, icon, on, rect.center(), t.round_button * 0.36, ink);
    // FIX-1 #4: desktop hover names the button even when focus is elsewhere
    let resp = if caption.is_empty() || (a > 0.01 && !labels) { resp } else { resp.on_hover_text(caption) };
    if (a > 0.01 || labels) && !caption.is_empty() {
        let font = t.font_w(t.small_size, Weight::Medium);
        let ink = if labels { t.muted.lerp_to_gamma(t.text, a) } else { t.text.gamma_multiply(a) };
        // always-on labels are one short word, so neighbours never collide; the full caption is the tooltip
        let text = if labels { short_label(icon).to_string() } else { caption.to_string() };
        let g = ui.fonts(|f| f.layout_no_wrap(text, font, ink));
        let at = pos2(rect.center().x - g.size().x / 2.0, rect.bottom() + t.gap / 2.0);
        ui.painter().galley(at, g, t.text);
    }
    resp
}

/// FIX-2 #10: the one-word label under a round button on the desktop.
pub fn short_label(icon: RoundIcon) -> &'static str {
    match icon {
        RoundIcon::Add => "Add",
        RoundIcon::Watched => "Watched",
        RoundIcon::WatchedAll => "Series",
        RoundIcon::Favourite => "Favourite",
        RoundIcon::More => "More",
        RoundIcon::Shuffle => "Shuffle",
        RoundIcon::Restart => "Restart",
        RoundIcon::Trailer => "Trailer",
        RoundIcon::Previous => "Previous",
        RoundIcon::Next => "Next",
    }
}

/// A glass pill with a chevron after the label; focused → white plate.
pub fn menu_pill(ui: &mut Ui, t: &Theme, label: &str, focused: bool) -> Response {
    let font = t.font_w(t.body_size, Weight::SemiBold);
    let g = ui.fonts(|f| f.layout_no_wrap(label.to_string(), font, Color32::PLACEHOLDER));
    let chev = t.body_size * 0.6;
    let w = g.size().x + t.gap / 3.0 + chev + 2.0 * t.button_pad;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, t.button_h), Sense::click());
    let a = widgets::focus_amount(ui, t, resp.id, focused);
    let rest = if resp.hovered() { t.glass_hi } else { t.glass };
    let ink = widgets::plate(ui, t, rect, rect.height() / 2.0, rest, a, t.pill_focus_scale);
    let x0 = rect.left() + t.button_pad;
    ui.painter().galley(pos2(x0, rect.center().y - g.size().y / 2.0), g.clone(), ink);
    let c = pos2(x0 + g.size().x + t.gap / 3.0 + chev / 2.0, rect.center().y);
    widgets::icon(ui, t, Icon::ChevronDown, c, chev, ink);
    resp
}

/// A translucent menu card (season picker, the ••• menu): pill rows `width` wide, focused = white plate. The card's
/// left edge sits at `left` (clamped into the row). Returns one response per row.
pub fn menu_card(
    ui: &mut Ui,
    t: &Theme,
    labels: &[String],
    focus: Option<usize>,
    left: f32,
    width: f32,
) -> Vec<Response> {
    let row_h = t.button_h + t.gap / 4.0;
    let size = vec2(width + 2.0 * t.menu_pad, labels.len() as f32 * row_h + 2.0 * t.menu_pad);
    let (slot, _) = ui.allocate_exact_size(vec2(ui.available_width(), size.y), Sense::hover());
    let x = left.clamp(slot.left(), (slot.right() - size.x).max(slot.left()));
    let rect = Rect::from_min_size(pos2(x, slot.top()), size);
    ui.painter().add(t.shadow(1.0).as_shape(rect, t.menu_radius));
    ui.painter().rect_filled(rect, t.menu_radius, t.menu_fill);
    let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(t.menu_pad)));
    inner.spacing_mut().item_spacing.y = t.gap / 4.0;
    labels
        .iter()
        .enumerate()
        .map(|(i, l)| widgets::pill_button_width(&mut inner, t, l, focus == Some(i), false, width))
        .collect()
}

/// A popover's muted line: none, only when it has no rows ("Loading…"), or a heading above the rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Note<'a> {
    None,
    WhenEmpty(&'a str),
    Heading(&'a str),
}

/// D6: a menu card floating next to the control that opened it (no inline push): under `anchor`, or above it when
/// there is no room; pill rows, focused = white plate, scrolls past `menu_max_h`. `note` is a muted line shown when
/// there are no rows ("Loading…"), or above the rows when `heading` ("No playlists yet"). Returns one response per row.
pub fn popover(
    ui: &Ui,
    t: &Theme,
    id: &str,
    anchor: Rect,
    labels: &[String],
    focus: Option<usize>,
    note: Note,
) -> Vec<Response> {
    let font = t.font_w(t.body_size, Weight::SemiBold);
    let widest = labels
        .iter()
        .map(|l| ui.fonts(|f| f.layout_no_wrap(l.clone(), font.clone(), t.text).size().x))
        .fold(0.0_f32, f32::max);
    let screen = ui.ctx().screen_rect();
    let w = (widest + 2.0 * t.button_pad).max(t.dropdown_w).min(screen.width() / 2.0);
    let row_h = t.button_h + t.gap / 4.0;
    let heading = matches!(note, Note::Heading(_));
    let rows_h = if labels.is_empty() { row_h } else { (labels.len() + usize::from(heading)) as f32 * row_h };
    let size = vec2(w + 2.0 * t.menu_pad, rows_h.min(t.menu_max_h) + 2.0 * t.menu_pad);
    let x = (anchor.left() - t.menu_pad).clamp(screen.left(), (screen.right() - size.x).max(screen.left()));
    let below = anchor.bottom() + t.gap / 4.0;
    let y =
        if below + size.y <= screen.bottom() { below } else { (anchor.top() - t.gap / 4.0 - size.y).max(screen.top()) };
    let moved = focus_moved(ui, Id::new(("at-popover", id)), focus);
    // egui hides a new area for its first (sizing) frame; ask for the next frame so it shows without waiting for input
    ui.ctx().request_repaint();
    let mut out = vec![];
    egui::Area::new(Id::new(("at-popover-area", id))).order(egui::Order::Foreground).fixed_pos(pos2(x, y)).show(
        ui.ctx(),
        |ui| {
            let rect = Rect::from_min_size(pos2(x, y), size);
            ui.painter().add(t.shadow(1.0).as_shape(rect, t.menu_radius));
            ui.painter().rect_filled(rect, t.menu_radius, t.menu_fill);
            let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(t.menu_pad)));
            inner.spacing_mut().item_spacing.y = t.gap / 4.0;
            // the note is the whole menu when there are no rows, else a heading above them ("No playlists yet")
            let text = match note {
                Note::None => None,
                Note::WhenEmpty(n) => labels.is_empty().then_some(n),
                Note::Heading(n) => Some(n),
            };
            if let Some(n) = text {
                let g = inner.fonts(|f| f.layout_no_wrap(n.to_string(), t.font(t.body_size), t.muted));
                let (r, _) = inner.allocate_exact_size(vec2(w, t.button_h), Sense::hover());
                inner.painter().galley(pos2(r.left() + t.button_pad, r.center().y - g.size().y / 2.0), g, t.muted);
            }
            if labels.is_empty() {
                return;
            }
            // D2: on the frame it opens, start with the focused row as the last one in view and as many rows above it
            // as fit (later focus moves just scroll the row into view).
            let pass = inner.ctx().cumulative_pass_nr();
            let seen_id = Id::new(("at-popover-seen", id));
            let fresh = inner.data(|d| d.get_temp::<u64>(seen_id)).is_none_or(|last| last + 3 < pass);
            inner.data_mut(|d| d.insert_temp(seen_id, pass));
            let mut area = egui::ScrollArea::vertical()
                .id_salt(("at-popover-scroll", id))
                .max_height(t.menu_max_h)
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
            if let (true, Some(f)) = (fresh, focus) {
                let view_h = (labels.len() as f32 * row_h).min(t.menu_max_h);
                area = area.vertical_scroll_offset(widgets::dropdown_open_offset(f, labels.len(), row_h, view_h));
            }
            let scrolled = area.show(&mut inner, |ui| {
                for (i, l) in labels.iter().enumerate() {
                    let r = widgets::pill_button_width(ui, t, l, focus == Some(i), false, w);
                    if moved && !fresh && focus == Some(i) {
                        r.scroll_to_me(None);
                    }
                    out.push(r);
                }
            });
            // FIX-4 #8: a fade and a chevron on each edge that has rows hidden past it.
            let view = scrolled.inner_rect;
            let (above, below) =
                widgets::dropdown_cues(scrolled.state.offset.y, scrolled.content_size.y, view.height());
            if above {
                widgets::dropdown_cue(&inner, t, view, true);
            }
            if below {
                widgets::dropdown_cue(&inner, t, view, false);
            }
        },
    );
    out
}

/// Seconds a toast stays up (it fades out over the last quarter).
pub const TOAST_SECS: f64 = 2.0;

/// FIX-1 #4: a short message pill at the bottom centre of the window ("Added to Favourites"), `age` seconds old.
/// Returns true once it has run its time (the caller drops it).
pub fn toast(ui: &Ui, t: &Theme, text: &str, age: f64) -> bool {
    if age >= TOAST_SECS {
        return true;
    }
    let a = ((TOAST_SECS - age) / (TOAST_SECS / 4.0)).min(1.0) as f32;
    let screen = ui.ctx().screen_rect();
    let font = t.font_w(t.body_size, Weight::SemiBold);
    let g = ui.fonts(|f| f.layout_no_wrap(text.to_string(), font, Color32::PLACEHOLDER));
    let size = vec2(g.size().x + 2.0 * t.pill_pad, t.pill_h);
    let rect = Rect::from_center_size(pos2(screen.center().x, screen.bottom() - t.row_gap - size.y / 2.0), size);
    let p = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::new("at-toast")));
    p.add(t.shadow(a).as_shape(rect, size.y / 2.0));
    p.rect_filled(rect, size.y / 2.0, t.menu_fill.gamma_multiply(a));
    p.galley(rect.center() - g.size() / 2.0, g, t.text.gamma_multiply(a));
    ui.ctx().request_repaint();
    false
}

/// The action row: buttons left to right with `gap / 2` between. Returns one response per button.
/// `labels`: the round buttons' captions always show (desktop; FIX-2 #10).
pub fn action_row(ui: &mut Ui, t: &Theme, btns: &[ActionBtn], focus: Option<usize>, labels: bool) -> Vec<Response> {
    let mut out = vec![];
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(t.gap / 2.0, t.gap);
        for (i, b) in btns.iter().enumerate() {
            let f = focus == Some(i);
            let r = match b {
                ActionBtn::Play(label) => widgets::play_pill_for(ui, t, label, f, !labels),
                ActionBtn::Pill(label) => widgets::pill_button(ui, t, label, f, false),
                ActionBtn::Menu(label) => menu_pill(ui, t, label, f),
                ActionBtn::Round { icon, on, caption } => round_button(ui, t, *icon, *on, caption, f, labels),
            };
            out.push(r);
        }
    });
    out
}

// ---------- 5. Sections and rows ----------

/// A section title ("Episodes", "Cast & Crew") with `row_gap` above it.
pub fn section_title(ui: &mut Ui, t: &Theme, text: &str) {
    ui.add_space(t.row_gap);
    let g = ui.fonts(|f| f.layout_no_wrap(text.to_string(), t.font(t.heading_size), t.text));
    let (rect, _) = ui.allocate_exact_size(g.size(), Sense::hover());
    ui.painter().galley(rect.min, g, t.text);
    ui.add_space(t.gap / 2.0);
}

/// A sideways row that bleeds to the window's right edge like Apple's: `count` cells of `cell_w`, `gap`
/// apart, each drawn by `draw(ui, i)`. Leaves room above and below for the focus lift and shadow, and keeps
/// the `focus` cell in view. Returns one response per cell.
pub fn row(
    ui: &mut Ui,
    t: &Theme,
    id: &str,
    count: usize,
    cell_h: f32,
    focus: Option<usize>,
    mut draw: impl FnMut(&mut Ui, usize) -> Response,
) -> Vec<Response> {
    let lift = cell_h * (t.focus_scale - 1.0) / 2.0 + t.shadow_offset;
    let screen = bleed_rect(ui, t);
    let (slot, _) = ui.allocate_exact_size(vec2(ui.available_width(), cell_h + 2.0 * lift), Sense::hover());
    let bleed = Rect::from_min_max(pos2(screen.left(), slot.top()), pos2(screen.right(), slot.bottom()));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bleed).id_salt(id));
    child.set_clip_rect(bleed.intersect(Rect::from_x_y_ranges(screen.x_range(), ui.clip_rect().y_range())));
    let mut out = vec![];
    let focus_changed = focus_moved(ui, Id::new(("at-row", id)), focus);
    // FIX-2 #4: on the desktop the row moves with the mouse too — the wheel scrolls it sideways while the pointer is
    // over it, arrows at its ends page it, and a fade on the right says there is more.
    let key = Id::new(("at-row-scroll", id));
    let (last_off, max_off): (f32, f32) = ui.data_mut(|d| d.get_temp(key)).unwrap_or((0.0, 0.0));
    let pending: f32 = ui.data_mut(|d| d.remove_temp(key.with("page")).unwrap_or(0.0));
    // by position, not layer: the end arrows sit on a layer above the row and must not hide themselves
    let hovered = ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| bleed.intersect(ui.clip_rect()).contains(p));
    // a focus change under the mouse (hover focuses) must not scroll the row or the page out from under the pointer
    let moved = focus_changed && !hovered;
    if max_off > 0.0 {
        // remember where the scrollable rows are: the page's scroller leaves the wheel over them (`row_wants_wheel`)
        let pass = ui.ctx().cumulative_pass_nr();
        let visible = bleed.intersect(ui.clip_rect());
        ui.data_mut(|d| {
            let rows = d.get_temp_mut_or_default::<(u64, Vec<Rect>)>(Id::new("at-row-rects"));
            if rows.0 != pass {
                *rows = (pass, vec![]);
            }
            rows.1.push(visible);
        });
    }
    // a vertical wheel scrolls this sideways-only row
    child.style_mut().always_scroll_the_only_direction = true;
    let mut area = egui::ScrollArea::horizontal()
        .id_salt(id)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .animated(true);
    if pending != 0.0 {
        area = area.scroll_offset(vec2((last_off + pending).clamp(0.0, max_off), 0.0));
    }
    // the end arrows sit over the cards: while the pointer is on one, the cards take no clicks (look unchanged)
    let arrow_at = |right: bool| {
        let cy = bleed.top() + lift + cell_h.min(t.wide_h) / 2.0;
        let cx = if right { bleed.right() - t.margin / 2.0 } else { bleed.left() + t.margin / 2.0 };
        Rect::from_center_size(pos2(cx, cy), vec2(t.round_button, t.round_button))
    };
    let on_arrow = hovered
        && ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| {
            (last_off > 1.0 && arrow_at(false).contains(p)) || (last_off < max_off - 1.0 && arrow_at(true).contains(p))
        });
    let shown = area.show(&mut child, |ui| {
        if on_arrow {
            ui.visuals_mut().disabled_alpha = 1.0;
            ui.disable();
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = t.gap;
            ui.add_space(slot.left() - screen.left());
            for i in 0..count {
                let r = ui.vertical(|ui| {
                    ui.add_space(lift);
                    draw(ui, i)
                });
                let r = r.inner;
                if moved && focus == Some(i) {
                    r.scroll_to_me(None);
                }
                out.push(r);
            }
            ui.add_space(t.margin - t.gap);
        });
    });
    let off = shown.state.offset.x;
    let max = (shown.content_size.x - shown.inner_rect.width()).max(0.0);
    ui.data_mut(|d| d.insert_temp(key, (off, max)));
    let p = ui.painter().with_clip_rect(child.clip_rect());
    let more_right = off < max - 1.0;
    if more_right {
        let fade = Rect::from_min_max(pos2(bleed.right() - t.margin, bleed.top()), bleed.max);
        gradient(&p, fade, &fade_stops(t.bg, 0.0, 0.9), false);
    }
    // FIX-3 #4: on the desktop both end arrows are there whenever the row overflows (the one at an end dimmed and
    // inert), hover or not. The TV has no pointer to click them, so it keeps its look.
    if max > 1.0 && ui.ctx().data(|d| d.get_temp::<bool>(Id::new("at-desktop"))).unwrap_or(false) {
        let page = (bleed.width() - 2.0 * t.margin) * 0.8;
        for (right, live) in [(false, off > 1.0), (true, more_right)] {
            let r = arrow_at(right);
            // registered after the cards in the same layer, so it is the one under the pointer
            let resp = ui.interact(
                r,
                key.with(("arrow", right)),
                if hovered && live { Sense::click() } else { Sense::hover() },
            );
            let lit = live && resp.hovered();
            let dim = |c: Color32| if live { c } else { c.gamma_multiply(0.35) };
            let fill = if lit { t.focus_fill } else { t.menu_fill };
            let ink = if lit { t.focus_ink } else { t.text };
            p.circle_filled(r.center(), r.width() / 2.0, dim(fill));
            let h = t.round_button * 0.14;
            let k = if right { 1.0 } else { -1.0 };
            let pts = vec![
                pos2(r.center().x - k * h * 0.6, r.center().y - h),
                pos2(r.center().x + k * h * 0.6, r.center().y),
                pos2(r.center().x - k * h * 0.6, r.center().y + h),
            ];
            p.add(egui::Shape::line(pts, Stroke::new(t.icon_stroke, dim(ink))));
            if hovered && live && resp.clicked() {
                ui.data_mut(|d| d.insert_temp(key.with("page"), if right { page } else { -page }));
            }
        }
    }
    // The sideways scroller swallows the page's vertical scroll request: bring the row (and its title) into view.
    if moved {
        ui.scroll_to_rect(slot.with_min_y(slot.top() - t.row_gap), None);
    }
    out
}

/// FIX-3 #4: the app tells the rows once a frame whether this is the desktop (pointer, end arrows) or the TV.
pub fn set_desktop(ctx: &egui::Context, desktop: bool) {
    ctx.data_mut(|d| d.insert_temp(Id::new("at-desktop"), desktop));
}

/// FIX-2 #4: true when the pointer is over a row that could scroll sideways when last drawn — the page's own scroller
/// then leaves the mouse wheel to that row (`ScrollArea::scroll_source`).
pub fn row_wants_wheel(ui: &Ui) -> bool {
    let Some(pos) = ui.input(|i| i.pointer.hover_pos()) else { return false };
    let (pass, rows) = ui.data_mut(|d| d.get_temp::<(u64, Vec<Rect>)>(Id::new("at-row-rects"))).unwrap_or_default();
    pass + 2 >= ui.ctx().cumulative_pass_nr() && rows.iter().any(|r| r.contains(pos))
}

/// The page scroller's input sources: everything, except the wheel while it belongs to a row.
pub fn page_scroll_source(ui: &Ui) -> egui::scroll_area::ScrollSource {
    egui::scroll_area::ScrollSource { mouse_wheel: !row_wants_wheel(ui), ..egui::scroll_area::ScrollSource::ALL }
}

/// One 16:9 episode card for the show page's season row, all plain data.
#[derive(Debug, Clone, Default)]
pub struct EpisodeCard {
    pub art: Option<TextureId>,
    /// "EPISODE 1" (or "SPECIAL")
    pub number: String,
    pub title: String,
    pub overview: String,
    /// "24 min", or the air date for an episode that has not aired yet
    pub detail: String,
    pub progress: Option<f32>,
    pub played: bool,
    pub unaired: bool,
}

/// Height of an episode card (art + text block) for row sizing.
pub fn episode_card_h(t: &Theme) -> f32 {
    t.ep_thumb_h + t.card_title_gap + line_h(t.small_size) * 4.0 + line_h(t.body_size) + t.gap / 4.0
}

/// FIX-1 #18: the ▶ spot on an episode card's art (desktop hover shows it; a click there plays, elsewhere opens).
pub fn episode_play_rect(t: &Theme, card: Rect) -> Rect {
    let img = Rect::from_min_size(card.min, vec2(t.ep_thumb_w, t.ep_thumb_h));
    Rect::from_center_size(img.center(), vec2(t.round_button, t.round_button))
}

/// Apple's episode card: 16:9 art that lifts when focused (progress inset along the bottom, dimmed if not
/// aired yet), then EPISODE n, the title, two lines of overview and the runtime with a tick once watched.
pub fn episode_card(ui: &mut Ui, t: &Theme, c: &EpisodeCard, focused: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(t.ep_thumb_w, episode_card_h(t)), Sense::click());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let a = widgets::focus_amount(ui, t, resp.id, focused);
    let img = Rect::from_min_size(rect.min, vec2(t.ep_thumb_w, t.ep_thumb_h));
    let r = art(ui, t, img, c.art, "", a, c.progress, c.unaired);
    if resp.hovered() && !c.unaired {
        let play = Rect::from_center_size(r.center(), vec2(t.round_button, t.round_button));
        ui.painter().circle_filled(play.center(), play.width() / 2.0, t.scrim);
        widgets::icon(ui, t, Icon::Play, play.center(), t.round_button * 0.36, t.text);
    }
    let w = t.ep_thumb_w;
    let mut y = img.bottom() + t.card_title_gap + (t.focus_scale - 1.0) * t.ep_thumb_h / 2.0 * a;
    let small_b = t.font_w(t.small_size, Weight::SemiBold);
    y += text_at(ui, pos2(rect.left(), y), text_job(&c.number.to_uppercase(), small_b.clone(), t.muted, w, 1)).y;
    let title_c = t.muted.lerp_to_gamma(t.text, 0.6 + 0.4 * a);
    let title = t.font_w(t.body_size, Weight::SemiBold);
    y += text_at(ui, pos2(rect.left(), y), text_job(&c.title, title, title_c, w, 1)).y + t.gap / 8.0;
    if !c.overview.is_empty() {
        y += text_at(ui, pos2(rect.left(), y), text_job(&c.overview, t.font(t.small_size), t.muted, w, 2)).y;
    }
    y += t.gap / 8.0;
    let d = text_at(ui, pos2(rect.left(), y), text_job(&c.detail, small_b, t.muted, w, 1));
    if c.played {
        let cx = rect.left() + d.x + t.gap / 3.0 + t.small_size / 2.0;
        widgets::icon(ui, t, Icon::Tick, pos2(cx, y + d.y / 2.0), t.small_size * 0.7, t.muted);
    }
    resp
}

/// Height of a wide card with `lines` caption lines.
pub fn wide_card_h(t: &Theme, lines: usize) -> f32 {
    t.wide_h + t.card_title_gap + line_h(t.body_size) * lines as f32
}

/// A wide 16:9 card (Up Next, a collection's parts): art (optionally a bold title over a bottom shade — Apple puts
/// the title logo there) and caption lines under it: the first in medium weight, the rest muted.
pub fn wide_card(
    ui: &mut Ui,
    t: &Theme,
    tex: Option<TextureId>,
    title: &str,
    caption: &[&str],
    progress: Option<f32>,
    focused: bool,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(t.wide_w, wide_card_h(t, caption.len())), Sense::click());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let a = widgets::focus_amount(ui, t, resp.id, focused);
    let img = Rect::from_min_size(rect.min, vec2(t.wide_w, t.wide_h));
    let r = art(ui, t, img, tex, "", a, progress.filter(|_| title.is_empty()), false);
    let p = ui.painter();
    let pad = t.gap / 2.0;
    if !title.is_empty() {
        let shade = Rect::from_min_max(pos2(r.left(), r.center().y), r.max);
        gradient(p, shade, &fade_stops(Color32::BLACK, 0.0, 0.75), true);
        let job = text_job(title, t.font_w(t.heading_size, Weight::Bold), t.text, r.width() - 2.0 * pad, 2);
        let g = ui.fonts(|f| f.layout_job(job));
        let bottom = if progress.is_some() { r.bottom() - pad - t.progress_h - pad / 2.0 } else { r.bottom() - pad };
        p.galley(pos2(r.left() + pad, bottom - g.size().y), g, t.text);
        if let Some(f) = progress {
            let bar = Rect::from_min_size(
                pos2(r.left() + pad, r.bottom() - pad - t.progress_h),
                vec2(r.width() - 2.0 * pad, t.progress_h),
            );
            p.rect_filled(bar, t.progress_h / 2.0, t.progress_track);
            let mut done = bar;
            done.set_right(bar.left() + bar.width() * f.clamp(0.0, 1.0));
            p.rect_filled(done, t.progress_h / 2.0, t.progress);
        }
    }
    let mut y = img.bottom() + t.card_title_gap + (t.focus_scale - 1.0) * t.wide_h / 2.0 * a;
    for (i, line) in caption.iter().enumerate() {
        let (font, c) = if i == 0 {
            (t.font_w(t.body_size, Weight::Medium), t.muted.lerp_to_gamma(t.text, 0.6 + 0.4 * a))
        } else {
            (t.font(t.body_size), t.muted)
        };
        y += text_at(ui, pos2(rect.left(), y), text_job(line, font, c, t.wide_w, 1)).y;
    }
    resp
}

/// A chapter still: 16:9 art that lifts when focused, one label line under it ("12:30 · The Chase").
pub fn still_card(ui: &mut Ui, t: &Theme, tex: Option<TextureId>, label: &str, focused: bool) -> Response {
    let h = t.thumb_h + t.card_title_gap + line_h(t.body_size);
    let (rect, resp) = ui.allocate_exact_size(vec2(t.thumb_w, h), Sense::click());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let a = widgets::focus_amount(ui, t, resp.id, focused);
    let img = Rect::from_min_size(rect.min, vec2(t.thumb_w, t.thumb_h));
    art(ui, t, img, tex, "", a, None, false);
    let y = img.bottom() + t.card_title_gap + (t.focus_scale - 1.0) * t.thumb_h / 2.0 * a;
    let c = t.muted.lerp_to_gamma(t.text, 0.6 + 0.4 * a);
    text_at(ui, pos2(rect.left(), y), text_job(label, t.font_w(t.body_size, Weight::Medium), c, t.thumb_w, 1));
    resp
}

/// What a focused poster draws: the one card focus rule (C3), so on TV it matches `parts::card`. On desktop the
/// details-page art keeps its old look (shadow + sheen, no glow).
pub fn poster_focus_style(tv: bool, focused: bool) -> super::parts::CardFocusStyle {
    let s = super::parts::card_focus_style(tv, focused, false);
    super::parts::CardFocusStyle { glow: s.glow && tv, ..s }
}

/// A 2:3 poster (Related, a collection's films): lifts when focused; the title shows under it only while
/// focused, or always when there is no art (Apple's posters carry their own title).
pub fn poster_card(ui: &mut Ui, t: &Theme, tex: Option<TextureId>, title: &str, focused: bool) -> Response {
    let h = t.poster_h + t.card_title_gap + line_h(t.body_size);
    let (rect, resp) = ui.allocate_exact_size(vec2(t.poster_w, h), Sense::click());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let a = widgets::focus_amount(ui, t, resp.id, focused);
    let img = Rect::from_min_size(rect.min, vec2(t.poster_w, t.poster_h));
    art(ui, t, img, tex, &initials(title), a, None, false);
    let show = if tex.is_some() { a } else { 1.0 };
    if show > 0.01 {
        let y = img.bottom() + t.card_title_gap + (t.focus_scale - 1.0) * t.poster_h / 2.0 * a;
        let c = t.text.gamma_multiply(show);
        let job = text_job(title, t.font_w(t.body_size, Weight::Medium), c, t.poster_w, 1);
        let g = ui.fonts(|f| f.layout_job(job));
        ui.painter().galley(pos2(rect.center().x - g.size().x / 2.0, y), g, c);
    }
    resp
}

/// Image clipped to a circle (cover crop, kept to the top of portrait photos), via a triangle fan.
fn circle_image(painter: &egui::Painter, ui: &Ui, tex: TextureId, c: Pos2, r: f32) {
    let uv = cover_uv(tex_size(ui, tex).unwrap_or(vec2(r, r)), vec2(r, r), 0.15);
    let mut mesh = Mesh::with_texture(tex);
    mesh.vertices.push(egui::epaint::Vertex { pos: c, uv: uv.center(), color: Color32::WHITE });
    let n = 64;
    for i in 0..=n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        let (s, co) = a.sin_cos();
        let pos = pos2(c.x + r * co, c.y + r * s);
        let uvp = pos2(uv.center().x + uv.width() / 2.0 * co, uv.center().y + uv.height() / 2.0 * s);
        mesh.vertices.push(egui::epaint::Vertex { pos, uv: uvp, color: Color32::WHITE });
        if i > 0 {
            mesh.add_triangle(0, i as u32, i as u32 + 1);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// A large round portrait (the person page header): the photo clipped to a circle, initials while there is none.
pub fn portrait(ui: &mut Ui, t: &Theme, tex: Option<TextureId>, name: &str, size: f32) -> Rect {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let p = ui.painter();
    p.add(t.shadow(1.0).as_shape(rect, size / 2.0));
    match tex {
        Some(tex) => circle_image(p, ui, tex, rect.center(), size / 2.0),
        None => {
            p.circle_filled(rect.center(), size / 2.0, t.surface_hi);
            p.text(
                rect.center(),
                Align2::CENTER_CENTER,
                initials(name),
                t.font_w(t.title_size, Weight::SemiBold),
                t.muted,
            );
        }
    }
    rect
}

/// Height of a cast cell (photo + name + role).
pub fn person_h(t: &Theme) -> f32 {
    t.person_size + t.card_title_gap + line_h(t.body_size) + line_h(t.small_size)
}

/// Apple's cast & crew cell: a round photo (initials while none) that lifts when focused, the name and the role
/// centred under it.
pub fn person_circle(
    ui: &mut Ui,
    t: &Theme,
    tex: Option<TextureId>,
    name: &str,
    role: &str,
    focused: bool,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(t.person_size, person_h(t)), Sense::click());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let a = widgets::focus_amount(ui, t, resp.id, focused);
    let img = Rect::from_min_size(rect.min, vec2(t.person_size, t.person_size));
    let r = widgets::lift(img, t.focus_scale, a);
    let p = ui.painter();
    if a > 0.01 {
        p.add(t.shadow(a).as_shape(r, r.width() / 2.0));
    }
    match tex {
        Some(tex) => circle_image(p, ui, tex, r.center(), r.width() / 2.0),
        None => {
            p.circle_filled(r.center(), r.width() / 2.0, t.surface_hi);
            p.text(
                r.center(),
                Align2::CENTER_CENTER,
                initials(name),
                t.font_w(t.heading_size, Weight::SemiBold),
                t.muted,
            );
        }
    }
    if a > 0.01 {
        p.circle_stroke(r.center(), r.width() / 2.0, Stroke::new(t.badge_stroke, t.sheen.gamma_multiply(a)));
        if super::parts::is_tv(ui.ctx()) {
            let ring = Stroke::new(t.focus_stroke, t.focus_color.gamma_multiply(a));
            p.circle_stroke(r.center(), r.width() / 2.0 + t.focus_stroke / 2.0, ring);
        }
    }
    let mut y = img.bottom() + t.card_title_gap + (t.focus_scale - 1.0) * t.person_size / 2.0 * a;
    let name_c = t.muted.lerp_to_gamma(t.text, 0.6 + 0.4 * a);
    let job = text_job(name, t.font_w(t.body_size, Weight::Medium), name_c, t.person_size + t.gap / 2.0, 1);
    let g = ui.fonts(|f| f.layout_job(job));
    let gh = g.size().y;
    ui.painter().galley(pos2(rect.center().x - g.size().x / 2.0, y), g, name_c);
    y += gh;
    if !role.is_empty() {
        let job = text_job(role, t.font(t.small_size), t.muted, t.person_size + t.gap / 2.0, 1);
        let g = ui.fonts(|f| f.layout_job(job));
        ui.painter().galley(pos2(rect.center().x - g.size().x / 2.0, y), g, t.muted);
    }
    resp
}

/// The Information section: columns (heading in `text` semibold, lines in `muted`), five to a band.
/// Each column is a response (a focus stop for the remote); `focus` turns that column's heading into a plate.
pub fn info_section(ui: &mut Ui, t: &Theme, cols: &[InfoColumn], focus: Option<usize>) -> Vec<Response> {
    let mut out = vec![];
    let moved = focus_moved(ui, Id::new("at-info"), focus);
    let per = 5;
    let col_w = (ui.available_width() - t.gap * (per as f32 - 1.0)) / per as f32;
    for (band, chunk) in cols.chunks(per).enumerate() {
        if band > 0 {
            ui.add_space(t.gap);
        }
        let top = ui.cursor().top();
        let mut bottom = top;
        for (j, c) in chunk.iter().enumerate() {
            let i = band * per + j;
            let x = ui.min_rect().left() + j as f32 * (col_w + t.gap);
            let a_id = Id::new(("info-col", i));
            let a = widgets::focus_amount(ui, t, a_id, focus == Some(i));
            let mut y = top;
            let head_font = t.font_w(t.body_size, Weight::SemiBold);
            let g = ui.fonts(|f| f.layout_no_wrap(c.heading.clone(), head_font, Color32::PLACEHOLDER));
            let pad = t.chip_pad / 2.0;
            let head = Rect::from_min_size(pos2(x - pad, y), vec2(g.size().x + 2.0 * pad, g.size().y + pad));
            let ink = widgets::plate(ui, t, head, head.height() / 2.0, Color32::TRANSPARENT, a, t.pill_focus_scale);
            ui.painter().galley(pos2(x, y + pad / 2.0), g, ink);
            y = head.bottom() + t.gap / 6.0;
            for l in &c.lines {
                y += text_at(ui, pos2(x, y), text_job(l, t.font(t.body_size), t.muted, col_w, 3)).y + t.gap / 8.0;
            }
            bottom = bottom.max(y);
            let col_rect = Rect::from_min_max(pos2(x, top), pos2(x + col_w, y));
            if moved && focus == Some(i) {
                ui.scroll_to_rect(col_rect.expand(t.gap), None);
            }
            out.push(ui.interact(col_rect, a_id, Sense::click()));
        }
        ui.allocate_exact_size(vec2(ui.available_width(), bottom - top), Sense::hover());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(kind: StreamKind, codec: &str, title: &str, w: i32, h: i32, ch: i32) -> StreamFacts {
        StreamFacts { kind, codec: codec.into(), title: title.into(), width: w, height: h, channels: ch }
    }

    #[test]
    fn runtime_long_reads_like_apple() {
        assert_eq!(runtime_long(40.0), "1 min");
        assert_eq!(runtime_long(2400.0), "40 min");
        assert_eq!(runtime_long(5520.0), "1 hr 32 min");
        assert_eq!(runtime_long(7200.0), "2 hr");
    }

    #[test]
    fn meta_parts_in_order_and_only_what_exists() {
        let mut it = Item { official_rating: Some("PG".into()), ..Default::default() };
        it.run_time_ticks = Some(5520 * 10_000_000);
        it.genres = vec!["Drama".into(), "Thriller".into(), "Mystery".into()];
        assert_eq!(meta_parts(&it, "2018"), vec!["2018", "PG", "1 hr 32 min", "Drama, Thriller"]);
        assert!(meta_parts(&Item::default(), "").is_empty());
    }

    #[test]
    fn badges_from_a_4k_hdr_atmos_film() {
        let v = vec![
            s(StreamKind::Video, "hevc", "4k hevc hdr10", 3840, 2160, 0),
            s(StreamKind::Audio, "truehd", "english - truehd atmos 7.1", 0, 0, 8),
            s(StreamKind::Audio, "aac", "english audio description", 0, 0, 2),
            s(StreamKind::Subtitle, "srt", "english sdh", 0, 0, 0),
        ];
        assert_eq!(
            badges(&v),
            vec![Badge::FourK, Badge::Hdr, Badge::DolbyAtmos, Badge::Surround7, Badge::Sdh, Badge::Ad]
        );
    }

    #[test]
    fn badges_from_a_plain_hd_file() {
        let v = vec![
            s(StreamKind::Video, "h264", "720p h264 sdr", 1280, 720, 0),
            s(StreamKind::Audio, "ac3", "english - dolby digital - 5.1", 0, 0, 6),
            s(StreamKind::Subtitle, "srt", "english", 0, 0, 0),
        ];
        assert_eq!(badges(&v), vec![Badge::Hd, Badge::Dolby, Badge::Surround5, Badge::Cc]);
        // titles only (the item's MediaStreams carry no sizes): still found
        let v = vec![s(StreamKind::Video, "hevc", "1080p hevc dolby vision", 0, 0, 0)];
        assert_eq!(badges(&v), vec![Badge::Hd, Badge::DolbyVision]);
        assert!(badges(&[]).is_empty());
    }

    #[test]
    fn info_columns_leave_out_empty_ones() {
        let it = Item { production_year: Some(2018), ..Default::default() };
        let cols = info_columns(&it, None, false);
        assert_eq!(cols, vec![InfoColumn { heading: "Released".into(), lines: vec!["2018".into()] }]);
        assert_eq!(release_date("2018-05-04T00:00:00.0000000Z").as_deref(), Some("4 May 2018"));
        assert_eq!(release_date("bad"), None);
    }

    #[test]
    fn cover_uv_crops_the_long_side() {
        let uv = cover_uv(vec2(1920.0, 1080.0), vec2(100.0, 100.0), 0.5);
        assert!((uv.width() - 0.5625).abs() < 1e-4 && (uv.height() - 1.0).abs() < 1e-4);
        let uv = cover_uv(vec2(600.0, 900.0), vec2(100.0, 100.0), 0.0);
        assert!((uv.min.y).abs() < 1e-4 && (uv.height() - 2.0 / 3.0).abs() < 1e-4);
    }
}
