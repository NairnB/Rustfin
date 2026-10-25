//! RR5 (P-ITEM-3, P-ITEM-21): the "filtered items" grid — every film and show of one genre or studio — with a
//! Shuffle button, plus the shuffle helpers the collection page shares.
use super::parts;
use super::player::{self, PlayOptions};
use crate::api::{ApiError, Client, Item, Result};
use crate::app::{Cx, Screen};
use crate::nav::{self, move_focus, Focus, Group};
use crate::platform::Key;

const G_SHUFFLE: usize = 0;
const G_GRID: usize = 1;
const G_RETRY: usize = 2;

/// What the screen lists.
#[derive(Clone, Debug, PartialEq)]
pub enum Filter {
    Genre(String),
    Studio { id: String, name: String },
}

impl Filter {
    fn title(&self) -> String {
        match self {
            Filter::Genre(g) => g.clone(),
            Filter::Studio { name, .. } => name.clone(),
        }
    }

    fn load(&self, c: &Client) -> Result<Vec<Item>> {
        match self {
            Filter::Genre(g) => c.items_by_genre(g),
            Filter::Studio { id, .. } => c.items_by_studio(id),
        }
    }
}

pub fn genre(name: &str) -> Box<dyn Screen> {
    screen(Filter::Genre(name.to_string()))
}

pub fn studio(id: &str, name: &str) -> Box<dyn Screen> {
    screen(Filter::Studio { id: id.to_string(), name: name.to_string() })
}

pub fn screen(filter: Filter) -> Box<dyn Screen> {
    Box::new(Browse {
        filter,
        items: None,
        started: false,
        busy: false,
        focus: Focus { group: G_GRID, index: 0 },
        groups: vec![],
    })
}

struct Browse {
    filter: Filter,
    items: Option<Result<Vec<Item>>>,
    started: bool,
    busy: bool,
    focus: Focus,
    groups: Vec<Group>,
}

/// `items` in a random order from `seed` (xorshift + Fisher–Yates).
pub fn shuffled(mut items: Vec<Item>, seed: u64) -> Vec<Item> {
    let mut x = seed | 1;
    for i in (1..items.len()).rev() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        items.swap(i, (x % (i as u64 + 1)) as usize);
    }
    items
}

fn seed() -> u64 {
    let mut b = [0u8; 8];
    let _ = getrandom::fill(&mut b);
    u64::from_le_bytes(b)
        ^ crate::time::SystemTime::now()
            .duration_since(crate::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0)
}

/// P-ITEM-21: looks up the playable children of `items` in the background; `finish_shuffle` plays them.
pub fn start_shuffle(cx: &Cx, items: Vec<Item>) {
    let client = cx.api();
    cx.spawn("shuffle", move || client.playable_under(&items));
}

/// Handles the "shuffle" answer: queues the playable items in random order. Returns false for a sign-out or an
/// error (nothing is queued), true when a queue was pushed or there was nothing to play.
pub fn finish_shuffle(cx: &mut Cx, payload: Box<dyn std::any::Any + Send>) {
    let Ok(r) = payload.downcast::<Result<Vec<Item>>>() else { return };
    match *r {
        Ok(v) if !v.is_empty() => {
            let q = shuffled(v, seed());
            cx.push(player::screen_queue(q, PlayOptions { from_start: true, ..Default::default() }));
        }
        Ok(_) => {}
        Err(ApiError::Unauthorized) => cx.signed_out(),
        Err(_) => {}
    }
}

impl Browse {
    fn start(&mut self, cx: &Cx) {
        self.items = None;
        let client = cx.api();
        let filter = self.filter.clone();
        cx.spawn("items", move || filter.load(&client));
    }

    fn item_at(&self, i: usize) -> Option<Item> {
        self.items.as_ref()?.as_ref().ok()?.get(i).cloned()
    }

    fn shuffle(&mut self, cx: &mut Cx) {
        if let Some(Ok(v)) = &self.items {
            if !v.is_empty() && !self.busy {
                self.busy = true;
                start_shuffle(cx, v.clone());
            }
        }
    }
}

impl Screen for Browse {
    fn name(&self) -> &'static str {
        "browse"
    }

    /// NAV: this screen's part of the toolbar breadcrumb.
    fn crumbs(&self) -> Vec<String> {
        vec![self.filter.title()]
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        match key {
            Key::Enter => {
                match self.focus.group {
                    G_SHUFFLE => self.shuffle(cx),
                    G_RETRY => self.start(cx),
                    _ => {
                        if let Some(it) = self.item_at(self.focus.index) {
                            cx.push(super::item::screen(it));
                        }
                    }
                }
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
            "items" => {
                let Ok(r) = payload.downcast::<Result<Vec<Item>>>() else { return };
                match &*r {
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    _ => self.items = Some(*r),
                }
            }
            "shuffle" => {
                self.busy = false;
                finish_shuffle(cx, payload);
            }
            _ => {}
        }
    }

    fn on_resume(&mut self, cx: &mut Cx) {
        self.start(cx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        if !self.started {
            self.started = true;
            self.start(cx);
        }
        let t = cx.theme;
        let pointer_moved = ui.input(|i| i.pointer.is_moving());

        ui.add_space(t.margin);
        parts::title(ui, t, &self.filter.title());
        ui.add_space(t.gap / 2.0);

        let items = match &self.items {
            None => {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = t.gap;
                    for _ in 0..6 {
                        parts::skeleton_card(ui, t, parts::CardKind::Poster);
                    }
                });
                self.groups = vec![];
                return;
            }
            Some(Err(e)) => {
                parts::error_line(ui, t, &e.to_string());
                let r = parts::button(ui, t, "Retry", true, false);
                if r.clicked() {
                    self.start(cx);
                }
                self.focus = Focus { group: G_RETRY, index: 0 };
                let mut groups = vec![Group::default(); 3];
                groups[G_RETRY] = Group { len: 1, cols: 1, xs: vec![r.rect.center().x] };
                self.groups = groups;
                return;
            }
            Some(Ok(v)) => v.clone(),
        };

        if items.is_empty() {
            super::at_library::empty_state(ui, t, "Nothing here", "Nothing in your libraries matches.");
            self.groups = vec![];
            return;
        }

        let r = parts::button(ui, t, "Shuffle", self.focus.group == G_SHUFFLE, true);
        let shuffle_x = r.rect.center().x;
        if pointer_moved && r.hovered() && parts::hover_moves_focus(ui.ctx()) {
            self.focus = Focus { group: G_SHUFFLE, index: 0 };
        }
        let shuffle_clicked = r.clicked();
        ui.add_space(t.gap / 2.0);

        let cols = nav::grid_cols(ui.available_width(), 0.0, t.poster_w, t.gap);
        let cells: Vec<Option<Item>> = items.iter().cloned().map(Some).collect();
        let mut out = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            // Room for the focused card's lift and shadow at the top edge of the scroll area.
            ui.add_space(t.poster_h * (t.focus_scale - 1.0) / 2.0 + t.shadow_blur / 4.0);
            out = Some(parts::grid(
                ui,
                cx,
                &cells,
                cols,
                (self.focus.group == G_GRID).then_some(self.focus.index),
                items.len(),
            ));
        });
        let out = out.unwrap();
        let mut groups = vec![Group::default(); 3];
        groups[G_SHUFFLE] = Group { len: 1, cols: 1, xs: vec![shuffle_x] };
        groups[G_GRID] = Group { len: items.len(), cols, xs: out.xs };
        self.groups = groups;
        self.focus = move_focus(&self.groups, self.focus, Key::Enter);
        if pointer_moved {
            if let Some(i) = out.hovered {
                self.focus = Focus { group: G_GRID, index: i };
            }
        }
        if shuffle_clicked {
            self.shuffle(cx);
        }
        if let Some(i) = out.clicked {
            if let Some(it) = self.item_at(i) {
                cx.push(super::item::screen(it));
            }
        }
    }
}
