//! G7 playlists (P-COLL-4, P-COLL-5): the list of the user's playlists, a playlist page (ordered entries, Play all,
//! move up / down, remove, delete) and the "New playlist" name screen opened from an item page.
use super::collection;
use super::parts::{self, CardKind};
use super::player::{self, PlayOptions};
use super::widgets;
use crate::api::{ApiError, Item, ItemKind, PlaylistEntry, Result};
use crate::app::{Cx, Screen};
use crate::nav::{move_focus, Focus, Group};
use crate::platform::Key;
use egui::{pos2, vec2, Align2, Rect, Sense};

/// P-COLL-4: the signed-in user's playlists.
pub fn list() -> Box<dyn Screen> {
    collection::grid_screen_art(
        "playlists",
        "Playlists",
        "No playlists yet. Open a film, choose Add to… and then New playlist….",
        |c| c.playlists(),
        screen,
        Some(|c, p| c.playlist_posters(&p.id, MOSAIC_READ).map(|v| mosaic_pick(&v)).unwrap_or_default()),
    )
}

/// FIX-3 #9: how many entries are read to find four distinct posters.
const MOSAIC_READ: usize = 24;

/// FIX-3 #9 / M1: the posters a playlist tile is drawn from — the first four distinct items that have one,
/// de-duplicated by item id, and by series for episodes (an episode shows its series' poster). Each result is an
/// `Item` whose id and `Primary` tag name the image.
pub fn mosaic_pick(entries: &[Item]) -> Vec<Item> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for e in entries {
        let (id, tag) = match (&e.series_id, &e.series_primary_image_tag) {
            (Some(s), Some(t)) if e.kind == ItemKind::Episode => (s.clone(), t.clone()),
            _ => match e.image_tags.get("Primary") {
                Some(t) => (e.id.clone(), t.clone()),
                None => continue,
            },
        };
        let key =
            if e.kind == ItemKind::Episode { e.series_id.clone().unwrap_or_else(|| id.clone()) } else { id.clone() };
        if !seen.insert(key) {
            continue;
        }
        let mut poster = Item { id, name: e.name.clone(), kind: e.kind.clone(), ..Default::default() };
        poster.image_tags.insert("Primary".into(), tag);
        out.push(poster);
        if out.len() == 4 {
            break;
        }
    }
    out
}

/// FIX-3 #9 / M2: where each poster goes in a tile `r`: 1 full; 2 side by side; 3 one tall half and two quarters;
/// 4 or more a 2×2 of the first four.
pub fn mosaic_rects(n: usize, r: Rect) -> Vec<Rect> {
    let (c, w, h) = (r.center(), r.width() / 2.0, r.height() / 2.0);
    let half_l = Rect::from_min_max(r.min, pos2(c.x, r.max.y));
    let q = |col: f32, row: f32| Rect::from_min_size(pos2(r.min.x + col * w, r.min.y + row * h), vec2(w, h));
    match n {
        0 => vec![],
        1 => vec![r],
        2 => vec![half_l, Rect::from_min_max(pos2(c.x, r.min.y), r.max)],
        3 => vec![half_l, q(1.0, 0.0), q(1.0, 1.0)],
        _ => vec![q(0.0, 0.0), q(1.0, 0.0), q(0.0, 1.0), q(1.0, 1.0)],
    }
}

/// FIX-3 #9 / M3: what a playlist tile shows.
pub enum TileArt {
    /// the playlist's own distinct posters
    Posters(Vec<Item>),
    /// no items: the server's image
    Server,
    /// no items and no server image: the card's placeholder
    Placeholder,
}

pub fn tile_art(posters: &[Item], playlist: &Item) -> TileArt {
    if !posters.is_empty() {
        TileArt::Posters(posters.to_vec())
    } else if playlist.image_tags.contains_key("Primary") {
        TileArt::Server
    } else {
        TileArt::Placeholder
    }
}

// ---------- the playlist page ----------

const G_BAR: usize = 0;
const G_ROWS: usize = 1;
const G_RETRY: usize = 2;
/// Focus targets per entry row: open, up, down, remove.
const COLS: usize = 4;
const BAR_PLAY: usize = 0;
const BAR_DELETE: usize = 1;

pub struct Playlist {
    item: Item,
    entries: Option<Result<Vec<PlaylistEntry>>>,
    busy: bool,
    error: Option<String>,
    confirm_delete: bool,
    confirm_focus: usize,
    started: bool,
    focus: Focus,
    groups: Vec<Group>,
}

/// P-COLL-4: a playlist page.
pub fn screen(item: Item) -> Box<dyn Screen> {
    Box::new(Playlist {
        item,
        entries: None,
        busy: false,
        error: None,
        confirm_delete: false,
        confirm_focus: 0,
        started: false,
        focus: Focus { group: G_BAR, index: BAR_PLAY },
        groups: vec![],
    })
}

enum Act {
    Play,
    AskDelete,
    Open(usize),
    Move(usize, usize),
    Remove(usize),
}

impl Playlist {
    fn load(&mut self, cx: &Cx) {
        let id = self.item.id.clone();
        let client = cx.api();
        cx.spawn("entries", move || client.playlist_items(&id));
    }

    fn rows(&self) -> Vec<PlaylistEntry> {
        match &self.entries {
            Some(Ok(v)) => v.clone(),
            _ => vec![],
        }
    }

    fn play_all(&self, cx: &mut Cx) {
        let first =
            self.rows().into_iter().map(|e| e.item).find(|i| matches!(i.kind, ItemKind::Movie | ItemKind::Episode));
        if let Some(it) = first {
            cx.push(player::screen(it, PlayOptions { from_start: true, ..Default::default() }));
        }
    }

    /// Move entry `from` to `to`: shown at once, sent to the server, then the list is re-read for the truth.
    fn move_entry(&mut self, cx: &Cx, from: usize, to: usize) {
        let rows = self.rows();
        if self.busy || from == to || from >= rows.len() || to >= rows.len() {
            return;
        }
        let ids: Vec<String> = rows.iter().map(|e| e.entry_id.clone()).collect();
        if let Some(Ok(v)) = &mut self.entries {
            let e = v.remove(from);
            v.insert(to, e);
        }
        self.busy = true;
        let id = self.item.id.clone();
        let client = cx.api();
        cx.spawn("changed", move || client.move_playlist_item(&id, &ids, from, to));
    }

    fn remove_entry(&mut self, cx: &Cx, index: usize) {
        let Some(entry) = self.rows().get(index).map(|e| e.entry_id.clone()) else { return };
        if self.busy {
            return;
        }
        self.busy = true;
        let id = self.item.id.clone();
        let client = cx.api();
        cx.spawn("changed", move || client.remove_from_playlist(&id, &[entry]));
    }

    fn delete(&mut self, cx: &Cx) {
        self.busy = true;
        let id = self.item.id.clone();
        let client = cx.api();
        cx.spawn("deleted", move || client.delete_item(&id));
    }

    fn act(&mut self, cx: &mut Cx, a: Act) {
        match a {
            Act::Play => self.play_all(cx),
            Act::AskDelete => {
                self.confirm_delete = true;
                self.confirm_focus = 0;
            }
            Act::Open(i) => {
                if let Some(e) = self.rows().get(i) {
                    cx.push(super::item::screen(e.item.clone()));
                }
            }
            Act::Move(from, to) => {
                self.move_entry(cx, from, to);
                self.focus = Focus { group: G_ROWS, index: to * COLS + (self.focus.index % COLS) };
            }
            Act::Remove(i) => self.remove_entry(cx, i),
        }
    }

    /// Enter / click on a focus target.
    fn activate(&mut self, cx: &mut Cx, f: Focus) {
        match f.group {
            G_BAR => match f.index {
                BAR_PLAY => self.act(cx, Act::Play),
                _ => self.act(cx, Act::AskDelete),
            },
            G_ROWS => {
                let row = f.index / COLS;
                match f.index % COLS {
                    0 => self.act(cx, Act::Open(row)),
                    1 if row > 0 => self.act(cx, Act::Move(row, row - 1)),
                    2 => self.act(cx, Act::Move(row, row + 1)),
                    3 => self.act(cx, Act::Remove(row)),
                    _ => {}
                }
            }
            G_RETRY => self.load(cx),
            _ => {}
        }
    }
}

fn small_button(
    ui: &mut egui::Ui,
    t: &crate::theme::Theme,
    rect: Rect,
    id: egui::Id,
    label: &str,
    focused: bool,
) -> egui::Response {
    let resp = ui.interact(rect, id, Sense::click());
    // Apple TV look: a glass pill, the white plate when focused.
    let a = super::widgets::focus_amount(ui, t, id, focused);
    let rest = if resp.hovered() { t.glass_hi } else { t.glass };
    let ink = super::widgets::plate(ui, t, rect, rect.height() / 2.0, rest, a, t.pill_focus_scale);
    let font = t.font_w(t.small_size, crate::theme::Weight::SemiBold);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, font, ink);
    resp
}

impl Screen for Playlist {
    fn name(&self) -> &'static str {
        "playlist"
    }

    /// NAV: this screen's part of the toolbar breadcrumb.
    fn crumbs(&self) -> Vec<String> {
        vec![self.item.name.clone()]
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        // BRIEF-coverflow: Info opens Poster Flow on the playlist, in its order.
        if key == Key::Info && !self.confirm_delete {
            if let Some(Ok(e)) = &self.entries {
                let v: Vec<Item> = e.iter().map(|x| x.item.clone()).collect();
                if !v.is_empty() {
                    cx.push(super::posterflow::items(v, 0));
                }
            }
            return true;
        }
        if self.confirm_delete {
            match key {
                Key::Left => self.confirm_focus = 0,
                Key::Right => self.confirm_focus = 1,
                Key::Enter => {
                    self.confirm_delete = false;
                    if self.confirm_focus == 1 {
                        self.delete(cx);
                    }
                }
                Key::Back => self.confirm_delete = false,
                _ => {}
            }
            return true;
        }
        match key {
            Key::Enter => {
                self.activate(cx, self.focus);
                true
            }
            Key::Up | Key::Down | Key::Left | Key::Right => {
                self.focus = move_focus(&self.groups, self.focus, key);
                true
            }
            _ => false,
        }
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn std::any::Any + Send>) {
        match tag {
            "entries" => {
                let Ok(r) = payload.downcast::<Result<Vec<PlaylistEntry>>>() else { return };
                match &*r {
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    _ => self.entries = Some(*r),
                }
            }
            "changed" => {
                let Ok(r) = payload.downcast::<Result<()>>() else { return };
                self.busy = false;
                match *r {
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(e) => self.error = Some(e.to_string()),
                    Ok(()) => self.error = None,
                }
                self.load(cx);
            }
            "deleted" => {
                let Ok(r) = payload.downcast::<Result<()>>() else { return };
                self.busy = false;
                match *r {
                    Ok(()) => cx.pop(),
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            _ => {}
        }
    }

    fn on_resume(&mut self, cx: &mut Cx) {
        self.load(cx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        if !self.started {
            self.started = true;
            self.load(cx);
        }
        let t = cx.theme;
        let pointer_moved = ui.input(|i| i.pointer.is_moving());

        ui.add_space(t.margin);
        parts::title(ui, t, &self.item.name);
        let rows = self.rows();
        if matches!(self.entries, Some(Ok(_))) {
            parts::muted(ui, t, &format!("{} {}", rows.len(), if rows.len() == 1 { "item" } else { "items" }));
        }
        ui.add_space(t.gap / 2.0);

        let mut groups = vec![Group::default(); 3];
        let mut clicked: Option<Focus> = None;
        let mut hovered: Option<Focus> = None;

        match &self.entries {
            None => {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = t.gap;
                    for _ in 0..6 {
                        parts::skeleton_card(ui, t, CardKind::Thumb);
                    }
                });
                self.groups = groups;
                return;
            }
            Some(Err(e)) => {
                parts::error_line(ui, t, &e.to_string());
                let r = parts::button(ui, t, "Retry", true, false);
                if r.clicked() {
                    self.load(cx);
                }
                self.focus = Focus { group: G_RETRY, index: 0 };
                groups[G_RETRY] = Group { len: 1, cols: 1, xs: vec![r.rect.center().x] };
                self.groups = groups;
                return;
            }
            Some(Ok(_)) => {}
        }

        if let Some(e) = self.error.clone() {
            parts::error_line(ui, t, &e);
            ui.add_space(t.gap / 3.0);
        }

        // Play all + Delete playlist.
        let can_play = rows.iter().any(|e| matches!(e.item.kind, ItemKind::Movie | ItemKind::Episode));
        let mut bar_xs = vec![];
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = t.gap / 2.0;
            let r = parts::button(ui, t, "Play all", self.focus == Focus { group: G_BAR, index: BAR_PLAY }, can_play);
            bar_xs.push(r.rect.center().x);
            if r.hovered() {
                hovered = Some(Focus { group: G_BAR, index: BAR_PLAY });
            }
            if r.clicked() {
                clicked = Some(Focus { group: G_BAR, index: BAR_PLAY });
            }
            let r =
                parts::button(ui, t, "Delete playlist", self.focus == Focus { group: G_BAR, index: BAR_DELETE }, false);
            bar_xs.push(r.rect.center().x);
            if r.hovered() {
                hovered = Some(Focus { group: G_BAR, index: BAR_DELETE });
            }
            if r.clicked() {
                clicked = Some(Focus { group: G_BAR, index: BAR_DELETE });
            }
        });
        groups[G_BAR] = Group { len: 2, cols: 2, xs: bar_xs };
        ui.add_space(t.gap / 2.0);

        if rows.is_empty() {
            super::at_library::empty_state(
                ui,
                t,
                "This playlist is empty",
                "Open a film and choose Add to… to put something in it.",
            );
        }

        let row_h = t.ep_thumb_h + t.gap / 2.0;
        let btn_w = t.button_h * 1.6;
        let mut xs: Vec<f32> = vec![];
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (n, e) in rows.iter().enumerate() {
                let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::hover());
                let area = Rect::from_min_size(rect.min, vec2(rect.width(), t.ep_thumb_h));
                if !ui.is_rect_visible(rect) {
                    xs.extend([0.0; COLS]);
                    continue;
                }
                // Buttons on the right: up, down, remove; the open area takes the rest.
                let by = area.center().y - t.button_h / 2.0;
                let mut right = area.right();
                let mut brects = [Rect::NOTHING; 3];
                for i in (0..3).rev() {
                    brects[i] = Rect::from_min_size(pos2(right - btn_w, by), vec2(btn_w, t.button_h));
                    right -= btn_w + t.gap / 2.0;
                }
                let open_rect = Rect::from_min_max(area.min, pos2(right, area.max.y));
                let focus_here = |c: usize| self.focus == Focus { group: G_ROWS, index: n * COLS + c };

                let open = ui.interact(open_rect, ui.id().with(("pl-open", n)), Sense::click());
                let ink = super::at_library::list_plate(ui, t, open.id, open_rect, focus_here(0), open.hovered());
                let sub_ink = if focus_here(0) { ink } else { t.muted };
                let img_w = t.ep_thumb_h * 2.0 / 3.0;
                let img = Rect::from_min_size(area.min, vec2(img_w, t.ep_thumb_h));
                let url = parts::card_image_url(cx, &e.item, CardKind::Poster, ui.ctx().pixels_per_point());
                match url.as_deref().and_then(|u| cx.images.get(u)) {
                    Some(tex) => parts::paint_image(ui, tex, img, t.radius),
                    None => {
                        ui.painter().rect_filled(img, t.radius, t.skeleton);
                    }
                }
                let (title, sub) = parts::card_labels(&e.item);
                let tx = img.right() + t.gap / 2.0;
                let tw = (open_rect.right() - tx).max(0.0);
                parts::label_line(
                    ui,
                    t,
                    pos2(tx, area.top() + t.gap / 3.0),
                    tw,
                    &format!("{}. {title}", n + 1),
                    t.body_size,
                    ink,
                );
                if !sub.is_empty() {
                    parts::label_line(
                        ui,
                        t,
                        pos2(tx, area.top() + t.gap / 3.0 + t.body_size + 6.0),
                        tw,
                        &sub,
                        t.small_size,
                        sub_ink,
                    );
                }
                xs.push(open_rect.center().x);
                if open.hovered() {
                    hovered = Some(Focus { group: G_ROWS, index: n * COLS });
                }
                if open.clicked() {
                    clicked = Some(Focus { group: G_ROWS, index: n * COLS });
                }
                for (c, label) in ["Up", "Down", "Remove"].iter().enumerate() {
                    let r = small_button(ui, t, brects[c], ui.id().with(("pl-btn", n, c)), label, focus_here(c + 1));
                    xs.push(brects[c].center().x);
                    if r.hovered() {
                        hovered = Some(Focus { group: G_ROWS, index: n * COLS + c + 1 });
                    }
                    if r.clicked() {
                        clicked = Some(Focus { group: G_ROWS, index: n * COLS + c + 1 });
                    }
                }
                if self.focus.group == G_ROWS
                    && self.focus.index / COLS == n
                    && crate::ui::shell::keyboard_focus(ui.ctx())
                {
                    ui.scroll_to_rect(rect.expand(t.gap), None);
                }
            }
        });
        // `xs` holds COLS centres per row, but the nav group wants one x per item in row-major order: same thing.
        if !rows.is_empty() {
            groups[G_ROWS] = Group { len: rows.len() * COLS, cols: COLS, xs };
        }

        if self.confirm_delete {
            let d = widgets::dialog(
                ui,
                t,
                "Delete this playlist?",
                &format!("\"{}\" will be removed. The films stay in your library.", self.item.name),
                &["Delete".to_string(), "Cancel".to_string()],
                Some(1 - self.confirm_focus),
            );
            // `dialog` lays its buttons right to left, so index 0 is the primary (right-hand) one.
            if let Some(i) = d.hovered {
                if pointer_moved {
                    self.confirm_focus = 1 - i;
                }
            }
            if let Some(i) = d.clicked {
                self.confirm_delete = false;
                if i == 0 {
                    self.delete(cx);
                }
            }
        }

        self.groups = groups;
        self.focus = move_focus(&self.groups, self.focus, Key::Enter);
        if pointer_moved && !self.confirm_delete {
            if let Some(f) = hovered {
                self.focus = f;
            }
        }
        if let Some(f) = clicked {
            if !self.confirm_delete {
                self.focus = f;
                self.activate(cx, f);
            }
        }
    }
}

// ---------- New playlist ----------

const N_NAME: usize = 0;
const N_BUTTONS: usize = 1;

pub struct NewPlaylist {
    collection: bool,
    ids: Vec<String>,
    name: String,
    busy: bool,
    error: Option<String>,
    focus: Focus,
    groups: Vec<Group>,
}

/// P-COLL-5: ask for a name, create the playlist holding `ids`, then open it.
pub fn new_screen(ids: Vec<String>, default_name: String) -> Box<dyn Screen> {
    new_named(false, ids, default_name)
}

/// P-COLL-2: the same name screen for a new collection (admins only; the server refuses anyone else).
pub fn new_collection_screen(ids: Vec<String>, default_name: String) -> Box<dyn Screen> {
    new_named(true, ids, default_name)
}

/// FIX-2 #2: the name field's hint — a plain label, never the film title (it read as real text on the TV).
pub fn name_hint(collection: bool) -> &'static str {
    if collection {
        "Collection name"
    } else {
        "Playlist name"
    }
}

fn new_named(collection: bool, ids: Vec<String>, _default_name: String) -> Box<dyn Screen> {
    Box::new(NewPlaylist {
        collection,
        ids,
        name: String::new(),
        busy: false,
        error: None,
        focus: Focus { group: N_NAME, index: 0 },
        groups: vec![],
    })
}

impl NewPlaylist {
    fn create(&mut self, cx: &Cx) {
        let name = self.name.trim().to_string();
        if self.busy {
            return;
        }
        if name.is_empty() {
            self.error = Some("Give it a name.".to_string());
            return;
        }
        self.busy = true;
        self.error = None;
        let ids = self.ids.clone();
        let client = cx.api();
        let collection = self.collection;
        cx.spawn("created", move || {
            let made =
                if collection { client.create_collection(&name, &ids) } else { client.create_playlist(&name, &ids) };
            made.map(|id| (id, name))
        });
    }
}

impl Screen for NewPlaylist {
    fn name(&self) -> &'static str {
        if self.collection {
            "new-collection"
        } else {
            "new-playlist"
        }
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        match key {
            Key::Enter => {
                match (self.focus.group, self.focus.index) {
                    (N_NAME, _) => self.create(cx),
                    (_, 0) => self.create(cx),
                    _ => cx.pop(),
                }
                true
            }
            Key::Up | Key::Down | Key::Right => {
                self.focus = move_focus(&self.groups, self.focus, key);
                true
            }
            Key::Left => {
                if self.focus.group == N_BUTTONS {
                    self.focus = move_focus(&self.groups, self.focus, key);
                }
                true
            }
            _ => false,
        }
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn std::any::Any + Send>) {
        if tag != "created" {
            return;
        }
        let Ok(r) = payload.downcast::<Result<(String, String)>>() else { return };
        self.busy = false;
        match *r {
            Ok((id, name)) => {
                cx.pop();
                if self.collection {
                    cx.push(collection::screen(Item { id, name, kind: ItemKind::BoxSet, ..Default::default() }));
                } else {
                    cx.push(screen(Item { id, name, ..Default::default() }));
                }
            }
            Err(ApiError::Unauthorized) => cx.signed_out(),
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        let pointer_moved = ui.input(|i| i.pointer.is_moving());
        ui.add_space(t.margin);
        parts::title(ui, t, if self.collection { "New collection" } else { "New playlist" });
        ui.add_space(t.gap / 2.0);
        let r = parts::text_field(ui, t, &mut self.name, name_hint(self.collection), self.focus.group == N_NAME, false);
        if self.focus.group == N_NAME {
            super::keyboard::mark_name(ui.ctx());
        }
        let name_x = r.rect.center().x;
        if r.clicked() {
            self.focus = Focus { group: N_NAME, index: 0 };
        }
        if let Some(e) = &self.error {
            ui.add_space(t.gap / 3.0);
            parts::error_line(ui, t, e);
        }
        ui.add_space(t.gap / 2.0);
        let mut xs = vec![];
        let mut act = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = t.gap / 2.0;
            let label = if self.busy { "Creating…" } else { "Create" };
            let r = parts::button(ui, t, label, self.focus == Focus { group: N_BUTTONS, index: 0 }, true);
            xs.push(r.rect.center().x);
            if r.hovered() && pointer_moved {
                self.focus = Focus { group: N_BUTTONS, index: 0 };
            }
            if r.clicked() {
                act = Some(true);
            }
            let r = parts::button(ui, t, "Cancel", self.focus == Focus { group: N_BUTTONS, index: 1 }, false);
            xs.push(r.rect.center().x);
            if r.hovered() && pointer_moved {
                self.focus = Focus { group: N_BUTTONS, index: 1 };
            }
            if r.clicked() {
                act = Some(false);
            }
        });
        self.groups = vec![Group { len: 1, cols: 1, xs: vec![name_x] }, Group { len: 2, cols: 2, xs }];
        match act {
            Some(true) => self.create(cx),
            Some(false) => cx.pop(),
            None => {}
        }
    }
}
