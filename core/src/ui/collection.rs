//! G7 collections: the BoxSet page (P-COLL-1) and the grid of collections (P-LIB-17). `grid_screen` is the
//! shared "title + poster grid of items loaded in one call" screen (the playlists list uses it too).
use super::player::{self, PlayOptions};
use super::{parts, widgets};
use crate::api::{ApiError, Client, Item, ItemKind, Result};
use crate::app::{Cx, Screen};
use crate::nav::{self, move_focus, Focus, Group};
use crate::order;
use crate::platform::Key;
use crate::tags;

// ---------- the grid screen ----------

const GRID: usize = 0;
const GRID_RETRY: usize = 1;

pub struct ItemGrid {
    name: &'static str,
    title: &'static str,
    empty: &'static str,
    load: fn(&Client) -> Result<Vec<Item>>,
    open: fn(Item) -> Box<dyn Screen>,
    items: Option<Result<Vec<Item>>>,
    started: bool,
    focus: Focus,
    groups: Vec<Group>,
}

/// A screen named `name` showing `title` over a poster grid of whatever `load` returns; Enter opens `open(item)`.
pub fn grid_screen(
    name: &'static str,
    title: &'static str,
    empty: &'static str,
    load: fn(&Client) -> Result<Vec<Item>>,
    open: fn(Item) -> Box<dyn Screen>,
) -> Box<dyn Screen> {
    Box::new(ItemGrid {
        name,
        title,
        empty,
        load,
        open,
        items: None,
        started: false,
        focus: Focus { group: GRID, index: 0 },
        groups: vec![],
    })
}

/// P-LIB-17: every collection on the server.
pub fn list() -> Box<dyn Screen> {
    grid_screen("collections", "Collections", "No collections on this server yet.", |c| c.collections(), screen)
}

impl ItemGrid {
    fn start(&mut self, cx: &Cx) {
        self.items = None;
        let client = cx.api();
        let load = self.load;
        cx.spawn("items", move || load(&client));
    }

    fn item_at(&self, i: usize) -> Option<Item> {
        self.items.as_ref()?.as_ref().ok()?.get(i).cloned()
    }
}

impl Screen for ItemGrid {
    fn name(&self) -> &'static str {
        self.name
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        match key {
            Key::Enter => {
                if self.focus.group == GRID_RETRY {
                    self.start(cx);
                } else if let Some(it) = self.item_at(self.focus.index) {
                    cx.push((self.open)(it));
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
        if tag != "items" {
            return;
        }
        let Ok(r) = payload.downcast::<Result<Vec<Item>>>() else { return };
        match &*r {
            Err(ApiError::Unauthorized) => cx.signed_out(),
            _ => self.items = Some(*r),
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
        parts::title(ui, t, self.title);
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
                self.focus = Focus { group: GRID_RETRY, index: 0 };
                self.groups = vec![Group::default(), Group { len: 1, cols: 1, xs: vec![r.rect.center().x] }];
                return;
            }
            Some(Ok(v)) => v.clone(),
        };

        if items.is_empty() {
            parts::muted(ui, t, self.empty);
            self.groups = vec![];
            return;
        }

        let cols = nav::grid_cols(ui.available_width(), 0.0, t.poster_w, t.gap);
        let cells: Vec<Option<Item>> = items.iter().cloned().map(Some).collect();
        let mut out = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            out = Some(parts::grid(
                ui,
                cx,
                &cells,
                cols,
                (self.focus.group == GRID).then_some(self.focus.index),
                items.len(),
            ));
        });
        let out = out.unwrap();
        self.groups = vec![Group { len: items.len(), cols, xs: out.xs }];
        self.focus = move_focus(&self.groups, self.focus, Key::Enter);
        if pointer_moved {
            if let Some(i) = out.hovered {
                self.focus = Focus { group: GRID, index: i };
            }
        }
        if let Some(i) = out.clicked {
            if let Some(it) = self.item_at(i) {
                cx.push((self.open)(it));
            }
        }
    }
}

// ---------- the collection page ----------

const G_PLAY: usize = 0;
const G_SORT: usize = 1;
const G_ITEMS: usize = 2;
const G_RETRY: usize = 3;

/// How the items of a collection are ordered on the page.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Sort {
    Server,
    Name,
    Year,
    MyOrder,
}

const SORTS: [(Sort, &str); 4] = [
    (Sort::Server, "Collection order"),
    (Sort::Name, "Name"),
    (Sort::Year, "Release year"),
    (Sort::MyOrder, "My order"),
];

pub struct Collection {
    item: Item,
    full: Option<Item>,
    items: Option<Result<Vec<Item>>>,
    sort: Sort,
    dropdown: widgets::DropdownState,
    started: bool,
    focus: Focus,
    groups: Vec<Group>,
}

/// P-COLL-1: a BoxSet page.
pub fn screen(item: Item) -> Box<dyn Screen> {
    Box::new(Collection {
        item,
        full: None,
        items: None,
        sort: Sort::Server,
        dropdown: widgets::DropdownState::default(),
        started: false,
        focus: Focus { group: G_PLAY, index: 0 },
        groups: vec![],
    })
}

/// The collection's items in the chosen order.
fn ordered(items: &[Item], sort: Sort, collection_id: &str) -> Vec<Item> {
    let mut v = items.to_vec();
    match sort {
        Sort::Server => {}
        Sort::Name => v.sort_by_key(|i| i.name.to_lowercase()),
        Sort::Year => v.sort_by(|a, b| {
            (a.production_year.unwrap_or(i32::MAX), a.name.to_lowercase())
                .cmp(&(b.production_year.unwrap_or(i32::MAX), b.name.to_lowercase()))
        }),
        Sort::MyOrder => order::sort_my_order(&mut v, &tags::place_id(collection_id)),
    }
    v
}

impl Collection {
    fn current(&self) -> &Item {
        self.full.as_ref().unwrap_or(&self.item)
    }

    fn start(&mut self, cx: &Cx) {
        self.items = None;
        let id = self.item.id.clone();
        let client = cx.api();
        cx.spawn("items", move || client.collection_items(&id));
        let id = self.item.id.clone();
        let client = cx.api();
        cx.spawn("item", move || client.item(&id));
    }

    fn shown(&self) -> Vec<Item> {
        match &self.items {
            Some(Ok(v)) => ordered(v, self.sort, &self.item.id),
            _ => vec![],
        }
    }

    fn play_all(&self, cx: &mut Cx) {
        let first = self.shown().into_iter().find(|i| matches!(i.kind, ItemKind::Movie | ItemKind::Episode));
        if let Some(it) = first {
            cx.push(player::screen(it, PlayOptions { from_start: true, ..Default::default() }));
        }
    }

    fn pick_sort(&mut self, i: usize) {
        if let Some((s, _)) = SORTS.get(i) {
            self.sort = *s;
        }
    }
}

impl Screen for Collection {
    fn name(&self) -> &'static str {
        "collection"
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        if self.dropdown.open {
            let (state, ev) = widgets::dropdown_key(self.dropdown, key, SORTS.len());
            self.dropdown = state;
            if let Some(widgets::DropdownEvent::Pick(i)) = ev {
                self.pick_sort(i);
            }
            return matches!(key, Key::Up | Key::Down | Key::Enter | Key::Back);
        }
        match key {
            Key::Enter => {
                match self.focus.group {
                    G_PLAY => self.play_all(cx),
                    G_SORT => {
                        let (state, _) = widgets::dropdown_key(self.dropdown, key, SORTS.len());
                        self.dropdown = state;
                    }
                    G_RETRY => self.start(cx),
                    _ => {
                        if let Some(it) = self.shown().get(self.focus.index).cloned() {
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
            "item" => {
                let Ok(r) = payload.downcast::<Result<Item>>() else { return };
                match *r {
                    Ok(it) => self.full = Some(it),
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(_) => {}
                }
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
        let shown = self.shown();
        let can_play = shown.iter().any(|i| matches!(i.kind, ItemKind::Movie | ItemKind::Episode));

        let mut play_clicked = false;
        let mut hovered: Option<Focus> = None;
        let mut groups = vec![Group::default(); 4];
        let mut grid_out = None;
        let mut retry_clicked = false;

        let cols = nav::grid_cols(ui.available_width(), 0.0, t.poster_w, t.gap);
        egui::ScrollArea::vertical().show(ui, |ui| {
            let rect = parts::backdrop(ui, cx, self.current());
            let mut col = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(t.margin)));
            col.set_max_width(t.field_w * 1.5);
            col.add_space(t.backdrop_h * 0.15);
            parts::title(&mut col, t, &self.current().name);
            if let Some(Ok(v)) = &self.items {
                col.add_space(t.gap / 4.0);
                parts::muted(&mut col, t, &format!("{} {}", v.len(), if v.len() == 1 { "item" } else { "items" }));
            }
            if let Some(ov) = self.current().overview.as_deref().map(str::trim).filter(|o| !o.is_empty()) {
                col.add_space(t.gap / 2.0);
                col.add(egui::Label::new(egui::RichText::new(ov).size(t.body_size).color(t.muted)).wrap());
            }
            ui.add_space(t.gap);

            if let Some(Err(e)) = &self.items {
                parts::error_line(ui, t, &e.to_string());
                let r = parts::button(ui, t, "Retry", self.focus.group == G_RETRY, false);
                if r.hovered() {
                    hovered = Some(Focus { group: G_RETRY, index: 0 });
                }
                retry_clicked = r.clicked();
                groups[G_RETRY] = Group { len: 1, cols: 1, xs: vec![r.rect.center().x] };
                return;
            }
            if self.items.is_none() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = t.gap;
                    for _ in 0..6 {
                        parts::skeleton_card(ui, t, parts::CardKind::Poster);
                    }
                });
                return;
            }

            if can_play {
                let r = parts::button(ui, t, "Play all", self.focus.group == G_PLAY, true);
                if r.hovered() {
                    hovered = Some(Focus { group: G_PLAY, index: 0 });
                }
                play_clicked = r.clicked();
                groups[G_PLAY] = Group { len: 1, cols: 1, xs: vec![r.rect.center().x] };
                ui.add_space(t.gap / 2.0);
            }

            let labels: Vec<String> = SORTS.iter().map(|(_, l)| l.to_string()).collect();
            if !self.dropdown.open {
                self.dropdown.highlighted = SORTS.iter().position(|(s, _)| *s == self.sort).unwrap_or(0);
            }
            let d = widgets::dropdown(ui, t, &mut self.dropdown, "Sort", &labels, self.focus.group == G_SORT);
            if d.button_hovered && !self.dropdown.open {
                hovered = Some(Focus { group: G_SORT, index: 0 });
            }
            if let Some(i) = d.item_hovered {
                hovered = Some(Focus { group: G_SORT, index: i });
            }
            if let Some(i) = d.item_clicked {
                self.dropdown.open = false;
                self.pick_sort(i);
            }
            groups[G_SORT] = if self.dropdown.open {
                Group { len: SORTS.len(), cols: 1, xs: d.item_xs }
            } else {
                Group { len: 1, cols: 1, xs: vec![d.button_x] }
            };
            ui.add_space(t.gap);

            let cells: Vec<Option<Item>> = shown.iter().cloned().map(Some).collect();
            let out = parts::grid(
                ui,
                cx,
                &cells,
                cols,
                (self.focus.group == G_ITEMS).then_some(self.focus.index),
                shown.len(),
            );
            groups[G_ITEMS] = Group { len: shown.len(), cols, xs: out.xs.clone() };
            grid_out = Some(out);
        });

        self.groups = groups;
        self.focus = move_focus(&self.groups, self.focus, Key::Enter);
        if pointer_moved {
            if let Some(f) = hovered {
                self.focus = f;
            }
            if let Some(i) = grid_out.as_ref().and_then(|o| o.hovered) {
                self.focus = Focus { group: G_ITEMS, index: i };
            }
        }
        if play_clicked {
            self.play_all(cx);
        }
        if retry_clicked {
            self.start(cx);
        }
        if let Some(i) = grid_out.and_then(|o| o.clicked) {
            if let Some(it) = shown.get(i).cloned() {
                cx.push(super::item::screen(it));
            }
        }
    }
}
