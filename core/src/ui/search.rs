//! R-UI-8 Search: a text field (focused on open) and results grouped into "Films", "Shows", "Episodes",
//! "Collections" and "People" rows; before typing, "Suggested" (G13). Rows. Typing is debounced by `theme.search_debounce`; stale results are dropped with a sequence number.
use super::parts::{self, CardKind};
use crate::api::{ApiError, Item, ItemKind, Result};
use crate::app::{Cx, Screen};
use crate::nav::{move_focus, Focus, Group};
use crate::platform::Key;

const G_FIELD: usize = 0;
const G_TOP: usize = 1;
const G_FILMS: usize = 2;
const G_SHOWS: usize = 3;
const G_EPS: usize = 4;
const G_COLLS: usize = 5;
const G_PEOPLE: usize = 6;
const G_SUGGESTED: usize = 7;

/// How many cards the Top results row holds.
const TOP_MAX: usize = 12;

/// D12: the best matches across films, shows, episodes, collections and people, by how well the name matches the
/// query (whole name, then name start, then a word start, then anywhere). Ties keep film, show, person,
/// collection, episode order. The bool is true for a person. Names that do not match at all are left out.
pub fn top_results(query: &str, items: &[Item], people: &[Item]) -> Vec<(Item, bool)> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return vec![];
    }
    let words: Vec<&str> = q.split_whitespace().collect();
    let score = |name: &str| -> Option<u8> {
        let n = name.to_lowercase();
        if n == q {
            Some(0)
        } else if n.starts_with(&q) {
            Some(1)
        } else if n.split_whitespace().any(|w| w.starts_with(&q)) {
            Some(2)
        } else if n.contains(&q) || words.iter().all(|w| n.contains(w)) {
            Some(3)
        } else {
            None
        }
    };
    let rank = |k: &ItemKind, person: bool| -> u8 {
        if person {
            return 2;
        }
        match k {
            ItemKind::Movie => 0,
            ItemKind::Series => 1,
            ItemKind::BoxSet => 3,
            _ => 4,
        }
    };
    let mut hits: Vec<(u8, u8, Item, bool)> = items
        .iter()
        .map(|i| (i, false))
        .chain(people.iter().map(|p| (p, true)))
        .filter_map(|(i, person)| score(&i.name).map(|sc| (sc, rank(&i.kind, person), i.clone(), person)))
        .collect();
    hits.sort_by_key(|h| (h.0, h.1));
    hits.truncate(TOP_MAX);
    hits.into_iter().map(|h| (h.2, h.3)).collect()
}

pub struct Search {
    query: String,
    last_query: String,
    top: Vec<Item>,
    /// Ids in `top` that are people (they open a person page).
    top_people: Vec<String>,
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
        top: vec![],
        top_people: vec![],
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
        !self.top.is_empty()
            || !self.films.is_empty()
            || !self.shows.is_empty()
            || !self.episodes.is_empty()
            || !self.collections.is_empty()
            || !self.people.is_empty()
    }

    fn set_results(&mut self, items: Vec<Item>, people: Vec<Item>) {
        let top = top_results(&self.last_query, &items, &people);
        self.top_people = top.iter().filter(|(_, p)| *p).map(|(i, _)| i.id.clone()).collect();
        self.top = top.into_iter().map(|(i, _)| i).collect();
        self.collections = items.iter().filter(|i| i.kind == ItemKind::BoxSet).cloned().collect();
        self.people = people;
        self.films = items.iter().filter(|i| i.kind == ItemKind::Movie).cloned().collect();
        self.shows = items.iter().filter(|i| i.kind == ItemKind::Series).cloned().collect();
        self.episodes = items.iter().filter(|i| i.kind == ItemKind::Episode).cloned().collect();
    }

    fn result_at(&self, f: Focus) -> Option<Item> {
        match f.group {
            G_TOP => self.top.get(f.index).cloned(),
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
            self.top.clear();
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
                        let person = self.focus.group == G_PEOPLE
                            || (self.focus.group == G_TOP && self.top_people.contains(&it.id));
                        open(cx, it, person);
                    }
                }
                true
            }
            Key::Up | Key::Down | Key::Left | Key::Right => {
                if self.focus.group == G_FIELD && matches!(key, Key::Left | Key::Right) {
                    return true;
                }
                if self.focus.group == G_FIELD && key == Key::Down {
                    // Straight onto the first result, not the card nearest the field's middle.
                    if let Some(gi) = self.groups.iter().skip(1).position(|g| g.len > 0) {
                        self.focus = Focus { group: gi + 1, index: 0 };
                    }
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

        // Apple TV look: the wide search field, centred at the top (the toolbar already says "Search").
        ui.add_space(t.gap);
        let focused = self.focus.group == G_FIELD;
        let r = super::at_library::search_field(ui, t, &mut self.query, "Films, shows, people and more", focused);
        let field_xs = vec![r.rect.center().x];
        if r.clicked() {
            self.focus = Focus { group: G_FIELD, index: 0 };
        }

        let mut top_xs: Vec<f32> = vec![];
        let mut films_xs: Vec<f32> = vec![];
        let mut shows_xs: Vec<f32> = vec![];
        let mut eps_xs: Vec<f32> = vec![];
        let mut colls_xs: Vec<f32> = vec![];
        let mut people_xs: Vec<f32> = vec![];
        let mut sugg_xs: Vec<f32> = vec![];
        let mut clicked: Option<Item> = None;
        let mut clicked_person = false;

        if (self.loading || self.deadline.is_some()) && !self.query.trim().is_empty() {
            ui.add_space(t.gap / 2.0);
            parts::muted(ui, t, "Searching…");
        }

        if !self.top.is_empty() {
            let focus = (self.focus.group == G_TOP).then_some(self.focus.index);
            let out = parts::row(ui, cx, "search-top", "Top results", &self.top, CardKind::Poster, focus);
            top_xs = out.xs;
            if let Some(i) = out.hovered {
                self.focus = Focus { group: G_TOP, index: i };
            }
            if let Some(i) = out.clicked {
                clicked = self.top.get(i).cloned();
                clicked_person = clicked.as_ref().is_some_and(|c| self.top_people.contains(&c.id));
            }
        }
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
            let body = format!(
                "Nothing found for \u{201c}{}\u{201d}. Try a different spelling or fewer words.",
                self.query.trim()
            );
            super::at_library::empty_state(ui, t, "No results", &body);
        }

        let mut groups = vec![Group { len: 1, cols: 1, xs: field_xs }];
        groups.push(Group { len: self.top.len(), cols: self.top.len(), xs: top_xs });
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

#[cfg(test)]
mod tests {
    use super::*;

    fn item(name: &str, kind: ItemKind) -> Item {
        Item { id: name.into(), name: name.into(), kind, ..Default::default() }
    }

    fn names(v: &[(Item, bool)]) -> Vec<&str> {
        v.iter().map(|(i, _)| i.name.as_str()).collect()
    }

    #[test]
    fn better_name_matches_come_first_across_types() {
        let items = [
            item("The Last Drift", ItemKind::Movie),
            item("Drift Episode", ItemKind::Episode),
            item("Drifters", ItemKind::Series),
            item("Drift", ItemKind::Movie),
        ];
        let people = [item("Drift Quill", ItemKind::Other)];
        let top = top_results("drift", &items, &people);
        assert_eq!(names(&top), ["Drift", "Drifters", "Drift Quill", "Drift Episode", "The Last Drift"]);
        assert!(top[2].1 && !top[0].1);
    }

    #[test]
    fn partial_words_match_and_non_matches_are_left_out() {
        let items = [item("Star Drift", ItemKind::BoxSet), item("Unrelated", ItemKind::Movie)];
        assert_eq!(names(&top_results("drif", &items, &[])), ["Star Drift"]);
        assert!(top_results("  ", &items, &[]).is_empty());
    }

    #[test]
    fn the_row_is_capped() {
        let items: Vec<Item> = (0..30).map(|i| item(&format!("Drift {i}"), ItemKind::Movie)).collect();
        assert_eq!(top_results("drift", &items, &[]).len(), TOP_MAX);
    }
}
