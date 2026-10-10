//! Admin users (P-ADM-5..10): the user list (with Add user) and one user's page with four tabs —
//! Profile, Access, Parental control, Password. Edits are held locally and saved with one button per tab
//! (the whole policy is posted); Delete and Reset password-style actions that lose something confirm or say so.
use super::sectioned_list;
use crate::api::admin::ParentalRating;
use crate::api::gen_admin::{AccessSchedule, UserDto, UserPolicy, VirtualFolderInfo};
use crate::api::{ApiError, Result};
use crate::app::{Cx, Screen};
use crate::nav::{move_focus, Focus, Group};
use crate::platform::Key;
use crate::theme::Theme;
use crate::ui::parts;
use crate::ui::widgets::{self, DropdownEvent, DropdownState, ListRow};
use egui::{vec2, Sense};

// ---------- pure helpers (unit-tested below) ----------

/// The feature switches "Enable all features" flips together.
fn all_features(p: &UserPolicy) -> bool {
    p.enable_media_playback
        && p.enable_audio_playback_transcoding
        && p.enable_video_playback_transcoding
        && p.enable_playback_remuxing
        && p.enable_content_downloading
        && p.enable_collection_management
        && p.enable_subtitle_management
        && p.enable_lyric_management
        && p.enable_live_tv_access
        && p.enable_user_preference_access
}

fn set_all_features(p: &mut UserPolicy, on: bool) {
    p.enable_media_playback = on;
    p.enable_audio_playback_transcoding = on;
    p.enable_video_playback_transcoding = on;
    p.enable_playback_remuxing = on;
    p.enable_content_downloading = on;
    p.enable_collection_management = on;
    p.enable_subtitle_management = on;
    p.enable_lyric_management = on;
    p.enable_live_tv_access = on;
    p.enable_user_preference_access = on;
}

/// (label, level) pairs sorted by level, one per level (the first name the server lists for it). A level the
/// user already has that the server does not list is added as "Rating N" so the dropdown can show it.
fn rating_options(list: &[ParentalRating], current: Option<i32>) -> Vec<(String, i32)> {
    let mut out: Vec<(String, i32)> = vec![];
    for r in list {
        if let Some(v) = r.value {
            if !out.iter().any(|(_, x)| *x == v) {
                out.push((r.name.clone(), v));
            }
        }
    }
    if let Some(v) = current {
        if !out.iter().any(|(_, x)| *x == v) {
            out.push((format!("Rating {v}"), v));
        }
    }
    out.sort_by_key(|(_, v)| *v);
    out
}

/// 9.5 → "09:30".
fn hour_label(h: f64) -> String {
    let mins = (h * 60.0).round() as i64;
    format!("{:02}:{:02}", mins / 60, mins % 60)
}

fn schedule_label(s: &AccessSchedule) -> String {
    format!("{}  {} – {}", s.day_of_week, hour_label(s.start_hour), hour_label(s.end_hour))
}

const DAYS: [&str; 10] =
    ["Everyday", "Weekday", "Weekend", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

/// "2026-10-10T17:00:00.0000000Z" → "2026-10-10 17:00".
pub(super) fn short_date(iso: &str) -> String {
    let mut s: String = iso.chars().take(16).collect();
    if let Some(pos) = s.find('T') {
        s.replace_range(pos..pos + 1, " ");
    }
    s
}

// ---------- shared control cursor ----------

/// Walks the focusable controls of one frame: counts them, collects their x centres, and tells each whether
/// it has focus and whether Enter / a click fired it. Focus index = draw order.
pub(super) struct Ctl {
    pub(super) idx: usize,
    focus: Option<usize>,
    enter: bool,
    moving: bool,
    pub(super) xs: Vec<f32>,
    pub(super) hover: Option<usize>,
    pub(super) clicked: Option<usize>,
    /// The focused control is a text field (Left/Right belong to the cursor).
    pub(super) on_field: bool,
    /// Enter pressed in a text field: move on to the next control.
    pub(super) advance: bool,
    /// (dropdown number, option count) of the focused dropdown, for the key path.
    pub(super) dd: Option<(usize, usize)>,
}

impl Ctl {
    pub(super) fn new(focus: Option<usize>, enter: bool, moving: bool) -> Ctl {
        Ctl {
            idx: 0,
            focus,
            enter,
            moving,
            xs: vec![],
            hover: None,
            clicked: None,
            on_field: false,
            advance: false,
            dd: None,
        }
    }

    fn here(&self) -> bool {
        self.focus == Some(self.idx)
    }

    fn reg(&mut self, ui: &egui::Ui, x: f32, hovered: bool, clicked: bool) {
        self.xs.push(x);
        if hovered && self.moving {
            self.hover = Some(self.idx);
        }
        if clicked {
            self.clicked = Some(self.idx);
        }
        if self.here() {
            ui.scroll_to_cursor(Some(egui::Align::Center));
        }
        self.idx += 1;
    }

    pub(super) fn toggle(&mut self, ui: &mut egui::Ui, t: &Theme, label: &str, on: bool) -> bool {
        let f = self.here();
        let o = widgets::toggle(ui, t, label, on, f);
        let fired = o.clicked || (f && self.enter);
        self.reg(ui, o.x, o.hovered, o.clicked);
        fired
    }

    pub(super) fn button(&mut self, ui: &mut egui::Ui, t: &Theme, label: &str, primary: bool) -> bool {
        let f = self.here();
        let r = parts::button(ui, t, label, f, primary);
        let fired = r.clicked() || (f && self.enter);
        self.reg(ui, r.rect.center().x, r.hovered(), r.clicked());
        fired
    }

    pub(super) fn field(&mut self, ui: &mut egui::Ui, t: &Theme, text: &mut String, hint: &str, password: bool) {
        let f = self.here();
        let r = parts::text_field(ui, t, text, hint, f, password);
        if f {
            self.on_field = true;
            if self.enter {
                self.advance = true;
            }
        }
        self.reg(ui, r.rect.center().x, false, r.clicked());
    }

    /// A dropdown (number `which`); returns the option clicked with the mouse, if any.
    pub(super) fn dropdown(
        &mut self,
        ui: &mut egui::Ui,
        t: &Theme,
        state: &mut DropdownState,
        which: usize,
        label: &str,
        options: &[String],
    ) -> Option<usize> {
        let f = self.here();
        let o = widgets::dropdown(ui, t, state, label, options, f);
        if f {
            self.dd = Some((which, options.len()));
        }
        self.reg(ui, o.button_x, o.button_hovered, false);
        o.item_clicked
    }
}

pub(super) fn skeleton_rows(ui: &mut egui::Ui, t: &Theme, n: usize) {
    for _ in 0..n {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), t.list_row_h), Sense::hover());
        ui.painter().rect_filled(rect.shrink2(vec2(0.0, t.gap / 4.0)), t.radius, t.skeleton);
    }
}

pub(super) fn fail(cx: &mut Cx, e: ApiError) -> Option<String> {
    match e {
        ApiError::Unauthorized => {
            cx.signed_out();
            None
        }
        ApiError::Forbidden => Some("Only an administrator can do that.".into()),
        e => Some(e.to_string()),
    }
}

// ---------- the list ----------

const L_ADD: usize = 0;
const L_LIST: usize = 1;

pub struct Users {
    users: Option<Vec<UserDto>>,
    error: Option<String>,
    started: bool,
    focus: Focus,
    groups: Vec<Group>,
}

pub fn screen() -> Box<dyn Screen> {
    Box::new(Users {
        users: None,
        error: None,
        started: false,
        focus: Focus { group: L_LIST, index: 0 },
        groups: vec![],
    })
}

impl Users {
    fn load(&mut self, cx: &Cx) {
        self.error = None;
        let c = cx.api();
        cx.spawn("users", move || c.users());
    }

    fn open(&self, idx: usize, cx: &mut Cx) {
        if let Some(u) = self.users.as_ref().and_then(|v| v.get(idx)) {
            cx.push(user_page(u.id.clone(), u.name.clone().unwrap_or_default()));
        }
    }
}

impl Screen for Users {
    fn name(&self) -> &'static str {
        "users"
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        match key {
            Key::Enter => {
                match self.focus.group {
                    L_ADD => cx.push(add_user()),
                    _ if self.error.is_some() => self.load(cx),
                    _ => self.open(self.focus.index, cx),
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
        if tag != "users" {
            return;
        }
        let Ok(r) = payload.downcast::<Result<Vec<UserDto>>>() else { return };
        match *r {
            Ok(mut v) => {
                v.sort_by_key(|u| u.name.clone().unwrap_or_default().to_lowercase());
                self.users = Some(v);
            }
            Err(e) => self.error = fail(cx, e),
        }
    }

    fn on_resume(&mut self, cx: &mut Cx) {
        self.load(cx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        if !self.started {
            self.started = true;
            self.load(cx);
        }
        let moving = ui.input(|i| i.pointer.is_moving());
        ui.add_space(t.margin);
        parts::title(ui, t, "Users");
        ui.add_space(t.gap / 2.0);

        let mut add_x = vec![];
        let mut add_clicked = false;
        let r = parts::button(ui, t, "Add user", self.focus.group == L_ADD, true);
        add_x.push(r.rect.center().x);
        if r.hovered() && moving {
            self.focus = Focus { group: L_ADD, index: 0 };
        }
        add_clicked |= r.clicked();
        ui.add_space(t.gap / 2.0);

        let mut list_xs = vec![];
        let mut hovered = None;
        let mut clicked = None;
        let mut retry = false;
        let mut count = 0;
        if let Some(e) = self.error.clone() {
            parts::error_line(ui, t, &e);
            let r = parts::button(ui, t, "Retry", self.focus.group == L_LIST, false);
            list_xs.push(r.rect.center().x);
            count = 1;
            if r.hovered() && moving {
                self.focus = Focus { group: L_LIST, index: 0 };
            }
            retry = r.clicked();
        } else if let Some(users) = &self.users {
            let labels: Vec<String> = users.iter().map(|u| u.name.clone().unwrap_or_default()).collect();
            let badges: Vec<String> = users
                .iter()
                .map(|u| {
                    let p = u.policy.clone().unwrap_or_default();
                    let mut b = vec![];
                    if p.is_administrator {
                        b.push("Administrator");
                    }
                    if p.is_disabled {
                        b.push("Disabled");
                    }
                    b.join(" · ")
                })
                .collect();
            let seen: Vec<String> = users
                .iter()
                .map(|u| match u.last_activity_date.as_deref() {
                    Some(d) if !d.is_empty() => format!("Last seen {}", short_date(d)),
                    _ => "Never seen".to_string(),
                })
                .collect();
            let rows: Vec<ListRow<'_>> = (0..users.len())
                .map(|i| ListRow { label: &labels[i], secondary: Some(&badges[i]), value: Some(&seen[i]) })
                .collect();
            let sections: Vec<(&str, Vec<ListRow<'_>>)> = vec![("", rows)];
            egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
                let out = sectioned_list(ui, t, &sections, (self.focus.group == L_LIST).then_some(self.focus.index));
                list_xs = out.xs;
                hovered = out.hovered;
                clicked = out.clicked;
            });
            count = users.len();
        } else {
            skeleton_rows(ui, t, 4);
        }

        self.groups = vec![Group { len: 1, cols: 1, xs: add_x }, Group { len: count, cols: 1, xs: list_xs }];
        self.focus = move_focus(&self.groups, self.focus, Key::Enter);
        if moving {
            if let Some(i) = hovered {
                self.focus = Focus { group: L_LIST, index: i };
            }
        }
        if add_clicked {
            cx.push(add_user());
        }
        if retry {
            self.load(cx);
        }
        if let Some(i) = clicked {
            self.focus = Focus { group: L_LIST, index: i };
            self.open(i, cx);
        }
    }
}

// ---------- add user ----------

struct AddUser {
    name: String,
    password: String,
    busy: bool,
    error: Option<String>,
    focus: Focus,
    groups: Vec<Group>,
    enter: bool,
}

fn add_user() -> Box<dyn Screen> {
    Box::new(AddUser {
        name: String::new(),
        password: String::new(),
        busy: false,
        error: None,
        focus: Focus { group: 0, index: 0 },
        groups: vec![],
        enter: false,
    })
}

impl AddUser {
    fn create(&mut self, cx: &Cx) {
        if self.busy {
            return;
        }
        let name = self.name.trim().to_string();
        if name.is_empty() {
            self.error = Some("Give the user a name.".into());
            return;
        }
        self.busy = true;
        self.error = None;
        let pw = self.password.clone();
        let c = cx.api();
        cx.spawn("create", move || c.create_user(&name, &pw));
    }
}

impl Screen for AddUser {
    fn name(&self) -> &'static str {
        "useradd"
    }

    fn on_key(&mut self, _cx: &mut Cx, key: Key) -> bool {
        match key {
            Key::Enter => {
                self.enter = true;
                true
            }
            Key::Left | Key::Right if self.focus.index < 2 => true,
            Key::Up | Key::Down | Key::Left | Key::Right => {
                self.focus = move_focus(&self.groups, self.focus, key);
                true
            }
            _ => false,
        }
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn std::any::Any + Send>) {
        if tag != "create" {
            return;
        }
        self.busy = false;
        let Ok(r) = payload.downcast::<Result<UserDto>>() else { return };
        match *r {
            Ok(_) => cx.pop(),
            Err(ApiError::Server(400, _)) => self.error = Some("That name is taken or not allowed.".into()),
            Err(e) => self.error = fail(cx, e),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        let moving = ui.input(|i| i.pointer.is_moving());
        let mut ctl = Ctl::new(Some(self.focus.index), std::mem::take(&mut self.enter), moving);
        ui.add_space(t.margin);
        parts::title(ui, t, "Add user");
        ui.add_space(t.gap / 2.0);
        ctl.field(ui, t, &mut self.name, "User name", false);
        ui.add_space(t.gap / 2.0);
        ctl.field(ui, t, &mut self.password, "Password", true);
        ui.add_space(t.gap / 2.0);
        let label = if self.busy { "Creating…" } else { "Create user" };
        let create = ctl.button(ui, t, label, true);
        if let Some(e) = &self.error {
            ui.add_space(t.gap / 2.0);
            parts::error_line(ui, t, e);
        }
        self.groups = vec![Group { len: ctl.idx, cols: 1, xs: ctl.xs.clone() }];
        self.focus.index = self.focus.index.min(ctl.idx.saturating_sub(1));
        if let Some(i) = ctl.hover.or(ctl.clicked) {
            self.focus.index = i;
        }
        if ctl.advance {
            self.focus = move_focus(&self.groups, self.focus, Key::Down);
        }
        if create {
            self.create(cx);
        }
    }
}

// ---------- one user ----------

const G_TABS: usize = 0;
const G_BODY: usize = 1;
const TABS: [&str; 4] = ["Profile", "Access", "Parental control", "Password"];
const DD_RATING: usize = 0;
const DD_DAY: usize = 1;
const DD_FROM: usize = 2;
const DD_TO: usize = 3;

/// A user's page. `name` is only the title until the user loads.
pub fn user_page(id: String, name: String) -> Box<dyn Screen> {
    Box::new(UserPage {
        id,
        title: name,
        user: None,
        policy: UserPolicy::default(),
        name: String::new(),
        folders: None,
        ratings: vec![],
        tab: 0,
        focus: Focus { group: G_BODY, index: 0 },
        groups: vec![],
        enter: false,
        busy: false,
        error: None,
        notice: None,
        started: false,
        confirm: false,
        confirm_focus: 0,
        new_password: String::new(),
        tag_input: String::new(),
        dd: [DropdownState::default(); 4],
        cur_dd: None,
        on_field: false,
    })
}

struct UserPage {
    id: String,
    title: String,
    user: Option<UserDto>,
    policy: UserPolicy,
    name: String,
    folders: Option<Vec<VirtualFolderInfo>>,
    ratings: Vec<(String, i32)>,
    tab: usize,
    focus: Focus,
    groups: Vec<Group>,
    enter: bool,
    busy: bool,
    error: Option<String>,
    notice: Option<String>,
    started: bool,
    confirm: bool,
    confirm_focus: usize,
    new_password: String,
    tag_input: String,
    dd: [DropdownState; 4],
    cur_dd: Option<(usize, usize)>,
    on_field: bool,
}

enum Act {
    Save,
    Delete,
    SetPassword,
    ResetPassword,
    AddTag,
    RemoveTag(usize),
    AddSchedule,
    RemoveSchedule(usize),
}

impl UserPage {
    fn load(&mut self, cx: &Cx) {
        self.error = None;
        let (c, id) = (cx.api(), self.id.clone());
        cx.spawn("user", move || c.user(&id));
        let c = cx.api();
        cx.spawn("folders", move || c.virtual_folders());
        let c = cx.api();
        cx.spawn("ratings", move || c.parental_ratings());
    }

    fn own_id(cx: &Cx) -> String {
        cx.client.as_ref().and_then(|c| c.session.as_ref()).map(|s| s.user_id.clone()).unwrap_or_default()
    }

    fn rating_labels(&self) -> Vec<String> {
        let mut v = vec!["No limit".to_string()];
        v.extend(self.ratings.iter().map(|(n, _)| n.clone()));
        v
    }

    /// Index into `rating_labels` for the policy's current limit.
    fn rating_index(&self) -> usize {
        match self.policy.max_parental_rating {
            None => 0,
            Some(v) => self.ratings.iter().position(|(_, x)| *x == v).map_or(0, |i| i + 1),
        }
    }

    fn picked(&mut self, which: usize, i: usize) {
        if which == DD_RATING {
            self.policy.max_parental_rating = if i == 0 { None } else { self.ratings.get(i - 1).map(|(_, v)| *v) };
        }
    }

    fn save(&mut self, cx: &Cx) {
        if self.busy {
            return;
        }
        let name = self.name.trim().to_string();
        if name.is_empty() {
            self.error = Some("The name can't be empty.".into());
            return;
        }
        let Some(user) = self.user.clone() else { return };
        self.busy = true;
        self.error = None;
        self.notice = None;
        let mut policy = self.policy.clone();
        for s in policy.access_schedules.iter_mut().flatten() {
            s.user_id = self.id.clone();
        }
        let renamed = user.name.as_deref() != Some(name.as_str());
        let mut dto = user;
        dto.name = Some(name);
        let (c, id) = (cx.api(), self.id.clone());
        cx.spawn("save", move || -> Result<(UserDto, UserPolicy)> {
            if renamed {
                c.update_user(&id, &dto)?;
            }
            c.set_user_policy(&id, &policy)?;
            Ok((dto, policy))
        });
    }

    fn run(&mut self, act: Act, cx: &Cx) {
        match act {
            Act::Save => self.save(cx),
            Act::Delete => {
                self.confirm = true;
                self.confirm_focus = 0;
            }
            Act::SetPassword => {
                if self.new_password.is_empty() {
                    self.error = Some("Type the new password first.".into());
                    return;
                }
                self.password_call(cx, Some(self.new_password.clone()));
            }
            Act::ResetPassword => self.password_call(cx, None),
            Act::AddTag => {
                let tag = self.tag_input.trim().to_string();
                if tag.is_empty() {
                    return;
                }
                let tags = self.policy.blocked_tags.get_or_insert_with(Vec::new);
                if !tags.contains(&tag) {
                    tags.push(tag);
                }
                self.tag_input.clear();
            }
            Act::RemoveTag(i) => {
                if let Some(tags) = self.policy.blocked_tags.as_mut() {
                    if i < tags.len() {
                        tags.remove(i);
                    }
                }
            }
            Act::AddSchedule => {
                let day = DAYS[self.dd[DD_DAY].highlighted.min(DAYS.len() - 1)];
                let from = self.dd[DD_FROM].highlighted as f64;
                let to = self.dd[DD_TO].highlighted as f64 + 1.0;
                if to <= from {
                    self.error = Some("“To” must be after “From”.".into());
                    return;
                }
                self.error = None;
                let s = AccessSchedule {
                    user_id: self.id.clone(),
                    day_of_week: day.to_string(),
                    start_hour: from,
                    end_hour: to,
                    ..Default::default()
                };
                self.policy.access_schedules.get_or_insert_with(Vec::new).push(s);
            }
            Act::RemoveSchedule(i) => {
                if let Some(v) = self.policy.access_schedules.as_mut() {
                    if i < v.len() {
                        v.remove(i);
                    }
                }
            }
        }
    }

    fn password_call(&mut self, cx: &Cx, new: Option<String>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        self.notice = None;
        let (c, id) = (cx.api(), self.id.clone());
        cx.spawn("password", move || -> Result<bool> {
            match new {
                Some(pw) => c.set_user_password(&id, &pw).map(|_| true),
                None => c.reset_user_password(&id).map(|_| false),
            }
        });
    }

    fn confirm_key(&mut self, cx: &mut Cx, key: Key) {
        match key {
            Key::Back => self.confirm = false,
            Key::Left => self.confirm_focus = self.confirm_focus.saturating_sub(1),
            Key::Right => self.confirm_focus = (self.confirm_focus + 1).min(1),
            Key::Enter => {
                let go = self.confirm_focus == 1;
                self.confirm = false;
                if go {
                    self.delete(cx);
                }
            }
            _ => {}
        }
    }

    fn delete(&mut self, cx: &Cx) {
        self.busy = true;
        let (c, id) = (cx.api(), self.id.clone());
        cx.spawn("delete", move || c.delete_user(&id));
    }
}

impl Screen for UserPage {
    fn name(&self) -> &'static str {
        "user"
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        if self.confirm {
            self.confirm_key(cx, key);
            return true;
        }
        if let Some((w, len)) = self.cur_dd {
            let st = self.dd[w];
            if matches!(key, Key::Enter | Key::Up | Key::Down | Key::Back) && (st.open || key == Key::Enter) {
                let (ns, ev) = widgets::dropdown_key(st, key, len);
                self.dd[w] = ns;
                match ev {
                    Some(DropdownEvent::Pick(i)) => self.picked(w, i),
                    // A closed menu goes back to the saved choice.
                    Some(DropdownEvent::Close) if w == DD_RATING => self.dd[w].highlighted = self.rating_index(),
                    _ => {}
                }
                return true;
            }
        }
        match key {
            Key::Enter => {
                self.enter = true;
                true
            }
            Key::Left | Key::Right if self.focus.group == G_BODY && self.on_field => true,
            Key::Up | Key::Down | Key::Left | Key::Right => {
                self.focus = move_focus(&self.groups, self.focus, key);
                if self.focus.group == G_TABS && self.focus.index != self.tab {
                    self.tab = self.focus.index;
                    self.dd = [DropdownState::default(); 4];
                    self.error = None;
                    self.notice = None;
                }
                true
            }
            _ => false,
        }
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn std::any::Any + Send>) {
        match tag {
            "user" => {
                let Ok(r) = payload.downcast::<Result<UserDto>>() else { return };
                match *r {
                    Ok(u) => {
                        self.policy = u.policy.clone().unwrap_or_default();
                        self.name = u.name.clone().unwrap_or_default();
                        self.title = self.name.clone();
                        self.user = Some(u);
                    }
                    Err(e) => self.error = fail(cx, e),
                }
            }
            "folders" => {
                let Ok(r) = payload.downcast::<Result<Vec<VirtualFolderInfo>>>() else { return };
                match *r {
                    Ok(f) => self.folders = Some(f),
                    Err(e) => self.error = fail(cx, e),
                }
            }
            "ratings" => {
                let Ok(r) = payload.downcast::<Result<Vec<ParentalRating>>>() else { return };
                match *r {
                    Ok(list) => self.ratings = rating_options(&list, self.policy.max_parental_rating),
                    Err(e) => self.error = fail(cx, e),
                }
            }
            "save" => {
                self.busy = false;
                let Ok(r) = payload.downcast::<Result<(UserDto, UserPolicy)>>() else { return };
                match *r {
                    Ok((dto, policy)) => {
                        self.title = dto.name.clone().unwrap_or_default();
                        let mut dto = dto;
                        dto.policy = Some(policy);
                        self.user = Some(dto);
                        self.notice = Some("Saved.".into());
                    }
                    Err(e) => self.error = fail(cx, e),
                }
            }
            "password" => {
                self.busy = false;
                let Ok(r) = payload.downcast::<Result<bool>>() else { return };
                match *r {
                    Ok(has) => {
                        if let Some(u) = self.user.as_mut() {
                            u.has_password = has;
                        }
                        self.new_password.clear();
                        self.notice = Some(if has { "Password changed." } else { "Password removed." }.into());
                    }
                    Err(e) => self.error = fail(cx, e),
                }
            }
            "delete" => {
                self.busy = false;
                let Ok(r) = payload.downcast::<Result<()>>() else { return };
                match *r {
                    Ok(()) => cx.pop(),
                    Err(e) => self.error = fail(cx, e),
                }
            }
            _ => {}
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        if !self.started {
            self.started = true;
            self.load(cx);
        }
        let moving = ui.input(|i| i.pointer.is_moving());
        ui.add_space(t.margin);
        parts::title(ui, t, &self.title);
        ui.add_space(t.gap / 4.0);

        let labels: Vec<String> = TABS.iter().map(|s| s.to_string()).collect();
        let tabs_focus = (self.focus.group == G_TABS).then_some(self.focus.index);
        let tabs = widgets::tabs(ui, t, &labels, self.tab, tabs_focus);
        if let Some(i) = tabs.hovered.filter(|_| moving) {
            self.focus = Focus { group: G_TABS, index: i };
        }
        if let Some(i) = tabs.clicked {
            self.focus = Focus { group: G_TABS, index: i };
            if self.tab != i {
                self.tab = i;
                self.dd = [DropdownState::default(); 4];
                self.error = None;
                self.notice = None;
            }
        }
        ui.add_space(t.gap / 2.0);

        let body_focus = (self.focus.group == G_BODY && !self.confirm).then_some(self.focus.index);
        let mut ctl = Ctl::new(body_focus, std::mem::take(&mut self.enter) && !self.confirm, moving);
        let mut acts: Vec<Act> = vec![];
        let mut retry = false;

        if let Some(e) = self.error.clone() {
            parts::error_line(ui, t, &e);
            if self.user.is_none() {
                retry = ctl.button(ui, t, "Retry", false);
            }
        }
        if let Some(n) = &self.notice {
            parts::muted(ui, t, n);
        }
        if self.user.is_none() {
            if self.error.is_none() {
                skeleton_rows(ui, t, 4);
            }
        } else {
            egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| match self.tab {
                0 => self.tab_profile(ui, t, &mut ctl, &mut acts, cx),
                1 => self.tab_access(ui, t, &mut ctl, &mut acts),
                2 => self.tab_parental(ui, t, &mut ctl, &mut acts),
                _ => self.tab_password(ui, t, &mut ctl, &mut acts),
            });
        }

        self.cur_dd = ctl.dd;
        self.on_field = ctl.on_field;
        self.groups = vec![
            Group { len: TABS.len(), cols: TABS.len(), xs: tabs.xs },
            Group { len: ctl.idx, cols: 1, xs: ctl.xs.clone() },
        ];
        self.focus = move_focus(&self.groups, self.focus, Key::Enter);
        if let Some(i) = ctl.hover.or(ctl.clicked) {
            self.focus = Focus { group: G_BODY, index: i };
        }
        if ctl.advance {
            self.focus = move_focus(&self.groups, self.focus, Key::Down);
        }
        if retry {
            self.load(cx);
        }
        for a in acts {
            self.run(a, cx);
        }

        if self.confirm {
            let buttons = vec!["Cancel".to_string(), "Delete user".to_string()];
            let body = format!("Delete {} and everything tied to this account? This can't be undone.", self.name);
            let out = widgets::dialog(ui, t, "Delete user", &body, &buttons, Some(self.confirm_focus));
            if let Some(i) = out.hovered.filter(|_| moving) {
                self.confirm_focus = i;
            }
            if let Some(i) = out.clicked {
                self.confirm = false;
                if i == 1 {
                    self.delete(cx);
                }
            }
        }
    }
}

impl UserPage {
    fn save_button(&self, ui: &mut egui::Ui, t: &Theme, ctl: &mut Ctl, acts: &mut Vec<Act>) {
        ui.add_space(t.gap / 2.0);
        let label = if self.busy { "Saving…" } else { "Save" };
        if ctl.button(ui, t, label, true) {
            acts.push(Act::Save);
        }
    }

    fn tab_profile(&mut self, ui: &mut egui::Ui, t: &Theme, ctl: &mut Ctl, acts: &mut Vec<Act>, cx: &Cx) {
        ctl.field(ui, t, &mut self.name, "Name", false);
        ui.add_space(t.gap / 2.0);
        if ctl.toggle(ui, t, "Administrator", self.policy.is_administrator) {
            self.policy.is_administrator ^= true;
        }
        ui.add_space(t.gap / 3.0);
        if ctl.toggle(ui, t, "Disabled", self.policy.is_disabled) {
            self.policy.is_disabled ^= true;
        }
        ui.add_space(t.gap / 3.0);
        if ctl.toggle(ui, t, "Allow remote access", self.policy.enable_remote_access) {
            self.policy.enable_remote_access ^= true;
        }
        ui.add_space(t.gap / 3.0);
        if ctl.toggle(ui, t, "Allow media deletion", self.policy.enable_content_deletion) {
            self.policy.enable_content_deletion ^= true;
        }
        ui.add_space(t.gap / 3.0);
        let all = all_features(&self.policy);
        if ctl.toggle(ui, t, "Enable all features", all) {
            set_all_features(&mut self.policy, !all);
        }
        self.save_button(ui, t, ctl, acts);
        ui.add_space(t.gap);
        if self.id == Self::own_id(cx) {
            parts::muted(ui, t, "You can't delete the account you are signed in with.");
        } else if ctl.button(ui, t, "Delete user", false) {
            acts.push(Act::Delete);
        }
    }

    fn tab_access(&mut self, ui: &mut egui::Ui, t: &Theme, ctl: &mut Ctl, acts: &mut Vec<Act>) {
        if ctl.toggle(ui, t, "Enable all libraries", self.policy.enable_all_folders) {
            self.policy.enable_all_folders ^= true;
        }
        if !self.policy.enable_all_folders {
            ui.add_space(t.gap / 2.0);
            parts::muted(ui, t, "Libraries this user can see");
            match &self.folders {
                None => parts::muted(ui, t, "Loading libraries…"),
                Some(folders) => {
                    for f in folders {
                        let (Some(name), Some(id)) = (f.name.as_deref(), f.item_id.as_deref()) else { continue };
                        let on = self.policy.enabled_folders.as_ref().is_some_and(|v| v.iter().any(|x| x == id));
                        ui.add_space(t.gap / 3.0);
                        if ctl.toggle(ui, t, name, on) {
                            let v = self.policy.enabled_folders.get_or_insert_with(Vec::new);
                            if on {
                                v.retain(|x| x != id);
                            } else {
                                v.push(id.to_string());
                            }
                        }
                    }
                }
            }
        }
        ui.add_space(t.gap / 2.0);
        if ctl.toggle(ui, t, "Enable all devices", self.policy.enable_all_devices) {
            self.policy.enable_all_devices ^= true;
        }
        self.save_button(ui, t, ctl, acts);
    }

    fn tab_parental(&mut self, ui: &mut egui::Ui, t: &Theme, ctl: &mut Ctl, acts: &mut Vec<Act>) {
        parts::muted(ui, t, "Maximum parental rating");
        if !self.dd[DD_RATING].open {
            self.dd[DD_RATING].highlighted = self.rating_index();
        }
        let opts = self.rating_labels();
        if let Some(i) = ctl.dropdown(ui, t, &mut self.dd[DD_RATING], DD_RATING, "", &opts) {
            self.picked(DD_RATING, i);
        }

        ui.add_space(t.gap);
        parts::muted(ui, t, "Blocked tags");
        let tags = self.policy.blocked_tags.clone().unwrap_or_default();
        for (i, tag) in tags.iter().enumerate() {
            if ctl.button(ui, t, &format!("Remove  {tag}"), false) {
                acts.push(Act::RemoveTag(i));
            }
        }
        ctl.field(ui, t, &mut self.tag_input, "New blocked tag", false);
        if ctl.button(ui, t, "Add tag", false) {
            acts.push(Act::AddTag);
        }

        ui.add_space(t.gap);
        parts::muted(ui, t, "Access schedule");
        let scheds = self.policy.access_schedules.clone().unwrap_or_default();
        if scheds.is_empty() {
            parts::muted(ui, t, "No schedule: any time of day.");
        }
        for (i, s) in scheds.iter().enumerate() {
            if ctl.button(ui, t, &format!("Remove  {}", schedule_label(s)), false) {
                acts.push(Act::RemoveSchedule(i));
            }
        }
        let days: Vec<String> = DAYS.iter().map(|s| s.to_string()).collect();
        let from: Vec<String> = (0..24).map(|h| format!("{h:02}:00")).collect();
        let to: Vec<String> = (1..=24).map(|h| format!("{h:02}:00")).collect();
        ctl.dropdown(ui, t, &mut self.dd[DD_DAY], DD_DAY, "Day", &days);
        ctl.dropdown(ui, t, &mut self.dd[DD_FROM], DD_FROM, "From", &from);
        ctl.dropdown(ui, t, &mut self.dd[DD_TO], DD_TO, "To", &to);
        if ctl.button(ui, t, "Add schedule", false) {
            acts.push(Act::AddSchedule);
        }
        self.save_button(ui, t, ctl, acts);
    }

    fn tab_password(&mut self, ui: &mut egui::Ui, t: &Theme, ctl: &mut Ctl, acts: &mut Vec<Act>) {
        let has = self.user.as_ref().is_some_and(|u| u.has_password);
        parts::muted(ui, t, if has { "This user has a password." } else { "This user has no password." });
        ui.add_space(t.gap / 2.0);
        ctl.field(ui, t, &mut self.new_password, "New password", true);
        ui.add_space(t.gap / 2.0);
        let label = if self.busy { "Working…" } else { "Set password" };
        if ctl.button(ui, t, label, true) {
            acts.push(Act::SetPassword);
        }
        ui.add_space(t.gap / 2.0);
        if ctl.button(ui, t, "Reset password (remove it)", false) {
            acts.push(Act::ResetPassword);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(name: &str, value: Option<i32>) -> ParentalRating {
        ParentalRating { name: name.into(), value }
    }

    #[test]
    fn features_flip_together() {
        let mut p = UserPolicy::default();
        assert!(!all_features(&p));
        set_all_features(&mut p, true);
        assert!(all_features(&p));
        p.enable_lyric_management = false;
        assert!(!all_features(&p));
        set_all_features(&mut p, false);
        assert!(!p.enable_media_playback && !p.enable_live_tv_access);
    }

    #[test]
    fn rating_options_are_sorted_and_one_per_level() {
        let list = [r("Unrated", None), r("PG", Some(8)), r("All", Some(0)), r("E", Some(0)), r("12", Some(12))];
        let o = rating_options(&list, None);
        assert_eq!(o, vec![("All".into(), 0), ("PG".into(), 8), ("12".into(), 12)]);
        let o = rating_options(&list, Some(15));
        assert_eq!(o.last(), Some(&("Rating 15".to_string(), 15)));
    }

    #[test]
    fn schedule_labels() {
        assert_eq!(hour_label(9.5), "09:30");
        assert_eq!(hour_label(24.0), "24:00");
        let s = AccessSchedule { day_of_week: "Monday".into(), start_hour: 9.0, end_hour: 17.0, ..Default::default() };
        assert_eq!(schedule_label(&s), "Monday  09:00 – 17:00");
    }

    #[test]
    fn dates_are_shortened() {
        assert_eq!(short_date("2026-10-10T17:00:00.0000000Z"), "2026-10-10 17:00");
    }
}
