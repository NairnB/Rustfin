//! D3 (Apple TV look, docs/DESIGN-appletv.md §5 Library / Search): parts for the library grid, browse, search,
//! playlists and favourites screens. Pure drawing from plain data and the theme; the screens keep their focus
//! groups and keys.
use super::widgets;
use crate::theme::{Theme, Weight};
use egui::{pos2, vec2, Align2, Color32, Rect, Response, Sense, Stroke, Ui};

/// Search: a wide rounded field centred at the top, magnifier on the left. Glass at rest; focused it turns into
/// the white plate with dark text (the Apple TV search bar). Keyboard focus follows `focused` like
/// `parts::text_field`.
pub fn search_field(ui: &mut Ui, t: &Theme, text: &mut String, hint: &str, focused: bool) -> Response {
    let (band, _) = ui.allocate_exact_size(vec2(ui.available_width(), t.search_h), Sense::hover());
    let w = t.search_w.min(band.width());
    let rect = Rect::from_center_size(band.center(), vec2(w, t.search_h));
    let id = ui.id().with("at-search-field");
    let a = widgets::focus_amount(ui, t, id, focused);
    let ink = widgets::plate(ui, t, rect, t.search_h / 2.0, t.glass, a, 1.0 + (t.pill_focus_scale - 1.0) / 2.0);
    let icon_c = pos2(rect.left() + t.search_h / 2.0 + t.gap / 4.0, rect.center().y);
    magnifier(ui, t, icon_c, t.search_icon, ink);
    let text_rect = Rect::from_min_max(
        pos2(icon_c.x + t.search_icon / 2.0 + t.gap / 2.0, rect.top()),
        pos2(rect.right() - t.search_h / 2.0, rect.bottom()),
    );
    let font = t.font_w(t.heading_size, Weight::Regular);
    let hint_ink = ink.gamma_multiply(0.55);
    let edit = egui::TextEdit::singleline(text)
        .frame(false)
        .font(font.clone())
        .text_color(ink)
        .hint_text(egui::RichText::new(hint).font(font).color(hint_ink))
        .vertical_align(egui::Align::Center)
        .desired_width(text_rect.width());
    let r = ui.put(text_rect, edit);
    if focused && !r.has_focus() {
        r.request_focus();
    } else if !focused && r.has_focus() {
        r.surrender_focus();
    }
    r
}

fn magnifier(ui: &Ui, t: &Theme, c: egui::Pos2, size: f32, ink: Color32) {
    let h = size / 2.0;
    let st = Stroke::new(t.icon_stroke, ink);
    ui.painter().circle_stroke(pos2(c.x - 0.15 * h, c.y - 0.15 * h), 0.62 * h, st);
    ui.painter().line_segment([pos2(c.x + 0.3 * h, c.y + 0.3 * h), pos2(c.x + 0.9 * h, c.y + 0.9 * h)], st);
}

/// A section title over a grid ("Movies  8"): heading weight, the count muted beside it. No box around the
/// section — the artwork carries the page.
pub fn section_heading(ui: &mut Ui, t: &Theme, label: &str, count: Option<usize>) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = t.gap / 3.0;
        ui.label(egui::RichText::new(label).font(t.font(t.heading_size)).color(t.text));
        if let Some(n) = count {
            ui.label(egui::RichText::new(n.to_string()).font(t.font_w(t.heading_size, Weight::Regular)).color(t.muted));
        }
    });
}

/// An empty screen's message, centred in the space left: a title and one muted line under it.
pub fn empty_state(ui: &mut Ui, t: &Theme, title: &str, body: &str) {
    let avail = ui.available_rect_before_wrap();
    let h = avail.height().min(t.empty_h);
    let (r, _) = ui.allocate_exact_size(vec2(avail.width(), h), Sense::hover());
    let c = r.center();
    ui.painter().text(pos2(c.x, c.y - t.body_size), Align2::CENTER_BOTTOM, title, t.font(t.title_size), t.text);
    let g = ui.fonts(|f| {
        f.layout(body.to_string(), t.font(t.body_size), t.muted, t.hero_text_w.min(r.width() - 2.0 * t.gap))
    });
    ui.painter().galley(pos2(c.x - g.size().x / 2.0, c.y + t.body_size / 2.0), g, t.muted);
}

/// One A–Z jump letter: muted, dimmer when no loaded item starts with it, a small white plate when focused or
/// hovered (no ring).
pub fn az_letter(ui: &Ui, t: &Theme, r: Rect, ch: char, focused: bool, hovered: bool, present: bool) {
    let id = ui.id().with(("at-az", ch));
    let a = widgets::focus_amount(ui, t, id, focused || hovered);
    let d = r.width().min(r.height());
    let plate = Rect::from_center_size(r.center(), vec2(d, d));
    let rest = if present { t.muted } else { t.muted.gamma_multiply(0.4) };
    if a > 0.01 {
        ui.painter().circle_filled(plate.center(), d / 2.0 * (0.8 + 0.2 * a), t.focus_fill.gamma_multiply(a));
    }
    let ink = rest.lerp_to_gamma(t.focus_ink, a);
    ui.painter().text(r.center(), Align2::CENTER_CENTER, ch, t.font_w(t.small_size, Weight::SemiBold), ink);
}

/// A list-view row background: transparent at rest, glass on hover, the white plate (no lift) when focused.
/// Returns the ink to draw the row's text with.
pub fn list_plate(ui: &Ui, t: &Theme, id: egui::Id, r: Rect, focused: bool, hovered: bool) -> Color32 {
    let a = widgets::focus_amount(ui, t, id, focused);
    let rest = if hovered { t.glass } else { Color32::TRANSPARENT };
    widgets::plate(ui, t, r, t.radius, rest, a, 1.0)
}

/// A photo cropped to a circle in `r` (square): cover-fit, keeping the top of the picture (faces sit high).
pub fn paint_round(ui: &Ui, tex: egui::TextureId, r: Rect) {
    let n = ui.ctx().tex_manager().read().meta(tex).map(|m| vec2(m.size[0] as f32, m.size[1] as f32));
    let n = n.filter(|n| n.x > 0.0 && n.y > 0.0).unwrap_or(r.size());
    let aspect = n.x / n.y;
    let uv = if aspect > 1.0 {
        let w = 1.0 / aspect;
        Rect::from_min_max(pos2((1.0 - w) / 2.0, 0.0), pos2((1.0 + w) / 2.0, 1.0))
    } else {
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, aspect))
    };
    egui::Image::from_texture(egui::load::SizedTexture::new(tex, r.size()))
        .uv(uv)
        .corner_radius(r.width() / 2.0)
        .paint_at(ui, r);
}
