//! R-UI-8 Search: a text field (focused on open) and results grouped into "Films", "Shows", "Episodes",
//! "Collections" and "People" rows; before typing, "Suggested" (G13). Rows. Typing is debounced by `theme.search_debounce`; stale results are dropped with a sequence number.
use super::parts::{self, CardKind};
use crate::api::{ApiError, Item, ItemKind, Result};
use crate::app::{Cx, Screen};
use crate::nav::{move_focus, Focus, Group};
use crate::platform::Key;

const G_FIELD: usize = 0;
const G_FILMS: usize = 1;
const G_SHOWS: usize = 2;
const G_EPS: usize = 3;
const G_COLLS: usize = 4;
const G_PEOPLE: usize = 5;
const G_SUGGESTED: usize = 6;

pub struct Search {
    query: String,
    last_query: String,
    films: Vec<Item>,
    shows: Vec<Item>,
    episodes: Vec<Item>,
    collections: Vec<Item>,
    people: Vec<Item>,
    suggested: Vec<Item>,
    suggested_started: bool,
    loading: bool,
    error: Option<String>,
    deadline: Option<f64>,
    seq: u64,
    focus: Focus,
    groups: Vec<Group>,
}

pub fn screen() -> Box<dyn Screen> {
    Box::new(Search {
        query: String::new(),
        last_query: String::new(),
        films: vec![],
        shows: vec![],
        episodes: vec![],
        collections: vec![],
        people: vec![],
        suggested: vec![],
        suggested_started: false,
        loading: false,
        error: None,
        deadline: None,
        seq: 0,
        focus: Focus { group: G_FIELD, index: 0 },
        groups: vec![],
    })
}

impl Search {
    fn any_results(&self) -> bool {
        !self.films.is_empty()
            || !self.shows.is_empty()
            || !self.episodes.is_empty()
            || !self.collections.is_empty()
            || !self.people.is_empty()
    }

    fn set_results(&mut self, items: Vec<Item>, people: Vec<Item>) {
        self.collections = items.iter().filter(|i| i.kind == ItemKind::BoxSet).cloned().collect();
        self.people = people;
        self.films = items.iter().filter(|i| i.kind == ItemKind::Movie).cloned().collect();
        self.shows = items.iter().filter(|i| i.kind == ItemKind::Series).cloned().collect();
        self.episodes = items.iter().filter(|i| i.kind == ItemKind::Episode).cloned().collect();
    }

    fn result_at(&self, f: Focus) -> Option<Item> {
        match f.group {
            G_FILMS => self.films.get(f.index).cloned(),
            G_SHOWS => self.shows.get(f.index).cloned(),
            G_EPS => self.episodes.get(f.index).cloned(),
            G_COLLS => self.collections.get(f.index).cloned(),
            G_PEOPLE => self.people.get(f.index).cloned(),
            G_SUGGESTED => self.suggested.get(f.index).cloned(),
            _ => None,
        }
    }

    fn maybe_search(&mut self, cx: &mut Cx) {
        if self.query != self.last_query {
            self.last_query = self.query.clone();
            self.deadline = Some(cx.now + cx.theme.search_debounce as f64);
            return;
        }
        let Some(d) = self.deadline else { return };
        if cx.now < d {
            return;
        }
        self.deadline = None;
        let term = self.query.trim().to_string();
        if term.is_empty() {
            self.films.clear();
            self.shows.clear();
            self.episodes.clear();
            self.collections.clear();
            self.people.clear();
            self.seq += 1; // an answer still on its way is stale
            self.loading = false;
            return;
        }
        self.seq += 1;
        let seq = self.seq;
        self.loading = true;
        let client = cx.api();
        cx.spawn("search", move || {
            // Items and people are two calls; run them side by side. People failing just means no People row.
            std::thread::scope(|s| {
                let people = s.spawn(|| client.search_people(&term).unwrap_or_default());
                let items = client.search(&term);
                (seq, items.map(|i| (i, people.join().unwrap_or_default())))
            })
        });
    }
}

impl Screen for Search {
    fn name(&self) -> &'static str {
        "search"
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        match key {
            Key::Enter => {
                if self.focus.group != G_FIELD {
                    if let Some(it) = self.result_at(self.focus) {
                        open(cx, it, self.focus.group == G_PEOPLE);
                    }
                }
                true
            }
            Key::Up | Key::Down | Key::Left | Key::Right => {
                if self.focus.group == G_FIELD && matches!(key, Key::Left | Key::Right) {
                    return true;
                }
                self.focus = move_focus(&self.groups, self.focus, key);
                true
            }
            Key::Back => false,
            _ => false,
        }
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn std::any::Any + Send>) {
        if tag == "suggested" {
            if let Ok(r) = payload.downcast::<Result<Vec<Item>>>() {
                match *r {
                    Ok(items) => self.suggested = items,
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(_) => {}
                }
            }
            return;
        }
        if tag != "search" {
            return;
        }
        let Ok(r) = payload.downcast::<(u64, Result<(Vec<Item>, Vec<Item>)>)>() else { return };
        let (seq, result) = *r;
        if seq != self.seq {
            return;
        }
        self.loading = false;
        match result {
            Ok((items, people)) => {
                self.set_results(items, people);
                self.error = None;
            }
            Err(ApiError::Unauthorized) => cx.signed_out(),
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        if !self.suggested_started {
            self.suggested_started = true;
            let client = cx.api();
            cx.spawn("suggested", move || client.suggestions(12));
        }
        self.maybe_search(cx);

        ui.add_space(t.margin);
        parts::heading(ui, t, "Search");
        ui.add_space(t.gap / 2.0);

        let focused = self.focus.group == G_FIELD;
        let r = parts::text_field(ui, t, &mut self.query, "Search films, shows, people and more", focused, false);
        let field_xs = vec![r.rect.center().x];
        if r.clicked() {
            self.focus = Focus { group: G_FIELD, index: 0 };
        }

        let mut films_xs: Vec<f32> = vec![];
        let mut shows_xs: Vec<f32> = vec![];
        let mut eps_xs: Vec<f32> = vec![];
        let mut colls_xs: Vec<f32> = vec![];
        let mut people_xs: Vec<f32> = vec![];
        let mut sugg_xs: Vec<f32> = vec![];
        let mut clicked: Option<Item> = None;
        let mut clicked_person = false;

        if !self.films.is_empty() {
            let focus = (self.focus.group == G_FILMS).then_some(self.focus.index);
            let out = parts::row(ui, cx, "search-films", "Films", &self.films, CardKind::Poster, focus);
            films_xs = out.xs;
            if let Some(i) = out.hovered {
                self.focus = Focus { group: G_FILMS, index: i };
            }
            if let Some(i) = out.clicked {
                clicked = self.films.get(i).cloned();
            }
        }
        if !self.shows.is_empty() {
            let focus = (self.focus.group == G_SHOWS).then_some(self.focus.index);
            let out = parts::row(ui, cx, "search-shows", "Shows", &self.shows, CardKind::Poster, focus);
            shows_xs = out.xs;
            if let Some(i) = out.hovered {
                self.focus = Focus { group: G_SHOWS, index: i };
            }
            if let Some(i) = out.clicked {
                clicked = self.shows.get(i).cloned();
            }
        }
        if !self.episodes.is_empty() {
            let focus = (self.focus.group == G_EPS).then_some(self.focus.index);
            let out = parts::row(ui, cx, "search-eps", "Episodes", &self.episodes, CardKind::Thumb, focus);
            eps_xs = out.xs;
            if let Some(i) = out.hovered {
                self.focus = Focus { group: G_EPS, index: i };
            }
            if let Some(i) = out.clicked {
                clicked = self.episodes.get(i).cloned();
            }
        }
        if !self.collections.is_empty() {
            let focus = (self.focus.group == G_COLLS).then_some(self.focus.index);
            let out = parts::row(ui, cx, "search-colls", "Collections", &self.collections, CardKind::Poster, focus);
            colls_xs = out.xs;
            if let Some(i) = out.hovered {
                self.focus = Focus { group: G_COLLS, index: i };
            }
            if let Some(i) = out.clicked {
                clicked = self.collections.get(i).cloned();
            }
        }
        if !self.people.is_empty() {
            let focus = (self.focus.group == G_PEOPLE).then_some(self.focus.index);
            let out = super::favourites::people_row(ui, cx, &self.people, focus);
            people_xs = out.xs;
            if let Some(i) = out.hovered {
                self.focus = Focus { group: G_PEOPLE, index: i };
            }
            if let Some(i) = out.clicked {
                clicked = self.people.get(i).cloned();
                clicked_person = true;
            }
        }
        if self.query.trim().is_empty() && !self.suggested.is_empty() {
            let focus = (self.focus.group == G_SUGGESTED).then_some(self.focus.index);
            let out = parts::row(ui, cx, "search-suggested", "Suggested", &self.suggested, CardKind::Poster, focus);
            sugg_xs = out.xs;
            if let Some(i) = out.hovered {
                self.focus = Focus { group: G_SUGGESTED, index: i };
            }
            if let Some(i) = out.clicked {
                clicked = self.suggested.get(i).cloned();
            }
        }

        if let Some(e) = &self.error {
            ui.add_space(t.gap / 2.0);
            parts::error_line(ui, t, e);
        }
        if !self.loading && self.deadline.is_none() && !self.query.trim().is_empty() && !self.any_results() {
            ui.add_space(t.gap / 2.0);
            parts::muted(ui, t, &format!("Nothing found for \u{201c}{}\u{201d}", self.query.trim()));
        }

        let mut groups = vec![Group { len: 1, cols: 1, xs: field_xs }];
        groups.push(Group { len: self.films.len(), cols: self.films.len(), xs: films_xs });
        groups.push(Group { len: self.shows.len(), cols: self.shows.len(), xs: shows_xs });
        groups.push(Group { len: self.episodes.len(), cols: self.episodes.len(), xs: eps_xs });
        groups.push(Group { len: self.collections.len(), cols: self.collections.len(), xs: colls_xs });
        groups.push(Group { len: self.people.len(), cols: self.people.len(), xs: people_xs });
        let n = sugg_xs.len();
        groups.push(Group { len: n, cols: n, xs: sugg_xs });
        self.groups = groups;
        self.focus = move_focus(&self.groups, self.focus, Key::Enter);

        if let Some(it) = clicked {
            open(cx, it, clicked_person);
        }
    }
}

/// A person opens their page; everything else opens like any item.
fn open(cx: &mut Cx, it: Item, person: bool) {
    if person {
        cx.push(super::person::screen(it.id, it.name));
    } else {
        cx.push(super::item::screen(it));
    }
}
