//! G3 favourites page (P-COLL-6): the user's favourites grouped into Films / Shows / Episodes / People.
//! Reached from the Home top bar. Loads `/Items?Filters=IsFavorite…` and `/Persons?isFavorite=true`.
use super::parts::{self, CardKind, RowOut};
use crate::api::{image_width_bucket, ApiError, ImageKind, Item, ItemKind, Result};
use crate::app::{Cx, Screen};
use crate::nav::{move_focus, Focus, Group};
use crate::platform::Key;
use egui::{pos2, vec2, Align2, Rect, Sense};

const G_FILMS: usize = 0;
const G_SHOWS: usize = 1;
const G_EPS: usize = 2;
const G_PEOPLE: usize = 3;
const G_RETRY: usize = 4;

pub struct Favourites {
    items: Option<Result<Vec<Item>>>,
    people: Option<Result<Vec<Item>>>,
    started: bool,
    focus: Focus,
    groups: Vec<Group>,
}

pub fn screen() -> Box<dyn Screen> {
    Box::new(Favourites {
        items: None,
        people: None,
        started: false,
        focus: Focus { group: G_FILMS, index: 0 },
        groups: vec![],
    })
}

impl Favourites {
    fn start(&mut self, cx: &Cx) {
        self.items = None;
        self.people = None;
        let client = cx.api();
        cx.spawn("favourites", move || client.favourites("Movie,Series,Episode"));
        let client = cx.api();
        cx.spawn("people", move || client.favourite_people());
    }

    fn by_kind(&self, kind: ItemKind) -> Vec<Item> {
        self.items
            .as_ref()
            .and_then(|r| r.as_ref().ok())
            .map(|v| v.iter().filter(|i| i.kind == kind).cloned().collect())
            .unwrap_or_default()
    }

    fn people(&self) -> Vec<Item> {
        self.people.as_ref().and_then(|r| r.as_ref().ok()).cloned().unwrap_or_default()
    }

    fn error(&self) -> Option<String> {
        match (&self.items, &self.people) {
            (Some(Err(e)), _) | (_, Some(Err(e))) => Some(e.to_string()),
            _ => None,
        }
    }

    fn any_results(&self) -> bool {
        !self.by_kind(ItemKind::Movie).is_empty()
            || !self.by_kind(ItemKind::Series).is_empty()
            || !self.by_kind(ItemKind::Episode).is_empty()
            || !self.people().is_empty()
    }

    fn activate(&mut self, cx: &mut Cx, group: usize, index: usize) {
        match group {
            G_FILMS => {
                if let Some(it) = self.by_kind(ItemKind::Movie).get(index).cloned() {
                    cx.push(super::item::screen(it));
                }
            }
            G_SHOWS => {
                if let Some(it) = self.by_kind(ItemKind::Series).get(index).cloned() {
                    cx.push(super::item::screen(it));
                }
            }
            G_EPS => {
                if let Some(it) = self.by_kind(ItemKind::Episode).get(index).cloned() {
                    cx.push(super::item::screen(it));
                }
            }
            G_PEOPLE => {
                if let Some(p) = self.people().get(index).cloned() {
                    cx.push(super::person::screen(p.id, p.name));
                }
            }
            _ => {}
        }
    }
}

fn initials(name: &str) -> String {
    name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect()
}

fn person_card(ui: &mut egui::Ui, cx: &mut Cx, person: &Item, focused: bool) -> egui::Response {
    let t = cx.theme;
    let size = vec2(t.person_size, t.person_size);
    let (rect, resp) = ui.allocate_exact_size(vec2(size.x, size.y + t.card_label_h), Sense::click());
    if ui.is_rect_visible(rect) {
        let img_rect = Rect::from_min_size(rect.min, size);
        let w = image_width_bucket(size.x, ui.ctx().pixels_per_point());
        let url = cx.client.as_ref().and_then(|c| c.image_url(person, ImageKind::Primary, w));
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
                        initials(&person.name),
                        t.font(t.heading_size),
                        t.muted,
                    );
                }
            }
        }
        if focused {
            parts::focus_ring(ui, t, img_rect);
        }
        parts::label_line(ui, t, pos2(rect.left(), img_rect.bottom() + 6.0), size.x, &person.name, t.body_size, t.text);
    }
    resp
}

pub(super) fn people_row(ui: &mut egui::Ui, cx: &mut Cx, people: &[Item], focus: Option<usize>) -> RowOut {
    let t = cx.theme;
    let mut out = RowOut { clicked: None, hovered: None, xs: vec![] };
    ui.add_space(t.row_gap / 2.0);
    parts::heading(ui, t, "People");
    ui.add_space(t.gap / 3.0);
    egui::ScrollArea::horizontal()
        .id_salt("favourite-people")
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = t.gap;
                for (i, p) in people.iter().enumerate() {
                    let r = person_card(ui, cx, p, focus == Some(i));
                    out.xs.push(r.rect.center().x);
                    if focus == Some(i) {
                        r.scroll_to_me(None);
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

impl Screen for Favourites {
    fn name(&self) -> &'static str {
        "favourites"
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        match key {
            Key::Enter => {
                if self.focus.group == G_RETRY {
                    self.start(cx);
                    return true;
                }
                if self.focus.group <= G_PEOPLE {
                    self.activate(cx, self.focus.group, self.focus.index);
                    return true;
                }
                false
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
            "favourites" => {
                let Ok(r) = payload.downcast::<Result<Vec<Item>>>() else { return };
                match &*r {
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    _ => self.items = Some(*r),
                }
            }
            "people" => {
                let Ok(r) = payload.downcast::<Result<Vec<Item>>>() else { return };
                match &*r {
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    _ => self.people = Some(*r),
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

        ui.add_space(t.margin);
        parts::title(ui, t, "Favourites");
        ui.add_space(t.gap / 2.0);

        let loading = self.items.is_none() || self.people.is_none();
        if loading {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = t.gap;
                for _ in 0..6 {
                    parts::skeleton_card(ui, t, CardKind::Poster);
                }
            });
            self.groups = vec![];
            return;
        }

        if let Some(e) = self.error() {
            parts::error_line(ui, t, &e);
            let focused = self.focus.group == G_RETRY;
            let r = parts::button(ui, t, "Retry", focused, false);
            let xs = vec![r.rect.center().x];
            if r.hovered() {
                self.focus = Focus { group: G_RETRY, index: 0 };
            }
            if r.clicked() {
                self.start(cx);
            }
            self.groups = vec![Group { len: 1, cols: 1, xs }];
            return;
        }

        let films = self.by_kind(ItemKind::Movie);
        let shows = self.by_kind(ItemKind::Series);
        let eps = self.by_kind(ItemKind::Episode);
        let people = self.people();

        let mut films_xs: Vec<f32> = vec![];
        let mut shows_xs: Vec<f32> = vec![];
        let mut eps_xs: Vec<f32> = vec![];
        let mut people_xs: Vec<f32> = vec![];
        let mut clicked: Option<Focus> = None;
        let mut hovered: Option<Focus> = None;

        egui::ScrollArea::vertical().show(ui, |ui| {
            if !films.is_empty() {
                let focus = (self.focus.group == G_FILMS).then_some(self.focus.index);
                let out = parts::row_favourites(ui, cx, "fav-films", "Films", &films, CardKind::Poster, focus);
                films_xs = out.xs;
                if let Some(i) = out.hovered {
                    hovered = Some(Focus { group: G_FILMS, index: i });
                }
                if let Some(i) = out.clicked {
                    clicked = Some(Focus { group: G_FILMS, index: i });
                }
            }
            if !shows.is_empty() {
                let focus = (self.focus.group == G_SHOWS).then_some(self.focus.index);
                let out = parts::row_favourites(ui, cx, "fav-shows", "Shows", &shows, CardKind::Poster, focus);
                shows_xs = out.xs;
                if let Some(i) = out.hovered {
                    hovered = Some(Focus { group: G_SHOWS, index: i });
                }
                if let Some(i) = out.clicked {
                    clicked = Some(Focus { group: G_SHOWS, index: i });
                }
            }
            if !eps.is_empty() {
                let focus = (self.focus.group == G_EPS).then_some(self.focus.index);
                let out = parts::row_favourites(ui, cx, "fav-eps", "Episodes", &eps, CardKind::Thumb, focus);
                eps_xs = out.xs;
                if let Some(i) = out.hovered {
                    hovered = Some(Focus { group: G_EPS, index: i });
                }
                if let Some(i) = out.clicked {
                    clicked = Some(Focus { group: G_EPS, index: i });
                }
            }
            if !people.is_empty() {
                let focus = (self.focus.group == G_PEOPLE).then_some(self.focus.index);
                let out = people_row(ui, cx, &people, focus);
                people_xs = out.xs;
                if let Some(i) = out.hovered {
                    hovered = Some(Focus { group: G_PEOPLE, index: i });
                }
                if let Some(i) = out.clicked {
                    clicked = Some(Focus { group: G_PEOPLE, index: i });
                }
            }
            if !self.any_results() {
                ui.add_space(t.gap);
                parts::muted(ui, t, "No favourites yet. Mark an item with the heart on its page.");
            }
        });

        let mut groups = vec![Group::default(); 5];
        groups[G_FILMS] = Group { len: films.len(), cols: films.len(), xs: films_xs };
        groups[G_SHOWS] = Group { len: shows.len(), cols: shows.len(), xs: shows_xs };
        groups[G_EPS] = Group { len: eps.len(), cols: eps.len(), xs: eps_xs };
        groups[G_PEOPLE] = Group { len: people.len(), cols: people.len(), xs: people_xs };
        self.groups = groups;
        self.focus = move_focus(&self.groups, self.focus, Key::Enter);

        if pointer_moved {
            if let Some(f) = hovered {
                self.focus = f;
            }
        }
        if let Some(f) = clicked {
            self.activate(cx, f.group, f.index);
        }
    }
}
