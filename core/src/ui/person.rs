//! F5b person page (SPEC R-F5-1, P-PPL-1/2): photo, name, dates, biography; an "On this server" grid; and the
//! TMDB Acting / Crew filmography (R-F5-2). Reached from cast rows.
use super::at_details as at;
use super::parts::{self, CardKind};
use super::widgets;
use crate::api::{image_width_bucket, ApiError, ImageKind, Item, Result};
use crate::app::{Cx, Screen};
use crate::nav::{self, Focus, Group};
use crate::platform::Key;
use crate::tmdb::{self, Credit, Tmdb, TmdbError};
use egui::{pos2, vec2, Rect, Sense};

const G_TAB: usize = 0;
const G_LIST: usize = 1;
const G_DETAIL: usize = 2;
const G_RETRY: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    OnServer,
    Acting,
    Crew,
}

impl Tab {
    const ALL: [Tab; 3] = [Tab::OnServer, Tab::Acting, Tab::Crew];

    fn label(self) -> &'static str {
        match self {
            Tab::OnServer => "On this server",
            Tab::Acting => "Acting",
            Tab::Crew => "Crew",
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|&t| t == self).unwrap()
    }
}

#[derive(PartialEq)]
enum TmdbState {
    Idle,
    NoKey,
    Loading,
    Loaded,
    Failed(String),
}

pub struct Person {
    person_id: String,
    name: String,
    person: Option<Item>,
    /// RR6 (P-PPL-1): born / died as the server sends them (`Item` has no end date).
    dates: (Option<String>, Option<String>),
    server_items: Vec<Item>,
    loading: bool,
    loaded: u8,
    error: Option<String>,
    tab: Tab,
    tmdb: TmdbState,
    acting: Vec<Credit>,
    crew: Vec<Credit>,
    detail: Option<Credit>,
    focus: Focus,
    groups: Vec<Group>,
    started: bool,
}

/// RR6 / P-PPL-1: the dates line under a person's name. D15: dates read "2 March 1950". Born and died →
/// "2 March 1950 – 1 July 2020 · aged 70"; born only → "Born 5 January 1981"; died only → "Died 5 May 2001". Dates
/// are the server's ISO strings; only the leading `YYYY-MM-DD` is read (a bare year stays a year).
pub fn life_line(born: Option<&str>, died: Option<&str>) -> Option<String> {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    fn ymd(s: &str) -> Option<(i32, Option<(u32, u32)>)> {
        let s = s.trim().get(..10).unwrap_or(s.trim());
        let mut it = s.split('-');
        let y = it.next()?.parse().ok()?;
        let md = match (it.next().and_then(|m| m.parse().ok()), it.next().and_then(|d| d.parse().ok())) {
            (Some(m), Some(d)) if (1..=12).contains(&m) && d >= 1 => Some((m, d)),
            _ => None,
        };
        Some((y, md))
    }
    let long = |(y, md): (i32, Option<(u32, u32)>)| match md {
        Some((m, d)) => format!("{d} {} {y}", MONTHS[m as usize - 1]),
        None => y.to_string(),
    };
    let b = born.and_then(ymd);
    let d = died.and_then(ymd);
    match (b, d) {
        (Some(b), Some(d)) => {
            let (bm, dm) = (b.1.unwrap_or((1, 1)), d.1.unwrap_or((1, 1)));
            let age = d.0 - b.0 - i32::from(dm < bm);
            Some(format!("{} \u{2013} {} \u{b7} aged {age}", long(b), long(d)))
        }
        (Some(b), None) => Some(format!("Born {}", long(b))),
        (None, Some(d)) => Some(format!("Died {}", long(d))),
        (None, None) => None,
    }
}

pub fn screen(person_id: String, name: String) -> Box<dyn Screen> {
    Box::new(Person {
        person_id,
        name,
        person: None,
        dates: (None, None),
        server_items: vec![],
        loading: true,
        loaded: 0,
        error: None,
        tab: Tab::OnServer,
        tmdb: TmdbState::Idle,
        acting: vec![],
        crew: vec![],
        detail: None,
        focus: Focus { group: G_TAB, index: 0 },
        groups: vec![],
        started: false,
    })
}

impl Person {
    fn spawn_loads(&mut self, cx: &mut Cx) {
        self.loading = true;
        self.loaded = 0;
        self.error = None;
        let id = self.person_id.clone();
        let client = cx.api();
        cx.spawn("person", move || client.person(&id));
        let id = self.person_id.clone();
        let client = cx.api();
        cx.spawn("person-items", move || client.person_items(&id));
        let id = self.person_id.clone();
        let client = cx.api();
        cx.spawn("person-dates", move || client.person_dates(&id));
    }

    /// Load the TMDB filmography once (R-F5-2): no key → a hint, bad key → its own message. Blocking TMDB
    /// calls run on the pool; results are cached on disk by `Tmdb`.
    fn ensure_tmdb(&mut self, cx: &mut Cx) {
        if self.tmdb != TmdbState::Idle {
            return;
        }
        let key = cx.settings.tmdb_key.trim().to_string();
        if key.is_empty() {
            self.tmdb = TmdbState::NoKey;
            return;
        }
        self.tmdb = TmdbState::Loading;
        let name = self.name.clone();
        let cache = cx.store.cache_dir();
        cx.spawn("tmdb", move || -> std::result::Result<(Vec<Credit>, Vec<Credit>), TmdbError> {
            let t = Tmdb::new(&key, Some(cache));
            match t.find_person(&name)? {
                Some(id) => Ok(tmdb::split_sorted(&t.credits(id)?)),
                None => Ok((vec![], vec![])),
            }
        });
    }

    fn credits(&self) -> &[Credit] {
        match self.tab {
            Tab::Acting => &self.acting,
            Tab::Crew => &self.crew,
            Tab::OnServer => &[],
        }
    }

    /// FIX-1 #18: without a TMDB key the Acting / Crew tabs could only say "key required", so they are not shown.
    fn tabs(cx: &Cx) -> &'static [Tab] {
        if cx.settings.tmdb_key.trim().is_empty() {
            &Tab::ALL[..1]
        } else {
            &Tab::ALL
        }
    }

    fn enter(&mut self, cx: &mut Cx) -> bool {
        match self.focus.group {
            G_TAB => {
                if let Some(&tab) = Self::tabs(cx).get(self.focus.index) {
                    self.tab = tab;
                    self.detail = None;
                    if matches!(tab, Tab::Acting | Tab::Crew) {
                        if matches!(self.tmdb, TmdbState::Failed(_)) {
                            self.tmdb = TmdbState::Idle;
                        }
                        self.ensure_tmdb(cx);
                    }
                }
            }
            G_LIST => match self.tab {
                Tab::OnServer => {
                    if let Some(item) = self.server_items.get(self.focus.index).cloned() {
                        cx.push(super::item::screen(item));
                    }
                }
                Tab::Acting | Tab::Crew => {
                    if let Some(c) = self.credits().get(self.focus.index).cloned() {
                        if let Some(item) = tmdb::on_server(&c, &self.server_items).cloned() {
                            cx.push(super::item::screen(item));
                        } else {
                            self.detail = Some(c);
                        }
                    }
                }
            },
            G_DETAIL => self.detail = None,
            G_RETRY => {
                self.error = None;
                self.spawn_loads(cx);
            }
            _ => {}
        }
        true
    }

    /// D2: Apple's person header — a large round portrait beside the name (hero title size), the life line and
    /// the biography.
    fn draw_header(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        ui.add_space(t.row_gap);
        ui.horizontal_top(|ui| {
            let ppp = ui.ctx().pixels_per_point();
            let url = self.person.as_ref().and_then(|p| {
                cx.client
                    .as_ref()
                    .and_then(|c| c.image_url(p, ImageKind::Primary, image_width_bucket(t.person_photo_w, ppp)))
            });
            let tex = url.as_deref().and_then(|u| cx.images.get(u));
            at::portrait(ui, t, tex, &self.name, t.person_photo_w);
            ui.add_space(t.gap * 2.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.add_space(t.gap / 2.0);
                at::hero_title(ui, t, &self.name, None);
                if let Some(p) = &self.person {
                    let born = self.dates.0.as_deref().or(p.premiere_date.as_deref());
                    if let Some(line) = life_line(born, self.dates.1.as_deref()) {
                        ui.add_space(t.gap / 2.0);
                        at::meta_row(ui, t, &[line], (None, None), &[]);
                    }
                    if let Some(ov) = p.overview.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                        ui.add_space(t.gap / 2.0);
                        at::overview(ui, t, ov, true, 0);
                    }
                } else if self.loading {
                    ui.add_space(t.gap / 2.0);
                    parts::muted(ui, t, "Loading…");
                }
            });
        });
        ui.add_space(t.gap);
    }

    /// D2: the three tabs as the segmented pill control (selected = brighter glass, focused = white plate).
    fn draw_tabs(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> (Vec<f32>, Option<Tab>) {
        let t = cx.theme;
        let labels: Vec<String> = Self::tabs(cx).iter().map(|tab| tab.label().to_string()).collect();
        let focus = (self.focus.group == G_TAB).then_some(self.focus.index);
        let out = widgets::tabs(ui, t, &labels, self.tab.index(), focus);
        if let Some(i) = out.hovered {
            if ui.input(|i| i.pointer.is_moving()) {
                self.focus = Focus { group: G_TAB, index: i };
            }
        }
        let clicked = out.clicked.and_then(|i| Self::tabs(cx).get(i).copied());
        (out.xs, clicked)
    }

    /// D2: what is on this server as Apple's sideways row of posters (lift on focus), left-aligned under the tabs.
    fn draw_server_grid(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> (Vec<f32>, Option<usize>) {
        let t = cx.theme;
        let mut xs = vec![];
        let mut clicked = None;
        if self.server_items.is_empty() {
            ui.add_space(t.gap);
            parts::muted(ui, t, "Nothing from this person on the server.");
            return (xs, clicked);
        }
        let items = self.server_items.clone();
        let focus = (self.focus.group == G_LIST).then_some(self.focus.index);
        let h = t.poster_h + t.card_title_gap + t.body_size * 1.3;
        let pointer_moved = ui.input(|i| i.pointer.is_moving());
        let rs = at::row(ui, t, "person-server", items.len(), h, focus, |ui, i| {
            let tx = at::tex(cx, ui, &items[i], ImageKind::Primary, t.poster_w);
            at::poster_card(ui, t, tx, &items[i].name, focus == Some(i))
        });
        for (i, r) in rs.iter().enumerate() {
            xs.push(r.rect.center().x);
            if pointer_moved && r.hovered() && parts::hover_moves_focus(ui.ctx()) {
                self.focus = Focus { group: G_LIST, index: i };
            }
            if r.clicked() {
                clicked = Some(i);
            }
        }
        (xs, clicked)
    }

    fn draw_credits(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> (Vec<f32>, Option<usize>) {
        let t = cx.theme;
        let credits: Vec<Credit> = self.credits().to_vec();
        let matches: Vec<Option<Item>> =
            credits.iter().map(|c| tmdb::on_server(c, &self.server_items).cloned()).collect();
        let mut xs = vec![];
        let mut clicked = None;
        let ppp = ui.ctx().pixels_per_point();
        let row_h = t.credit_poster_h + t.gap / 2.0;
        match self.tmdb {
            TmdbState::NoKey => {
                ui.add_space(t.gap);
                parts::muted(
                    ui,
                    t,
                    "Add a TMDB API key in Settings to see the full filmography (free at themoviedb.org)",
                );
            }
            TmdbState::Loading => {
                ui.add_space(t.gap);
                for _ in 0..4 {
                    parts::skeleton_card(ui, t, CardKind::Poster);
                    ui.add_space(t.gap / 2.0);
                }
            }
            TmdbState::Failed(ref msg) => {
                ui.add_space(t.gap);
                parts::error_line(ui, t, msg);
            }
            _ if credits.is_empty() => {
                ui.add_space(t.gap);
                parts::muted(ui, t, "Nothing found.");
            }
            _ => {
                egui::ScrollArea::vertical().id_salt("person-credits").show(ui, |ui| {
                    for (i, credit) in credits.iter().enumerate() {
                        let focused = self.focus.group == G_LIST && self.focus.index == i;
                        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());
                        if ui.is_rect_visible(rect) {
                            let poster = Rect::from_min_size(rect.min, vec2(t.credit_poster_w, t.credit_poster_h));
                            let on_server = matches[i].as_ref();
                            let url = match on_server {
                                Some(item) => cx.client.as_ref().and_then(|c| {
                                    c.image_url(item, ImageKind::Primary, image_width_bucket(t.credit_poster_w, ppp))
                                }),
                                None => credit.poster_path.as_ref().map(|p| Tmdb::poster_url(p)),
                            };
                            match url.as_deref().and_then(|u| cx.images.get(u)) {
                                Some(tex) => {
                                    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                                    ui.painter().image(tex, poster, uv, egui::Color32::WHITE);
                                }
                                None => {
                                    ui.painter().rect_filled(poster, t.radius, t.skeleton);
                                }
                            }
                            if on_server.is_none() {
                                ui.painter().rect_filled(poster, t.radius, t.dim_scrim);
                            }
                            if focused {
                                parts::focus_ring(ui, t, poster);
                            }
                            let x = poster.right() + t.gap / 2.0;
                            let w = rect.right() - x - t.gap;
                            let title_color = if on_server.is_some() { t.text } else { t.muted };
                            parts::label_line(
                                ui,
                                t,
                                pos2(x, rect.top() + 4.0),
                                w,
                                &credit.title,
                                t.body_size,
                                title_color,
                            );
                            let mut sub = vec![];
                            if let Some(y) = credit.year {
                                sub.push(y.to_string());
                            }
                            if !credit.role.is_empty() {
                                sub.push(credit.role.clone());
                            }
                            if !sub.is_empty() {
                                parts::label_line(
                                    ui,
                                    t,
                                    pos2(x, rect.top() + 4.0 + t.body_size + 4.0),
                                    w,
                                    &sub.join(" · "),
                                    t.small_size,
                                    t.muted,
                                );
                            }
                            if on_server.is_some() {
                                let g = ui.fonts(|f| {
                                    f.layout_no_wrap("On server".to_string(), t.font(t.small_size), t.accent)
                                });
                                ui.painter().galley(pos2(rect.right() - g.size().x, rect.top() + 4.0), g, t.accent);
                            }
                        }
                        xs.push(rect.center().x);
                        if resp.hovered() && parts::hover_moves_focus(&resp.ctx) {
                            self.focus = Focus { group: G_LIST, index: i };
                        }
                        if focused && crate::ui::shell::keyboard_focus(&resp.ctx) {
                            resp.scroll_to_me(None);
                        }
                        if resp.clicked() {
                            clicked = Some(i);
                        }
                    }
                });
            }
        }
        (xs, clicked)
    }

    fn draw_detail(&mut self, ui: &mut egui::Ui, cx: &mut Cx, c: &Credit) -> Vec<f32> {
        let t = cx.theme;
        let mut xs = vec![];
        ui.add_space(t.gap);
        ui.horizontal(|ui| {
            let (poster, _) = ui.allocate_exact_size(vec2(t.credit_poster_w, t.credit_poster_h), Sense::hover());
            let url = c.poster_path.as_ref().map(|p| Tmdb::poster_url(p));
            match url.as_deref().and_then(|u| cx.images.get(u)) {
                Some(tex) => {
                    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                    ui.painter().image(tex, poster, uv, egui::Color32::WHITE);
                }
                None => {
                    ui.painter().rect_filled(poster, t.radius, t.skeleton);
                }
            }
            ui.add_space(t.gap);
            ui.vertical(|ui| {
                ui.set_max_width(t.field_w * 1.2);
                parts::heading(ui, t, &c.title);
                let mut sub = vec![];
                if let Some(y) = c.year {
                    sub.push(y.to_string());
                }
                if !c.role.is_empty() {
                    sub.push(c.role.clone());
                }
                if !sub.is_empty() {
                    ui.add_space(t.gap / 4.0);
                    parts::muted(ui, t, &sub.join(" · "));
                }
                if !c.overview.is_empty() {
                    ui.add_space(t.gap / 2.0);
                    ui.label(egui::RichText::new(c.overview.clone()).size(t.body_size).color(t.muted));
                }
                ui.add_space(t.gap / 2.0);
                ui.label(egui::RichText::new("Not on your server").size(t.body_size).color(t.text));
                ui.add_space(t.gap / 2.0);
                let focused = self.focus.group == G_DETAIL;
                let r = parts::button(ui, t, "Back", focused, false);
                xs.push(r.rect.center().x);
                if r.hovered() && parts::hover_moves_focus(ui.ctx()) {
                    self.focus = Focus { group: G_DETAIL, index: 0 };
                }
                if r.clicked() {
                    self.detail = None;
                }
            });
        });
        xs
    }
}

impl Screen for Person {
    fn name(&self) -> &'static str {
        "person"
    }

    /// NAV: this screen's part of the toolbar breadcrumb.
    fn crumbs(&self) -> Vec<String> {
        vec![self.name.clone()]
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        match key {
            Key::Enter => self.enter(cx),
            Key::Back => {
                if self.detail.is_some() {
                    self.detail = None;
                    true
                } else {
                    false
                }
            }
            Key::Up | Key::Down | Key::Left | Key::Right => {
                self.focus = nav::move_focus(&self.groups, self.focus, key);
                true
            }
            _ => false,
        }
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn std::any::Any + Send>) {
        match tag {
            "person-dates" => {
                let Ok(r) = payload.downcast::<Result<(Option<String>, Option<String>)>>() else { return };
                match *r {
                    Ok(d) => self.dates = d,
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(_) => {}
                }
            }
            "person" => {
                let Ok(r) = payload.downcast::<Result<Item>>() else { return };
                match *r {
                    Ok(it) => {
                        self.person = Some(it);
                        self.error = None;
                    }
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(e) => self.error = Some(e.to_string()),
                }
                self.loaded += 1;
                self.loading = self.loaded < 2;
            }
            "person-items" => {
                let Ok(r) = payload.downcast::<Result<Vec<Item>>>() else { return };
                match *r {
                    Ok(v) => {
                        self.server_items = v;
                        self.error = None;
                    }
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(e) => self.error = Some(e.to_string()),
                }
                self.loaded += 1;
                self.loading = self.loaded < 2;
            }
            "tmdb" => {
                let Ok(r) = payload.downcast::<std::result::Result<(Vec<Credit>, Vec<Credit>), TmdbError>>() else {
                    return;
                };
                match *r {
                    Ok((acting, crew)) => {
                        self.acting = acting;
                        self.crew = crew;
                        self.tmdb = TmdbState::Loaded;
                    }
                    Err(TmdbError::NoKey) => self.tmdb = TmdbState::NoKey,
                    Err(TmdbError::BadKey) => self.tmdb = TmdbState::Failed("TMDB didn't accept the key".to_string()),
                    Err(TmdbError::Network(s)) | Err(TmdbError::Decode(s)) => self.tmdb = TmdbState::Failed(s),
                }
            }
            _ => {}
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        if !self.started {
            self.started = true;
            self.spawn_loads(cx);
        }
        let t = cx.theme;

        self.draw_header(ui, cx);

        ui.add_space(t.gap / 2.0);
        let (tab_xs, tab_clicked) = self.draw_tabs(ui, cx);

        let mut list_xs = vec![];
        let mut list_clicked = None;
        let mut detail_xs = vec![];
        let mut retry_xs = vec![];

        if let Some(e) = self.error.clone() {
            ui.add_space(t.gap / 2.0);
            parts::error_line(ui, t, &e);
            let focused = self.focus.group == G_RETRY;
            let r = parts::button(ui, t, "Retry", focused, false);
            retry_xs.push(r.rect.center().x);
            if r.hovered() && parts::hover_moves_focus(ui.ctx()) {
                self.focus = Focus { group: G_RETRY, index: 0 };
            }
            if r.clicked() {
                self.error = None;
                self.spawn_loads(cx);
            }
        } else if let Some(detail) = self.detail.clone() {
            detail_xs = self.draw_detail(ui, cx, &detail);
        } else if self.loading && self.tab == Tab::OnServer {
            ui.add_space(t.gap);
            for _ in 0..4 {
                parts::skeleton_card(ui, t, CardKind::Poster);
            }
        } else {
            match self.tab {
                Tab::OnServer => {
                    let (xs, clicked) = self.draw_server_grid(ui, cx);
                    list_xs = xs;
                    list_clicked = clicked;
                }
                Tab::Acting | Tab::Crew => {
                    let (xs, clicked) = self.draw_credits(ui, cx);
                    list_xs = xs;
                    list_clicked = clicked;
                }
            }
        }

        let mut groups = vec![Group::default(); 4];
        groups[G_TAB] = Group { len: 3, cols: 3, xs: tab_xs };
        if !detail_xs.is_empty() {
            groups[G_DETAIL] = Group { len: 1, cols: 1, xs: detail_xs };
        } else if !retry_xs.is_empty() {
            groups[G_RETRY] = Group { len: 1, cols: 1, xs: retry_xs };
        } else if self.tab == Tab::OnServer {
            groups[G_LIST] = Group { len: list_xs.len(), cols: list_xs.len(), xs: list_xs };
        } else {
            groups[G_LIST] = Group { len: list_xs.len(), cols: 1, xs: list_xs };
        }
        self.groups = groups;
        self.focus = nav::move_focus(&self.groups, self.focus, Key::Enter);

        if let Some(tab) = tab_clicked {
            self.tab = tab;
            self.detail = None;
            if matches!(tab, Tab::Acting | Tab::Crew) {
                if matches!(self.tmdb, TmdbState::Failed(_)) {
                    self.tmdb = TmdbState::Idle;
                }
                self.ensure_tmdb(cx);
            }
        }
        if let Some(i) = list_clicked {
            match self.tab {
                Tab::OnServer => {
                    if let Some(item) = self.server_items.get(i).cloned() {
                        cx.push(super::item::screen(item));
                    }
                }
                Tab::Acting | Tab::Crew => {
                    let credits = self.credits().to_vec();
                    if let Some(c) = credits.get(i).cloned() {
                        if let Some(item) = tmdb::on_server(&c, &self.server_items).cloned() {
                            cx.push(super::item::screen(item));
                        } else {
                            self.detail = Some(c);
                        }
                    }
                }
            }
        }
    }
}
