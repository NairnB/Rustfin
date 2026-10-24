//! R-UI-4 Home: a full-bleed hero of a featured item (Apple TV look, docs/DESIGN-appletv.md) with the top pill
//! bar floating over it, then rows (Continue watching, Next up, Latest per library, Libraries).
//! All loads start at once on the worker pool; focus groups are rebuilt every frame from what is drawn.
use super::parts::{self, CardKind, RowOut};
use crate::api::{ApiError, Item, Result};
use crate::app::{Cx, Screen};
use crate::nav::{move_focus, Focus, Group};
use crate::platform::Key;
use crate::settings::HomeSection;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum RowId {
    Continue,
    NextUp,
    Favourites,
    Latest(usize),
    Libraries,
}

#[derive(Clone, Debug)]
enum Role {
    Banner,
    Topbar,
    Hero,
    Row(RowId),
}

enum RowState {
    Loading,
    Hidden,
    Loaded(Vec<Item>),
    Error(String),
}

pub struct Home {
    /// PP review: the `cx.poster_gen` Home last loaded at.
    poster_seen: Option<u64>,
    started: bool,
    verify: bool,
    resume: Option<Result<Vec<Item>>>,
    next_up: Option<Result<Vec<Item>>>,
    favourites: Option<Result<Vec<Item>>>,
    views: Option<Result<Vec<Item>>>,
    latest: HashMap<String, Result<Vec<Item>>>,
    video_views: Vec<Item>,
    offline: Option<String>,
    focus: Focus,
    groups: Vec<Group>,
    roles: Vec<Role>,
    auto: bool,
    /// P-HOME-7: the user menu — `Some(highlighted entry)` while open.
    user_menu: Option<usize>,
    /// Set while the server sign-out runs; the next sign-in screen opens on the public user tiles when true.
    switching: bool,
    /// TV bar "Library": move focus to the Libraries row on the next frame.
    to_libraries: bool,
    /// The hero's item with its full fields (overview, streams for badges), fetched when the featured item changes.
    hero_full: Option<Item>,
    hero_for: Option<String>,
    /// D10: Sign out / Switch user is waiting on the server — say so.
    leaving: bool,
    /// Who is signed in, noted when leaving so Sign out can drop their saved session.
    user_id: Option<String>,
}

/// The user menu. On the TV, Favourites and Playlists live here (the bar holds the libraries, UX R2).
const USER_MENU: [&str; 5] = ["Favourites", "Playlists", "Settings", "Switch user", "Sign out"];
const MENU_SWITCH: usize = 3;
/// TV bar: at most this many libraries as tabs; more → the first ones + "Library" (jumps to the Libraries row).
const BAR_MAX_VIEWS: usize = 5;

/// One segment of the TV top bar (the user segment follows the last one).
#[derive(Clone)]
enum BarItem {
    Home,
    View(Box<Item>),
    Library,
    Search,
}

pub fn screen(verify_session: bool) -> Box<dyn Screen> {
    Box::new(Home {
        poster_seen: None,
        started: false,
        verify: verify_session,
        resume: None,
        next_up: None,
        favourites: None,
        views: None,
        latest: HashMap::new(),
        video_views: vec![],
        offline: None,
        focus: Focus { group: 0, index: 0 },
        groups: vec![],
        roles: vec![],
        auto: true,
        user_menu: None,
        switching: false,
        to_libraries: false,
        hero_full: None,
        hero_for: None,
        leaving: false,
        user_id: None,
    })
}

impl Home {
    fn state_of(res: Option<&Result<Vec<Item>>>) -> RowState {
        match res {
            None => RowState::Loading,
            Some(Ok(v)) if v.is_empty() => RowState::Hidden,
            Some(Ok(v)) => RowState::Loaded(v.clone()),
            Some(Err(e)) => RowState::Error(e.to_string()),
        }
    }

    fn start(&mut self, cx: &Cx) {
        let client = cx.api();
        cx.spawn("resume", move || client.resume(20));
        let client = cx.api();
        let (rw, nd) = (cx.settings.next_up_rewatching, cx.settings.next_up_days);
        cx.spawn("next_up", move || client.next_up_with(20, rw, nd));
        let client = cx.api();
        cx.spawn("favourites", move || client.favourites("Movie,Series"));
        let client = cx.api();
        cx.spawn("views", move || client.views());
        if self.verify {
            let client = cx.api();
            cx.spawn("verify", move || client.verify_session());
        }
    }

    fn reload(&mut self, cx: &Cx) {
        self.resume = None;
        self.next_up = None;
        self.favourites = None;
        self.views = None;
        self.latest.clear();
        self.video_views.clear();
        let client = cx.api();
        cx.spawn("resume", move || client.resume(20));
        let client = cx.api();
        let (rw, nd) = (cx.settings.next_up_rewatching, cx.settings.next_up_days);
        cx.spawn("next_up", move || client.next_up_with(20, rw, nd));
        let client = cx.api();
        cx.spawn("favourites", move || client.favourites("Movie,Series"));
        let client = cx.api();
        cx.spawn("views", move || client.views());
    }

    fn spawn_latest(&mut self, cx: &Cx) {
        let views = self.video_views.clone();
        if views.is_empty() {
            return;
        }
        let client = cx.api();
        cx.spawn("latest", move || {
            views
                .into_iter()
                .map(|v| {
                    let r = client.latest(&v.id, 20);
                    (v.id.clone(), r)
                })
                .collect::<Vec<(String, Result<Vec<Item>>)>>()
        });
    }

    fn retry_row(&mut self, cx: &Cx, id: RowId) {
        match id {
            RowId::Continue => {
                self.resume = None;
                let client = cx.api();
                cx.spawn("resume", move || client.resume(20));
            }
            RowId::NextUp => {
                self.next_up = None;
                let client = cx.api();
                let (rw, nd) = (cx.settings.next_up_rewatching, cx.settings.next_up_days);
                cx.spawn("next_up", move || client.next_up_with(20, rw, nd));
            }
            RowId::Favourites => {
                self.favourites = None;
                let client = cx.api();
                cx.spawn("favourites", move || client.favourites("Movie,Series"));
            }
            RowId::Latest(_) => {
                self.latest.clear();
                self.spawn_latest(cx);
            }
            RowId::Libraries => {
                self.views = None;
                let client = cx.api();
                cx.spawn("views", move || client.views());
            }
        }
    }

    fn item_for(&self, id: RowId, index: usize) -> Option<Item> {
        let items = match id {
            RowId::Continue => self.resume.as_ref()?.as_ref().ok()?,
            RowId::NextUp => self.next_up.as_ref()?.as_ref().ok()?,
            RowId::Favourites => self.favourites.as_ref()?.as_ref().ok()?,
            RowId::Latest(i) => self.video_views.get(i).and_then(|v| self.latest.get(&v.id))?.as_ref().ok()?,
            RowId::Libraries => self.views.as_ref()?.as_ref().ok()?,
        };
        items.get(index).cloned()
    }

    /// The hero shows what you are most likely to play next: the first Continue watching item, else the first
    /// Next up episode, else the newest item of the first film/TV library.
    fn featured(&self) -> Option<Item> {
        let first =
            |r: &Option<Result<Vec<Item>>>| r.as_ref().and_then(|r| r.as_ref().ok()).and_then(|v| v.first().cloned());
        first(&self.resume)
            .or_else(|| first(&self.next_up))
            .or_else(|| self.video_views.iter().find_map(|v| self.latest.get(&v.id)?.as_ref().ok()?.first().cloned()))
    }

    /// The featured item, with the full fields once they have loaded.
    fn hero_item(&self) -> Option<Item> {
        let f = self.featured()?;
        Some(self.hero_full.clone().filter(|h| h.id == f.id).unwrap_or(f))
    }

    fn activate_hero(&mut self, cx: &mut Cx, index: usize) {
        let Some(item) = self.hero_item() else { return };
        if index == 0 && matches!(item.kind, crate::api::ItemKind::Movie | crate::api::ItemKind::Episode) {
            cx.push(super::player::screen(item, super::player::PlayOptions::default()));
        } else {
            cx.push(super::item::screen(item));
        }
    }

    fn row_is_error(&self, id: RowId) -> bool {
        match id {
            RowId::Continue => matches!(&self.resume, Some(Err(_))),
            RowId::NextUp => matches!(&self.next_up, Some(Err(_))),
            RowId::Favourites => matches!(&self.favourites, Some(Err(_))),
            RowId::Latest(i) => {
                self.video_views.get(i).map(|v| matches!(self.latest.get(&v.id), Some(Err(_)))).unwrap_or(false)
            }
            RowId::Libraries => matches!(&self.views, Some(Err(_))),
        }
    }

    /// P-HOME-7: the menu under the user chip — Settings, Switch user, Sign out.
    fn draw_user_menu(&mut self, ui: &mut egui::Ui, cx: &mut Cx, chip: egui::Rect, highlighted: usize) {
        let t = cx.theme;
        let w = t.dropdown_w * 0.75;
        let pos = egui::pos2(chip.right() - w, chip.bottom() + t.gap / 4.0);
        let mut hovered = None;
        let mut clicked = None;
        let area = egui::Area::new(egui::Id::new("home-user-menu")).order(egui::Order::Foreground).fixed_pos(pos).show(
            ui.ctx(),
            |ui| {
                // FIX-1 #18: a dark material card behind the entries (pills alone floated over the backdrop).
                egui::Frame::new()
                    .fill(t.menu_fill)
                    .corner_radius(t.menu_radius)
                    .stroke(egui::Stroke::new(1.0, t.bar_stroke))
                    .shadow(t.shadow(1.0))
                    .inner_margin(egui::Margin::same(t.bar_inset as i8))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = t.gap / 6.0;
                        for (i, label) in USER_MENU.iter().enumerate() {
                            let r = parts::button_width(ui, t, label, highlighted == i, false, w);
                            if r.hovered() {
                                hovered = Some(i);
                            }
                            if r.clicked() {
                                clicked = Some(i);
                            }
                        }
                    });
            },
        );
        if let Some(i) = hovered {
            self.user_menu = Some(i);
        }
        if let Some(i) = clicked {
            self.pick_user_menu(cx, i);
        } else if ui.input(|i| i.pointer.any_click()) {
            let pos = ui.input(|i| i.pointer.interact_pos());
            if pos.is_some_and(|p| !area.response.rect.contains(p) && !chip.contains(p)) {
                self.user_menu = None;
            }
        }
    }

    /// UX R2: the TV bar holds Home, the libraries (like the Apple TV app's tabs) and Search.
    fn bar_items(&self) -> Vec<BarItem> {
        let views: Vec<Item> = match &self.views {
            Some(Ok(v)) => super::shell::sidebar_views(v).cloned().collect(),
            _ => vec![],
        };
        let mut out = vec![BarItem::Home];
        if views.len() <= BAR_MAX_VIEWS {
            out.extend(views.into_iter().map(|v| BarItem::View(Box::new(v))));
        } else {
            out.extend(views.into_iter().take(BAR_MAX_VIEWS - 1).map(|v| BarItem::View(Box::new(v))));
            out.push(BarItem::Library);
        }
        out.push(BarItem::Search);
        out
    }

    fn activate_bar(&mut self, cx: &mut Cx, i: usize) {
        match self.bar_items().get(i).cloned() {
            None => self.user_menu = Some(0),
            Some(BarItem::Home) => self.auto = true,
            Some(BarItem::View(v)) => cx.push(super::library::screen(*v)),
            Some(BarItem::Library) => self.to_libraries = true,
            Some(BarItem::Search) => cx.push(super::search::screen()),
        }
    }

    fn pick_user_menu(&mut self, cx: &mut Cx, i: usize) {
        self.user_menu = None;
        match i {
            0 => cx.push(super::favourites::screen()),
            1 => cx.push(super::playlists::list()),
            2 => cx.push(super::settings_screen::screen()),
            _ => {
                if self.leaving {
                    return;
                }
                // Switch user and Sign out both end this session and keep the server; only Sign out also logs
                // out server side and forgets the saved user.
                self.switching = i == MENU_SWITCH;
                self.leaving = true;
                self.user_id = cx.client.as_ref().and_then(|c| c.session.as_ref()).map(|s| s.user_id.clone());
                let mut client = cx.api();
                let switching = self.switching;
                cx.spawn("signout", move || {
                    if switching {
                        // Switch user keeps their token on this device so they can come straight back (D10).
                        client.session = None;
                    } else {
                        let _ = client.sign_out();
                    }
                    client
                });
            }
        }
    }

    fn activate(&mut self, cx: &mut Cx) {
        let Some(role) = self.roles.get(self.focus.group).cloned() else { return };
        match role {
            Role::Banner => {
                if self.focus.index == 0 {
                    self.offline = None;
                    self.reload(cx);
                    let client = cx.api();
                    cx.spawn("verify", move || client.verify_session());
                } else {
                    cx.reset(super::connect::screen(None));
                }
            }
            Role::Hero => self.activate_hero(cx, self.focus.index),
            Role::Topbar => self.activate_bar(cx, self.focus.index),
            Role::Row(id) => {
                if let Some(item) = self.item_for(id, self.focus.index) {
                    if id == RowId::Libraries {
                        cx.push(super::library::screen(item));
                    } else {
                        cx.push(super::item::screen(item));
                    }
                } else if self.row_is_error(id) {
                    self.retry_row(cx, id);
                }
            }
        }
    }

    fn draw_row(
        ui: &mut egui::Ui,
        cx: &mut Cx,
        heading: &str,
        state: &RowState,
        kind: CardKind,
        focus: Option<usize>,
        hearts: bool,
    ) -> RowOut {
        let t = cx.theme;
        match state {
            RowState::Loaded(items) => {
                if hearts {
                    parts::row_favourites(ui, cx, heading, heading, items, kind, focus)
                } else {
                    parts::row(ui, cx, heading, heading, items, kind, focus)
                }
            }
            RowState::Loading => {
                ui.add_space(t.row_gap / 2.0);
                parts::heading(ui, t, heading);
                ui.add_space(t.gap / 3.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = t.gap;
                    for _ in 0..6 {
                        parts::skeleton_card(ui, t, kind);
                    }
                });
                RowOut { clicked: None, hovered: None, xs: vec![] }
            }
            RowState::Error(msg) => {
                ui.add_space(t.row_gap / 2.0);
                parts::heading(ui, t, heading);
                parts::error_line(ui, t, msg);
                let r = parts::button(ui, t, "Retry", focus == Some(0), false);
                RowOut {
                    clicked: r.clicked().then_some(0),
                    hovered: r.hovered().then_some(0),
                    xs: vec![r.rect.center().x],
                }
            }
            RowState::Hidden => RowOut { clicked: None, hovered: None, xs: vec![] },
        }
    }
}

fn clamp(groups: &[Group], f: Focus) -> Focus {
    if groups.is_empty() {
        return Focus::default();
    }
    let gi = if f.group < groups.len() && groups[f.group].len > 0 {
        f.group
    } else {
        groups
            .iter()
            .enumerate()
            .filter(|(_, g)| g.len > 0)
            .map(|(i, _)| i)
            .min_by_key(|&i| i.abs_diff(f.group))
            .unwrap_or(0)
    };
    Focus { group: gi, index: f.index.min(groups[gi].len - 1) }
}

impl Screen for Home {
    fn name(&self) -> &'static str {
        "home"
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        if let Some(h) = self.user_menu {
            match key {
                Key::Up => self.user_menu = Some(h.saturating_sub(1)),
                Key::Down => self.user_menu = Some((h + 1).min(USER_MENU.len() - 1)),
                Key::Enter => self.pick_user_menu(cx, h),
                Key::Back => self.user_menu = None,
                _ => {}
            }
            return true;
        }
        match key {
            Key::Enter => {
                self.auto = false;
                self.activate(cx);
                true
            }
            Key::Up | Key::Down | Key::Left | Key::Right => {
                self.auto = false;
                self.focus = move_focus(&self.groups, self.focus, key);
                true
            }
            _ => false,
        }
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn std::any::Any + Send>) {
        match tag {
            "resume" => match *payload.downcast::<Result<Vec<Item>>>().unwrap() {
                Err(ApiError::Unauthorized) => cx.signed_out(),
                r => self.resume = Some(r),
            },
            "next_up" => match *payload.downcast::<Result<Vec<Item>>>().unwrap() {
                Err(ApiError::Unauthorized) => cx.signed_out(),
                r => self.next_up = Some(r),
            },
            "favourites" => match *payload.downcast::<Result<Vec<Item>>>().unwrap() {
                Err(ApiError::Unauthorized) => cx.signed_out(),
                r => self.favourites = Some(r),
            },
            "views" => match *payload.downcast::<Result<Vec<Item>>>().unwrap() {
                Err(ApiError::Unauthorized) => cx.signed_out(),
                Ok(views) => {
                    self.video_views = views
                        .iter()
                        .filter(|v| matches!(v.collection_type.as_deref(), Some("movies") | Some("tvshows")))
                        .cloned()
                        .collect();
                    self.spawn_latest(cx);
                    self.views = Some(Ok(views));
                }
                Err(e) => self.views = Some(Err(e)),
            },
            "latest" => {
                let Ok(rows) = payload.downcast::<Vec<(String, Result<Vec<Item>>)>>() else { return };
                let mut unauth = false;
                for (id, r) in *rows {
                    if r == Err(ApiError::Unauthorized) {
                        unauth = true;
                        continue;
                    }
                    self.latest.insert(id, r);
                }
                if unauth {
                    cx.signed_out();
                }
            }
            "hero" => {
                if let Ok(r) = payload.downcast::<Result<Item>>() {
                    if let Ok(item) = *r {
                        self.hero_full = Some(item);
                    }
                }
            }
            "signout" => {
                let Ok(c) = payload.downcast::<crate::api::Client>() else { return };
                let client = *c;
                if let Some(c) = cx.client.as_mut() {
                    c.session = None;
                }
                if let Some(id) = cx.settings.last_server.clone() {
                    cx.settings.forget_session(&id);
                    if !self.switching {
                        if let Some(uid) = self.user_id.take() {
                            cx.settings.forget_saved_user(&id, &uid);
                        }
                    }
                }
                cx.save_settings();
                if self.switching {
                    cx.reset(super::signin::switch_user(client));
                } else {
                    cx.reset(super::signin::screen(client, None));
                }
            }
            "verify" => match *payload.downcast::<Result<()>>().unwrap() {
                Ok(()) => self.offline = None,
                Err(ApiError::Unauthorized) => cx.signed_out(),
                Err(ApiError::Network(_)) => {
                    let name = cx
                        .settings
                        .last()
                        .map(|e| e.name.clone())
                        .filter(|s| !s.is_empty())
                        .or_else(|| cx.client.as_ref().map(|c| c.base.clone()))
                        .unwrap_or_else(|| "the server".to_string());
                    self.offline = Some(name);
                }
                Err(_) => {}
            },
            _ => {}
        }
    }

    fn poster_tags(&self) -> Vec<(String, Option<String>)> {
        let mut lists: Vec<&Vec<Item>> = vec![];
        for r in [&self.resume, &self.next_up, &self.favourites] {
            lists.extend(r.as_ref().and_then(|r| r.as_ref().ok()));
        }
        lists.extend(self.latest.values().filter_map(|r| r.as_ref().ok()));
        lists.into_iter().flatten().map(|i| (i.id.clone(), i.image_tags.get("Primary").cloned())).collect()
    }

    fn on_resume(&mut self, cx: &mut Cx) {
        // PP review #4: a shared poster changed while Home was underneath: re-read every row (new ImageTags).
        if self.poster_seen.is_some_and(|g| g != cx.poster_gen) {
            self.poster_seen = Some(cx.poster_gen);
            self.reload(cx);
            return;
        }
        self.resume = None;
        self.next_up = None;
        self.favourites = None;
        let client = cx.api();
        cx.spawn("resume", move || client.resume(20));
        let client = cx.api();
        let (rw, nd) = (cx.settings.next_up_rewatching, cx.settings.next_up_days);
        cx.spawn("next_up", move || client.next_up_with(20, rw, nd));
        let client = cx.api();
        cx.spawn("favourites", move || client.favourites("Movie,Series"));
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        if !self.started {
            self.started = true;
            self.poster_seen = Some(cx.poster_gen);
            self.start(cx);
        }
        let t = cx.theme;
        let pointer_moved = ui.input(|i| i.pointer.is_moving());

        // G12 / P-HOME-5: rows follow the user's Home order and hide list.
        let order = cx.settings.home_order.clone();
        let mut plan: Vec<(RowId, String, CardKind, RowState)> = vec![];
        for section in order {
            if !cx.settings.home_visible(section) {
                continue;
            }
            match section {
                HomeSection::Continue => plan.push((
                    RowId::Continue,
                    "Continue watching".to_string(),
                    CardKind::Thumb,
                    Self::state_of(self.resume.as_ref()),
                )),
                HomeSection::NextUp => plan.push((
                    RowId::NextUp,
                    "Next up".to_string(),
                    CardKind::Thumb,
                    Self::state_of(self.next_up.as_ref()),
                )),
                HomeSection::Favourites => plan.push((
                    RowId::Favourites,
                    "Favourites".to_string(),
                    CardKind::Poster,
                    Self::state_of(self.favourites.as_ref()),
                )),
                HomeSection::Latest => {
                    for (i, v) in self.video_views.iter().enumerate() {
                        let st = Self::state_of(self.latest.get(&v.id));
                        plan.push((RowId::Latest(i), format!("Latest {}", v.name), CardKind::Poster, st));
                    }
                }
                HomeSection::Libraries => plan.push((
                    RowId::Libraries,
                    "Libraries".to_string(),
                    CardKind::Thumb,
                    Self::state_of(self.views.as_ref()),
                )),
            }
        }

        // Group skeleton: banner? + topbar + one group per visible row, in draw order.
        let topbar_gi = usize::from(self.offline.is_some());
        let mut roles: Vec<Role> = vec![];
        let mut groups: Vec<Group> = vec![];
        let mut group_of: HashMap<RowId, usize> = HashMap::new();
        if self.offline.is_some() {
            roles.push(Role::Banner);
            groups.push(Group { len: 2, cols: 2, xs: vec![] });
        }
        // TV: the top pill bar. Desktop: the app shell's sidebar replaces it, so its group stays empty.
        let tv_bar = !cx.sidebar;
        roles.push(Role::Topbar);
        let bar = if tv_bar { self.bar_items() } else { vec![] };
        let bar_len = if tv_bar { bar.len() + 1 } else { 0 };
        groups.push(Group { len: bar_len, cols: bar_len.max(1), xs: vec![] });
        let hero = self.hero_item();
        if let Some(h) = &hero {
            if self.hero_for.as_deref() != Some(h.id.as_str()) {
                self.hero_for = Some(h.id.clone());
                let client = cx.api();
                let id = h.id.clone();
                cx.spawn("hero", move || client.item(&id));
            }
            roles.push(Role::Hero);
            groups.push(Group { len: 2, cols: 2, xs: vec![] });
        }
        let hero_gi = topbar_gi + 1;
        let mut gi = topbar_gi + 1 + usize::from(hero.is_some());
        for (id, _, _, st) in &plan {
            match st {
                RowState::Hidden | RowState::Loading => {}
                RowState::Loaded(items) => {
                    roles.push(Role::Row(*id));
                    groups.push(Group { len: items.len(), cols: items.len(), xs: vec![] });
                    group_of.insert(*id, gi);
                    gi += 1;
                }
                RowState::Error(_) => {
                    roles.push(Role::Row(*id));
                    groups.push(Group { len: 1, cols: 1, xs: vec![] });
                    group_of.insert(*id, gi);
                    gi += 1;
                }
            }
        }

        if self.auto {
            // Focus starts on the hero's Play pill (Apple TV), else the first row.
            let first_row = roles.iter().position(|r| matches!(r, Role::Hero | Role::Row(_)));
            self.focus = match first_row {
                Some(gi) => Focus { group: gi, index: 0 },
                None => Focus { group: topbar_gi, index: 0 },
            };
        } else {
            self.focus = clamp(&groups, self.focus);
        }
        if self.to_libraries {
            self.to_libraries = false;
            if let Some(&g) = group_of.get(&RowId::Libraries) {
                self.focus = Focus { group: g, index: 0 };
                self.auto = false;
            }
        }

        // Full-bleed page: the scroll area runs under the side margins; parts line up with the margin inside.
        let page = ui.max_rect();
        let bleed = page.expand2(egui::vec2(t.margin, 0.0));
        // Offline banner (R-SET-2).
        if let Some(name) = self.offline.clone() {
            let mut xs = vec![];
            let mut clicked = None;
            let mut hovered = None;
            ui.horizontal(|ui| {
                ui.add_space(t.margin);
                parts::error_line(ui, t, &format!("Can't reach {name}"));
                ui.add_space(t.gap);
                let r = parts::button(ui, t, "Retry", self.focus == Focus { group: 0, index: 0 }, true);
                xs.push(r.rect.center().x);
                if r.clicked() {
                    clicked = Some(0);
                }
                if r.hovered() {
                    hovered = Some(0);
                }
                ui.add_space(t.gap / 2.0);
                let r = parts::button(ui, t, "Change server", self.focus == Focus { group: 0, index: 1 }, false);
                xs.push(r.rect.center().x);
                if r.clicked() {
                    clicked = Some(1);
                }
                if r.hovered() {
                    hovered = Some(1);
                }
            });
            groups[0].xs = xs;
            if pointer_moved {
                if let Some(i) = hovered {
                    self.focus = Focus { group: 0, index: i };
                    self.auto = false;
                }
            }
            if let Some(i) = clicked {
                self.auto = false;
                if i == 0 {
                    self.offline = None;
                    self.reload(cx);
                    let client = cx.api();
                    cx.spawn("verify", move || client.verify_session());
                } else {
                    cx.reset(super::connect::screen(None));
                }
            }
        }

        // Rows, in one vertical ScrollArea so the focused row scrolls into view.
        let focus_group = self.focus.group;
        let focus_index = self.focus.index;
        let mut clicked: Option<(RowId, usize)> = None;
        let mut hovered: Option<(RowId, usize)> = None;
        let mut xs_map: Vec<Option<Vec<f32>>> = vec![None; plan.len()];
        let mut hero_out = None;
        let mut scroll_y = 0.0;
        ui.scope_builder(egui::UiBuilder::new().max_rect(bleed), |ui| {
            scroll_y = egui::ScrollArea::vertical()
                .show(ui, |ui| {
                    egui::Frame::NONE.inner_margin(egui::Margin::symmetric(t.margin as i8, 0)).show(ui, |ui| {
                        ui.set_min_width(page.width());
                        if let Some(h) = &hero {
                            let focus = (focus_group == hero_gi).then_some(focus_index);
                            let top = ui.cursor().top();
                            let out = parts::hero(ui, cx, h, focus);
                            if focus.is_some() && crate::ui::shell::keyboard_focus(ui.ctx()) {
                                ui.scroll_to_rect(
                                    egui::Rect::from_min_size(egui::pos2(page.left(), top), egui::vec2(1.0, 1.0)),
                                    None,
                                );
                            }
                            hero_out = Some(out);
                        } else {
                            ui.add_space(if tv_bar { t.topbar_h + t.margin } else { t.margin });
                        }
                        for (k, (id, heading, kind, st)) in plan.iter().enumerate() {
                            if matches!(st, RowState::Hidden) {
                                continue;
                            }
                            let focus_idx = group_of.get(id).and_then(|g| (*g == focus_group).then_some(focus_index));
                            let top = ui.cursor().top();
                            let out = Self::draw_row(ui, cx, heading, st, *kind, focus_idx, *id == RowId::Favourites);
                            let bottom = ui.cursor().top();
                            if focus_idx.is_some() && crate::ui::shell::keyboard_focus(ui.ctx()) {
                                let rect = egui::Rect::from_min_max(
                                    egui::pos2(ui.min_rect().left(), top),
                                    egui::pos2(ui.max_rect().right(), bottom),
                                );
                                ui.scroll_to_rect(rect, None);
                            }
                            xs_map[k] = Some(out.xs);
                            if let Some(i) = out.clicked {
                                clicked = Some((*id, i));
                            }
                            if let Some(i) = out.hovered {
                                hovered = Some((*id, i));
                            }
                        }
                        ui.add_space(t.margin);
                    });
                })
                .state
                .offset
                .y;
        });
        if let Some(out) = hero_out {
            groups[hero_gi].xs = out.xs;
            if pointer_moved {
                if let Some(i) = out.hovered {
                    self.focus = Focus { group: hero_gi, index: i };
                    self.auto = false;
                }
            }
            if let Some(i) = out.clicked {
                self.auto = false;
                self.activate_hero(cx, i);
            }
        }
        for (k, xs) in xs_map.into_iter().enumerate() {
            if let (Some(g), Some(xs)) = (group_of.get(&plan[k].0), xs) {
                groups[*g].xs = xs;
            }
        }

        if pointer_moved {
            if let Some((id, i)) = hovered {
                if let Some(&g) = group_of.get(&id) {
                    self.focus = Focus { group: g, index: i };
                    self.auto = false;
                }
            }
        }
        if let Some((id, i)) = clicked {
            self.auto = false;
            if let Some(item) = self.item_for(id, i) {
                if id == RowId::Libraries {
                    cx.push(super::library::screen(item));
                } else {
                    cx.push(super::item::screen(item));
                }
            } else if self.row_is_error(id) {
                self.retry_row(cx, id);
            }
        }

        // Topbar (TV only).
        if tv_bar {
            let tb_focus = (self.focus.group == topbar_gi).then_some(self.focus.index);
            let user_name = cx.settings.last().and_then(|e| e.session.as_ref()).map(|s| s.user_name.clone());
            let user_name =
                user_name.or_else(|| cx.client.as_ref().and_then(|c| c.session.as_ref()).map(|s| s.user_name.clone()));
            // The tvOS top pill bar floats over the hero (drawn last, so it is on top).
            let bar_rect = egui::Rect::from_min_size(page.min, egui::vec2(page.width(), t.topbar_h));
            let user = Some(user_name.as_deref().unwrap_or("Account"));
            // UX N1: once Home scrolls, a pre-darkened band fades in behind the bar so rows don't show through.
            let k = (scroll_y / t.hero_fade_h).clamp(0.0, 1.0);
            if k > 0.0 {
                let solid =
                    egui::Rect::from_min_size(page.min, egui::vec2(page.width(), t.bar_top + t.bar_h + t.gap / 2.0))
                        .expand2(egui::vec2(t.margin, 0.0));
                ui.painter().rect_filled(solid, 0.0, t.bg.gamma_multiply(k));
                let fade = egui::Rect::from_min_size(solid.left_bottom(), egui::vec2(solid.width(), t.gap * 1.5));
                parts::gradient(ui, fade, t.bg.gamma_multiply(k), egui::Color32::TRANSPARENT, false);
            }
            let labels: Vec<String> = bar
                .iter()
                .map(|b| match b {
                    BarItem::Home => "Home".to_string(),
                    BarItem::View(v) => v.name.clone(),
                    BarItem::Library => "Library".to_string(),
                    BarItem::Search => "Search".to_string(),
                })
                .collect();
            let labels: Vec<&str> = labels.iter().map(|l| l.as_str()).collect();
            let (tb, chip) = ui
                .scope_builder(egui::UiBuilder::new().max_rect(bar_rect), |ui| {
                    parts::pill_bar(ui, cx, &labels, 0, tb_focus, user)
                })
                .inner;
            groups[topbar_gi].xs = tb.xs;
            if pointer_moved {
                if let Some(i) = tb.hovered {
                    self.focus = Focus { group: topbar_gi, index: i };
                    self.auto = false;
                }
            }
            if let Some(i) = tb.clicked {
                self.focus = Focus { group: topbar_gi, index: i };
                self.auto = false;
                self.activate_bar(cx, i);
            }
            if let (Some(h), Some(chip)) = (self.user_menu, chip) {
                self.draw_user_menu(ui, cx, chip, h);
            }
        }

        // D10: Sign out / Switch user waits on the server — say so (a translucent pill, desktop and TV).
        if self.leaving {
            let label = if self.switching { "Switching user…" } else { "Signing out…" };
            let at = page.center_top() + egui::vec2(0.0, t.bar_top + t.bar_h / 2.0);
            let g = ui.fonts(|f| {
                f.layout_no_wrap(label.to_string(), t.font_w(t.body_size, crate::theme::Weight::Medium), t.text)
            });
            let pill = egui::Rect::from_center_size(at, egui::vec2(g.size().x + 2.0 * t.bar_item_pad, t.bar_h));
            ui.painter().add(t.shadow(0.6).as_shape(pill, t.bar_h / 2.0));
            ui.painter().rect_filled(pill, t.bar_h / 2.0, t.menu_fill);
            ui.painter().galley(pill.center() - g.size() / 2.0, g, t.text);
        }

        self.groups = groups;
        self.roles = roles;
    }
}
