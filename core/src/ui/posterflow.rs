//! Poster Flow (BRIEF-coverflow): the iPod touch landscape album flow, with film and show posters.
//! Geometry and motion follow the measured reference in `docs/coverflow-reference.md`. Covers, reflections and the
//! back panel are real perspective quads (`posterflow_gl`); labels and the back's rows are egui on top.
//! Input: drag / flick, wheel or trackpad, click a side cover, arrows (held = faster), Home / End, type-ahead;
//! Enter, Info, a click on the centre cover or the ⓘ button flips it; Back flips it back.
use super::player::{self, PlayOptions};
use super::posterflow_gl::{self as pgl, Quad, Vert};
use super::{parts, widgets};
use crate::api::{image_width_bucket, ticks_to_secs, ApiError, ImageKind, Item, ItemKind, LibraryQuery, Result};
use crate::api::{SortBy, StreamKind};
use crate::app::{Cx, Screen};
use crate::platform::Key;
use crate::theme::{Theme, Weight};
use egui::{pos2, vec2, Align2, Color32, Pos2, Rect, Sense};
use std::collections::{HashMap, HashSet};

// ---- the reference, measured (docs/coverflow-reference.md). Lengths are in cover heights (the iPod's square
// covers), so a 2:3 poster keeps the iPod's strip widths and spacing. ----
/// Side covers turn this far about their vertical axis (outer edge towards you).
pub const SIDE_ANGLE_DEG: f32 = 84.0;
/// FIX-2 #7: how bright the poster on the turning back is (it sits under the title).
const BACK_DIM: f32 = 0.35;

/// Camera distance from the plane of the side covers.
pub const CAM_DIST: f32 = 3.7;
/// How far the centre cover stands forward of the side covers.
pub const CENTRE_Z: f32 = 0.92;
/// Each further side cover sits one stride out from the one before.
pub const STRIDE: f32 = 0.175;
/// Reflection brightness at the seam; it fades linearly to nothing over one cover height.
pub const REFLECT: f32 = 0.5;
/// Poster width per height (2:3).
pub const COVER_W: f32 = 2.0 / 3.0;
/// The flipped back: 1.34 × the iPod's square cover, so a square panel a third wider than the cover is tall
/// (it grows from the narrower poster as it turns).
pub const BACK_SCALE: f32 = 1.34;

// ---- layout + motion ----
/// Centre poster height and top, as shares of the view height.
const COVER_FRAC: f32 = 0.70;
const TOP_FRAC: f32 = 0.08;
/// The flipped back's top, as a share of the view height (the iPod's sits just under the status bar).
const BACK_TOP_FRAC: f32 = 0.025;
/// Glide rate (1/s): a tap or an arrow press settles 95 % in 0.3 s, ease-out.
const GLIDE_K: f32 = 10.0;
/// Flick friction (1/s): a flick at v covers/s runs about v / FRICTION covers before it lands.
const FRICTION: f32 = 3.0;
const FLIP_SECS: f32 = 0.5;
/// Held arrows may queue at most this many covers ahead of the glide.
const MAX_AHEAD: f32 = 12.0;
/// Covers kept warm either side of the centre.
const PREFETCH: usize = 10;
/// Drag: screen px per cover = this × the centre cover's width (the cover stays under the finger).
const DRAG_PX: f32 = 0.55;
/// Wheel / trackpad: px per cover.
const WHEEL_PX: f32 = 40.0;
/// Type-ahead: letters typed within this many seconds build one prefix.
const TYPE_GAP: f64 = 1.0;
/// Library pages fetched per request while loading the whole list.
const PAGE: usize = 200;

/// Where a cover sits at offset `d` (its index − the flow position): x, forward z, turn (radians).
/// Between the centre and the first side slot every value blends linearly (the classic swing).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub x: f32,
    pub z: f32,
    pub angle: f32,
}

/// Centre of the first side cover, for covers `w` wide: its inner edge just meets the centre cover's edge on
/// screen (the iPod's 2 px gap). For square covers this gives the measured 0.82.
pub fn side_x(w: f32) -> f32 {
    let h = w / 2.0;
    let (s, c) = SIDE_ANGLE_DEG.to_radians().sin_cos();
    h * (CAM_DIST + h * s) / (CAM_DIST - CENTRE_Z) + h * c
}

pub fn pose(d: f32) -> Pose {
    let th = SIDE_ANGLE_DEG.to_radians();
    let sx = side_x(COVER_W);
    let a = d.abs();
    if a >= 1.0 {
        let s = d.signum();
        Pose { x: s * (sx + (a - 1.0) * STRIDE), z: 0.0, angle: s * th }
    } else {
        Pose { x: d * sx, z: (1.0 - a) * CENTRE_Z, angle: d * th }
    }
}

/// Smoothstep for the flip.
fn ease(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// The camera: on the horizon (cover centre height), looking at the middle of the view.
#[derive(Clone, Copy, Debug)]
pub struct Cam {
    view: Rect,
    centre: Pos2,
    /// Screen px per world unit at depth 1.
    f: f32,
}

impl Cam {
    pub fn new(view: Rect) -> Cam {
        let h = view.height() * COVER_FRAC;
        let f = h * (CAM_DIST - CENTRE_Z);
        Cam { view, centre: pos2(view.center().x, view.top() + view.height() * TOP_FRAC + h / 2.0), f }
    }
    /// World point → screen point and depth.
    pub fn project(&self, p: [f32; 3]) -> (Pos2, f32) {
        let w = (CAM_DIST - p[2]).max(0.05);
        (pos2(self.centre.x + self.f * p[0] / w, self.centre.y - self.f * p[1] / w), w)
    }
    fn vert(&self, p: [f32; 3], uv: [f32; 2], rgba: [f32; 4]) -> (Vert, Pos2) {
        let (s, w) = self.project(p);
        let nx = (s.x - self.view.center().x) / (self.view.width() / 2.0);
        let ny = -(s.y - self.view.center().y) / (self.view.height() / 2.0);
        (Vert { pos: [nx * w, ny * w, 0.0, w], uv, rgba }, s)
    }
    /// Screen height and width of the centre cover.
    pub fn cover_h(&self) -> f32 {
        self.f / (CAM_DIST - CENTRE_Z)
    }
    pub fn cover_w(&self) -> f32 {
        self.cover_h() * COVER_W
    }
}

/// A cover placed in the world: pose plus the flip (extra turn, growth, lift).
#[derive(Clone, Copy, Debug)]
struct Placed {
    x: f32,
    y: f32,
    z: f32,
    angle: f32,
    /// Growth across and down (the back is wider than a poster).
    sx: f32,
    sy: f32,
}

impl Placed {
    /// World corner for local (lx, ly) as shares of the cover (each −0.5..0.5).
    fn at(&self, lx: f32, ly: f32) -> [f32; 3] {
        let (s, c) = self.angle.sin_cos();
        let lx = lx * COVER_W * self.sx;
        [self.x + lx * c, self.y + ly * self.sy, self.z + lx * s]
    }
    fn floor(&self) -> f32 {
        self.y - 0.5 * self.sy
    }
}

fn rgba(c: Color32) -> [f32; 4] {
    let [r, g, b, a] = c.to_array();
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a as f32 / 255.0]
}

fn scaled(c: [f32; 4], k: f32) -> [f32; 4] {
    [c[0] * k, c[1] * k, c[2] * k, c[3] * k]
}

/// A quad between local rows `y0` (top) and `y1` with per-corner colours [tl, tr, br, bl]; `None` tex = solid.
fn quad(
    cam: &Cam,
    p: &Placed,
    x: (f32, f32),
    y: (f32, f32),
    tex: Option<egui::TextureId>,
    cols: [[f32; 4]; 4],
) -> (Quad, [Pos2; 4]) {
    let corners = [(x.0, y.0, 0.0, 0.0), (x.1, y.0, 1.0, 0.0), (x.1, y.1, 1.0, 1.0), (x.0, y.1, 0.0, 1.0)];
    let mut v = [Vert::default(); 4];
    let mut s = [Pos2::ZERO; 4];
    for (i, (lx, ly, u, w)) in corners.into_iter().enumerate() {
        let uv = if tex.is_some() { [u, w] } else { [0.0, 0.0] };
        (v[i], s[i]) = cam.vert(p.at(lx, ly), uv, cols[i]);
    }
    (Quad { tex: tex.unwrap_or_default(), v }, s)
}

/// The cover's mirror image under the floor, fading from REFLECT to nothing over one cover height.
fn reflection(cam: &Cam, p: &Placed, tex: Option<egui::TextureId>, base: [f32; 4]) -> Quad {
    let fl = p.floor();
    let top = scaled(base, REFLECT);
    let bottom = [0.0; 4];
    let corners = [
        (-0.5, 0.0, 0.0, 1.0, top),
        (0.5, 0.0, 1.0, 1.0, top),
        (0.5, 1.0, 1.0, 0.0, bottom),
        (-0.5, 1.0, 0.0, 0.0, bottom),
    ];
    let mut v = [Vert::default(); 4];
    for (i, (lx, k, u, w, c)) in corners.into_iter().enumerate() {
        let mut pt = p.at(lx, 0.0);
        pt[1] = fl - k * p.sy;
        let uv = if tex.is_some() { [u, w] } else { [0.0, 0.0] };
        v[i] = cam.vert(pt, uv, c).0;
    }
    Quad { tex: tex.unwrap_or_default(), v }
}

fn inside(q: &[Pos2; 4], p: Pos2) -> bool {
    let mut sign = 0.0;
    for i in 0..4 {
        let a = q[i];
        let b = q[(i + 1) % 4];
        let c = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
        if c.abs() < f32::EPSILON {
            continue;
        }
        if sign == 0.0 {
            sign = c.signum();
        } else if c.signum() != sign {
            return false;
        }
    }
    true
}

/// "Name" for type-ahead: lower case, without a leading "the ".
fn sort_key(name: &str) -> String {
    let n = name.trim().to_lowercase();
    n.strip_prefix("the ").map(str::to_string).unwrap_or(n)
}

/// First item whose name starts with `prefix`; else the first one after it alphabetically.
pub fn type_ahead(items: &[Item], prefix: &str) -> Option<usize> {
    let p = prefix.to_lowercase();
    items
        .iter()
        .position(|i| sort_key(&i.name).starts_with(&p))
        .or_else(|| items.iter().position(|i| sort_key(&i.name).as_str() > p.as_str()))
}

/// Under a minute in counts as not started: offer Play, not "Resume from 0:00".
const RESUME_MIN_SECS: f64 = 60.0;

fn resumable(position_ticks: i64) -> bool {
    ticks_to_secs(position_ticks) >= RESUME_MIN_SECS
}

/// Label lines under the centre cover: film → title / year · runtime; show → title / years · N seasons.
pub fn labels(item: &Item, full: Option<&Item>) -> (String, String) {
    let it = full.unwrap_or(item);
    let mut v = vec![];
    let years = crate::story::year_line(it, true, false);
    if !years.is_empty() {
        v.push(years);
    }
    match it.kind {
        ItemKind::Series => {
            if let Some(n) = it.child_count.filter(|&n| n > 0) {
                v.push(if n == 1 { "1 season".to_string() } else { format!("{n} seasons") });
            }
        }
        _ => {
            if let Some(t) = it.run_time_ticks {
                v.push(parts::runtime(ticks_to_secs(t)));
            }
        }
    }
    (it.name.clone(), v.join(" · "))
}

/// FIX-1 #17: where the centre cover is in the flow, "7 of 15" (1-based); "7 of 15+" while more are loading.
pub fn position_label(centre: usize, len: usize, complete: bool) -> String {
    format!("{} of {}{}", centre + 1, len, if complete { "" } else { "+" })
}

/// Where the flow's items come from.
enum Src {
    /// A library: the flow loads every page itself (the grid hands over what it already has).
    Library { view_id: String, query: LibraryQuery, types: &'static str },
    /// A fixed list (collection, favourites, playlist).
    Fixed,
}

#[derive(Clone, Copy)]
struct Drag {
    start_x: f32,
    start_pos: f32,
}

/// Frame time from the app clock (so recorded tours run at a fixed step), capped for a stalled frame.
fn frame_dt(last: &mut Option<f64>, now: f64) -> f32 {
    let dt = last.map(|l| (now - l) as f32).unwrap_or(0.0).clamp(0.0, 0.05);
    *last = Some(now);
    dt
}

/// One row on the back of the cover.
#[derive(Clone, Debug, PartialEq)]
enum Row {
    Play { from_start: bool, label: String },
    Audio(String),
    Subtitles(String),
    Season(String),
    Episode(usize),
}

#[derive(Default)]
struct Back {
    id: String,
    focus: usize,
    first: usize,
    audio: Option<i32>,
    subtitle: Option<Option<i32>>,
    tracks_for: Option<String>,
    seasons: Option<Vec<Item>>,
    season: usize,
    episodes: Option<(String, Vec<Item>)>,
    /// The season was picked by hand: keep the focus on the season row when its episodes arrive.
    picked: bool,
    error: Option<String>,
}

pub struct PosterFlow {
    src: Src,
    items: Vec<Item>,
    complete: bool,
    loading: bool,
    error: Option<String>,
    started: bool,
    pos: f32,
    vel: f32,
    target: f32,
    k: f32,
    drag: Option<Drag>,
    wheel: f32,
    flip: f32,
    flip_to: f32,
    full: HashMap<String, Item>,
    asked: HashSet<String>,
    back: Back,
    typed: (String, f64),
    last_now: Option<f64>,
    dt: f32,
    /// Cover outlines on screen, front first (for clicks).
    hits: Vec<(usize, [Pos2; 4])>,
    back_rect: Option<Rect>,
    /// FIX-2 #7: the back while it turns (item index, screen rect), for its title + year.
    turning: Option<(usize, Rect)>,
}

/// Poster Flow over a library. `items` are what the grid has loaded so far (in its order), `complete` when
/// that is all of them; `start` is the item to open on.
pub fn library(
    view_id: &str,
    query: LibraryQuery,
    types: &'static str,
    items: Vec<Item>,
    complete: bool,
    start: usize,
) -> Box<dyn Screen> {
    Box::new(PosterFlow::new(Src::Library { view_id: view_id.to_string(), query, types }, items, complete, start))
}

/// Poster Flow over a fixed list (collection, favourites, playlist).
pub fn items(items: Vec<Item>, start: usize) -> Box<dyn Screen> {
    Box::new(PosterFlow::new(Src::Fixed, items, true, start))
}

impl PosterFlow {
    fn new(src: Src, items: Vec<Item>, complete: bool, start: usize) -> PosterFlow {
        let start = start.min(items.len().saturating_sub(1)) as f32;
        PosterFlow {
            src,
            items,
            complete,
            loading: false,
            error: None,
            started: false,
            pos: start,
            vel: 0.0,
            target: start,
            k: GLIDE_K,
            drag: None,
            wheel: 0.0,
            flip: 0.0,
            flip_to: 0.0,
            full: HashMap::new(),
            asked: HashSet::new(),
            back: Back::default(),
            typed: (String::new(), 0.0),
            last_now: None,
            dt: 0.0,
            hits: vec![],
            back_rect: None,
            turning: None,
        }
    }

    fn last(&self) -> f32 {
        self.items.len().saturating_sub(1) as f32
    }

    fn centre(&self) -> usize {
        (self.pos.round().max(0.0) as usize).min(self.items.len().saturating_sub(1))
    }

    fn flipped(&self) -> bool {
        self.flip_to > 0.5
    }

    /// Glide to cover `i` (ease-out, lands exactly on it).
    fn go(&mut self, i: f32) {
        self.target = i.round().clamp(0.0, self.last());
        self.k = GLIDE_K;
    }

    /// Arrow step; held arrows queue ahead, so the glide speeds up the longer they are held.
    fn step(&mut self, n: f32) {
        let t = (self.target + n).clamp(self.pos - MAX_AHEAD, self.pos + MAX_AHEAD);
        self.go(t);
    }

    /// Let go after a drag: friction picks the landing cover, the glide starts at the finger's speed.
    fn fling(&mut self, v: f32) {
        self.target = (self.pos + v / FRICTION).round().clamp(0.0, self.last());
        let dist = self.target - self.pos;
        self.k = if dist.abs() > 0.01 && v * dist > 0.0 { (v / dist).clamp(2.5, 14.0) } else { GLIDE_K };
    }

    fn advance(&mut self, dt: f32) {
        if self.drag.is_none() {
            let np = self.target - (self.target - self.pos) * (-self.k * dt).exp();
            self.vel = if dt > 0.0 { (np - self.pos) / dt } else { 0.0 };
            self.pos = np;
            if (self.target - self.pos).abs() < 0.0005 {
                self.pos = self.target;
                self.vel = 0.0;
                self.k = GLIDE_K;
            }
        }
        let step = dt / FLIP_SECS;
        if self.flip_to > self.flip {
            self.flip = (self.flip + step).min(self.flip_to);
        } else if self.flip_to < self.flip {
            self.flip = (self.flip - step).max(self.flip_to);
        }
    }

    fn moving(&self) -> bool {
        self.drag.is_some() || self.pos != self.target || self.flip != self.flip_to
    }

    fn start_flip(&mut self, cx: &mut Cx) {
        let Some(item) = self.items.get(self.centre()).cloned() else { return };
        self.go(self.centre() as f32);
        if self.back.id != item.id {
            self.back = Back { id: item.id.clone(), ..Back::default() };
        }
        self.back.focus = 0;
        self.flip_to = 1.0;
        self.want_full(cx, &item);
        if item.kind == ItemKind::Series && self.back.seasons.is_none() {
            let c = cx.api();
            let id = item.id.clone();
            cx.spawn("pf-seasons", move || (id.clone(), c.seasons(&id)));
        }
    }

    fn want_full(&mut self, cx: &Cx, item: &Item) {
        if self.asked.insert(item.id.clone()) {
            let c = cx.api();
            let id = item.id.clone();
            cx.spawn("pf-full", move || (id.clone(), c.item(&id)));
        }
    }

    fn load_season(&mut self, cx: &Cx) {
        let Some(s) = self.back.seasons.as_ref().and_then(|v| v.get(self.back.season)).cloned() else { return };
        if self.back.episodes.as_ref().is_some_and(|(id, _)| *id == s.id) {
            return;
        }
        self.back.episodes = None;
        let c = cx.api();
        let series = self.back.id.clone();
        cx.spawn("pf-eps", move || (s.id.clone(), c.episodes(&series, Some(&s.id))));
    }

    fn load_all(&mut self, cx: &Cx) {
        let Src::Library { view_id, query, types } = &self.src else { return };
        let (c, id, q, types) = (cx.api(), view_id.clone(), query.clone(), *types);
        self.loading = true;
        cx.spawn("pf-all", move || -> Result<Vec<Item>> {
            let mut v: Vec<Item> = vec![];
            loop {
                let p = c.library_query_types(&id, v.len(), PAGE, &q, types)?;
                let n = p.items.len();
                v.extend(p.items);
                if n == 0 || v.len() >= p.total_record_count {
                    break Ok(v);
                }
            }
        });
    }

    // ---------- the back of the cover ----------

    fn back_item(&self) -> Option<&Item> {
        let it = self.items.iter().find(|i| i.id == self.back.id)?;
        Some(self.full.get(&it.id).unwrap_or(it))
    }

    fn init_tracks(&mut self, cx: &Cx) {
        let Some(it) = self.full.get(&self.back.id) else { return };
        if self.back.tracks_for.as_deref() == Some(it.id.as_str()) {
            return;
        }
        let ms = it.media_sources.first();
        let first_audio =
            ms.and_then(|m| m.media_streams.iter().find(|s| s.kind == StreamKind::Audio)).map(|s| s.index);
        self.back.audio = ms.and_then(|m| m.default_audio_stream_index).or(first_audio);
        let remembered = cx.remembered_subtitle(it);
        self.back.subtitle = Some(ms.and_then(|m| {
            crate::playreq::default_subtitle(&cx.settings.subtitles, m, self.back.audio, remembered.as_ref())
        }));
        self.back.tracks_for = Some(it.id.clone());
    }

    /// The tracks of one kind, with the names a person reads ("German", "English (embedded)"), as the player shows them.
    fn streams(&self, kind: StreamKind) -> Vec<(String, i32)> {
        let Some(ms) = self.full.get(&self.back.id).and_then(|i| i.media_sources.first()) else { return vec![] };
        let v: Vec<_> = ms.media_streams.iter().filter(|s| s.kind == kind).cloned().collect();
        player::track_labels(&v).into_iter().zip(v.iter().map(|s| s.index)).collect()
    }

    fn rows(&self) -> Vec<Row> {
        let Some(it) = self.back_item() else { return vec![] };
        if it.kind == ItemKind::Series {
            let mut v = vec![];
            if let Some(s) = self.back.seasons.as_ref().and_then(|s| s.get(self.back.season)) {
                v.push(Row::Season(s.name.clone()));
            }
            if let Some((_, eps)) = &self.back.episodes {
                v.extend((0..eps.len()).map(Row::Episode));
            }
            return v;
        }
        let mut v = vec![];
        let pos = it.user_data.as_ref().map(|u| u.playback_position_ticks).unwrap_or(0);
        if resumable(pos) {
            v.push(Row::Play { from_start: false, label: format!("Resume from {}", parts::clock(ticks_to_secs(pos))) });
            v.push(Row::Play { from_start: true, label: "Play from start".into() });
        } else {
            v.push(Row::Play { from_start: true, label: "Play".into() });
        }
        if self.full.contains_key(&it.id) {
            let audio = self.streams(StreamKind::Audio);
            let a = audio.iter().find(|(_, i)| Some(*i) == self.back.audio).map(|(l, _)| l.clone());
            v.push(Row::Audio(a.unwrap_or_else(|| "Default".into())));
            let subs = self.streams(StreamKind::Subtitle);
            let s = match self.back.subtitle {
                Some(Some(i)) => {
                    subs.iter().find(|(_, x)| *x == i).map(|(l, _)| l.clone()).unwrap_or_else(|| "Default".into())
                }
                _ => "Off".into(),
            };
            v.push(Row::Subtitles(s));
        }
        v
    }

    /// Enter on a back row (`dir` = ±1 for Left / Right on a choice row).
    fn activate(&mut self, cx: &mut Cx, row: &Row, dir: i32) {
        match row {
            Row::Play { from_start, .. } => {
                let Some(it) = self.back_item().cloned() else { return };
                let opts = PlayOptions {
                    from_start: *from_start,
                    audio: self.back.audio,
                    subtitle: self.back.subtitle,
                    ..Default::default()
                };
                cx.push(player::screen(it, opts));
            }
            Row::Audio(_) => {
                let v: Vec<i32> = self.streams(StreamKind::Audio).into_iter().map(|(_, i)| i).collect();
                if !v.is_empty() {
                    let cur = v.iter().position(|&i| Some(i) == self.back.audio).unwrap_or(0) as i32;
                    self.back.audio = Some(v[(cur + dir).rem_euclid(v.len() as i32) as usize]);
                }
            }
            Row::Subtitles(_) => {
                let mut v: Vec<Option<i32>> = vec![None];
                v.extend(self.streams(StreamKind::Subtitle).into_iter().map(|(_, i)| Some(i)));
                let cur = v.iter().position(|&i| Some(i) == self.back.subtitle).unwrap_or(0) as i32;
                let pick = v[(cur + dir).rem_euclid(v.len() as i32) as usize];
                self.back.subtitle = Some(pick);
                // FIX-4: the viewer's pick is remembered like one on the film page.
                if let Some(it) = self.full.get(&self.back.id) {
                    cx.remember_subtitle(it, it.media_sources.first(), pick);
                }
            }
            Row::Season(_) => {
                let n = self.back.seasons.as_ref().map(|s| s.len()).unwrap_or(0) as i32;
                if n > 0 {
                    self.back.season = (self.back.season as i32 + dir).rem_euclid(n) as usize;
                    self.back.picked = true;
                    self.load_season(cx);
                }
            }
            Row::Episode(i) => {
                if let Some(ep) = self.back.episodes.as_ref().and_then(|(_, e)| e.get(*i)).cloned() {
                    cx.push(player::screen(ep, PlayOptions::default()));
                }
            }
        }
    }

    fn back_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        let rows = self.rows();
        let n = rows.len();
        let is_play = |i: usize| matches!(rows.get(i), Some(Row::Play { .. }));
        let f = self.back.focus;
        match key {
            // The Play pills sit side by side in one row: Up/Down skip across it, Left/Right move along it.
            Key::Up if f > 0 && is_play(f - 1) => self.back.focus = 0,
            Key::Up => self.back.focus = f.saturating_sub(1),
            Key::Down if is_play(f) => self.back.focus = (0..n).find(|&i| !is_play(i)).unwrap_or(f),
            Key::Down => self.back.focus = (f + 1).min(n.saturating_sub(1)),
            Key::Left | Key::Right if is_play(f) => {
                let to = if key == Key::Left { f.saturating_sub(1) } else { f + 1 };
                if is_play(to) {
                    self.back.focus = to;
                }
            }
            Key::Left | Key::Right => {
                let dir = if key == Key::Left { -1 } else { 1 };
                if let Some(r) = rows.get(f) {
                    if matches!(r, Row::Audio(_) | Row::Subtitles(_) | Row::Season(_)) {
                        self.activate(cx, &r.clone(), dir);
                    }
                }
            }
            Key::Enter | Key::PlayPause => {
                if let Some(r) = rows.get(f).cloned() {
                    self.activate(cx, &r, 1);
                }
            }
            Key::Back | Key::Info => self.flip_to = 0.0,
            _ => return false,
        }
        true
    }

    // ---------- drawing ----------

    /// The cover at index `i` placed for this frame (with the flip on the flipped one).
    fn placed(&self, i: usize, cam: &Cam) -> Placed {
        let d = i as f32 - self.pos;
        let p = pose(d);
        let e = ease(self.flip);
        // Neighbours make room for the bigger back.
        // Neighbours step aside by the back's extra half-width (scaled to their depth), as on the iPod.
        let room = (BACK_SCALE - COVER_W) / 2.0 * CAM_DIST / (CAM_DIST - CENTRE_Z);
        let push = e * room * d.clamp(-1.0, 1.0);
        let mut pl = Placed { x: p.x + push, y: 0.0, z: p.z, angle: p.angle, sx: 1.0, sy: 1.0 };
        if self.flip > 0.0 && self.items.get(i).is_some_and(|it| it.id == self.back.id) {
            pl.angle += std::f32::consts::PI * e;
            pl.sx = 1.0 + (BACK_SCALE / COVER_W - 1.0) * e;
            pl.sy = 1.0 + (BACK_SCALE - 1.0) * e;
            // Lift so the grown back's top sits just under the view top.
            let w0 = CAM_DIST - pl.z;
            let top_px = cam.view.top() + cam.view.height() * BACK_TOP_FRAC;
            let top_world = (cam.centre.y - top_px) * w0 / cam.f;
            let lift = top_world - BACK_SCALE / 2.0;
            pl.y = lift * e;
        }
        pl
    }

    /// The back's panel in GL while it turns. FIX-2 #7: the item's own poster, dimmed (title + year go on top in
    /// `draw_turning`), so the turn never shows a blank grey slab; a plain surface card only when there is no art.
    /// Once it faces you, egui draws the full card with its rounded corners and contents, so the GL one steps aside.
    fn back_quads(
        &self,
        t: &Theme,
        cam: &Cam,
        pl: &Placed,
        tex: Option<egui::TextureId>,
        out: &mut Vec<Quad>,
    ) -> [Pos2; 4] {
        let c = if tex.is_some() { [BACK_DIM, BACK_DIM, BACK_DIM, 1.0] } else { rgba(t.surface) };
        // Seen from behind: local x runs the other way (so the poster reads the right way round).
        let (q, s) = quad(cam, pl, (0.5, -0.5), (0.5, -0.5), tex, [c; 4]);
        if self.flip < 1.0 {
            out.push(q);
        }
        s
    }

    fn draw_flow(&mut self, ui: &mut egui::Ui, cx: &mut Cx, cam: &Cam) {
        let t = cx.theme;
        let n = self.items.len();
        if n == 0 {
            return;
        }
        let ppp = ui.ctx().pixels_per_point();
        let hi_w = image_width_bucket(cam.cover_w() * 1.05, ppp);
        // Visible covers either side: enough to reach past the view edge.
        let stride_px = cam.f * STRIDE / CAM_DIST;
        let side = (((cam.view.width() / 2.0) / stride_px).ceil() as usize + 2).min(40);
        let c = self.centre();
        let lo_i = c.saturating_sub(side);
        let hi_i = (c + side).min(n - 1);
        let mut order: Vec<usize> = (lo_i..=hi_i).collect();
        // Far ones first; the flipped / centre cover last (on top).
        order.sort_by(|a, b| {
            let da = (*a as f32 - self.pos).abs();
            let db = (*b as f32 - self.pos).abs();
            db.partial_cmp(&da).unwrap_or(std::cmp::Ordering::Equal)
        });
        let client = cx.client.clone();
        let white = rgba(Color32::WHITE);
        let placeholder = rgba(t.skeleton);
        let mut quads = vec![];
        let mut hits = vec![];
        self.back_rect = None;
        self.turning = None;
        for &i in &order {
            let item = &self.items[i];
            let pl = self.placed(i, cam);
            // Image: the sharp one if it is in, else the grid's small one, else a placeholder.
            let hi_url = client.as_ref().and_then(|c| c.image_url(item, ImageKind::Primary, hi_w));
            let lo_url = parts::card_image_url(cx, item, parts::CardKind::Poster, ppp);
            let mut tex = hi_url.as_ref().and_then(|u| cx.images.get(u));
            if tex.is_none() {
                tex = lo_url.as_ref().and_then(|u| cx.images.get(u));
            }
            let col = if tex.is_some() { white } else { placeholder };
            let showing_back = pl.angle.cos() < 0.0 && pl.sy > 1.0;
            if showing_back {
                let s = self.back_quads(t, cam, &pl, tex, &mut quads);
                hits.push((i, s));
                if self.flip >= 1.0 {
                    self.back_rect = Some(Rect::from_two_pos(s[0], s[2]).intersect(cam.view));
                } else {
                    self.turning = Some((i, Rect::from_two_pos(s[0], s[2]).intersect(cam.view)));
                }
            } else {
                quads.push(reflection(cam, &pl, tex, col));
                let (q, s) = quad(cam, &pl, (-0.5, 0.5), (0.5, -0.5), tex, [col; 4]);
                quads.push(q);
                hits.push((i, s));
            }
        }
        hits.reverse();
        self.hits = hits;
        // Keep ±PREFETCH warm behind what is on screen.
        if let Some(cl) = client.as_ref() {
            for i in c.saturating_sub(PREFETCH)..=(c + PREFETCH).min(n - 1) {
                if let Some(u) = cl.image_url(&self.items[i], ImageKind::Primary, hi_w) {
                    cx.images.prefetch(&u);
                }
            }
        }
        ui.painter().add(pgl::callback(cam.view, quads));
    }

    fn draw_labels(&mut self, ui: &egui::Ui, cx: &Cx, cam: &Cam) {
        let t = cx.theme;
        let Some(item) = self.items.get(self.centre()) else { return };
        let off = (self.pos - self.pos.round()).abs();
        let a = ((1.0 - off * 2.5) * (1.0 - ease(self.flip) * 2.0)).clamp(0.0, 1.0);
        if a <= 0.0 {
            return;
        }
        let (title, sub) = labels(item, self.full.get(&item.id));
        let rows = if sub.is_empty() { 1.3 } else { 2.6 };
        let vh = cam.view.height();
        let bottom = cam.centre.y + cam.cover_h() / 2.0;
        let y = bottom + vh * 0.035;
        let p = ui.painter();
        let ts = vh * t.flow_label_frac;
        p.text(
            pos2(cam.view.center().x, y),
            Align2::CENTER_TOP,
            title,
            t.font_w(ts, Weight::Bold),
            t.text.gamma_multiply(a),
        );
        if !sub.is_empty() {
            p.text(
                pos2(cam.view.center().x, y + ts * 1.3),
                Align2::CENTER_TOP,
                sub,
                t.font_w(vh * t.flow_sub_frac, Weight::Bold),
                t.flow_sub.gamma_multiply(a),
            );
        }
        // FIX-1 #17: the position, so a named item can be found without guessing the steps.
        p.text(
            pos2(cam.view.center().x, y + ts * rows),
            Align2::CENTER_TOP,
            position_label(self.centre(), self.items.len(), self.complete),
            t.font_w(t.body_size, Weight::SemiBold),
            t.muted.gamma_multiply(a),
        );
    }

    /// FIX-2 #7: title and year centred on the back while it turns, from the first frame of the flip.
    fn draw_turning(&self, ui: &egui::Ui, cx: &Cx) {
        let t = cx.theme;
        let Some((i, r)) = self.turning else { return };
        let Some(item) = self.items.get(i) else { return };
        let (title, sub) = labels(item, self.full.get(&item.id));
        let p = ui.painter().with_clip_rect(r);
        let tr = p.text(r.center(), Align2::CENTER_BOTTOM, title, t.font_w(t.heading_size, Weight::Bold), t.text);
        if !sub.is_empty() {
            p.text(
                pos2(r.center().x, tr.bottom() + t.gap / 4.0),
                Align2::CENTER_TOP,
                sub,
                t.font(t.body_size),
                t.muted,
            );
        }
    }

    /// The flipped back, in the app's own look (docs/DESIGN-appletv.md): a rounded card with the backdrop fading in
    /// at the top, title and metadata on one left edge, then Play pills and setting rows (film) or a season
    /// control and episode rows (show). Every row shares the same column and the focused one is the white plate.
    fn draw_back(&mut self, ui: &mut egui::Ui, cx: &mut Cx, r: Rect) {
        let t = cx.theme;
        self.init_tracks(cx);
        let Some(it) = self.back_item().cloned() else { return };
        let ppp = ui.ctx().pixels_per_point();
        let p = ui.painter().clone();
        p.add(t.shadow(1.0).as_shape(r, t.dialog_radius));
        p.rect_filled(r, t.dialog_radius, t.surface);
        let pad = t.gap * 1.25;
        let x = r.left() + pad;
        let col_w = r.width() - 2.0 * pad;

        // Backdrop band, cover-cropped to the card's width, fading into the card.
        let band = Rect::from_min_size(r.min, vec2(r.width(), r.width() * 0.4));
        let url =
            cx.client.as_ref().and_then(|c| c.image_url(&it, ImageKind::Backdrop, image_width_bucket(r.width(), ppp)));
        if let Some(tex) = url.and_then(|u| cx.images.get(&u)) {
            let h = (band.height() / band.width()) * (16.0 / 9.0);
            let uv = Rect::from_min_max(pos2(0.0, (1.0 - h) / 2.0), pos2(1.0, (1.0 + h) / 2.0));
            let radius = t.dialog_radius as u8;
            egui::Image::new((tex, band.size()))
                .uv(uv)
                .corner_radius(egui::CornerRadius { nw: radius, ne: radius, sw: 0, se: 0 })
                .paint_at(ui, band);
            let fade = Rect::from_min_max(pos2(band.left(), band.top() + band.height() * 0.3), band.max);
            vgrad(&p, fade, t.surface.gamma_multiply(0.0), t.surface);
        }

        // Title and metadata, bottom-left of the band.
        let (_, meta) = labels(&it, Some(&it));
        let tg = p.layout_no_wrap(it.name.clone(), t.font_w(t.title_size, Weight::Bold), t.text);
        let tg = clip_galley(ui, tg, col_w);
        let mg = p.layout_no_wrap(meta, t.font(t.body_size), t.muted);
        let mg = clip_galley(ui, mg, col_w);
        let meta_y = band.bottom() - mg.size().y;
        let title_y = meta_y - t.gap / 4.0 - tg.size().y;
        p.galley(pos2(x, title_y), tg, t.text);
        p.galley(pos2(x, meta_y), mg, t.muted);
        let mut y = band.bottom() + pad * 0.75;

        let rows = self.rows();
        let pointer_moved = ui.input(|i| i.pointer.is_moving());
        let mut hovered = None;
        let mut clicked = None;
        let row_h = t.button_h + t.gap / 4.0;
        let font = t.font_w(t.body_size, Weight::Medium);

        if it.kind != ItemKind::Series {
            if let Some(ov) = it.overview.as_deref().filter(|s| !s.is_empty()) {
                let mut job = egui::text::LayoutJob::simple(ov.to_string(), t.font(t.body_size), t.muted, col_w);
                job.wrap = egui::text::TextWrapping {
                    max_width: col_w,
                    max_rows: 3,
                    break_anywhere: false,
                    overflow_character: Some('…'),
                };
                let g = ui.fonts(|f| f.layout_job(job));
                let h = g.size().y;
                p.galley(pos2(x, y), g, t.muted);
                y += h + pad * 0.75;
            }
            // Play pills, side by side.
            let plays: Vec<(usize, String)> = rows
                .iter()
                .enumerate()
                .filter_map(|(i, r)| match r {
                    Row::Play { label, .. } => Some((i, label.clone())),
                    _ => None,
                })
                .collect();
            let area = Rect::from_min_size(pos2(x, y), vec2(col_w, t.pill_h));
            ui.scope_builder(egui::UiBuilder::new().max_rect(area), |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = t.gap / 2.0;
                    for (k, (i, label)) in plays.iter().enumerate() {
                        let focused = self.back.focus == *i;
                        let resp = if k == 0 {
                            widgets::play_pill_for(ui, t, label, focused, parts::is_tv(ui.ctx()))
                        } else {
                            widgets::pill_button(ui, t, label, focused, false)
                        };
                        if resp.hovered() {
                            hovered = Some(*i);
                        }
                        if resp.clicked() {
                            clicked = Some(*i);
                        }
                    }
                });
            });
            y += t.pill_h + pad * 0.5;
            // Audio / Subtitles rows.
            for (i, row) in rows.iter().enumerate() {
                let (k, v) = match row {
                    Row::Audio(v) => ("Audio", v),
                    Row::Subtitles(v) => ("Subtitles", v),
                    _ => continue,
                };
                let rr = Rect::from_min_size(pos2(x - t.menu_pad, y), vec2(col_w + 2.0 * t.menu_pad, t.button_h));
                let (h, c, ink) = setting_row(ui, t, rr, i, self.back.focus == i);
                hovered = hovered.or(h);
                clicked = clicked.or(c);
                let cy = rr.center().y;
                p.text(pos2(x, cy), Align2::LEFT_CENTER, k, font.clone(), ink);
                let vg = p.layout_no_wrap(v.clone(), font.clone(), ink);
                let vg = clip_galley(ui, vg, col_w * 0.6);
                let chev = t.body_size * 0.8;
                let vx = x + col_w - chev - t.gap / 4.0 - vg.size().x;
                p.galley(pos2(vx, cy - vg.size().y / 2.0), vg, ink);
                p.text(pos2(x + col_w, cy), Align2::RIGHT_CENTER, "›", font.clone(), ink);
                y += row_h;
            }
        } else {
            // Season control: the app's segmented control (a pill with arrows when there are many).
            let focused = matches!(rows.get(self.back.focus), Some(Row::Season(_)));
            if let Some(seasons) = self.back.seasons.clone() {
                let area = Rect::from_min_size(pos2(x, y), vec2(col_w, t.tab_h));
                let mut pick = None;
                ui.scope_builder(egui::UiBuilder::new().max_rect(area), |ui| {
                    if seasons.len() <= 6 {
                        let labels: Vec<String> = seasons.iter().map(|s| s.name.clone()).collect();
                        let out = widgets::tabs(ui, t, &labels, self.back.season, focused.then_some(self.back.season));
                        if out.hovered.is_some() {
                            hovered = Some(0);
                        }
                        pick = out.clicked;
                    } else if let Some(s) = seasons.get(self.back.season) {
                        let resp = widgets::pill_button(ui, t, &format!("‹  {}  ›", s.name), focused, false);
                        if resp.hovered() {
                            hovered = Some(0);
                        }
                        if resp.clicked() {
                            clicked = Some(0);
                        }
                    }
                });
                if let Some(i) = pick {
                    self.back.season = i;
                    self.back.picked = true;
                    self.back.focus = 0;
                    self.load_season(cx);
                }
                y += t.tab_h + pad * 0.5;
            }
            // Episodes: number · title · runtime, one column each, scrolled to keep the focus in view.
            let eps = self.back.episodes.as_ref().map(|(_, e)| e.clone()).unwrap_or_default();
            let room = ((r.bottom() - pad * 0.5 - y) / row_h).floor().max(1.0) as usize;
            let ep_focus = self.back.focus.saturating_sub(1);
            if self.back.focus >= 1 {
                if ep_focus < self.back.first {
                    self.back.first = ep_focus;
                }
                if ep_focus >= self.back.first + room {
                    self.back.first = ep_focus + 1 - room;
                }
            }
            let num_w = t.body_size * 3.0;
            let dur_w = t.body_size * 4.5;
            for (k, ep) in eps.iter().enumerate().skip(self.back.first).take(room) {
                let i = k + 1;
                let rr = Rect::from_min_size(pos2(x - t.menu_pad, y), vec2(col_w + 2.0 * t.menu_pad, t.button_h));
                let (h, c, ink) = setting_row(ui, t, rr, i, self.back.focus == i);
                hovered = hovered.or(h);
                clicked = clicked.or(c);
                let cy = rr.center().y;
                let focused = self.back.focus == i;
                let dim = if focused { ink } else { t.muted };
                let num = ep.index_number.map(|n| n.to_string()).unwrap_or_default();
                p.text(pos2(x, cy), Align2::LEFT_CENTER, num, font.clone(), dim);
                let played = ep.user_data.as_ref().is_some_and(|u| u.played);
                let g = p.layout_no_wrap(ep.name.clone(), font.clone(), if played && !focused { t.muted } else { ink });
                let g = clip_galley(ui, g, col_w - num_w - dur_w);
                p.galley(pos2(x + num_w, cy - g.size().y / 2.0), g, ink);
                let dur = ep.run_time_ticks.map(|x| parts::runtime(ticks_to_secs(x))).unwrap_or_default();
                p.text(pos2(x + col_w, cy), Align2::RIGHT_CENTER, dur, font.clone(), dim);
                y += row_h;
            }
            if eps.is_empty() {
                let msg = self.back.error.clone().unwrap_or_else(|| "Loading…".into());
                p.text(pos2(x, y + row_h / 2.0), Align2::LEFT_CENTER, msg, t.font(t.body_size), t.muted);
            }
        }
        if pointer_moved {
            if let Some(i) = hovered {
                self.back.focus = i;
            }
        }
        if let Some(i) = clicked {
            if let Some(row) = rows.get(i).cloned() {
                self.back.focus = i;
                self.activate(cx, &row, 1);
            }
        }
    }

    /// The round ⓘ at bottom right (flips the centre cover, as on the iPod).
    fn info_button(&mut self, ui: &mut egui::Ui, cx: &mut Cx, view: Rect) {
        let t = cx.theme;
        // The iPod's ⓘ: an eighth of the screen height, in the bottom-right corner.
        let d = view.height() * 0.11;
        let c = pos2(view.right() - d * 0.75, view.bottom() - d * 0.75);
        let r = Rect::from_center_size(c, vec2(d, d));
        let resp = ui.interact(r, ui.id().with("pf-info"), Sense::click());
        let p = ui.painter();
        p.circle_filled(c, d / 2.0, if resp.hovered() { t.glass_hi } else { t.glass });
        p.text(c, Align2::CENTER_CENTER, "i", t.font_w(d * 0.6, Weight::Bold), t.text);
        if resp.clicked() {
            if self.flipped() {
                self.flip_to = 0.0;
            } else {
                self.start_flip(cx);
            }
        }
    }

    fn pointer(&mut self, ui: &mut egui::Ui, cx: &mut Cx, cam: &Cam, view: Rect) {
        let resp = ui.interact(view, ui.id().with("pf-stage"), Sense::click_and_drag());
        let px_per = (cam.cover_w() * DRAG_PX).max(1.0);
        if !self.flipped() {
            if resp.drag_started() {
                if let Some(p) = resp.interact_pointer_pos() {
                    self.drag = Some(Drag { start_x: p.x, start_pos: self.pos });
                    self.vel = 0.0;
                }
            }
            if let (Some(d), Some(p)) = (self.drag, resp.interact_pointer_pos()) {
                // Follows the finger; a little give past either end. Speed: smoothed over the last frames.
                let np = (d.start_pos - (p.x - d.start_x) / px_per).clamp(-0.4, self.last() + 0.4);
                if self.dt > 0.0 && np != self.pos {
                    self.vel = self.vel * 0.4 + (np - self.pos) / self.dt * 0.6;
                } else {
                    self.vel *= 0.85;
                }
                self.pos = np;
                self.target = self.pos;
            }
            if resp.drag_stopped() && self.drag.take().is_some() {
                self.fling(self.vel);
            }
            if resp.hovered() {
                let s = ui.input(|i| i.raw_scroll_delta);
                let d = if s.x.abs() > s.y.abs() { -s.x } else { -s.y };
                self.wheel += d / WHEEL_PX;
                while self.wheel >= 1.0 {
                    self.wheel -= 1.0;
                    self.step(1.0);
                }
                while self.wheel <= -1.0 {
                    self.wheel += 1.0;
                    self.step(-1.0);
                }
            }
        }
        if resp.clicked() {
            let at = resp.interact_pointer_pos().unwrap_or_default();
            if self.flipped() {
                if !self.back_rect.is_some_and(|r| r.contains(at)) {
                    self.flip_to = 0.0;
                }
                return;
            }
            if let Some(&(i, _)) = self.hits.iter().find(|(_, q)| inside(q, at)) {
                if i == self.centre() && (self.pos - self.target).abs() < 0.5 {
                    self.start_flip(cx);
                } else {
                    self.go(i as f32);
                }
            }
        }
    }

    /// Home / End and type-ahead letters (egui events: the shell maps letters to nothing else here).
    fn typing(&mut self, ui: &egui::Ui, cx: &Cx) {
        if self.flipped() {
            return;
        }
        let (home, end, text) = ui.input(|i| {
            let text: String = i
                .events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Text(s) => Some(s.clone()),
                    _ => None,
                })
                .collect();
            (i.key_pressed(egui::Key::Home), i.key_pressed(egui::Key::End), text)
        });
        if home {
            self.go(0.0);
        }
        if end {
            self.go(self.last());
        }
        let letters: String = text.chars().filter(|c| c.is_alphanumeric()).collect();
        if !letters.is_empty() {
            if cx.now - self.typed.1 > TYPE_GAP {
                self.typed.0.clear();
            }
            self.typed.0.push_str(&letters);
            self.typed.1 = cx.now;
            if let Some(i) = type_ahead(&self.items, &self.typed.0) {
                self.go(i as f32);
            }
        }
    }
}

/// A one-line galley cut to `max_w` with an ellipsis.
fn clip_galley(ui: &egui::Ui, g: std::sync::Arc<egui::Galley>, max_w: f32) -> std::sync::Arc<egui::Galley> {
    if g.size().x <= max_w || max_w <= 0.0 {
        return g;
    }
    let mut job = (*g.job).clone();
    job.wrap =
        egui::text::TextWrapping {
            max_width: max_w, max_rows: 1, break_anywhere: true, overflow_character: Some('…')
        };
    ui.fonts(|f| f.layout_job(job))
}

/// A vertical gradient over `r` (top colour → bottom colour).
fn vgrad(p: &egui::Painter, r: Rect, top: Color32, bottom: Color32) {
    let mut m = egui::Mesh::default();
    for (pt, c) in [(r.left_top(), top), (r.right_top(), top), (r.right_bottom(), bottom), (r.left_bottom(), bottom)] {
        m.colored_vertex(pt, c);
    }
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    p.add(m);
}

/// A full-width row on the back: transparent at rest, the white focus plate when focused (no lift inside the card,
/// like the app's lists). Returns (hovered row, clicked row, ink).
fn setting_row(ui: &egui::Ui, t: &Theme, rr: Rect, i: usize, focused: bool) -> (Option<usize>, Option<usize>, Color32) {
    let resp = ui.interact(rr, ui.id().with(("pf-row", i)), Sense::click());
    let a = widgets::focus_amount(ui, t, resp.id, focused);
    let rest = if resp.hovered() { t.glass } else { Color32::TRANSPARENT };
    let ink = widgets::plate(ui, t, rr, t.field_radius, rest, a, 1.0);
    (resp.hovered().then_some(i), resp.clicked().then_some(i), ink)
}

impl Screen for PosterFlow {
    fn name(&self) -> &'static str {
        "posterflow"
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        if self.flipped() {
            return self.back_key(cx, key);
        }
        match key {
            Key::Left => self.step(-1.0),
            Key::Right => self.step(1.0),
            Key::Enter | Key::Info => self.start_flip(cx),
            _ => return false,
        }
        true
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn std::any::Any + Send>) {
        match tag {
            "pf-all" => {
                self.loading = false;
                let Ok(r) = payload.downcast::<Result<Vec<Item>>>() else { return };
                match *r {
                    Ok(mut v) => {
                        if let Src::Library { view_id, query, .. } = &self.src {
                            match query.sort {
                                SortBy::StoryOrder => crate::story::sort_story(&mut v),
                                SortBy::MyOrder => crate::order::sort_my_order(&mut v, &crate::tags::place_id(view_id)),
                                _ => {}
                            }
                        }
                        // Stay on the same film while the list grows.
                        let keep = self.items.get(self.centre()).map(|i| i.id.clone());
                        self.items = v;
                        self.complete = true;
                        if let Some(i) = keep.and_then(|id| self.items.iter().position(|x| x.id == id)) {
                            let d = i as f32 - self.pos.round();
                            self.pos += d;
                            self.target += d;
                        }
                        self.target = self.target.clamp(0.0, self.last());
                        self.pos = self.pos.clamp(0.0, self.last());
                    }
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            "pf-full" => {
                let Ok(r) = payload.downcast::<(String, Result<Item>)>() else { return };
                match r.1 {
                    Ok(it) => {
                        self.full.insert(r.0, it);
                    }
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(_) => {
                        self.asked.remove(&r.0);
                    }
                }
            }
            "pf-seasons" => {
                let Ok(r) = payload.downcast::<(String, Result<Vec<Item>>)>() else { return };
                if r.0 != self.back.id {
                    return;
                }
                match r.1 {
                    Ok(s) => {
                        // Start on the first real season (specials last), as the show page does.
                        self.back.season = s.iter().position(|x| x.index_number.unwrap_or(0) > 0).unwrap_or(0);
                        self.back.seasons = Some(s);
                        self.load_season(cx);
                    }
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(e) => self.back.error = Some(e.to_string()),
                }
            }
            "pf-eps" => {
                let Ok(r) = payload.downcast::<(String, Result<Vec<Item>>)>() else { return };
                let want = self.back.seasons.as_ref().and_then(|s| s.get(self.back.season)).map(|s| s.id.clone());
                if want.as_deref() != Some(r.0.as_str()) {
                    return;
                }
                match r.1 {
                    Ok(e) => {
                        // Focus the next one to watch (the iPod's ▶ row).
                        let next = e.iter().position(|x| !x.user_data.as_ref().is_some_and(|u| u.played)).unwrap_or(0);
                        self.back.episodes = Some((r.0, e));
                        if self.back.focus == 0 && !self.back.picked {
                            self.back.focus = next + 1;
                        }
                    }
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(e) => self.back.error = Some(e.to_string()),
                }
            }
            _ => {}
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        if !self.started {
            self.started = true;
            if !self.complete {
                self.load_all(cx);
            }
        }
        let t = cx.theme;
        // Full window, edge to edge (the iPod's flow fills the screen).
        let view = ui.ctx().screen_rect();
        ui.set_clip_rect(view);
        ui.painter().rect_filled(view, 0.0, t.flow_bg);
        self.dt = frame_dt(&mut self.last_now, cx.now);
        let dt = self.dt;
        let cam = Cam::new(view);
        self.pointer(ui, cx, &cam, view);
        self.typing(ui, cx);
        self.advance(dt);
        if self.items.is_empty() {
            let msg = match (&self.error, self.loading) {
                (Some(e), _) => e.clone(),
                (None, true) => "Loading…".to_string(),
                _ => "Nothing here.".to_string(),
            };
            ui.painter().text(view.center(), Align2::CENTER_CENTER, msg, t.font(t.body_size), t.muted);
            return;
        }
        // Fetch the centre's details once it has settled (labels' season count, a ready back).
        if self.pos == self.target {
            if let Some(it) = self.items.get(self.centre()).cloned() {
                self.want_full(cx, &it);
            }
        }
        self.draw_flow(ui, cx, &cam);
        self.draw_labels(ui, cx, &cam);
        self.draw_turning(ui, cx);
        if let Some(r) = self.back_rect {
            self.draw_back(ui, cx, r);
        }
        self.info_button(ui, cx, view);
        if self.moving() {
            ui.ctx().request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(n: &str) -> Item {
        Item { name: n.into(), ..Default::default() }
    }

    #[test]
    fn centre_cover_is_flat_and_forward_side_covers_turned() {
        let c = pose(0.0);
        assert_eq!((c.x, c.angle), (0.0, 0.0));
        assert!((c.z - CENTRE_Z).abs() < 1e-6);
        let sx = side_x(COVER_W);
        let r = pose(1.0);
        assert!((r.angle - SIDE_ANGLE_DEG.to_radians()).abs() < 1e-6 && r.z == 0.0 && (r.x - sx).abs() < 1e-6);
        let l = pose(-3.0);
        assert!((l.x + sx + 2.0 * STRIDE).abs() < 1e-6 && l.angle < 0.0);
        // Half-way: half-turned, half-forward.
        let h = pose(0.5);
        assert!((h.angle - SIDE_ANGLE_DEG.to_radians() / 2.0).abs() < 1e-6 && (h.z - CENTRE_Z / 2.0).abs() < 1e-6);
    }

    #[test]
    fn the_model_reproduces_the_reference_still() {
        // Square covers (the iPod's): first side cover at the measured 0.82 …
        assert!((side_x(1.0) - 0.82).abs() < 0.02, "{}", side_x(1.0));
        // … and its outer edge taller than its inner edge by the measured 194 / 148.
        let half = SIDE_ANGLE_DEG.to_radians().sin() / 2.0;
        let ratio = (CAM_DIST + half) / (CAM_DIST - half);
        assert!((ratio - 194.0 / 148.0).abs() < 0.03, "{ratio}");
        // The centre poster is COVER_FRAC of the view tall.
        let cam = Cam::new(Rect::from_min_size(Pos2::ZERO, vec2(1920.0, 1080.0)));
        assert!((cam.cover_h() - 1080.0 * COVER_FRAC).abs() < 0.5);
    }

    #[test]
    fn flick_lands_exactly_on_a_cover() {
        let mut f = PosterFlow::new(Src::Fixed, (0..50).map(|i| named(&format!("F{i}"))).collect(), true, 10);
        f.pos = 10.3;
        f.fling(9.0);
        assert_eq!(f.target, (10.3f32 + 3.0).round());
        for _ in 0..240 {
            f.advance(1.0 / 60.0);
        }
        assert_eq!(f.pos, f.target);
        // The flip opens and stays open (no jitter once there).
        f.flip_to = 1.0;
        for _ in 0..40 {
            f.advance(1.0 / 30.0);
        }
        assert_eq!(f.flip, 1.0);
        f.advance(1.0 / 30.0);
        assert_eq!(f.flip, 1.0);
        // Never past either end.
        f.fling(-500.0);
        assert_eq!(f.target, 0.0);
    }

    #[test]
    fn held_arrows_queue_but_stay_bounded() {
        let mut f = PosterFlow::new(Src::Fixed, (0..100).map(|i| named(&format!("F{i}"))).collect(), true, 0);
        for _ in 0..40 {
            f.step(1.0);
        }
        assert_eq!(f.target, MAX_AHEAD);
        f.advance(0.3);
        assert!(f.pos > MAX_AHEAD * 0.9);
    }

    #[test]
    fn type_ahead_skips_the_and_falls_forward() {
        let v: Vec<Item> = ["Alpha", "The Bravo", "Charlie", "Delta"].iter().map(|n| named(n)).collect();
        assert_eq!(type_ahead(&v, "b"), Some(1));
        assert_eq!(type_ahead(&v, "ch"), Some(2));
        assert_eq!(type_ahead(&v, "cz"), Some(3));
        assert_eq!(type_ahead(&v, "z"), None);
    }

    #[test]
    fn labels_for_films_and_shows() {
        let film = Item {
            name: "Star Drift".into(),
            kind: ItemKind::Movie,
            production_year: Some(2019),
            run_time_ticks: Some(5520 * 10_000_000),
            ..Default::default()
        };
        assert_eq!(labels(&film, None), ("Star Drift".into(), "2019 · 1h 32m".into()));
        let show = Item {
            name: "Iron Tide".into(),
            kind: ItemKind::Series,
            child_count: Some(3),
            production_year: Some(2008),
            ..Default::default()
        };
        assert_eq!(labels(&show, None).1, "2008 · 3 seasons");
        assert_eq!(position_label(6, 15, true), "7 of 15");
        assert_eq!(position_label(0, 50, false), "1 of 50+");
    }

    #[test]
    fn resume_only_after_a_minute() {
        assert!(!resumable(0));
        assert!(!resumable(30 * 10_000_000));
        assert!(resumable(60 * 10_000_000));
    }

    #[test]
    fn hit_test_inside_a_quad() {
        let q = [pos2(0.0, 0.0), pos2(10.0, 2.0), pos2(10.0, 8.0), pos2(0.0, 10.0)];
        assert!(inside(&q, pos2(5.0, 5.0)));
        assert!(!inside(&q, pos2(11.0, 5.0)));
        assert!(!inside(&q, pos2(9.0, 1.0)));
    }
}

/// FIX-4: a subtitle picked on the back of a cover is remembered like one on the film page.
#[cfg(test)]
mod fix4_tests {
    use super::*;
    use crate::app::test_support::World;

    // fixture: 2 eng audio (default), 3 fre audio, 0 ger external sub, 4 eng embedded sub
    fn flipped(w: &mut World) -> PosterFlow {
        let info: crate::api::PlaybackInfo = serde_json::from_str(
            &std::fs::read_to_string(format!("{}/tests/fixtures/playbackinfo.json", env!("CARGO_MANIFEST_DIR")))
                .unwrap(),
        )
        .unwrap();
        let mut it: Item = serde_json::from_str(r#"{"Id":"i","Name":"Film","Type":"Movie"}"#).unwrap();
        it.media_sources = info.media_sources;
        let mut f = PosterFlow::new(Src::Fixed, vec![it.clone()], true, 0);
        f.back.id = it.id.clone();
        f.full.insert(it.id.clone(), it);
        w.cx(|cx| f.init_tracks(cx));
        f
    }

    #[test]
    fn a_poster_flow_pick_is_remembered_after_a_restart() {
        let mut w = World::signed_in(true);
        let mut f = flipped(&mut w);
        assert_eq!(f.back.subtitle, Some(None), "eng audio → Off");
        w.cx(|cx| f.activate(cx, &Row::Subtitles(String::new()), 1));
        assert_eq!(f.back.subtitle, Some(Some(0)));
        w.restart();
        assert_eq!(flipped(&mut w).back.subtitle, Some(Some(0)), "flipped again after a restart: the German sub");
    }
}
