//! Apple TV look for the player overlay (D4, docs/DESIGN-appletv.md §5): title top-left over a soft top shade,
//! a bottom shade with the scrubber (elapsed left, remaining right, trickplay preview above the playhead), a row
//! of round icon buttons (transport left, options right, the focused option captioned underneath), volume
//! under the transport, and every menu as a translucent card that slides up above the button that opened it.
//!
//! Everything here is drawing from plain data: the player screen keeps its state, focus groups and key
//! handling, and feeds these parts what to show; they hand back x centres (for `nav::move_focus`), hover and
//! clicks. `gallery_screen` shows every part with test data (`--screen gallery-player`).
use super::widgets;
use crate::app::{Cx, Screen};
use crate::nav::{move_focus, Focus, Group};
use crate::platform::Key;
use crate::theme::{Theme, Weight};
use egui::{pos2, vec2, Align2, Color32, Id, Mesh, Pos2, Rect, Response, Sense, Shape, Stroke, TextureId, Ui};

// ---------- icons ----------

/// Player icons, drawn with the painter (no icon font, no Apple artwork).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PIcon {
    Play,
    Pause,
    SkipBack(u32),
    SkipForward(u32),
    Previous,
    Next,
    Audio,
    Subtitles,
    Chapters,
    Quality,
    Speed,
    Aspect,
    Repeat,
    RepeatOne,
    Queue,
    Stats,
    Info,
    Fullscreen,
    ExitFullscreen,
    /// Speaker with 0–2 sound waves.
    Volume(u8),
    Muted,
    Minus,
    Plus,
}

fn arc(c: Pos2, r: f32, a0: f32, a1: f32) -> Vec<Pos2> {
    let n = 24;
    (0..=n).map(|i| a0 + (a1 - a0) * i as f32 / n as f32).map(|a| pos2(c.x + r * a.cos(), c.y + r * a.sin())).collect()
}

fn tri(p: &egui::Painter, pts: [Pos2; 3], ink: Color32) {
    p.add(Shape::convex_polygon(pts.to_vec(), ink, Stroke::NONE));
}

/// Draws `icon` centred on `c` in a box `s` wide.
pub fn picon(ui: &Ui, t: &Theme, icon: PIcon, c: Pos2, s: f32, ink: Color32) {
    let p = ui.painter();
    let h = s / 2.0;
    let st = Stroke::new(t.icon_stroke, ink);
    let line = |a: Pos2, b: Pos2| p.line_segment([a, b], st);
    match icon {
        PIcon::Play => widgets::icon(ui, t, widgets::Icon::Play, c, s * 0.8, ink),
        PIcon::Pause => {
            let w = s * 0.24;
            for dx in [-s * 0.21, s * 0.21] {
                let r = Rect::from_center_size(pos2(c.x + dx, c.y), vec2(w, s * 0.8));
                p.rect_filled(r, w / 3.0, ink);
            }
        }
        PIcon::SkipBack(n) | PIcon::SkipForward(n) => {
            let back = matches!(icon, PIcon::SkipBack(_));
            let r = h * 0.92;
            let gap = 0.9_f32;
            let top = -std::f32::consts::FRAC_PI_2;
            let (a0, a1) = if back { (top - gap / 2.0, top + gap / 2.0 - std::f32::consts::TAU) } else { (top + gap / 2.0, top - gap / 2.0 + std::f32::consts::TAU) };
            let pts = arc(c, r, a1, a0);
            let tip = pts[pts.len() - 1];
            p.add(Shape::line(pts, st));
            // Arrow head at the open end of the circle, pointing along the turn.
            let dir = if back { 1.0 } else { -1.0 };
            let k = s * 0.2;
            tri(p, [pos2(tip.x - dir * k, tip.y), pos2(tip.x + dir * k * 0.4, tip.y - k), pos2(tip.x + dir * k * 0.4, tip.y + k)], ink);
            p.text(c + vec2(0.0, s * 0.04), Align2::CENTER_CENTER, n.to_string(), t.font_w(s * 0.34, Weight::Bold), ink);
        }
        PIcon::Previous | PIcon::Next => {
            let d = if icon == PIcon::Next { 1.0 } else { -1.0 };
            let bar = Rect::from_center_size(pos2(c.x + d * h * 0.62, c.y), vec2(s * 0.12, s * 0.7));
            p.rect_filled(bar, s * 0.04, ink);
            tri(p, [pos2(c.x - d * h * 0.62, c.y - h * 0.7), pos2(c.x + d * h * 0.42, c.y), pos2(c.x - d * h * 0.62, c.y + h * 0.7)], ink);
        }
        PIcon::Audio => {
            for (i, f) in [0.35, 0.75, 1.0, 0.6, 0.3].iter().enumerate() {
                let x = c.x + (i as f32 - 2.0) * s * 0.2;
                let hh = s * 0.85 * f / 2.0;
                p.line_segment([pos2(x, c.y - hh), pos2(x, c.y + hh)], Stroke::new(t.icon_stroke * 1.2, ink));
            }
        }
        PIcon::Subtitles => {
            let r = Rect::from_center_size(c, vec2(s, s * 0.74));
            p.rect_stroke(r, s * 0.14, st, egui::StrokeKind::Middle);
            let y1 = c.y + s * 0.05;
            let y2 = c.y + s * 0.2;
            line(pos2(r.left() + s * 0.2, y1), pos2(c.x + s * 0.05, y1));
            line(pos2(c.x + s * 0.17, y1), pos2(r.right() - s * 0.2, y1));
            line(pos2(r.left() + s * 0.2, y2), pos2(c.x - s * 0.1, y2));
            line(pos2(c.x + s * 0.02, y2), pos2(r.right() - s * 0.2, y2));
        }
        PIcon::Chapters | PIcon::Queue => {
            for (i, dy) in [-0.32, 0.0, 0.32].iter().enumerate() {
                let y = c.y + dy * s;
                if icon == PIcon::Queue && i == 0 {
                    tri(p, [pos2(c.x - h, y - s * 0.13), pos2(c.x - h + s * 0.2, y), pos2(c.x - h, y + s * 0.13)], ink);
                } else if icon == PIcon::Chapters {
                    p.circle_filled(pos2(c.x - h + s * 0.06, y), t.icon_stroke, ink);
                }
                let x0 = if icon == PIcon::Queue && i == 0 { c.x - h + s * 0.32 } else { c.x - h + s * 0.26 };
                line(pos2(x0, y), pos2(c.x + h, y));
            }
        }
        PIcon::Quality => {
            let r = Rect::from_center_size(c, vec2(s, s * 0.7));
            p.rect_stroke(r, s * 0.12, st, egui::StrokeKind::Middle);
            p.text(c, Align2::CENTER_CENTER, "HD", t.font_w(s * 0.36, Weight::Bold), ink);
        }
        PIcon::Speed => {
            let cc = pos2(c.x, c.y + s * 0.18);
            p.add(Shape::line(arc(cc, h * 0.95, std::f32::consts::PI * 1.05, std::f32::consts::PI * 1.95), st));
            let a = -std::f32::consts::FRAC_PI_4;
            line(cc, pos2(cc.x + h * 0.7 * a.cos(), cc.y + h * 0.7 * a.sin()));
            p.circle_filled(cc, t.icon_stroke * 1.3, ink);
        }
        PIcon::Aspect => {
            let r = Rect::from_center_size(c, vec2(s, s * 0.66));
            p.rect_stroke(r, s * 0.1, st, egui::StrokeKind::Middle);
            let i = r.shrink(s * 0.18);
            let k = s * 0.14;
            p.add(Shape::line(vec![pos2(i.left(), i.top() + k), i.left_top(), pos2(i.left() + k, i.top())], st));
            p.add(Shape::line(vec![pos2(i.right(), i.bottom() - k), i.right_bottom(), pos2(i.right() - k, i.bottom())], st));
        }
        PIcon::Repeat | PIcon::RepeatOne => {
            let y = s * 0.26;
            let k = s * 0.14;
            p.add(Shape::line(vec![pos2(c.x - h, c.y + s * 0.05), pos2(c.x - h, c.y - y), pos2(c.x + h * 0.7, c.y - y)], st));
            tri(p, [pos2(c.x + h, c.y - y), pos2(c.x + h - k * 1.4, c.y - y - k), pos2(c.x + h - k * 1.4, c.y - y + k)], ink);
            p.add(Shape::line(vec![pos2(c.x + h, c.y - s * 0.05), pos2(c.x + h, c.y + y), pos2(c.x - h * 0.7, c.y + y)], st));
            tri(p, [pos2(c.x - h, c.y + y), pos2(c.x - h + k * 1.4, c.y + y - k), pos2(c.x - h + k * 1.4, c.y + y + k)], ink);
            if icon == PIcon::RepeatOne {
                p.text(c, Align2::CENTER_CENTER, "1", t.font_w(s * 0.34, Weight::Bold), ink);
            }
        }
        PIcon::Stats => {
            for (i, f) in [0.45, 0.75, 1.0].iter().enumerate() {
                let x = c.x + (i as f32 - 1.0) * s * 0.32;
                let top = c.y + h * 0.85 - s * 0.85 * f;
                p.rect_filled(Rect::from_min_max(pos2(x - s * 0.09, top), pos2(x + s * 0.09, c.y + h * 0.85)), s * 0.03, ink);
            }
        }
        PIcon::Info => {
            p.circle_stroke(c, h * 0.95, st);
            p.circle_filled(pos2(c.x, c.y - s * 0.2), t.icon_stroke * 0.9, ink);
            line(pos2(c.x, c.y - s * 0.05), pos2(c.x, c.y + s * 0.26));
        }
        PIcon::Fullscreen | PIcon::ExitFullscreen => {
            let k = s * 0.3;
            for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                let corner = pos2(c.x + sx * h, c.y + sy * h * 0.8);
                let (a, b) = if icon == PIcon::Fullscreen {
                    (pos2(corner.x - sx * k, corner.y), pos2(corner.x, corner.y - sy * k))
                } else {
                    let inner = pos2(c.x + sx * h * 0.35, c.y + sy * h * 0.3);
                    (pos2(inner.x + sx * k, inner.y), pos2(inner.x, inner.y + sy * k))
                };
                let mid = if icon == PIcon::Fullscreen { corner } else { pos2(c.x + sx * h * 0.35, c.y + sy * h * 0.3) };
                p.add(Shape::line(vec![a, mid, b], st));
            }
        }
        PIcon::Volume(_) | PIcon::Muted => {
            let x0 = c.x - h * 0.9;
            let body = vec![
                pos2(x0, c.y - s * 0.14),
                pos2(x0 + s * 0.2, c.y - s * 0.14),
                pos2(x0 + s * 0.46, c.y - s * 0.38),
                pos2(x0 + s * 0.46, c.y + s * 0.38),
                pos2(x0 + s * 0.2, c.y + s * 0.14),
                pos2(x0, c.y + s * 0.14),
            ];
            p.add(Shape::convex_polygon(body, ink, Stroke::NONE));
            let sc = pos2(x0 + s * 0.46, c.y);
            match icon {
                PIcon::Muted => {
                    let m = pos2(c.x + h * 0.55, c.y);
                    let k = s * 0.16;
                    line(pos2(m.x - k, m.y - k), pos2(m.x + k, m.y + k));
                    line(pos2(m.x - k, m.y + k), pos2(m.x + k, m.y - k));
                }
                PIcon::Volume(n) => {
                    for w in 0..n.min(2) {
                        let r = s * (0.2 + 0.18 * w as f32);
                        p.add(Shape::line(arc(sc, r, -0.9, 0.9), st));
                    }
                }
                _ => {}
            }
        }
        PIcon::Minus => line(pos2(c.x - h * 0.6, c.y), pos2(c.x + h * 0.6, c.y)),
        PIcon::Plus => widgets::icon(ui, t, widgets::Icon::Plus, c, s * 0.6, ink),
    }
}

// ---------- shades, motion ----------

/// A vertical gradient over `r` from `top` to `bottom` colour, eased so the fade has no visible edge.
pub fn shade(ui: &Ui, r: Rect, top: Color32, bottom: Color32) {
    let mut m = Mesh::default();
    let steps = 6;
    for i in 0..=steps {
        let f = i as f32 / steps as f32;
        let e = f * f * (3.0 - 2.0 * f); // smoothstep
        let c = top.lerp_to_gamma(bottom, e);
        let y = r.top() + r.height() * f;
        m.colored_vertex(pos2(r.left(), y), c);
        m.colored_vertex(pos2(r.right(), y), c);
        if i > 0 {
            let b = (i as u32 - 1) * 2;
            m.add_triangle(b, b + 1, b + 2);
            m.add_triangle(b + 1, b + 3, b + 2);
        }
    }
    ui.painter().add(Shape::mesh(m));
}

/// 0..1 eased "how far in" for a card that appears: 0 on the first frame it is drawn, 1 after `focus_time`.
/// A card not drawn for a frame starts again from 0 the next time.
pub fn appear(ctx: &egui::Context, t: &Theme, id: Id) -> f32 {
    let now = ctx.input(|i| i.time);
    let pass = ctx.cumulative_pass_nr();
    let (start, last) = ctx.data(|d| d.get_temp::<(f64, u64)>(id)).unwrap_or((now, 0));
    let start = if last + 1 < pass { now } else { start };
    ctx.data_mut(|d| d.insert_temp(id, (start, pass)));
    let a = ((now - start) as f32 / t.focus_time).clamp(0.0, 1.0);
    if a < 1.0 {
        ctx.request_repaint();
    }
    t.ease(a)
}

/// `secs` as the player shows it: `1:02:03` / `4:05`.
pub fn clock(secs: f64) -> String {
    super::parts::clock(secs)
}

/// The remaining-time label: `-1:02:03`.
pub fn remaining(pos: f64, dur: f64) -> String {
    format!("-{}", clock((dur - pos).max(0.0)))
}

// ---------- the OSD ----------

pub struct OsdButton {
    pub icon: PIcon,
    /// Shown under the button while it is focused (options only), e.g. "Audio · English".
    pub label: String,
}

/// What the scrubber previews above the playhead: a trickplay cell (texture + uv) or just the time.
pub struct Preview {
    pub secs: f64,
    pub image: Option<(TextureId, Rect)>,
}

pub struct OsdView<'a> {
    pub title: &'a str,
    pub subtitle: &'a str,
    /// A small chip next to the subtitle ("Transcoding"); empty for none.
    pub note: &'a str,
    pub pos: f64,
    pub dur: f64,
    pub buttons: &'a [OsdButton],
    /// The first `transport` buttons sit left (skip, play, previous/next); the rest right.
    pub transport: usize,
    /// Which button is play/pause (drawn larger).
    pub play_index: usize,
    pub focus_bar: bool,
    pub focus_btn: Option<usize>,
    pub focus_vol: Option<usize>,
    pub volume: f32,
    pub muted: bool,
    pub preview: Option<Preview>,
    /// A card is open over the controls: draw them dimmed.
    pub dim: bool,
}

#[derive(Default)]
pub struct OsdOut {
    pub bar_x: f32,
    pub btn_xs: Vec<f32>,
    pub btn_rects: Vec<Rect>,
    /// Top of the button row: cards open with their bottom just above it.
    pub row_top: f32,
    pub vol_xs: Vec<f32>,
    /// A click on the scrubber: the position 0..1.
    pub seek: Option<f32>,
    /// Where the pointer is over the scrubber, 0..1 (for the trickplay preview).
    pub bar_hover: Option<f32>,
    pub btn_hovered: Option<usize>,
    pub btn_clicked: Option<usize>,
    pub vol_hovered: Option<usize>,
    pub vol_clicked: Option<usize>,
    /// A click on the volume bar: the new volume 0..1.
    pub set_volume: Option<f32>,
}

/// One round icon button at `rect` (glass at rest, white plate when focused).
fn icon_button(ui: &Ui, t: &Theme, id: Id, rect: Rect, icon: PIcon, focused: bool) -> Response {
    let resp = ui.interact(rect, id, Sense::click());
    let a = widgets::focus_amount(ui, t, id, focused);
    let rest = if resp.hovered() { t.glass_hi } else { t.glass };
    let ink = widgets::plate(ui, t, rect, rect.height() / 2.0, rest, a, t.pill_focus_scale);
    picon(ui, t, icon, rect.center(), rect.height() * t.osd_icon, ink);
    resp
}

/// The whole OSD: shades, title, scrubber + times + preview, the button row and the volume row.
pub fn osd(ui: &mut Ui, t: &Theme, v: &OsdView) -> OsdOut {
    let screen = ui.ctx().screen_rect();
    let mut out = OsdOut::default();

    shade(ui, Rect::from_min_max(screen.left_top(), pos2(screen.right(), screen.top() + t.osd_top_shade_h)), t.osd_shade, Color32::TRANSPARENT);
    shade(ui, Rect::from_min_max(pos2(screen.left(), screen.bottom() - t.osd_shade_h), screen.right_bottom()), Color32::TRANSPARENT, t.osd_shade);

    let ui = &mut ui.new_child(egui::UiBuilder::new().max_rect(screen));
    if v.dim {
        ui.set_opacity(t.osd_dim);
    }

    // Title top-left: bold title, then the episode / year line with an optional chip.
    let p = ui.painter();
    let tr = p.text(pos2(screen.left() + t.margin, screen.top() + t.osd_top), Align2::LEFT_TOP, v.title, t.font(t.title_size), t.text);
    let sub_y = tr.bottom() + t.gap / 4.0;
    let mut x = screen.left() + t.margin;
    if !v.subtitle.is_empty() {
        let r = p.text(pos2(x, sub_y), Align2::LEFT_TOP, v.subtitle, t.font_w(t.body_size, Weight::Medium), t.muted);
        x = r.right() + t.gap / 2.0;
    }
    if !v.note.is_empty() {
        let g = ui.fonts(|f| f.layout_no_wrap(v.note.to_string(), t.font_w(t.small_size, Weight::SemiBold), t.text));
        let r = Rect::from_min_size(pos2(x, sub_y), vec2(g.size().x + 2.0 * t.badge_pad, t.badge_h));
        p.rect_filled(r, t.badge_h / 2.0, t.glass);
        p.galley(pos2(r.left() + t.badge_pad, r.center().y - g.size().y / 2.0), g, t.text);
    }

    // Rows from the bottom up: volume, buttons, times, scrubber.
    let left = screen.left() + t.margin;
    let right = screen.right() - t.margin;
    let vol_rect = Rect::from_min_max(pos2(left, screen.bottom() - t.row_gap - t.button_h), pos2(right, screen.bottom() - t.row_gap));
    let row_h = t.osd_play_button;
    let row = Rect::from_min_max(pos2(left, vol_rect.top() - t.gap / 2.0 - row_h), pos2(right, vol_rect.top() - t.gap / 2.0));
    out.row_top = row.top();
    let times_y = row.top() - t.gap / 2.0;
    let bar_hit = Rect::from_min_max(pos2(left, times_y - t.small_size - t.gap / 4.0 - t.button_h), pos2(right, times_y - t.small_size - t.gap / 4.0));

    // Button row: transport from the left, options from the right.
    let n = v.buttons.len();
    let mut rects = vec![Rect::NOTHING; n];
    let mut x = left;
    for (i, r) in rects.iter_mut().enumerate().take(v.transport.min(n)) {
        let d = if i == v.play_index { t.osd_play_button } else { t.round_button };
        *r = Rect::from_center_size(pos2(x + d / 2.0, row.center().y), vec2(d, d));
        x += d + t.gap / 2.0;
    }
    let mut x = right;
    for r in rects.iter_mut().skip(v.transport.min(n)).rev() {
        let d = t.round_button;
        *r = Rect::from_center_size(pos2(x - d / 2.0, row.center().y), vec2(d, d));
        x -= d + t.gap / 2.0;
    }
    for (i, (b, r)) in v.buttons.iter().zip(&rects).enumerate() {
        let focused = v.focus_btn == Some(i);
        let resp = icon_button(ui, t, ui.id().with(("osd-btn", i)), *r, b.icon, focused);
        if resp.hovered() {
            out.btn_hovered = Some(i);
        }
        if resp.clicked() {
            out.btn_clicked = Some(i);
        }
        if focused && i >= v.transport && !b.label.is_empty() {
            let g = ui.fonts(|f| f.layout_no_wrap(b.label.clone(), t.font_w(t.small_size, Weight::SemiBold), t.text));
            let cx = (r.center().x).min(right - g.size().x / 2.0);
            ui.painter().galley(pos2(cx - g.size().x / 2.0, r.bottom() + t.gap / 2.0), g, t.text);
        }
    }
    out.btn_xs = rects.iter().map(|r| r.center().x).collect();
    out.btn_rects = rects;

    // Volume under the transport: speaker button, slim bar, percentage.
    let d = t.button_h;
    let mute_rect = Rect::from_min_size(pos2(left, vol_rect.top()), vec2(d, d));
    let level = if v.volume <= 0.0 { 0 } else if v.volume < 0.5 { 1 } else { 2 };
    let icon = if v.muted { PIcon::Muted } else { PIcon::Volume(level) };
    let resp = icon_button(ui, t, ui.id().with("osd-mute"), mute_rect, icon, v.focus_vol == Some(0));
    if resp.hovered() {
        out.vol_hovered = Some(0);
    }
    if resp.clicked() {
        out.vol_clicked = Some(0);
    }
    let vbar_hit = Rect::from_min_size(pos2(mute_rect.right() + t.gap / 2.0, vol_rect.top()), vec2(t.volume_bar_w, d));
    let vresp = ui.interact(vbar_hit, ui.id().with("osd-vol"), Sense::click());
    let va = widgets::focus_amount(ui, t, vresp.id, v.focus_vol == Some(1) || vresp.hovered());
    let vh = t.scrub_h + (t.scrub_focus_h - t.scrub_h) * va;
    let vtrack = Rect::from_center_size(vbar_hit.center(), vec2(vbar_hit.width(), vh));
    let shown = if v.muted { 0.0 } else { v.volume };
    track(ui, t, vtrack, shown, va);
    ui.painter().text(
        pos2(vbar_hit.right() + t.gap / 2.0, vbar_hit.center().y),
        Align2::LEFT_CENTER,
        format!("{}%", (v.volume * 100.0).round() as i32),
        t.font_w(t.small_size, Weight::SemiBold),
        t.muted.lerp_to_gamma(t.text, va),
    );
    if vresp.hovered() {
        out.vol_hovered = Some(1);
    }
    if vresp.clicked() {
        if let Some(p) = vresp.interact_pointer_pos() {
            out.set_volume = Some(((p.x - vtrack.left()) / vtrack.width()).clamp(0.0, 1.0));
        }
    }
    out.vol_xs = vec![mute_rect.center().x, vbar_hit.center().x];

    // Scrubber.
    let bresp = ui.interact(bar_hit, ui.id().with("osd-bar"), Sense::click());
    let hover = bresp.hover_pos().map(|p| ((p.x - bar_hit.left()) / bar_hit.width()).clamp(0.0, 1.0));
    out.bar_hover = hover;
    if bresp.clicked() {
        out.seek = hover;
    }
    let ba = widgets::focus_amount(ui, t, bresp.id, v.focus_bar || hover.is_some());
    let bh = t.scrub_h + (t.scrub_focus_h - t.scrub_h) * ba;
    let bar = Rect::from_center_size(bar_hit.center(), vec2(bar_hit.width(), bh));
    let frac = if v.dur > 0.0 { (v.pos / v.dur).clamp(0.0, 1.0) as f32 } else { 0.0 };
    track(ui, t, bar, frac, ba);
    let head = pos2(bar.left() + bar.width() * frac, bar.center().y);
    if ba > 0.01 {
        let k = t.scrub_knob * ba;
        ui.painter().add(t.shadow(ba).as_shape(Rect::from_center_size(head, vec2(k, k)), k / 2.0));
        ui.painter().circle_filled(head, k / 2.0, t.focus_fill);
    }
    out.bar_x = head.x;

    // Times under the bar.
    let tf = t.font_w(t.small_size, Weight::SemiBold);
    ui.painter().text(pos2(left, times_y), Align2::LEFT_BOTTOM, clock(v.pos), tf.clone(), t.text);
    ui.painter().text(pos2(right, times_y), Align2::RIGHT_BOTTOM, remaining(v.pos, v.dur), tf.clone(), t.muted);

    // Preview above the bar: a trickplay frame with its time, or a time bubble.
    if let Some(pv) = &v.preview {
        let f = if v.dur > 0.0 { (pv.secs / v.dur).clamp(0.0, 1.0) as f32 } else { 0.0 };
        let px = bar.left() + bar.width() * f;
        if (px - head.x).abs() > 1.0 {
            let m = Rect::from_center_size(pos2(px, bar.center().y), vec2(t.icon_stroke, t.scrub_knob));
            ui.painter().rect_filled(m, t.icon_stroke / 2.0, t.text);
        }
        let label = clock(pv.secs);
        let g = ui.fonts(|fo| fo.layout_no_wrap(label, tf.clone(), t.text));
        let bottom = bar.top() - t.gap / 2.0;
        match pv.image {
            Some((tex, uv)) => {
                let w = t.trick_w;
                let h = t.trick_h;
                let x = px.clamp(left + w / 2.0, right - w / 2.0);
                let label_r = Rect::from_center_size(pos2(x, bottom - g.size().y / 2.0), g.size());
                let img = Rect::from_center_size(pos2(x, label_r.top() - t.gap / 4.0 - h / 2.0), vec2(w, h));
                ui.painter().add(t.shadow(1.0).as_shape(img, t.card_radius));
                egui::Image::new((tex, img.size())).uv(uv).corner_radius(t.card_radius).paint_at(ui, img);
                ui.painter().rect_stroke(img, t.card_radius, Stroke::new(t.badge_stroke, t.bar_stroke), egui::StrokeKind::Inside);
                ui.painter().galley(label_r.min, g, t.text);
            }
            None => {
                let size = g.size() + vec2(2.0 * t.badge_pad, 2.0 * t.badge_pad);
                let x = px.clamp(left + size.x / 2.0, right - size.x / 2.0);
                let r = Rect::from_center_size(pos2(x, bottom - size.y / 2.0), size);
                ui.painter().rect_filled(r, size.y / 2.0, t.menu_fill);
                ui.painter().galley(r.center() - g.size() / 2.0, g, t.text);
            }
        }
    }
    out
}

/// A rounded track with its filled part (scrubber, volume, progress). `a` = focus, brightens the track.
fn track(ui: &Ui, t: &Theme, r: Rect, frac: f32, a: f32) {
    let rad = r.height() / 2.0;
    ui.painter().rect_filled(r, rad, t.progress_track.lerp_to_gamma(t.glass_hi, a));
    if frac > 0.0 {
        let mut done = r;
        done.set_right(r.left() + (r.width() * frac).max(r.height()));
        ui.painter().rect_filled(done, rad, t.progress);
    }
}

// ---------- cards ----------

/// A translucent card that slides up into place: bottom centre at `anchor` (kept on screen), `width` wide,
/// with an optional title. `id` must be stable while the card stays open.
pub fn card<R>(ctx: &egui::Context, t: &Theme, id: &str, anchor: Pos2, align: Align2, width: f32, title: Option<&str>, add: impl FnOnce(&mut Ui) -> R) -> R {
    let id = Id::new(("at-player-card", id));
    let a = appear(ctx, t, id);
    let pos = anchor + vec2(0.0, (1.0 - a) * t.gap);
    egui::Area::new(id)
        .order(egui::Order::Foreground)
        .pivot(align)
        .fixed_pos(pos)
        .constrain_to(ctx.screen_rect().shrink2(vec2(t.margin, t.gap)))
        .show(ctx, |ui| {
            ui.set_opacity(a);
            egui::Frame::new()
                .fill(t.menu_fill)
                .stroke(Stroke::new(1.0, t.bar_stroke))
                .corner_radius(t.menu_radius)
                .shadow(t.shadow(1.0))
                .inner_margin(t.menu_pad)
                .show(ui, |ui| {
                    ui.set_width(width - 2.0 * t.menu_pad);
                    ui.spacing_mut().item_spacing = vec2(t.gap / 4.0, t.gap / 8.0);
                    if let Some(title) = title {
                        ui.add_space(t.gap / 8.0);
                        ui.label(egui::RichText::new(title).font(t.font_w(t.small_size, Weight::SemiBold)).color(t.muted));
                        ui.add_space(t.gap / 8.0);
                    }
                    add(ui)
                })
                .inner
        })
        .inner
}

/// Where a card opened by a button sits: bottom centre just above the button row, over that button.
pub fn card_anchor(t: &Theme, out_row_top: f32, button_x: f32) -> Pos2 {
    pos2(button_x, out_row_top - t.gap / 2.0)
}

pub struct MenuRow {
    pub label: String,
    /// Right-aligned extra (a time, "Default", "Played").
    pub detail: String,
    pub selected: bool,
    /// Greyed (played queue items).
    pub dim: bool,
}

#[derive(Default)]
pub struct ListOut {
    pub xs: Vec<f32>,
    pub hovered: Option<usize>,
    pub clicked: Option<usize>,
}

/// One menu row: tick column, label, detail; the focused row is a white plate (no lift inside menus).
pub fn menu_row(ui: &mut Ui, t: &Theme, row: &MenuRow, focused: bool) -> Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, t.menu_item_h), Sense::click());
    let a = widgets::focus_amount(ui, t, resp.id, focused);
    let rest = if resp.hovered() { t.glass } else { Color32::TRANSPARENT };
    let ink = widgets::plate(ui, t, rect, t.field_radius, rest, a, 1.0);
    let ink = if row.dim { ink.lerp_to_gamma(t.muted, 1.0 - a) } else { ink };
    let tick_w = t.body_size + t.gap / 4.0;
    let x = rect.left() + t.menu_pad / 2.0;
    if row.selected {
        widgets::icon(ui, t, widgets::Icon::Tick, pos2(x + t.body_size / 2.0, rect.center().y), t.body_size * 0.8, ink);
    }
    let font = if row.selected { t.font_w(t.body_size, Weight::SemiBold) } else { t.font(t.body_size) };
    let mut dx = 0.0;
    if !row.detail.is_empty() {
        let r = ui.painter().text(pos2(rect.right() - t.menu_pad / 2.0, rect.center().y), Align2::RIGHT_CENTER, &row.detail, t.font(t.small_size), t.muted.lerp_to_gamma(ink, a * 0.6));
        dx = r.width() + t.gap / 2.0;
    }
    let clip = Rect::from_min_max(rect.min, pos2(rect.right() - t.menu_pad / 2.0 - dx, rect.bottom()));
    ui.painter().with_clip_rect(clip).text(pos2(x + tick_w, rect.center().y), Align2::LEFT_CENTER, &row.label, font, ink);
    resp
}

/// A list of menu rows (scrolls past `menu_max_h`, keeping the focused row in view).
pub fn menu_list(ui: &mut Ui, t: &Theme, rows: &[MenuRow], focus: Option<usize>) -> ListOut {
    let mut out = ListOut::default();
    let mut body = |ui: &mut Ui| {
        for (i, row) in rows.iter().enumerate() {
            let focused = focus == Some(i);
            let r = menu_row(ui, t, row, focused);
            if focused {
                ui.scroll_to_rect(r.rect, None);
            }
            out.xs.push(r.rect.center().x);
            if r.hovered() {
                out.hovered = Some(i);
            }
            if r.clicked() {
                out.clicked = Some(i);
            }
        }
    };
    let full = rows.len() as f32 * (t.menu_item_h + ui.spacing().item_spacing.y);
    if full > t.menu_max_h {
        egui::ScrollArea::vertical().max_height(t.menu_max_h).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| body(ui));
    } else {
        body(ui);
    }
    out
}

/// A labelled "− value +" pair of small round buttons (subtitle offset, position). Focus index 0 = −, 1 = +.
pub fn stepper(ui: &mut Ui, t: &Theme, label: &str, value: &str, focus: Option<usize>) -> ListOut {
    let mut out = ListOut::default();
    ui.horizontal(|ui| {
        ui.set_min_height(t.button_h);
        let lw = t.form_label_w / 2.0;
        let (lr, _) = ui.allocate_exact_size(vec2(lw, t.button_h), Sense::hover());
        ui.painter().text(lr.left_center(), Align2::LEFT_CENTER, label, t.font(t.body_size), t.muted);
        for (i, icon) in [PIcon::Minus, PIcon::Plus].into_iter().enumerate() {
            if i == 1 {
                let (vr, _) = ui.allocate_exact_size(vec2(t.button_h * 2.0, t.button_h), Sense::hover());
                ui.painter().text(vr.center(), Align2::CENTER_CENTER, value, t.font_w(t.body_size, Weight::SemiBold), t.text);
            }
            let (r, _) = ui.allocate_exact_size(vec2(t.button_h, t.button_h), Sense::hover());
            let resp = icon_button(ui, t, ui.id().with((label, i)), r.shrink(t.toggle_inset), icon, focus == Some(i));
            out.xs.push(r.center().x);
            if resp.hovered() {
                out.hovered = Some(i);
            }
            if resp.clicked() {
                out.clicked = Some(i);
            }
        }
    });
    out
}

// ---------- the other overlays ----------

/// "Skip intro" style pill, bottom right, lifted above the controls while the OSD shows.
pub fn skip_pill(ctx: &egui::Context, t: &Theme, label: &str, focused: bool, lift: f32) -> bool {
    let mut clicked = false;
    let a = appear(ctx, t, Id::new("at-player-skip"));
    egui::Area::new(Id::new("player-skip"))
        .anchor(Align2::RIGHT_BOTTOM, vec2(-t.margin, -(t.row_gap + lift) + (1.0 - a) * t.gap))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui.set_opacity(a);
            clicked = widgets::pill_button(ui, t, label, focused, false).clicked();
        });
    clicked
}

pub struct UpNext<'a> {
    pub title: &'a str,
    pub subtitle: &'a str,
    /// Seconds left before it starts; `None` = waits for "Play now".
    pub countdown: Option<i64>,
    pub image: Option<TextureId>,
}

/// The up-next card, bottom right: thumbnail, title, countdown, "Play now" / "Cancel". Focus 0 / 1.
pub fn up_next(ui: &mut Ui, t: &Theme, u: &UpNext, focus: usize) -> ListOut {
    let screen = ui.ctx().screen_rect();
    shade(ui, Rect::from_min_max(pos2(screen.left(), screen.bottom() - t.osd_shade_h), screen.right_bottom()), Color32::TRANSPARENT, t.osd_shade);
    let mut out = ListOut::default();
    let w = t.trick_w + 2.0 * t.menu_pad;
    card(ui.ctx(), t, "upnext", pos2(screen.right() - t.margin, screen.bottom() - t.row_gap), Align2::RIGHT_BOTTOM, w, Some("Up Next"), |ui| {
        let (r, _) = ui.allocate_exact_size(vec2(t.trick_w, t.trick_h), Sense::hover());
        match u.image {
            Some(tex) => {
                egui::Image::new((tex, r.size())).corner_radius(t.card_radius).paint_at(ui, r);
            }
            None => {
                ui.painter().rect_filled(r, t.card_radius, t.glass);
                picon(ui, t, PIcon::Play, r.center(), t.round_button / 2.0, t.muted);
            }
        }
        if let Some(n) = u.countdown {
            // Countdown ring on the thumbnail.
            let c = pos2(r.right() - t.round_button / 2.0 - t.gap / 4.0, r.top() + t.round_button / 2.0 + t.gap / 4.0);
            let rad = t.round_button / 2.0 - t.icon_stroke;
            ui.painter().circle_filled(c, t.round_button / 2.0, t.scrim);
            ui.painter().circle_stroke(c, rad, Stroke::new(t.icon_stroke, t.progress_track));
            let left = (n as f32 / t.up_next_countdown).clamp(0.0, 1.0);
            let top = -std::f32::consts::FRAC_PI_2;
            ui.painter().add(Shape::line(arc(c, rad, top, top + left * std::f32::consts::TAU), Stroke::new(t.icon_stroke, t.text)));
            ui.painter().text(c, Align2::CENTER_CENTER, n.to_string(), t.font_w(t.body_size, Weight::Bold), t.text);
        }
        ui.add_space(t.gap / 4.0);
        ui.label(egui::RichText::new(u.title).font(t.font_w(t.body_size, Weight::SemiBold)).color(t.text));
        if !u.subtitle.is_empty() {
            ui.label(egui::RichText::new(u.subtitle).font(t.font(t.small_size)).color(t.muted));
        }
        ui.add_space(t.gap / 4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = t.gap / 4.0;
            let bw = (t.trick_w - t.gap / 4.0) / 2.0;
            for (i, l) in ["Play now", "Cancel"].iter().enumerate() {
                let r = widgets::pill_button_width(ui, t, l, focus == i, false, bw);
                out.xs.push(r.rect.center().x);
                if r.hovered() {
                    out.hovered = Some(i);
                }
                if r.clicked() {
                    out.clicked = Some(i);
                }
            }
        });
    });
    out
}

/// Stats card, top right: label / value rows.
pub fn stats(ctx: &egui::Context, t: &Theme, rows: &[(&str, String)]) {
    let screen = ctx.screen_rect();
    card(ctx, t, "stats", pos2(screen.right() - t.margin, screen.top() + t.osd_top), Align2::RIGHT_TOP, t.player_card_w, Some("Playback info"), |ui| {
        egui::Grid::new("at-player-stats").num_columns(2).spacing(vec2(t.gap / 2.0, t.gap / 8.0)).show(ui, |ui| {
            for (label, value) in rows {
                ui.label(egui::RichText::new(*label).font(t.font(t.small_size)).color(t.muted));
                ui.label(egui::RichText::new(value).font(t.font_w(t.small_size, Weight::SemiBold)).color(t.text));
                ui.end_row();
            }
        });
    });
}

pub struct InfoView<'a> {
    pub title: &'a str,
    pub meta: &'a str,
    pub overview: &'a str,
    pub image: Option<TextureId>,
    /// Keyboard shortcuts: (keys, what).
    pub keys: &'a [(&'a str, &'a str)],
}

/// The info card, bottom left above the controls: still, title, metadata, overview, then the keys.
pub fn info(ctx: &egui::Context, t: &Theme, v: &InfoView, row_top: f32) {
    let screen = ctx.screen_rect();
    let w = (screen.width() - 2.0 * t.margin).min(t.info_card_w);
    card(ctx, t, "info", pos2(screen.left() + t.margin, row_top - t.gap / 2.0), Align2::LEFT_BOTTOM, w, None, |ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = t.gap * 0.75;
            let (r, _) = ui.allocate_exact_size(vec2(t.trick_w, t.trick_h), Sense::hover());
            match v.image {
                Some(tex) => {
                    egui::Image::new((tex, r.size())).corner_radius(t.card_radius).paint_at(ui, r);
                }
                None => {
                    ui.painter().rect_filled(r, t.card_radius, t.glass);
                }
            }
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = t.gap / 6.0;
                ui.label(egui::RichText::new(v.title).font(t.font_w(t.heading_size, Weight::Bold)).color(t.text));
                if !v.meta.is_empty() {
                    ui.label(egui::RichText::new(v.meta).font(t.font_w(t.small_size, Weight::Medium)).color(t.muted));
                }
                if !v.overview.is_empty() {
                    ui.add_space(t.gap / 8.0);
                    ui.add(egui::Label::new(egui::RichText::new(v.overview).font(t.font(t.body_size)).color(t.text)).wrap());
                }
            });
        });
        if !v.keys.is_empty() {
            ui.add_space(t.gap / 2.0);
            ui.label(egui::RichText::new("Keys").font(t.font_w(t.small_size, Weight::SemiBold)).color(t.muted));
            let cols = 3;
            egui::Grid::new("at-player-keys").num_columns(cols * 2).spacing(vec2(t.gap / 2.0, t.gap / 8.0)).show(ui, |ui| {
                for (i, (k, what)) in v.keys.iter().enumerate() {
                    ui.label(egui::RichText::new(*k).font(t.font_w(t.small_size, Weight::SemiBold)).color(t.text));
                    ui.label(egui::RichText::new(*what).font(t.font(t.small_size)).color(t.muted));
                    if i % cols == cols - 1 {
                        ui.end_row();
                    }
                }
            });
        }
    });
}

/// Loading: a spinner and a quiet label in the middle of the black screen.
pub fn loading(ui: &mut Ui, t: &Theme) {
    let screen = ui.ctx().screen_rect();
    ui.painter().rect_filled(screen, 0.0, t.bg);
    let r = Rect::from_center_size(screen.center() - vec2(0.0, t.gap), vec2(t.round_button, t.round_button));
    ui.put(r, egui::Spinner::new().size(t.round_button * 0.75).color(t.muted));
    ui.painter().text(pos2(screen.center().x, r.bottom() + t.gap / 2.0), Align2::CENTER_TOP, "Loading…", t.font_w(t.body_size, Weight::Medium), t.muted);
}

/// An error card in the middle with a "Back" pill (always focused). Returns (pill x, clicked).
pub fn error(ui: &mut Ui, t: &Theme, msg: &str) -> (f32, bool) {
    let screen = ui.ctx().screen_rect();
    ui.painter().rect_filled(screen, 0.0, t.scrim);
    let mut x = 0.0;
    let mut clicked = false;
    card(ui.ctx(), t, "error", screen.center() + vec2(0.0, t.dialog_w / 4.0), Align2::CENTER_BOTTOM, t.dialog_w, None, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(t.gap / 2.0);
            ui.label(egui::RichText::new("Can’t play this").font(t.font_w(t.heading_size, Weight::Bold)).color(t.text));
            ui.add_space(t.gap / 4.0);
            ui.add(egui::Label::new(egui::RichText::new(msg).font(t.font(t.body_size)).color(t.muted)).wrap());
            ui.add_space(t.gap);
            let r = widgets::pill_button(ui, t, "Back", true, true);
            x = r.rect.center().x;
            clicked = r.clicked();
            ui.add_space(t.gap / 4.0);
        });
    });
    (x, clicked)
}

// ---------- gallery ----------

/// A stand-in video frame for the gallery: dusk sky, sun, dark hills (no artwork needed).
fn fake_frame(ui: &Ui, t: &Theme) {
    let s = ui.ctx().screen_rect();
    shade(ui, s, Color32::from_rgb(0x1b, 0x2a, 0x4a), Color32::from_rgb(0xd9, 0x7a, 0x4a));
    ui.painter().circle_filled(pos2(s.left() + s.width() * 0.66, s.top() + s.height() * 0.62), s.height() * 0.11, Color32::from_rgb(0xff, 0xd8, 0x9a));
    let hill = |x0: f32, x1: f32, top: f32, c: Color32| {
        let pts = vec![pos2(s.left() + s.width() * x0, s.bottom()), pos2(s.left() + s.width() * (x0 + x1) / 2.0, s.top() + s.height() * top), pos2(s.left() + s.width() * x1, s.bottom())];
        ui.painter().add(Shape::convex_polygon(pts, c, Stroke::NONE));
    };
    hill(-0.3, 0.7, 0.55, Color32::from_rgb(0x2a, 0x1e, 0x2e));
    hill(0.3, 1.4, 0.6, Color32::from_rgb(0x1e, 0x16, 0x24));
    hill(0.0, 1.0, 0.78, Color32::from_rgb(0x12, 0x0e, 0x18));
    let _ = t;
}

/// A 2×2 trickplay-like sheet made at run time, so the preview path (texture + uv crop) is exercised.
fn fake_sheet(ctx: &egui::Context) -> egui::TextureHandle {
    let (w, h) = (640, 360);
    let mut img = egui::ColorImage::new([w, h], vec![Color32::BLACK; w * h]);
    for y in 0..h {
        for x in 0..w {
            let cell = (x / 320) + 2 * (y / 180);
            let fy = (y % 180) as f32 / 180.0;
            let base = [(0x1b, 0x2a, 0x4a), (0x3a, 0x2a, 0x5a), (0x5a, 0x3a, 0x4a), (0x2a, 0x4a, 0x4a)][cell];
            let warm = (0xd9, 0x7a, 0x4a);
            let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * fy) as u8;
            let horizon = (x % 320) as f32 / 320.0 * 40.0 + 120.0;
            img.pixels[y * w + x] = if ((y % 180) as f32) > horizon {
                Color32::from_rgb(0x14, 0x10, 0x1a)
            } else {
                Color32::from_rgb(mix(base.0, warm.0), mix(base.1, warm.1), mix(base.2, warm.2))
            };
        }
    }
    ctx.load_texture("gallery-player-trick", img, egui::TextureOptions::LINEAR)
}

const SCENES: [&str; 12] = ["bar", "transport", "option", "audio", "subtitles", "speed", "queue", "info", "upnext", "skip", "stats", "error"];

/// `--screen gallery-player`: each part with test data. N / P step through the scenes; arrows move focus.
pub struct Gallery {
    scene: usize,
    focus: Focus,
    groups: Vec<Group>,
    sheet: Option<egui::TextureHandle>,
}

pub fn gallery_screen() -> Box<dyn Screen> {
    let mut g = Gallery { scene: 0, focus: Focus::default(), groups: vec![], sheet: None };
    g.reset();
    Box::new(g)
}

const G_BAR: usize = 0;
const G_BTN: usize = 1;
const G_VOL: usize = 2;
const G_LIST: usize = 3;
const G_SUB: usize = 4;

impl Gallery {
    fn reset(&mut self) {
        self.focus = match SCENES[self.scene] {
            "bar" => Focus { group: G_BAR, index: 0 },
            "transport" => Focus { group: G_BTN, index: 1 },
            "option" => Focus { group: G_BTN, index: 7 },
            "audio" | "speed" => Focus { group: G_LIST, index: 1 },
            "subtitles" => Focus { group: G_LIST, index: 2 },
            "queue" => Focus { group: G_LIST, index: 2 },
            _ => Focus { group: G_BTN, index: 0 },
        };
    }

    fn buttons() -> Vec<OsdButton> {
        let b = |icon, label: &str| OsdButton { icon, label: label.to_string() };
        vec![
            b(PIcon::SkipBack(10), ""),
            b(PIcon::Pause, ""),
            b(PIcon::SkipForward(30), ""),
            b(PIcon::Previous, "Previous episode"),
            b(PIcon::Next, "Next episode"),
            b(PIcon::Audio, "Audio · English 5.1"),
            b(PIcon::Subtitles, "Subtitles · English"),
            b(PIcon::Chapters, "Chapters"),
            b(PIcon::Queue, "Queue"),
            b(PIcon::Quality, "Quality · Auto"),
            b(PIcon::Speed, "Speed · 1×"),
            b(PIcon::Aspect, "Aspect · Auto"),
            b(PIcon::Repeat, "Repeat · Off"),
            b(PIcon::Stats, "Stats"),
            b(PIcon::Fullscreen, "Full screen"),
        ]
    }
}

impl Screen for Gallery {
    fn name(&self) -> &'static str {
        "gallery-player"
    }

    fn on_key(&mut self, _cx: &mut Cx, key: Key) -> bool {
        match key {
            Key::Next => {
                self.scene = (self.scene + 1) % SCENES.len();
                self.reset();
            }
            Key::Previous => {
                self.scene = (self.scene + SCENES.len() - 1) % SCENES.len();
                self.reset();
            }
            Key::Up | Key::Down | Key::Left | Key::Right => self.focus = move_focus(&self.groups, self.focus, key),
            Key::Back => return false,
            _ => {}
        }
        true
    }

    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.theme;
        fake_frame(ui, t);
        let scene = SCENES[self.scene];
        let sheet = self.sheet.get_or_insert_with(|| fake_sheet(ui.ctx())).id();
        let buttons = Gallery::buttons();
        let dur = 5520.0;
        let pos = 1931.0;
        let card_open = matches!(scene, "audio" | "subtitles" | "speed" | "queue" | "info");
        let osd_scene = !matches!(scene, "upnext" | "skip" | "error");
        let mut groups = vec![Group::default(); 5];
        let mut row_top = 0.0;
        let mut btn_xs = vec![];
        if osd_scene {
            let preview = (self.focus.group == G_BAR).then(|| Preview { secs: 2410.0, image: Some((sheet, Rect::from_min_max(pos2(0.5, 0.0), pos2(1.0, 0.5)))) });
            let v = OsdView {
                title: "The Night Shift",
                subtitle: "S1, E3 · Episode 3",
                note: if scene == "stats" { "Transcoding" } else { "" },
                pos,
                dur,
                buttons: &buttons,
                transport: 5,
                play_index: 1,
                focus_bar: self.focus.group == G_BAR,
                focus_btn: (self.focus.group == G_BTN).then_some(self.focus.index),
                focus_vol: (self.focus.group == G_VOL).then_some(self.focus.index),
                volume: 0.7,
                muted: false,
                preview,
                dim: card_open,
            };
            let out = osd(ui, t, &v);
            row_top = out.row_top;
            btn_xs = out.btn_xs.clone();
            if !card_open {
                groups[G_BAR] = Group { len: 1, cols: 1, xs: vec![out.bar_x] };
                groups[G_BTN] = Group { len: out.btn_xs.len(), cols: out.btn_xs.len(), xs: out.btn_xs };
                groups[G_VOL] = Group { len: 2, cols: 2, xs: out.vol_xs };
                if let Some(i) = out.btn_hovered {
                    self.focus = Focus { group: G_BTN, index: i };
                }
            }
        }
        let lf = (self.focus.group == G_LIST).then_some(self.focus.index);
        let row = |label: &str, detail: &str, selected: bool| MenuRow { label: label.into(), detail: detail.into(), selected, dim: false };
        match scene {
            "audio" => {
                let rows = vec![row("English 5.1", "Default", true), row("English Stereo", "", false), row("Commentary", "", false), row("Français 5.1", "", false)];
                let o = card(ui.ctx(), t, "audio", card_anchor(t, row_top, btn_xs[5]), Align2::CENTER_BOTTOM, t.player_card_w, Some("Audio"), |ui| menu_list(ui, t, &rows, lf));
                groups[G_LIST] = Group { len: o.xs.len(), cols: 1, xs: o.xs };
            }
            "speed" => {
                let rows: Vec<MenuRow> = ["0.5×", "0.75×", "1×", "1.25×", "1.5×", "2×"].iter().map(|s| row(s, "", *s == "1×")).collect();
                let o = card(ui.ctx(), t, "speed", card_anchor(t, row_top, btn_xs[10]), Align2::CENTER_BOTTOM, t.player_card_w, Some("Speed"), |ui| menu_list(ui, t, &rows, lf));
                groups[G_LIST] = Group { len: o.xs.len(), cols: 1, xs: o.xs };
            }
            "queue" => {
                let mut rows = vec![];
                for (i, (name, d)) in [("Episode 1", "42:10"), ("Episode 2", "44:02"), ("Episode 3", "Playing"), ("Episode 4", "41:37"), ("Episode 5", "43:20")].iter().enumerate() {
                    rows.push(MenuRow { label: format!("{}. {name}", i + 1), detail: d.to_string(), selected: i == 2, dim: i < 2 });
                }
                let o = card(ui.ctx(), t, "queue", card_anchor(t, row_top, btn_xs[8]), Align2::CENTER_BOTTOM, t.player_card_w * 1.25, Some("Up Next in Queue"), |ui| menu_list(ui, t, &rows, lf));
                groups[G_LIST] = Group { len: o.xs.len(), cols: 1, xs: o.xs };
            }
            "subtitles" => {
                let rows = vec![row("Off", "", false), row("English", "", true), row("English SDH", "", false), row("Français · External", "", false)];
                let f = self.focus;
                let (lo, so) = card(ui.ctx(), t, "subtitles", card_anchor(t, row_top, btn_xs[6]), Align2::CENTER_BOTTOM, t.player_card_w * 2.0, None, |ui| {
                    ui.columns(2, |c| {
                        c[0].label(egui::RichText::new("Subtitles").font(t.font_w(t.small_size, Weight::SemiBold)).color(t.muted));
                        let lo = menu_list(&mut c[0], t, &rows, lf);
                        c[1].label(egui::RichText::new("Style").font(t.font_w(t.small_size, Weight::SemiBold)).color(t.muted));
                        let so = stepper(&mut c[1], t, "Offset", "0.0 s", (f.group == G_SUB).then_some(f.index));
                        stepper(&mut c[1], t, "Position", "100", None);
                        widgets::chips(&mut c[1], t, &["S".into(), "M".into(), "L".into(), "XL".into()], &[false, true, false, false], None);
                        widgets::toggle(&mut c[1], t, "Background box", false, false);
                        (lo, so)
                    })
                });
                groups[G_LIST] = Group { len: lo.xs.len(), cols: 1, xs: lo.xs };
                groups[G_SUB] = Group { len: so.xs.len(), cols: so.xs.len(), xs: so.xs };
            }
            "info" => {
                let keys = [("Space", "Pause"), ("← →", "Seek"), ("↑ ↓", "Volume"), ("F", "Full screen"), ("M", "Mute"), ("N / P", "Next / previous"), ("C", "Chapters"), ("S", "Stats"), ("i", "Info")];
                let v = InfoView {
                    title: "Episode 3",
                    meta: "The Night Shift · Season 1 · 2019 · 45 min · TV-14",
                    overview: "A quiet night on the ward turns strange when the power fails and the backup lights come up on a patient nobody admitted.",
                    image: Some(sheet),
                    keys: &keys,
                };
                info(ui.ctx(), t, &v, row_top);
            }
            "stats" => stats(ui.ctx(), t, &[("Play method", "Transcoding".into()), ("Container", "ts (HLS)".into()), ("Video", "h264 1920×1080".into()), ("Source bitrate", "12.4 Mbit/s".into()), ("Bitrate", "8.0 Mbit/s".into()), ("Dropped frames", "0".into()), ("Buffer", "24 s".into())]),
            "upnext" => {
                let o = up_next(ui, t, &UpNext { title: "Episode 4", subtitle: "S1, E4 · starts in 8 s", countdown: Some(8), image: Some(sheet) }, self.focus.index.min(1));
                groups = vec![Group { len: 2, cols: 2, xs: o.xs }];
                if self.focus.group != 0 {
                    self.focus = Focus { group: 0, index: 0 };
                }
            }
            "skip" => {
                skip_pill(ui.ctx(), t, "Skip Intro", true, 0.0);
            }
            "error" => {
                error(ui, t, "The server could not transcode this video");
            }
            _ => {}
        }
        self.groups = groups;
        ui.painter().text(ui.ctx().screen_rect().center_top() + vec2(0.0, t.gap / 2.0), Align2::CENTER_TOP, format!("gallery · {scene}"), t.font(t.small_size), t.muted.gamma_multiply(0.5));
    }
}
