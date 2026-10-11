//! Small reusable parts (R-UI-0). Screens are arrangements of these; every value comes from `Theme`.
use crate::api::{image_width_bucket, ticks_to_secs, ImageKind, Item, ItemKind};
use crate::app::Cx;
use crate::theme::Theme;
use egui::{pos2, vec2, Align2, Rect, Response, Sense, Ui};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardKind {
    /// 2:3 poster (films, series, seasons)
    Poster,
    /// 16:9 thumb (episodes in rows, continue watching, library tiles)
    Thumb,
    /// P-LIB-14: wide strip (Banner image, else Thumb, else Backdrop)
    Banner,
}

impl CardKind {
    pub fn size(self, t: &Theme) -> egui::Vec2 {
        match self {
            CardKind::Poster => vec2(t.poster_w, t.poster_h),
            CardKind::Thumb => vec2(t.thumb_w, t.thumb_h),
            CardKind::Banner => vec2(t.banner_w, t.banner_h),
        }
    }
    pub fn image(self) -> ImageKind {
        match self {
            CardKind::Poster => ImageKind::Primary,
            CardKind::Thumb | CardKind::Banner => ImageKind::Primary,
        }
    }
}

/// White outline around a focused thing.
pub fn focus_ring(ui: &Ui, t: &Theme, r: Rect) {
    ui.painter().rect_stroke(
        r.expand(t.focus_gap),
        t.radius + t.focus_gap,
        egui::Stroke::new(t.focus_stroke, t.focus_color),
        egui::StrokeKind::Outside,
    );
}

/// Thin progress bar along the bottom of `r` (`frac` 0..1).
pub fn progress_bar(ui: &Ui, t: &Theme, r: Rect, frac: f32) {
    let bar = Rect::from_min_max(pos2(r.left(), r.bottom() - t.progress_h), r.max);
    ui.painter().rect_filled(bar, 0.0, t.progress_track);
    let mut done = bar;
    done.set_right(bar.left() + bar.width() * frac.clamp(0.0, 1.0));
    ui.painter().rect_filled(done, 0.0, t.progress);
}

/// The image URL a card shows (and the one to prefetch).
pub fn card_image_url(cx: &Cx, item: &Item, kind: CardKind, scale: f32) -> Option<String> {
    let w = image_width_bucket(kind.size(cx.theme).x, scale);
    if kind == CardKind::Banner {
        return cx.client.as_ref()?.banner_image_url(item, w);
    }
    cx.client.as_ref()?.image_url(item, kind.image(), w)
}

/// Card label lines: (title, subtitle). Both year switches on; see `card_labels_with`.
pub fn card_labels(item: &Item) -> (String, String) {
    card_labels_with(item, true, true)
}

/// Card label lines with the two Settings switches (R-F3-1): the subtitle is the year line.
pub fn card_labels_with(item: &Item, show_release: bool, show_story: bool) -> (String, String) {
    match item.kind {
        ItemKind::Episode => (
            item.series_name.clone().unwrap_or_else(|| item.name.clone()),
            match (item.parent_index_number, item.index_number) {
                (Some(s), Some(e)) => format!("S{s} · E{e} · {}", item.name),
                _ => item.name.clone(),
            },
        ),
        _ => (item.name.clone(), crate::story::year_line(item, show_release, show_story)),
    }
}

/// Fraction watched for the progress bar, if partly watched.
pub fn resume_fraction(item: &Item) -> Option<f32> {
    let pos = item.user_data.as_ref()?.playback_position_ticks;
    let total = item.run_time_ticks?;
    (pos > 0 && total > 0).then(|| (pos as f64 / total as f64) as f32)
}

/// "2019 · 1h 32m · PG" (parts that exist only).
pub fn meta_line(item: &Item) -> String {
    meta_line_with(item, true, true)
}

/// `meta_line` with the two year switches (R-F3-1): the first part is the year line ("1977 · 0 BBY").
pub fn meta_line_with(item: &Item, show_release: bool, show_story: bool) -> String {
    let mut v = vec![];
    let years = crate::story::year_line(item, show_release, show_story);
    if !years.is_empty() {
        v.push(years);
    }
    if let Some(t) = item.run_time_ticks {
        v.push(runtime(ticks_to_secs(t)));
    }
    if let Some(r) = &item.official_rating {
        v.push(r.clone());
    }
    v.join(" · ")
}

/// 40 s → "1 min", 2400 s → "40 min", 5520 s → "1h 32m".
pub fn runtime(secs: f64) -> String {
    let m = (secs / 60.0).round().max(1.0) as i64;
    if m < 60 {
        format!("{m} min")
    } else {
        format!("{}h {:02}m", m / 60, m % 60)
    }
}

/// "1:02:03" or "2:03".
pub fn clock(secs: f64) -> String {
    let s = secs.max(0.0) as i64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// A card: image (skeleton while loading), focus ring, progress bar, one or two label lines.
/// Returns the response for the whole card (hover → caller focuses it, click → activate).
pub fn card(ui: &mut Ui, cx: &mut Cx, item: &Item, kind: CardKind, focused: bool) -> Response {
    let t = cx.theme;
    let size = kind.size(t);
    let (rect, resp) = ui.allocate_exact_size(vec2(size.x, size.y + t.card_label_h), Sense::click());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let img_rect = Rect::from_min_size(rect.min, size);
    let url = card_image_url(cx, item, kind, ui.ctx().pixels_per_point());
    match url.as_deref().and_then(|u| cx.images.get(u)) {
        Some(tex) => {
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            ui.painter().image(tex, img_rect, uv, egui::Color32::WHITE);
        }
        None => {
            ui.painter().rect_filled(img_rect, t.radius, t.skeleton);
            if url.is_none() {
                ui.painter().text(
                    img_rect.center(),
                    Align2::CENTER_CENTER,
                    initials(&item.name),
                    t.font(t.heading_size),
                    t.muted,
                );
            }
        }
    }
    if let Some(f) = resume_fraction(item) {
        progress_bar(ui, t, img_rect, f);
    }
    if focused {
        focus_ring(ui, t, img_rect);
    }
    let (title, sub) = card_labels_with(item, cx.settings.show_release_year, cx.settings.show_story_year);
    let tx = rect.left();
    let w = size.x;
    label_line(ui, t, pos2(tx, img_rect.bottom() + 6.0), w, &title, t.body_size, t.text);
    if !sub.is_empty() {
        label_line(ui, t, pos2(tx, img_rect.bottom() + 8.0 + t.body_size), w, &sub, t.small_size, t.muted);
    }
    resp
}

fn initials(name: &str) -> String {
    name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect()
}

/// One line of text cut to `w` with an ellipsis.
pub fn label_line(ui: &Ui, t: &Theme, at: egui::Pos2, w: f32, text: &str, size: f32, color: egui::Color32) {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_string(),
        egui::TextFormat { font_id: t.font(size), color, ..Default::default() },
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(w);
    let g = ui.fonts(|f| f.layout_job(job));
    ui.painter().galley(at, g, color);
}

/// A skeleton card while a row/grid is loading.
pub fn skeleton_card(ui: &mut Ui, t: &Theme, kind: CardKind) {
    let size = kind.size(t);
    let (rect, _) = ui.allocate_exact_size(vec2(size.x, size.y + t.card_label_h), Sense::hover());
    ui.painter().rect_filled(Rect::from_min_size(rect.min, size), t.radius, t.skeleton);
}

/// A button. `primary` = filled accent. Focus ring when focused.
pub fn button(ui: &mut Ui, t: &Theme, label: &str, focused: bool, primary: bool) -> Response {
    let g = ui.fonts(|f| f.layout_no_wrap(label.to_string(), t.font(t.body_size), t.text));
    let size = vec2(g.size().x + 2.0 * t.button_pad, t.button_h);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let (fill, ink) = if primary { (t.accent, t.accent_text) } else { (t.surface_hi, t.text) };
    let fill = if resp.hovered() && !primary { t.surface_hi.gamma_multiply(1.3) } else { fill };
    ui.painter().rect_filled(rect, t.radius, fill);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, t.font(t.body_size), ink);
    if focused {
        focus_ring(ui, t, rect);
    }
    resp
}

/// Single-line text field. Focus follows our focus model: when `focused` it takes keyboard focus, otherwise
/// it gives it up. `password` hides the text.
pub fn text_field(ui: &mut Ui, t: &Theme, text: &mut String, hint: &str, focused: bool, password: bool) -> Response {
    let r = ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(hint)
            .password(password)
            .desired_width(t.field_w)
            .font(t.font(t.body_size))
            .margin(egui::Margin::symmetric(12, 10)),
    );
    if focused && !r.has_focus() {
        r.request_focus();
    } else if !focused && r.has_focus() {
        r.surrender_focus();
    }
    if focused {
        focus_ring(ui, t, r.rect);
    }
    r
}

pub fn title(ui: &mut Ui, t: &Theme, text: &str) {
    ui.label(egui::RichText::new(text).size(t.title_size).color(t.text).strong());
}
pub fn heading(ui: &mut Ui, t: &Theme, text: &str) {
    ui.label(egui::RichText::new(text).size(t.heading_size).color(t.text));
}
pub fn muted(ui: &mut Ui, t: &Theme, text: &str) {
    ui.label(egui::RichText::new(text).size(t.body_size).color(t.muted));
}
pub fn error_line(ui: &mut Ui, t: &Theme, text: &str) {
    ui.label(egui::RichText::new(text).size(t.body_size).color(t.error));
}

/// A horizontally scrolling row of cards with a heading. `focus` = focused index in this row, if the row
/// has focus. Returns (clicked index, hovered index, x centres of the cards for `nav::Group::xs`).
pub struct RowOut {
    pub clicked: Option<usize>,
    pub hovered: Option<usize>,
    pub xs: Vec<f32>,
}

pub fn row(
    ui: &mut Ui,
    cx: &mut Cx,
    id: &str,
    heading_text: &str,
    items: &[Item],
    kind: CardKind,
    focus: Option<usize>,
) -> RowOut {
    let t = cx.theme;
    let mut out = RowOut { clicked: None, hovered: None, xs: vec![] };
    ui.add_space(t.row_gap / 2.0);
    heading(ui, t, heading_text);
    ui.add_space(t.gap / 3.0);
    egui::ScrollArea::horizontal()
        .id_salt(id)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = cx.theme.gap;
                for (i, it) in items.iter().enumerate() {
                    let r = card(ui, cx, it, kind, focus == Some(i));
                    out.xs.push(r.rect.center().x);
                    if focus == Some(i) {
                        r.scroll_to_me(None);
                        // prefetch the next two cards (R-IMG-2)
                        for next in items.iter().skip(i + 1).take(2) {
                            if let Some(u) = card_image_url(cx, next, kind, ui.ctx().pixels_per_point()) {
                                cx.images.prefetch(&u);
                            }
                        }
                    }
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

/// Full-width backdrop behind an item page: surface colour, the backdrop image (if any), then a scrim so
/// the text drawn over it reads. Returns the backdrop rect.
pub fn backdrop(ui: &mut Ui, cx: &mut Cx, item: &Item) -> Rect {
    let t = cx.theme;
    let (alloc, _) = ui.allocate_exact_size(vec2(ui.available_width(), t.backdrop_h), Sense::hover());
    ui.painter().rect_filled(alloc, 0.0, t.surface);
    let w = image_width_bucket(alloc.width(), ui.ctx().pixels_per_point());
    if let Some(url) = cx.client.as_ref().and_then(|c| c.image_url(item, ImageKind::Backdrop, w)) {
        if let Some(tex) = cx.images.get(&url) {
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            ui.painter().image(tex, alloc, uv, egui::Color32::WHITE);
        }
    }
    ui.painter().rect_filled(alloc, 0.0, t.scrim);
    alloc
}

/// A vertical grid of poster cards inside the page's ScrollArea, laid out only for visible rows.
/// `cols` from `nav::grid_cols`. Returns the same as `row`.
pub fn grid(
    ui: &mut Ui,
    cx: &mut Cx,
    items: &[Option<Item>],
    cols: usize,
    focus: Option<usize>,
    total: usize,
) -> RowOut {
    let t = cx.theme;
    let mut out = RowOut { clicked: None, hovered: None, xs: vec![] };
    let size = CardKind::Poster.size(t);
    let cell = vec2(size.x + t.gap, size.y + t.card_label_h + t.gap);
    let rows = total.div_ceil(cols.max(1));
    let left = ui.min_rect().left() + ((ui.available_width() - (cols as f32 * cell.x - t.gap)) / 2.0).max(0.0);
    let top = ui.cursor().top();
    let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), rows as f32 * cell.y), Sense::hover());
    let clip = ui.clip_rect();
    for i in 0..total {
        let (r, c) = (i / cols.max(1), i % cols.max(1));
        let min = pos2(left + c as f32 * cell.x, top + r as f32 * cell.y);
        out.xs.push(min.x + size.x / 2.0);
        let rect = Rect::from_min_size(min, vec2(size.x, size.y + t.card_label_h));
        let visible = rect.intersects(clip);
        let near =
            rect.translate(vec2(0.0, -cell.y)).intersects(clip) || rect.translate(vec2(0.0, cell.y)).intersects(clip);
        if focus == Some(i) {
            ui.scroll_to_rect(rect.expand(t.gap), None);
        }
        match items.get(i).and_then(|x| x.as_ref()) {
            Some(it) if visible => {
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                let resp = card(&mut child, cx, it, CardKind::Poster, focus == Some(i));
                if resp.hovered() {
                    out.hovered = Some(i);
                }
                if resp.clicked() {
                    out.clicked = Some(i);
                }
            }
            Some(it) if near => {
                if let Some(u) = card_image_url(cx, it, CardKind::Poster, ui.ctx().pixels_per_point()) {
                    cx.images.prefetch(&u);
                }
            }
            None if visible => {
                ui.painter().rect_filled(Rect::from_min_size(min, size), t.radius, t.skeleton);
            }
            _ => {}
        }
    }
    let _ = area;
    out
}

/// The fixed top app bar: app name on the left, Home · Search · Settings · Favourites · Playlists buttons on the right.
/// `focused` is the focused button (0..5 → Home, Search, Settings, Favourites, Playlists); the returned `RowOut` maps
/// `clicked` / `hovered` / `xs` the same way.
pub fn topbar(ui: &mut Ui, cx: &mut Cx, focused: Option<usize>) -> RowOut {
    topbar_with_user(ui, cx, focused, None).0
}

/// P-HOME-7: `topbar` plus a user chip (initial in a small circle + name) at the far right as button 5. Returns the
/// chip's rect too, so the caller can hang its menu under it.
pub fn topbar_with_user(
    ui: &mut Ui,
    cx: &mut Cx,
    focused: Option<usize>,
    user: Option<&str>,
) -> (RowOut, Option<Rect>) {
    let t = cx.theme;
    let mut out = RowOut { clicked: None, hovered: None, xs: vec![] };
    const BUTTONS: [&str; 6] = ["Home", "Search", "Settings", "Favourites", "Playlists", ""];
    let n = if user.is_some() { 6 } else { 5 };

    let bar_rect = Rect::from_min_size(ui.cursor().left_top(), vec2(ui.available_width(), t.topbar_h));
    ui.advance_cursor_after_rect(bar_rect);
    ui.painter().rect_filled(bar_rect, 0.0, t.surface);

    let name = ui.fonts(|f| f.layout_no_wrap("Rustfin".to_string(), t.font(t.title_size), t.text));
    ui.painter().galley(pos2(bar_rect.left() + t.margin, bar_rect.center().y), name, t.text);

    let avatar = t.button_h * 0.65;
    let mut widths = [0.0f32; 6];
    for i in 0..n {
        let label = if i == 5 { user.unwrap_or_default() } else { BUTTONS[i] };
        let g = ui.fonts(|f| f.layout_no_wrap(label.to_string(), t.font(t.body_size), t.text));
        widths[i] = g.size().x + 2.0 * t.button_pad + if i == 5 { avatar + t.gap / 3.0 } else { 0.0 };
    }
    let mut right = bar_rect.right() - t.margin;
    let mut rects = [Rect::NOTHING; 6];
    for i in (0..n).rev() {
        rects[i] =
            Rect::from_center_size(pos2(right - widths[i] / 2.0, bar_rect.center().y), vec2(widths[i], t.button_h));
        right -= widths[i] + t.gap;
    }
    for (i, rect) in rects.iter().take(n).enumerate() {
        let label = BUTTONS[i];
        let resp = ui.interact(*rect, ui.id().with(("topbar", i)), Sense::click());
        let fill = if resp.hovered() { t.surface_hi.gamma_multiply(1.3) } else { t.surface_hi };
        ui.painter().rect_filled(*rect, t.radius, fill);
        if i == 5 {
            let name = user.unwrap_or_default();
            let c = pos2(rect.left() + t.button_pad / 2.0 + avatar / 2.0, rect.center().y);
            ui.painter().circle_filled(c, avatar / 2.0, t.accent);
            let initial = name.chars().next().map(|ch| ch.to_uppercase().to_string()).unwrap_or_default();
            ui.painter().text(c, Align2::CENTER_CENTER, initial, t.font(t.small_size), t.accent_text);
            let tx = rect.left() + t.button_pad / 2.0 + avatar + t.gap / 3.0;
            ui.painter().text(pos2(tx, rect.center().y), Align2::LEFT_CENTER, name, t.font(t.body_size), t.text);
        } else {
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, t.font(t.body_size), t.text);
        }
        if focused == Some(i) {
            focus_ring(ui, t, *rect);
        }
        out.xs.push(rect.center().x);
        if resp.hovered() {
            out.hovered = Some(i);
        }
        if resp.clicked() {
            out.clicked = Some(i);
        }
    }
    (out, user.map(|_| rects[5]))
}

/// A horizontal row centred in the available width. Centring uses the row's width from the previous frame, so
/// the very first frame may sit left (the app redraws continuously, so it settles at once).
pub fn centered_row<R>(ui: &mut Ui, id_salt: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let id = ui.id().with(id_salt);
    let last_w: f32 = ui.ctx().data(|d| d.get_temp(id)).unwrap_or(0.0);
    let pad = ((ui.available_width() - last_w) / 2.0).max(0.0);
    let (out, used) = ui
        .horizontal(|ui| {
            ui.add_space(pad);
            let start = ui.cursor().left();
            let out = add(ui);
            (out, ui.min_rect().right() - start)
        })
        .inner;
    ui.ctx().data_mut(|d| d.insert_temp(id, used));
    out
}

/// A button of a fixed width (the show page's season drop-down and its entries).
pub fn button_width(ui: &mut Ui, t: &Theme, label: &str, focused: bool, primary: bool, width: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, t.button_h), Sense::click());
    let (fill, ink) = if primary { (t.accent, t.accent_text) } else { (t.surface_hi, t.text) };
    let fill = if resp.hovered() && !primary { t.surface_hi.gamma_multiply(1.3) } else { fill };
    ui.painter().rect_filled(rect, t.radius, fill);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, t.font(t.body_size), ink);
    if focused {
        focus_ring(ui, t, rect);
    }
    resp
}

/// `backdrop` with an explicit height (the show page hero).
pub fn backdrop_sized(ui: &mut Ui, cx: &mut Cx, item: &Item, height: f32) -> Rect {
    let t = cx.theme;
    let (alloc, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    ui.painter().rect_filled(alloc, 0.0, t.surface);
    let w = image_width_bucket(alloc.width(), ui.ctx().pixels_per_point());
    if let Some(url) = cx.client.as_ref().and_then(|c| c.image_url(item, ImageKind::Backdrop, w)) {
        if let Some(tex) = cx.images.get(&url) {
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            ui.painter().image(tex, alloc, uv, egui::Color32::WHITE);
        }
    }
    ui.painter().rect_filled(alloc, 0.0, t.scrim);
    alloc
}

/// Text wrapped to at most two lines (elided with …), drawn at `at` (the episode overview).
pub fn two_line(ui: &Ui, t: &Theme, at: egui::Pos2, w: f32, text: &str, size: f32, color: egui::Color32) {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_string(),
        egui::TextFormat { font_id: t.font(size), color, ..Default::default() },
    );
    job.wrap =
        egui::text::TextWrapping { max_width: w, max_rows: 2, break_anywhere: false, overflow_character: Some('…') };
    let g = ui.fonts(|f| f.layout_job(job));
    ui.painter().galley(at, g, color);
}

/// A tick (check mark) drawn with the painter, not a font glyph. `center` is the centre of the bounding box.
pub fn tick(ui: &Ui, t: &Theme, center: egui::Pos2, size: f32) {
    let s = size * 0.5;
    let a = center + vec2(-s * 0.6, 0.0);
    let b = center + vec2(-s * 0.15, s * 0.55);
    let c = center + vec2(s * 0.6, -s * 0.5);
    let stroke = egui::Stroke::new((size * 0.14).max(2.0), t.accent);
    ui.painter().line_segment([a, b], stroke);
    ui.painter().line_segment([b, c], stroke);
}

/// A five-pointed star drawn with the painter (community-rating glyph; the bundled fonts have none).
pub fn star(ui: &Ui, t: &Theme, center: egui::Pos2, size: f32) {
    let r = size * 0.5;
    let inner = r * 0.42;
    let mut pts = Vec::with_capacity(10);
    for i in 0..10 {
        let ang = -std::f32::consts::FRAC_PI_2 + (i as f32) * std::f32::consts::PI / 5.0;
        let rad = if i % 2 == 0 { r } else { inner };
        pts.push(center + vec2(rad * ang.cos(), rad * ang.sin()));
    }
    ui.painter().add(egui::Shape::Path(egui::epaint::PathShape {
        points: pts,
        closed: true,
        fill: t.muted,
        stroke: egui::Stroke::NONE.into(),
    }));
}

/// The item page facts line (P-ITEM-1): "1999 · 1 min · PG · ★ 7.4 · 84%". The community-rating star is
/// painted with `star`; the critic rating is a whole percent. Segments that don't exist are omitted.
pub fn facts_line(ui: &mut Ui, t: &Theme, item: &Item) {
    facts_line_with(ui, t, item, true, true);
}

/// `facts_line` with the two year switches (R-F3-1).
pub fn facts_line_with(ui: &mut Ui, t: &Theme, item: &Item, show_release: bool, show_story: bool) {
    let base = meta_line_with(item, show_release, show_story);
    let community = item.community_rating;
    let critic = item.critic_rating;
    if base.is_empty() && community.is_none() && critic.is_none() {
        return;
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = t.gap / 4.0;
        let mut sep = false;
        if !base.is_empty() {
            ui.label(egui::RichText::new(base).size(t.small_size).color(t.muted));
            sep = true;
        }
        if let Some(r) = community {
            if sep {
                ui.label(egui::RichText::new("·").size(t.small_size).color(t.muted));
            }
            sep = true;
            let (rect, _) = ui.allocate_exact_size(vec2(t.small_size, t.small_size), Sense::hover());
            star(ui, t, rect.center(), t.small_size * 0.9);
            ui.label(egui::RichText::new(format!("{r:.1}")).size(t.small_size).color(t.muted));
        }
        if let Some(c) = critic {
            if sep {
                ui.label(egui::RichText::new("·").size(t.small_size).color(t.muted));
            }
            ui.label(egui::RichText::new(format!("{c:.0}%")).size(t.small_size).color(t.muted));
        }
    });
}

/// A heart drawn with the painter (the bundled fonts have no heart glyph). `size` is the full width;
/// `filled` = solid heart, otherwise an outline.
pub fn heart(ui: &Ui, t: &Theme, center: egui::Pos2, size: f32, filled: bool) {
    let scale = size / 32.0;
    let n = 48;
    let mut pts = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let a = (i as f32) / (n as f32) * std::f32::consts::TAU;
        let x = 16.0 * a.sin().powi(3);
        let y = 13.0 * a.cos() - 5.0 * (2.0 * a).cos() - 2.0 * (3.0 * a).cos() - (4.0 * a).cos();
        pts.push(center + vec2(x * scale, -(y + 3.0) * scale));
    }
    if filled {
        ui.painter().add(egui::Shape::Path(egui::epaint::PathShape {
            points: pts,
            closed: true,
            fill: t.favourite,
            stroke: egui::Stroke::NONE.into(),
        }));
    } else {
        ui.painter().add(egui::Shape::Path(egui::epaint::PathShape {
            points: pts,
            closed: true,
            fill: egui::Color32::TRANSPARENT,
            stroke: egui::Stroke::new((size * 0.07).max(2.0), t.favourite).into(),
        }));
    }
}

/// A small filled heart on the top-right corner of a card image, over a scrim disc so it reads on any poster
/// (P-ITEM-7: cards of favourites carry the mark).
pub fn heart_mark(ui: &Ui, t: &Theme, img_rect: Rect) {
    let r = t.body_size * 0.9;
    let center = img_rect.right_top() + vec2(-r - 6.0, r + 6.0);
    ui.painter().circle_filled(center, r + 4.0, t.scrim);
    heart(ui, t, center, r * 1.6, true);
}

/// A square icon button holding a heart (filled = favourited), used on the item/show pages.
pub fn heart_button(ui: &mut Ui, t: &Theme, filled: bool, focused: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(t.button_h, t.button_h), Sense::click());
    let fill = if resp.hovered() { t.surface_hi.gamma_multiply(1.3) } else { t.surface_hi };
    ui.painter().rect_filled(rect, t.radius, fill);
    heart(ui, t, rect.center(), t.button_h * 0.55, filled);
    if focused {
        focus_ring(ui, t, rect);
    }
    resp
}

/// `card` with a `heart_mark` in the corner (favourites rows and the favourites page).
pub fn card_with_heart(ui: &mut Ui, cx: &mut Cx, item: &Item, kind: CardKind, focused: bool) -> Response {
    let r = card(ui, cx, item, kind, focused);
    heart_mark(ui, cx.theme, Rect::from_min_size(r.rect.min, kind.size(cx.theme)));
    r
}

/// `row` whose cards carry a favourite heart mark.
pub fn row_favourites(
    ui: &mut Ui,
    cx: &mut Cx,
    id: &str,
    heading_text: &str,
    items: &[Item],
    kind: CardKind,
    focus: Option<usize>,
) -> RowOut {
    let t = cx.theme;
    let mut out = RowOut { clicked: None, hovered: None, xs: vec![] };
    ui.add_space(t.row_gap / 2.0);
    heading(ui, t, heading_text);
    ui.add_space(t.gap / 3.0);
    egui::ScrollArea::horizontal()
        .id_salt(id)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = cx.theme.gap;
                for (i, it) in items.iter().enumerate() {
                    let r = card_with_heart(ui, cx, it, kind, focus == Some(i));
                    out.xs.push(r.rect.center().x);
                    if focus == Some(i) {
                        r.scroll_to_me(None);
                        for next in items.iter().skip(i + 1).take(2) {
                            if let Some(u) = card_image_url(cx, next, kind, ui.ctx().pixels_per_point()) {
                                cx.images.prefetch(&u);
                            }
                        }
                    }
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

/// A vertical drop marker between grid cards (R-F4-2): an accent bar at `x`, spanning `[top, top + height]`.
pub fn drop_marker(ui: &Ui, t: &Theme, x: f32, top: f32, height: f32) {
    let bar = Rect::from_min_size(pos2(x - t.drop_marker_w / 2.0, top), vec2(t.drop_marker_w, height));
    ui.painter().rect_filled(bar, t.drop_marker_w / 2.0, t.accent);
}

/// The dragged card's ghost at `center` (R-F4-2): the card image dimmed under a light scrim. Drawn on top of
/// the grid while a drag or move is in progress.
pub fn drag_ghost(ui: &mut Ui, cx: &mut Cx, item: &Item, kind: CardKind, center: egui::Pos2) {
    let t = cx.theme;
    let size = kind.size(t);
    let rect = Rect::from_center_size(center, size);
    let url = card_image_url(cx, item, kind, ui.ctx().pixels_per_point());
    match url.as_deref().and_then(|u| cx.images.get(u)) {
        Some(tex) => {
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            ui.painter().image(tex, rect, uv, egui::Color32::WHITE);
        }
        None => {
            ui.painter().rect_filled(rect, t.radius, t.skeleton);
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, &item.name, t.font(t.small_size), t.muted);
        }
    }
    ui.painter().rect_filled(rect, t.radius, t.ghost_scrim);
    focus_ring(ui, t, rect);
}

/// G11 (P-LIB-14): `grid` for any card kind (the thumbs view mode). Same return and layout rules as `grid`.
pub fn grid_kind(
    ui: &mut Ui,
    cx: &mut Cx,
    items: &[Option<Item>],
    cols: usize,
    focus: Option<usize>,
    total: usize,
    kind: CardKind,
) -> RowOut {
    let t = cx.theme;
    let mut out = RowOut { clicked: None, hovered: None, xs: vec![] };
    let size = kind.size(t);
    let cell = vec2(size.x + t.gap, size.y + t.card_label_h + t.gap);
    let rows = total.div_ceil(cols.max(1));
    let left = ui.min_rect().left() + ((ui.available_width() - (cols as f32 * cell.x - t.gap)) / 2.0).max(0.0);
    let top = ui.cursor().top();
    let (_area, _) = ui.allocate_exact_size(vec2(ui.available_width(), rows as f32 * cell.y), Sense::hover());
    let clip = ui.clip_rect();
    for i in 0..total {
        let (r, c) = (i / cols.max(1), i % cols.max(1));
        let min = pos2(left + c as f32 * cell.x, top + r as f32 * cell.y);
        out.xs.push(min.x + size.x / 2.0);
        let rect = Rect::from_min_size(min, vec2(size.x, size.y + t.card_label_h));
        let visible = rect.intersects(clip);
        if focus == Some(i) {
            ui.scroll_to_rect(rect.expand(t.gap), None);
        }
        match items.get(i).and_then(|x| x.as_ref()) {
            Some(it) if visible => {
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                let resp = if kind == CardKind::Thumb {
                    thumb_card(&mut child, cx, it, focus == Some(i))
                } else {
                    card(&mut child, cx, it, kind, focus == Some(i))
                };
                if resp.hovered() {
                    out.hovered = Some(i);
                }
                if resp.clicked() {
                    out.clicked = Some(i);
                }
            }
            None if visible => {
                ui.painter().rect_filled(Rect::from_min_size(min, size), t.radius, t.skeleton);
            }
            _ => {}
        }
    }
    out
}

/// G11 (P-LIB-14): a 16:9 card for any item — its Thumb image, else Backdrop, else Primary cropped to fit.
pub fn thumb_card(ui: &mut Ui, cx: &mut Cx, item: &Item, focused: bool) -> Response {
    let t = cx.theme;
    let size = CardKind::Thumb.size(t);
    let (rect, resp) = ui.allocate_exact_size(vec2(size.x, size.y + t.card_label_h), Sense::click());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let img_rect = Rect::from_min_size(rect.min, size);
    let w = image_width_bucket(size.x, ui.ctx().pixels_per_point());
    let pick = [ImageKind::Thumb, ImageKind::Backdrop, ImageKind::Primary]
        .into_iter()
        .find_map(|k| cx.client.as_ref()?.image_url(item, k, w).map(|u| (k, u)));
    match pick.as_ref().and_then(|(k, u)| cx.images.get(u).map(|tex| (*k, tex))) {
        Some((kind, tex)) => {
            // Thumb / Backdrop are 16:9; a Primary poster (2:3) is cropped to a centre band.
            let (v0, v1) = if kind == ImageKind::Primary && item.kind != ItemKind::Episode {
                let keep = (size.x / size.y) / (2.0 / 3.0);
                ((1.0 - keep) / 2.0, (1.0 + keep) / 2.0)
            } else {
                (0.0, 1.0)
            };
            ui.painter().image(tex, img_rect, Rect::from_min_max(pos2(0.0, v0), pos2(1.0, v1)), egui::Color32::WHITE);
        }
        None => {
            ui.painter().rect_filled(img_rect, t.radius, t.skeleton);
            if pick.is_none() {
                ui.painter().text(
                    img_rect.center(),
                    Align2::CENTER_CENTER,
                    initials(&item.name),
                    t.font(t.heading_size),
                    t.muted,
                );
            }
        }
    }
    if let Some(f) = resume_fraction(item) {
        progress_bar(ui, t, img_rect, f);
    }
    if focused {
        focus_ring(ui, t, img_rect);
    }
    let (title, sub) = card_labels(item);
    label_line(ui, t, pos2(rect.left(), img_rect.bottom() + 6.0), size.x, &title, t.body_size, t.text);
    if !sub.is_empty() {
        label_line(
            ui,
            t,
            pos2(rect.left(), img_rect.bottom() + 8.0 + t.body_size),
            size.x,
            &sub,
            t.small_size,
            t.muted,
        );
    }
    resp
}

/// D20: how a server is shown — its name; the address only when it has no name.
pub fn server_label(name: &str, address: &str) -> String {
    if name.trim().is_empty() {
        address.to_string()
    } else {
        name.trim().to_string()
    }
}

#[cfg(test)]
mod server_label_tests {
    use super::server_label;

    #[test]
    fn name_wins_and_address_is_the_fallback() {
        assert_eq!(server_label("Den", "http://10.0.0.20:8096"), "Den");
        assert_eq!(server_label("  ", "http://x:8096"), "http://x:8096");
    }
}
