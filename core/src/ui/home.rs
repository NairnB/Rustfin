//! R-UI-4 Home: a fixed top bar above rows (Continue watching, Next up, Latest per library, Libraries).
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
    Row(RowId),
}

enum RowState {
    Loading,
    Hidden,
    Loaded(Vec<Item>),
    Error(String),
}

pub struct Home {
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
}

pub fn screen(verify_session: bool) -> Box<dyn Screen> {
    Box::new(Home {
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
        cx.spawn("next_up", move || client.next_up(20));
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
        cx.spawn("next_up", move || client.next_up(20));
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
                cx.spawn("next_up", move || client.next_up(20));
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
            Role::Topbar => match self.focus.index {
                1 => cx.push(super::search::screen()),
                2 => cx.push(super::settings_screen::screen()),
                3 => cx.push(super::favourites::screen()),
                4 => cx.push(super::playlists::list()),
                _ => {}
            },
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

    fn on_resume(&mut self, cx: &mut Cx) {
        self.resume = None;
        self.next_up = None;
        self.favourites = None;
        let client = cx.api();
        cx.spawn("resume", move || client.resume(20));
        let client = cx.api();
        cx.spawn("next_up", move || client.next_up(20));
        let client = cx.api();
        cx.spawn("favourites", move || client.favourites("Movie,Series"));
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        if !self.started {
            self.started = true;
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
        roles.push(Role::Topbar);
        groups.push(Group { len: 5, cols: 5, xs: vec![] });
        let mut gi = topbar_gi + 1;
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
            let first_row = roles.iter().position(|r| matches!(r, Role::Row(_)));
            self.focus = match first_row {
                Some(gi) => Focus { group: gi, index: 0 },
                None => Focus { group: topbar_gi, index: 0 },
            };
        } else {
            self.focus = clamp(&groups, self.focus);
        }

        // Topbar.
        let tb_focus = (self.focus.group == topbar_gi).then_some(self.focus.index);
        let tb = parts::topbar(ui, cx, tb_focus);
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
            match i {
                1 => cx.push(super::search::screen()),
                2 => cx.push(super::settings_screen::screen()),
                3 => cx.push(super::favourites::screen()),
                4 => cx.push(super::playlists::list()),
                _ => {}
            }
        }

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
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(t.margin);
            for (k, (id, heading, kind, st)) in plan.iter().enumerate() {
                if matches!(st, RowState::Hidden) {
                    continue;
                }
                let focus_idx = group_of.get(id).and_then(|g| (*g == focus_group).then_some(focus_index));
                let top = ui.cursor().top();
                let out = Self::draw_row(ui, cx, heading, st, *kind, focus_idx, *id == RowId::Favourites);
                let bottom = ui.cursor().top();
                if focus_idx.is_some() {
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

        self.groups = groups;
        self.roles = roles;
    }
}
