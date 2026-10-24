//! Poster picker (PP): choose a film's or show's Primary image in one step. A grid of large candidate thumbs
//! (current first, marked "Current"), chips to narrow by language / provider / size, an in-place preview styled
//! as the item's hero, one press to apply, and an Undo toast. Admins write the shared Jellyfin image (after
//! backing up the current one to the local poster store); everyone else gets a personal pick stored in
//! DisplayPreferences. Pure rules live in `crate::api::poster`.
use super::parts;
use super::widgets;
use crate::api::poster::{
    chips_for, file_url, filter_candidates, picture_extension, poster_store_dir, run_apply, PosterChoice, PosterFilter,
    RemoteImage, POSTER_PREFS_ID,
};
use crate::api::{image_width_bucket, ApiError, ImageKind, Item, Result};
use crate::app::{Cx, PosterUndo, Screen};
use crate::nav::{move_focus, Focus, Group};
use crate::platform::Key;
use crate::theme::Weight;
use egui::{Pos2, Rect, Sense, Vec2};

const G_CHIPS: usize = 0;
const G_GRID: usize = 1;
const G_BTN: usize = 2;
const N_GROUPS: usize = 3;
/// PP-6: own images larger than this are refused before they are read.
const MAX_OWN_BYTES: u64 = 20 << 20;

/// One cell of the grid: the current poster, or a candidate.
#[derive(Clone)]
struct Cell {
    current: bool,
    thumb: String,
    full: String,
    label: String,
}

/// What a successful apply produced, so the caller can arm Undo and apply the personal override.
struct Applied {
    previous: Option<Vec<u8>>,
    personal_url: Option<String>,
    display_id: Option<String>,
}

pub struct PosterPicker {
    item: Item,
    admin: bool,
    full: Option<Item>,
    candidates: Vec<RemoteImage>,
    providers: Vec<String>,
    loading: bool,
    error: Option<String>,
    filter: PosterFilter,
    selected: usize,
    focus: Focus,
    groups: Vec<Group>,
    applying: bool,
    paste_armed: bool,
    started: bool,
}

pub fn screen(item: Item, admin: bool) -> Box<dyn Screen> {
    Box::new(PosterPicker {
        item,
        admin,
        full: None,
        candidates: vec![],
        providers: vec![],
        loading: true,
        error: None,
        filter: PosterFilter::AllLanguages,
        selected: 0,
        focus: Focus { group: G_CHIPS, index: 0 },
        groups: vec![],
        applying: false,
        paste_armed: false,
        started: false,
    })
}

fn unix_now() -> u64 {
    crate::time::SystemTime::now().duration_since(crate::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// PP-6: save the item's previous Primary into the poster store, with its real extension. A write failure is
/// returned (the apply must fail, PP review #6).
fn save_backup(
    store: &dyn crate::platform::Storage,
    item_id: &str,
    bytes: &[u8],
    content_type: Option<&str>,
) -> Result<()> {
    let dir = poster_store_dir(&store.cache_dir()).join(item_id);
    std::fs::create_dir_all(&dir).map_err(|e| ApiError::Io(e.to_string()))?;
    let ext = picture_extension(content_type, bytes);
    let path = dir.join(format!("{}.{}", unix_now(), ext));
    std::fs::write(&path, bytes).map_err(|e| ApiError::Io(e.to_string()))?;
    Ok(())
}

/// The chips currently on offer (always "All languages" first), and the candidates the active chip keeps.
fn chips_and_filtered(all: &[RemoteImage], filter: &PosterFilter) -> (Vec<PosterFilter>, Vec<RemoteImage>) {
    let chips = chips_for(all);
    let shown = filter_candidates(all, filter);
    (chips, shown)
}

impl PosterPicker {
    /// The full item (current image tag, DisplayPreferencesId); the card list may not carry the latter.
    fn item(&self) -> &Item {
        self.full.as_ref().unwrap_or(&self.item)
    }

    fn label(&self) -> String {
        if self.admin {
            "Change poster".to_string()
        } else {
            "Change poster (just for you)".to_string()
        }
    }

    fn current_url(&self, cx: &Cx, w: u32) -> Option<String> {
        // The viewer's own pick (if any) is what "Current" means for them.
        if let Some(u) = cx.poster_url(self.item()) {
            return Some(crate::api::poster::sized_poster_url(&u, w));
        }
        cx.client.as_ref()?.image_url(self.item(), ImageKind::Primary, w)
    }

    /// The grid cells: current first (marked), then the filtered candidates.
    fn cells(&self, cx: &Cx) -> Vec<Cell> {
        let ppp_thumb = image_width_bucket(cx.theme.pp_thumb_w, 1.0);
        let mut out = Vec::new();
        if let Some(u) = self.current_url(cx, ppp_thumb) {
            out.push(Cell { current: true, thumb: u.clone(), full: u, label: "Current".to_string() });
        }
        let (_, shown) = chips_and_filtered(&self.candidates, &self.filter);
        for c in shown {
            let (Some(thumb), Some(full)) = (c.thumb_url().map(str::to_string), c.full_url().map(str::to_string))
            else {
                continue;
            };
            out.push(Cell { current: false, thumb, full, label: candidate_label(&c) });
        }
        out
    }

    fn apply_cell(&mut self, cx: &mut Cx, index: usize) {
        let cells = self.cells(cx);
        let Some(cell) = cells.get(index).cloned() else { return };
        if cell.current {
            return; // already the current poster
        }
        self.apply_remote(cx, cell.full);
    }

    fn apply_remote(&mut self, cx: &mut Cx, url: String) {
        if self.applying {
            return;
        }
        self.applying = true;
        let item_id = self.item.id.clone();
        let admin = self.admin;
        let store = cx.store.clone();
        let client = cx.api();
        cx.spawn("pp-apply", move || -> Result<Applied> {
            // PP-10: the apply is `run_apply` — the plan functions, executed step by step.
            let previous = run_apply(
                admin,
                &PosterChoice::Remote { url: url.clone() },
                |step| client.run_poster_step(&item_id, Some(POSTER_PREFS_ID), step),
                |bytes, ct| save_backup(store.as_ref(), &item_id, bytes, ct),
            )?;
            if admin {
                Ok(Applied { previous, personal_url: None, display_id: None })
            } else {
                Ok(Applied { previous: None, personal_url: Some(url), display_id: Some(POSTER_PREFS_ID.to_string()) })
            }
        });
    }

    /// PP-6: apply an own image (file or pasted image data). Admins upload it; others store it locally and
    /// point the personal pref at `file://…`.
    fn apply_own(&mut self, cx: &mut Cx, bytes: Vec<u8>) {
        // PP-8: one apply in flight at a time.
        if self.applying {
            return;
        }
        if bytes.len() as u64 > MAX_OWN_BYTES {
            self.error = Some("That image is larger than 20 MB.".to_string());
            return;
        }
        // PP review #7: verify the bytes really are a picture before doing anything with them.
        if image::load_from_memory(&bytes).is_err() {
            self.error = Some("That is not a picture (jpg, png, gif or webp).".to_string());
            return;
        }
        self.applying = true;
        let item_id = self.item.id.clone();
        let admin = self.admin;
        let store = cx.store.clone();
        let client = cx.api();
        cx.spawn("pp-apply", move || -> Result<Applied> {
            if admin {
                let previous = run_apply(
                    true,
                    &PosterChoice::OwnImage { bytes: bytes.clone() },
                    |step| client.run_poster_step(&item_id, Some(POSTER_PREFS_ID), step),
                    |b, ct| save_backup(store.as_ref(), &item_id, b, ct),
                )?;
                Ok(Applied { previous, personal_url: None, display_id: None })
            } else {
                // Store the image in the poster store, then point the pref at it (personal, this device).
                let dir = poster_store_dir(&store.cache_dir()).join(&item_id);
                std::fs::create_dir_all(&dir).map_err(|e| ApiError::Io(e.to_string()))?;
                let path = dir.join(format!("{}.{}", unix_now(), picture_extension(None, &bytes)));
                std::fs::write(&path, &bytes).map_err(|e| ApiError::Io(e.to_string()))?;
                let url = file_url(&path);
                run_apply(
                    false,
                    &PosterChoice::Remote { url: url.clone() },
                    |step| client.run_poster_step(&item_id, Some(POSTER_PREFS_ID), step),
                    |_, _| Ok(()),
                )?;
                Ok(Applied { previous: None, personal_url: Some(url), display_id: Some(POSTER_PREFS_ID.to_string()) })
            }
        });
    }

    fn spawn_loads(&mut self, cx: &mut Cx) {
        self.loading = true;
        let id = self.item.id.clone();
        let client = cx.api();
        cx.spawn("pp-item", move || client.item(&id));
        // PP (test-only): `--poster-candidates` replaces the RemoteImages reply with a fixture (no server call).
        let fixture = cx.opts.poster_candidates.clone();
        let id = self.item.id.clone();
        let client = cx.api();
        let fc = fixture.clone();
        cx.spawn("pp-candidates", move || match fc {
            Some(json) => crate::api::poster::fixture_candidates(&json).map(|(imgs, _)| imgs),
            None => client.remote_images(&id),
        });
        let id = self.item.id.clone();
        let client = cx.api();
        cx.spawn("pp-providers", move || match fixture {
            Some(json) => crate::api::poster::fixture_candidates(&json).map(|(_, provs)| provs),
            None => client.remote_image_providers(&id),
        });
    }

    fn keys_enter(&mut self, cx: &mut Cx, f: Focus) {
        match f.group {
            G_CHIPS => {
                let (chips, _) = chips_and_filtered(&self.candidates, &self.filter);
                if let Some(chip) = chips.get(f.index).cloned() {
                    self.filter = chip;
                }
            }
            G_GRID => {
                let idx = f.index;
                self.apply_cell(cx, idx);
            }
            G_BTN => match f.index {
                0 => {
                    let idx = self.selected;
                    self.apply_cell(cx, idx);
                }
                1 => self.pick_file(cx),
                2 => self.paste_armed = true,
                _ => {}
            },
            _ => {}
        }
    }

    fn pick_file(&mut self, cx: &mut Cx) {
        if cx.opts.desktop {
            cx.shell.pick_file = true;
        }
    }

    fn read_own_file(&mut self, cx: &mut Cx, path: String) {
        // PP review #8: refuse a file larger than the cap before reading it.
        if let Ok(meta) = std::fs::metadata(&path) {
            if meta.len() > MAX_OWN_BYTES {
                self.error = Some("That image is larger than 20 MB.".to_string());
                return;
            }
        }
        match std::fs::read(&path) {
            Ok(bytes) if crate::api::poster::is_picture(&bytes) => {
                self.apply_own(cx, bytes);
            }
            Ok(_) => self.error = Some("That file is not a picture (jpg, png or webp).".to_string()),
            Err(e) => self.error = Some(format!("Could not read that file ({e})")),
        }
    }

    fn handle_paste(&mut self, cx: &mut Cx, text: String) {
        let t = text.trim();
        if t.starts_with("http://") || t.starts_with("https://") {
            self.apply_remote(cx, t.to_string());
            return;
        }
        // Otherwise assume pasted image data (base64); `apply_own` verifies it is really a picture.
        let cleaned: String = t.chars().filter(|c| !c.is_whitespace()).collect();
        let body = cleaned.strip_prefix("data:image").and_then(|s| s.split_once(',').map(|(_, b)| b.to_string()));
        if let Some(bytes) = crate::api::poster::decode_base64(body.as_deref().unwrap_or(&cleaned)) {
            self.apply_own(cx, bytes);
        } else {
            self.error = Some("That paste is neither an image link nor image data.".to_string());
        }
    }

    fn draw(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        // PP review #7: a paste is only read after "Paste link/image" was chosen.
        if self.paste_armed {
            if let Some(text) = ui.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Paste(s) => Some(s.clone()),
                    _ => None,
                })
            }) {
                self.paste_armed = false;
                self.handle_paste(cx, text);
            }
        }

        ui.add_space(t.gap / 2.0);
        ui.label(egui::RichText::new(self.label()).font(t.font(t.title_size)).color(t.text));
        ui.label(egui::RichText::new(&self.item.name).font(t.font(t.body_size)).color(t.muted));
        ui.add_space(t.gap / 2.0);

        // Chips: All languages · languages · No text · providers · min width.
        let (chips, shown) = chips_and_filtered(&self.candidates, &self.filter);
        let mut chip_xs = vec![];
        if !chips.is_empty() {
            let labels: Vec<String> = chips.iter().map(|c| c.label()).collect();
            let selected: Vec<bool> = chips.iter().map(|c| *c == self.filter).collect();
            let out =
                widgets::chips(ui, t, &labels, &selected, (self.focus.group == G_CHIPS).then_some(self.focus.index));
            chip_xs = out.xs;
            if let Some(i) = out.hovered {
                self.focus = Focus { group: G_CHIPS, index: i };
            }
            if let Some(i) = out.clicked {
                self.filter = chips[i].clone();
            }
        }
        ui.add_space(t.gap / 2.0);

        // No candidates (PP-7).
        if !self.loading && self.error.is_none() && self.candidates.is_empty() {
            let who =
                if self.providers.is_empty() { "the image providers".to_string() } else { self.providers.join(", ") };
            parts::muted(ui, t, &format!("No posters found from {who}. Use a file or paste a link."));
        }

        // Grid + preview side by side.
        let grid_w = (ui.available_width() - t.pp_preview_w - t.gap).max(t.pp_thumb_w * 2.0);
        let mut grid_xs = vec![];
        let cells = self.cells(cx);
        let cols = crate::nav::grid_cols(grid_w, 0.0, t.pp_thumb_w, t.gap).max(1);
        let mut clicked_cell: Option<usize> = None;
        let mut hovered_cell: Option<usize> = None;
        // Two explicit rects (grid left, preview right) so neither can draw over the other.
        let avail = ui.available_rect_before_wrap();
        let band_h = (avail.height() - t.button_h - t.gap * 2.0).max(t.pp_thumb_h);
        let grid_rect = Rect::from_min_size(avail.min, Vec2::new(grid_w, band_h));
        let preview_rect = Rect::from_min_size(
            Pos2::new(avail.left() + grid_w + t.gap, avail.top()),
            Vec2::new(t.pp_preview_w, band_h),
        );
        ui.allocate_rect(Rect::from_min_size(avail.min, Vec2::new(avail.width(), band_h)), Sense::hover());
        ui.scope_builder(egui::UiBuilder::new().max_rect(grid_rect), |ui| {
            let grid_h = band_h;
            egui::ScrollArea::vertical().id_salt("pp-grid").max_height(grid_h).auto_shrink([false, false]).show(
                ui,
                |ui| {
                    let cell_w = t.pp_thumb_w + t.gap;
                    // room for the focus ring around the first row / column (the scroll area clips it)
                    let pad = t.focus_gap + t.focus_stroke;
                    let rows = cells.len().div_ceil(cols);
                    let (area, _) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), rows as f32 * (t.pp_thumb_h + t.body_size + t.gap) + pad * 2.0),
                        Sense::hover(),
                    );
                    let left = area.left() + pad;
                    let top = area.top() + pad;
                    for (i, cell) in cells.iter().enumerate() {
                        let (r, c) = (i / cols, i % cols);
                        let min =
                            Pos2::new(left + c as f32 * cell_w, top + r as f32 * (t.pp_thumb_h + t.body_size + t.gap));
                        let rect = Rect::from_min_size(min, Vec2::new(t.pp_thumb_w, t.pp_thumb_h + t.body_size));
                        let focused = self.focus.group == G_GRID && self.focus.index == i;
                        let resp = ui.interact(rect, ui.id().with(("pp-cell", i)), Sense::click());
                        grid_xs.push(rect.center().x);
                        if ui.is_rect_visible(rect) {
                            let img = Rect::from_min_size(min, Vec2::new(t.pp_thumb_w, t.pp_thumb_h));
                            let tex = cx.images.get(&cell.thumb);
                            match tex {
                                Some(tex) => parts::paint_image(ui, tex, img, t.card_radius),
                                None => {
                                    ui.painter().rect_filled(img, t.card_radius, t.skeleton);
                                }
                            }
                            if cell.current {
                                let badge = "Current".to_string();
                                let g = ui.fonts(|f| {
                                    f.layout_no_wrap(badge, t.font_w(t.small_size, Weight::SemiBold), t.text)
                                });
                                let b = Rect::from_min_size(
                                    Pos2::new(img.left() + t.gap / 4.0, img.top() + t.gap / 4.0),
                                    Vec2::new(g.size().x + t.badge_pad * 2.0, t.badge_h),
                                );
                                ui.painter().rect_filled(b, t.badge_h / 2.0, t.scrim);
                                ui.painter().galley(b.center() - g.size() / 2.0, g, t.text);
                            }
                            if focused {
                                parts::focus_ring(ui, t, img);
                            }
                            let ink = if focused { t.text } else { t.muted };
                            parts::label_line(
                                ui,
                                t,
                                Pos2::new(min.x, img.bottom() + 4.0),
                                t.pp_thumb_w,
                                &cell.label,
                                t.small_size,
                                ink,
                            );
                        }
                        if resp.hovered() {
                            hovered_cell = Some(i);
                        }
                        if resp.clicked() {
                            clicked_cell = Some(i);
                        }
                    }
                },
            );
        });
        // Preview panel, styled like the item hero.
        let cell = cells.get(self.selected).cloned();
        ui.scope_builder(egui::UiBuilder::new().max_rect(preview_rect), |ui| {
            self.draw_preview(ui, cx, cell);
        });
        let _ = shown;

        // Buttons: placed under the band by its own rect, whatever the layout cursor did.
        let mut btn_xs = vec![];
        let btn_rect = Rect::from_min_size(
            Pos2::new(avail.left(), avail.top() + band_h + t.gap / 2.0),
            Vec2::new(avail.width(), t.button_h),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(btn_rect), |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = t.gap / 2.0;
                let r = parts::button(ui, t, "Apply", self.focus.group == G_BTN && self.focus.index == 0, true);
                btn_xs.push(r.rect.center().x);
                if r.hovered() {
                    self.focus = Focus { group: G_BTN, index: 0 };
                }
                if r.clicked() {
                    clicked_cell = Some(self.selected);
                }
                if cx.opts.desktop {
                    let r =
                        parts::button(ui, t, "Use a file…", self.focus.group == G_BTN && self.focus.index == 1, false);
                    btn_xs.push(r.rect.center().x);
                    if r.hovered() {
                        self.focus = Focus { group: G_BTN, index: 1 };
                    }
                    if r.clicked() {
                        self.pick_file(cx);
                    }
                    let r = parts::button(
                        ui,
                        t,
                        "Paste link/image",
                        self.focus.group == G_BTN && self.focus.index == 2,
                        false,
                    );
                    btn_xs.push(r.rect.center().x);
                    if r.hovered() {
                        self.focus = Focus { group: G_BTN, index: 2 };
                    }
                    if r.clicked() {
                        self.paste_armed = true;
                    }
                }
            })
        });
        ui.allocate_rect(btn_rect, Sense::hover());
        if self.paste_armed {
            ui.add_space(t.gap / 4.0);
            parts::muted(ui, t, "Paste (Ctrl+V) an image link or image now.");
        }

        let mut groups = vec![Group::default(); N_GROUPS];
        groups[G_CHIPS] = Group { len: chips.len(), cols: chips.len(), xs: chip_xs };
        groups[G_GRID] = Group { len: cells.len(), cols, xs: grid_xs };
        groups[G_BTN] =
            Group { len: if cx.opts.desktop { 3 } else { 1 }, cols: if cx.opts.desktop { 3 } else { 1 }, xs: btn_xs };
        self.groups = groups;
        self.focus = move_focus(&self.groups, self.focus, Key::Enter);

        if let Some(i) = hovered_cell {
            if ui.input(|i| i.pointer.is_moving()) {
                self.focus = Focus { group: G_GRID, index: i };
            }
            self.selected = i;
        }
        if let Some(i) = clicked_cell {
            self.selected = i;
            self.apply_cell(cx, i);
        }

        if let Some(e) = self.error.clone() {
            ui.add_space(t.gap / 2.0);
            parts::error_line(ui, t, &e);
        }
    }

    fn draw_preview(&mut self, ui: &mut egui::Ui, cx: &mut Cx, cell: Option<Cell>) {
        let t = cx.theme;
        let item = self.item().clone();
        let w = image_width_bucket(t.pp_preview_w, ui.ctx().pixels_per_point());
        let img_h = t.pp_preview_w * 1.5;
        let (rect, _) = ui.allocate_exact_size(Vec2::new(t.pp_preview_w, img_h + t.gap * 2.0), Sense::hover());
        let img = Rect::from_min_size(rect.min, Vec2::new(t.pp_preview_w, img_h));
        let url = cell.map(|c| c.full).or_else(|| self.current_url(cx, w));
        match url.as_deref().and_then(|u| cx.images.get(u)) {
            Some(tex) => parts::paint_cover(ui, tex, img),
            None => {
                ui.painter().rect_filled(img, t.card_radius, t.skeleton);
            }
        }
        let y = img.bottom() + t.gap / 2.0;
        parts::label_line(ui, t, Pos2::new(rect.left(), y), t.pp_preview_w, &item.name, t.heading_size, t.text);
        let year = item.production_year.map(|y| y.to_string()).unwrap_or_default();
        let sub = if year.is_empty() { "Preview".to_string() } else { format!("{year} · preview") };
        parts::label_line(
            ui,
            t,
            Pos2::new(rect.left(), y + t.heading_size + 4.0),
            t.pp_preview_w,
            &sub,
            t.body_size,
            t.muted,
        );
    }
}

/// A candidate's grid label: provider · rating · votes · language.
fn candidate_label(c: &RemoteImage) -> String {
    let mut parts = vec![];
    if let Some(p) = c.provider_name.as_deref().filter(|p| !p.is_empty()) {
        parts.push(p.to_string());
    }
    if let Some(r) = c.community_rating {
        parts.push(format!("★ {r:.1}"));
    }
    if let Some(v) = c.vote_count {
        parts.push(format!("{v} votes"));
    }
    if let Some(l) = c.language.as_deref().filter(|l| !l.is_empty()) {
        parts.push(l.to_string());
    }
    parts.join(" · ")
}

impl Screen for PosterPicker {
    fn name(&self) -> &'static str {
        "poster-picker"
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        match key {
            Key::Enter => {
                let f = self.focus;
                self.keys_enter(cx, f);
                true
            }
            Key::Back => false,
            Key::Up | Key::Down | Key::Left | Key::Right => {
                self.focus = move_focus(&self.groups, self.focus, key);
                // The in-place preview follows the keyboard focus into the grid.
                if self.focus.group == G_GRID {
                    self.selected = self.focus.index;
                }
                true
            }
            _ => false,
        }
    }

    /// An Undo (app-level toast) changed the image while the picker was underneath or on top: re-read the item.
    fn on_resume(&mut self, cx: &mut Cx) {
        let id = self.item.id.clone();
        let client = cx.api();
        cx.spawn("pp-item", move || client.item(&id));
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn std::any::Any + Send>) {
        match tag {
            "pp-item" => {
                let Ok(r) = payload.downcast::<Result<Item>>() else { return };
                match *r {
                    Ok(it) => self.full = Some(it),
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    // PP review #9: a refresh failure after a successful apply must not replace the screen
                    // with an error (the change is already done; Undo is app-level).
                    Err(e) if self.full.is_none() => self.error = Some(e.to_string()),
                    Err(_) => {}
                }
            }
            "pp-candidates" => {
                self.loading = false;
                let Ok(r) = payload.downcast::<Result<Vec<RemoteImage>>>() else { return };
                match *r {
                    Ok(v) => {
                        self.candidates = crate::api::poster::sort_candidates(v);
                    }
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(e) => {
                        self.candidates = vec![];
                        self.error = Some(e.to_string());
                    }
                }
            }
            "pp-providers" => {
                let Ok(r) = payload.downcast::<Result<Vec<crate::api::poster::RemoteImageProvider>>>() else {
                    return;
                };
                match *r {
                    Ok(v) => {
                        self.providers = v
                            .into_iter()
                            .filter(|p| p.supported_images.iter().any(|s| s == "Primary"))
                            .map(|p| p.name)
                            .collect();
                    }
                    Err(_) => self.providers = vec![],
                }
            }
            "pp-apply" => {
                self.applying = false;
                let Ok(r) = payload.downcast::<Result<Applied>>() else { return };
                match *r {
                    Ok(a) => {
                        let was_admin = self.admin;
                        // PP-5: the Undo toast lives at the app level so it survives leaving the picker.
                        *cx.undo = Some(PosterUndo {
                            item_id: self.item.id.clone(),
                            admin: was_admin,
                            previous: a.previous,
                            display_id: a.display_id.clone(),
                            at: cx.now,
                        });
                        if was_admin {
                            cx.set_poster_override(&self.item.id, None);
                            cx.poster_changed.push(self.item.id.clone());
                        } else if let Some(url) = a.personal_url {
                            cx.set_poster_override(&self.item.id, Some(url));
                        }
                        self.error = None;
                        // Refresh the picker's own "Current" cell; a failure here must not hide Undo (PP review #9).
                        let id = self.item.id.clone();
                        let client = cx.api();
                        cx.spawn("pp-item", move || client.item(&id));
                    }
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(e) => {
                        self.error = Some(e.to_string());
                    }
                }
            }
            "picked-file" => {
                let Ok(path) = payload.downcast::<String>() else { return };
                self.read_own_file(cx, *path);
            }
            _ => {}
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        if !self.started {
            self.started = true;
            self.spawn_loads(cx);
        }
        self.draw(ui, cx);
    }
}
