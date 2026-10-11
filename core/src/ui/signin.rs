//! R-UI-2 / R-SET-1: sign in with a password, public-user tiles, and a Quick Connect entry point.
use super::{parts, widgets};
use crate::api::{
    image_width_bucket, ApiError, Branding, Client, ForgotAction, ForgotPassword, PinRedeem, PublicInfo, PublicUser,
    Result, Session,
};
use crate::app::{Cx, Screen};
use crate::nav::{move_focus, Focus, Group};
use crate::platform::Key;
use crate::settings::ServerEntry;
use egui::{pos2, vec2, Align2, Rect, Sense};

const G_USERS: usize = 0;
const G_USERNAME: usize = 1;
const G_PASSWORD: usize = 2;
const G_REMEMBER: usize = 3;
const G_BUTTONS: usize = 4;

/// One tile in the Switch user picker (D10 / R9): a user who signed in here before, or a public user of the server.
#[derive(Debug, Clone, PartialEq)]
pub struct PickerEntry {
    pub user_id: String,
    pub name: String,
    /// The saved session, when this device has one for them (picking signs straight in).
    pub saved: Option<Session>,
}

/// The picker list: saved users first (newest first, as given), then the server's public users who are not saved.
/// "Other user…" is added by the screen.
pub fn picker_entries(saved: &[Session], public: &[PublicUser]) -> Vec<PickerEntry> {
    let mut out: Vec<PickerEntry> = saved
        .iter()
        .map(|s| PickerEntry { user_id: s.user_id.clone(), name: s.user_name.clone(), saved: Some(s.clone()) })
        .collect();
    for u in public {
        if !out.iter().any(|e| e.user_id == u.id) {
            out.push(PickerEntry { user_id: u.id.clone(), name: u.name.clone(), saved: None });
        }
    }
    out
}

pub struct SignIn {
    client: Client,
    info: Option<PublicInfo>,
    users: Vec<PublicUser>,
    /// D10: sessions saved on this device for this server, newest first.
    saved: Vec<Session>,
    username: String,
    password: String,
    error: Option<String>,
    busy: bool,
    quick_connect: bool,
    quick_connect_known: bool,
    disclaimer: Option<String>,
    focus: Focus,
    groups: Vec<Group>,
    started: bool,
    /// P-AUTH-5: keep the session on disk (default on); off = in memory only.
    remember: bool,
    /// P-AUTH-8: opened by Switch user — focus the public user tiles once they load.
    prefer_users: bool,
}

pub fn screen(client: Client, message: Option<String>) -> Box<dyn Screen> {
    Box::new(SignIn::new(client, message))
}

impl SignIn {
    fn new(client: Client, message: Option<String>) -> SignIn {
        SignIn {
            client,
            info: None,
            users: vec![],
            saved: vec![],
            username: String::new(),
            password: String::new(),
            error: message,
            busy: false,
            quick_connect: false,
            quick_connect_known: false,
            disclaimer: None,
            focus: Focus { group: G_USERNAME, index: 0 },
            groups: vec![],
            started: false,
            remember: true,
            prefer_users: false,
        }
    }
}

/// P-AUTH-8: Switch user — the same server's sign-in with the public user tiles focused first.
pub fn switch_user(client: Client) -> Box<dyn Screen> {
    let mut s = SignIn::new(client, None);
    s.prefer_users = true;
    Box::new(s)
}

impl SignIn {
    fn qc_shown(&self) -> bool {
        self.quick_connect_known && self.quick_connect
    }

    /// The saved server this client points at (its id keys the saved users).
    fn server_id(&self, cx: &Cx) -> Option<String> {
        cx.settings.servers.iter().find(|e| e.address == self.client.base).map(|e| e.id.clone())
    }

    fn forgot_index(&self) -> usize {
        if self.qc_shown() {
            2
        } else {
            1
        }
    }

    fn open_forgot(&mut self, cx: &mut Cx) {
        cx.push(forgot_screen(self.client.clone(), self.username.clone()));
    }

    fn entries(&self) -> Vec<PickerEntry> {
        picker_entries(&self.saved, &self.users)
    }

    fn pick_user(&mut self, name: String) {
        self.username = name;
        self.focus = Focus { group: G_PASSWORD, index: 0 };
    }

    /// D10: a tile was chosen; the last one is "Other user…" (the blank form).
    fn pick_entry(&mut self, cx: &mut Cx, i: usize) {
        let entries = self.entries();
        let Some(e) = entries.get(i) else {
            self.username.clear();
            self.password.clear();
            self.focus = Focus { group: G_USERNAME, index: 0 };
            return;
        };
        let Some(session) = e.saved.clone() else {
            self.pick_user(e.name.clone());
            return;
        };
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        let mut client = self.client.clone();
        let (id, name) = (session.user_id.clone(), session.user_name.clone());
        cx.spawn("resume", move || {
            client.session = Some(session.clone());
            let r = client.verify_session().map(|_| (client, session));
            (id, name, r)
        });
    }

    fn sign_in(&mut self, cx: &mut Cx) {
        if self.busy {
            return;
        }
        if self.username.trim().is_empty() {
            self.error = Some("Type your user name first.".into());
            self.focus = Focus { group: G_USERNAME, index: 0 };
            return;
        }
        self.busy = true;
        self.error = None;
        let user = self.username.clone();
        let password = self.password.clone();
        let mut client = self.client.clone();
        cx.spawn("signin", move || match client.sign_in(&user, &password) {
            Ok(session) => Ok((client, session)),
            Err(e) => Err(e),
        });
    }

    fn signed_in(&mut self, cx: &mut Cx, client: Client, session: Session) {
        let id = self.info.as_ref().map(|i| i.id.clone()).unwrap_or_default();
        let name = self.info.as_ref().map(|i| i.server_name.clone()).unwrap_or_default();
        let entry = ServerEntry { id, name, address: client.base.clone(), session: Some(session) };
        cx.settings.remember_signed_in(entry, self.remember);
        cx.save_settings();
        *cx.client = Some(client);
        cx.reset(super::home::screen(false));
    }
}

enum Act {
    Pick(usize),
    SignIn,
    QuickConnect,
    Forgot,
    ChangeServer,
    Remember,
}

impl Screen for SignIn {
    fn name(&self) -> &'static str {
        "signin"
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        match key {
            Key::Enter => {
                match self.focus.group {
                    G_USERS => self.pick_entry(cx, self.focus.index),
                    G_USERNAME => self.focus = Focus { group: G_PASSWORD, index: 0 },
                    G_PASSWORD => self.sign_in(cx),
                    G_REMEMBER => self.remember = !self.remember,
                    G_BUTTONS => match self.focus.index {
                        0 => self.sign_in(cx),
                        1 if self.qc_shown() => {
                            cx.push(super::quickconnect::screen(self.client.clone(), self.remember))
                        }
                        i if i == self.forgot_index() => self.open_forgot(cx),
                        _ => cx.reset(super::connect::screen(None)),
                    },
                    _ => {}
                }
                true
            }
            Key::Up | Key::Down | Key::Left | Key::Right => {
                if matches!(self.focus.group, G_USERNAME | G_PASSWORD) && matches!(key, Key::Left | Key::Right) {
                    return true; // cursor movement inside a field
                }
                self.focus = move_focus(&self.groups, self.focus, key);
                true
            }
            Key::Back => false,
            _ => false,
        }
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn std::any::Any + Send>) {
        match tag {
            "info" => {
                if let Ok(v) = payload.downcast::<Result<PublicInfo>>() {
                    if let Ok(info) = *v {
                        self.info = Some(info);
                    }
                }
            }
            "users" => {
                if let Ok(v) = payload.downcast::<Result<Vec<PublicUser>>>() {
                    if let Ok(users) = *v {
                        self.users = users;
                        if self.prefer_users && !self.entries().is_empty() {
                            self.prefer_users = false;
                            self.focus = Focus { group: G_USERS, index: 0 };
                        }
                    }
                }
            }
            "resume" => {
                self.busy = false;
                let Ok(r) = payload.downcast::<(String, String, Result<(Client, Session)>)>() else { return };
                let (id, name, result) = *r;
                match result {
                    Ok((client, session)) => self.signed_in(cx, client, session),
                    Err(ApiError::Unauthorized) => {
                        // The saved token is dead: forget it and ask for the password.
                        if let Some(sid) = self.server_id(cx) {
                            cx.settings.forget_saved_user(&sid, &id);
                            cx.save_settings();
                        }
                        self.saved.retain(|s| s.user_id != id);
                        self.error = Some(format!("{name}'s sign-in has expired. Enter the password."));
                        self.pick_user(name);
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            "qc" => {
                self.quick_connect_known = true;
                if let Ok(v) = payload.downcast::<Result<bool>>() {
                    if let Ok(b) = *v {
                        self.quick_connect = b;
                    }
                }
            }
            "branding" => {
                if let Ok(v) = payload.downcast::<Result<Branding>>() {
                    if let Ok(b) = *v {
                        self.disclaimer = b.login_disclaimer.map(|d| d.trim().to_string()).filter(|d| !d.is_empty());
                    }
                }
            }
            "signin" => {
                self.busy = false;
                let Ok(r) = payload.downcast::<Result<(Client, Session)>>() else { return };
                match *r {
                    Ok((client, session)) => self.signed_in(cx, client, session),
                    Err(ApiError::Unauthorized) => self.error = Some("Wrong user name or password.".into()),
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            _ => {}
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        if !self.started {
            self.started = true;
            if let Some(id) = self.server_id(cx) {
                self.saved = cx.settings.saved_for(&id);
            }
            if self.prefer_users && !self.entries().is_empty() {
                self.prefer_users = false;
                self.focus = Focus { group: G_USERS, index: 0 };
            }
            let c = self.client.clone();
            cx.spawn("info", move || c.public_info());
            let c = self.client.clone();
            cx.spawn("users", move || c.public_users());
            let c = self.client.clone();
            cx.spawn("qc", move || c.quick_connect_enabled());
            let c = self.client.clone();
            cx.spawn("branding", move || c.branding());
        }
        let t = cx.theme;
        let server = match &self.info {
            Some(i) if !i.server_name.trim().is_empty() => i.server_name.clone(),
            _ => self.client.base.clone(),
        };
        let entries = self.entries();
        let mut user_xs = vec![];
        let mut username_x = vec![];
        let mut password_x = vec![];
        let mut remember_x = vec![];
        let mut button_xs = vec![];
        let mut act: Option<Act> = None;
        ui.vertical_centered(|ui| {
            ui.add_space(t.margin * 2.0);
            parts::title(ui, t, "Sign in");
            ui.add_space(t.gap / 2.0);
            parts::muted(ui, t, &server);
            ui.add_space(t.gap);
            if !entries.is_empty() {
                parts::centered_row(ui, "users", |ui| {
                    for i in 0..=entries.len() {
                        let focused = self.focus == Focus { group: G_USERS, index: i };
                        let label = entries.get(i).map(|e| e.name.as_str()).unwrap_or("Other user…");
                        let uid = entries.get(i).map(|e| e.user_id.as_str());
                        let r = user_tile(ui, cx, &self.client, uid, label, focused);
                        user_xs.push(r.rect.center().x);
                        if r.hovered() {
                            self.focus = Focus { group: G_USERS, index: i };
                        }
                        if r.clicked() {
                            act = Some(Act::Pick(i));
                        }
                    }
                });
                ui.add_space(t.gap);
            }
            let focused = self.focus.group == G_USERNAME;
            let r = parts::text_field(ui, t, &mut self.username, "User name", focused, false);
            username_x.push(r.rect.center().x);
            if r.clicked() {
                self.focus = Focus { group: G_USERNAME, index: 0 };
            }
            ui.add_space(t.gap / 2.0);
            let focused = self.focus.group == G_PASSWORD;
            let r = parts::text_field(ui, t, &mut self.password, "Password", focused, true);
            password_x.push(r.rect.center().x);
            if r.clicked() {
                self.focus = Focus { group: G_PASSWORD, index: 0 };
            }
            ui.add_space(t.gap / 2.0);
            let focused = self.focus.group == G_REMEMBER;
            let tg = widgets::toggle(ui, t, "Remember me", self.remember, focused);
            remember_x.push(tg.x);
            if tg.hovered {
                self.focus = Focus { group: G_REMEMBER, index: 0 };
            }
            if tg.clicked {
                act = Some(Act::Remember);
            }
            ui.add_space(t.gap);
            parts::centered_row(ui, "buttons", |ui| {
                let focused = self.focus == Focus { group: G_BUTTONS, index: 0 };
                let label = if self.busy { "Signing in…" } else { "Sign in" };
                let r = parts::button(ui, t, label, focused, true);
                button_xs.push(r.rect.center().x);
                if r.hovered() {
                    self.focus = Focus { group: G_BUTTONS, index: 0 };
                }
                if r.clicked() {
                    act = Some(Act::SignIn);
                }
                if self.qc_shown() {
                    let focused = self.focus == Focus { group: G_BUTTONS, index: 1 };
                    let r = parts::button(ui, t, "Use Quick Connect", focused, false);
                    button_xs.push(r.rect.center().x);
                    if r.hovered() {
                        self.focus = Focus { group: G_BUTTONS, index: 1 };
                    }
                    if r.clicked() {
                        act = Some(Act::QuickConnect);
                    }
                }
                let fi = self.forgot_index();
                let focused = self.focus == Focus { group: G_BUTTONS, index: fi };
                let r = parts::button(ui, t, "Forgot password?", focused, false);
                button_xs.push(r.rect.center().x);
                if r.hovered() {
                    self.focus = Focus { group: G_BUTTONS, index: fi };
                }
                if r.clicked() {
                    act = Some(Act::Forgot);
                }
                let ci = fi + 1;
                let focused = self.focus == Focus { group: G_BUTTONS, index: ci };
                let r = parts::button(ui, t, "Change server", focused, false);
                button_xs.push(r.rect.center().x);
                if r.hovered() {
                    self.focus = Focus { group: G_BUTTONS, index: ci };
                }
                if r.clicked() {
                    act = Some(Act::ChangeServer);
                }
            });
            if let Some(e) = &self.error {
                ui.add_space(t.gap / 2.0);
                parts::error_line(ui, t, e);
            }
            if let Some(d) = &self.disclaimer {
                ui.add_space(t.gap);
                ui.scope(|ui| {
                    ui.set_max_width(t.field_w);
                    parts::muted(ui, t, d);
                });
            }
        });
        match act {
            Some(Act::Pick(i)) => self.pick_entry(cx, i),
            Some(Act::SignIn) => self.sign_in(cx),
            Some(Act::QuickConnect) => cx.push(super::quickconnect::screen(self.client.clone(), self.remember)),
            Some(Act::Remember) => self.remember = !self.remember,
            Some(Act::Forgot) => self.open_forgot(cx),
            Some(Act::ChangeServer) => cx.reset(super::connect::screen(None)),
            None => {}
        }
        let buttons = if self.qc_shown() { 4 } else { 3 };
        self.groups = vec![
            Group { len: user_xs.len(), cols: user_xs.len().max(1), xs: user_xs },
            Group { len: 1, cols: 1, xs: username_x },
            Group { len: 1, cols: 1, xs: password_x },
            Group { len: 1, cols: 1, xs: remember_x },
            Group { len: buttons, cols: buttons, xs: button_xs },
        ];
    }
}

/// A picker tile: the user's picture (or their initial) over their name; `None` id = "Other user…".
fn user_tile(
    ui: &mut egui::Ui,
    cx: &mut Cx,
    client: &Client,
    user_id: Option<&str>,
    label: &str,
    focused: bool,
) -> egui::Response {
    let t = cx.theme;
    let d = t.avatar_size * 0.75;
    let g = ui.fonts(|f| f.layout_no_wrap(label.to_string(), t.font(t.body_size), t.text));
    let size = vec2(d.max(g.size().x) + t.gap, d + t.body_size + t.gap);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let circle = Rect::from_center_size(pos2(rect.center().x, rect.top() + t.gap / 2.0 + d / 2.0), vec2(d, d));
    let painter = ui.painter();
    painter.circle_filled(circle.center(), d / 2.0, t.surface_hi);
    let mut drawn = false;
    if let Some(id) = user_id {
        let url = client.user_image_url(id, image_width_bucket(d, ui.ctx().pixels_per_point()));
        if let Some(tex) = cx.images.get(&url) {
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            ui.painter().image(tex, circle, uv, egui::Color32::WHITE);
            drawn = true;
        }
    }
    if !drawn {
        let initial = if user_id.is_some() { label.chars().next().map(|c| c.to_uppercase().to_string()) } else { None };
        ui.painter().text(
            circle.center(),
            Align2::CENTER_CENTER,
            initial.unwrap_or_else(|| "+".into()),
            t.font(t.title_size),
            t.text,
        );
    }
    ui.painter().text(
        pos2(rect.center().x, circle.bottom() + t.gap / 2.0),
        Align2::CENTER_TOP,
        label,
        t.font(t.body_size),
        t.text,
    );
    if focused {
        parts::focus_ring(ui, t, rect);
    }
    resp
}

const F_USER: usize = 0;
const F_PIN: usize = 1;
const F_BUTTONS: usize = 2;

/// G13 / P-AUTH-6: forgot password. Ask the server for a reset, show its answer, take the PIN from the server's PIN file.
struct Forgot {
    client: Client,
    username: String,
    pin: String,
    answer: Option<ForgotPassword>,
    done: Option<String>,
    error: Option<String>,
    busy: bool,
    focus: Focus,
    groups: Vec<Group>,
}

fn forgot_screen(client: Client, username: String) -> Box<dyn Screen> {
    Box::new(Forgot {
        client,
        username,
        pin: String::new(),
        answer: None,
        done: None,
        error: None,
        busy: false,
        focus: Focus { group: F_USER, index: 0 },
        groups: vec![],
    })
}

impl Forgot {
    fn pin_shown(&self) -> bool {
        matches!(&self.answer, Some(a) if a.action == ForgotAction::Pin) && self.done.is_none()
    }

    fn ask(&mut self, cx: &mut Cx) {
        if self.busy || self.username.trim().is_empty() {
            return;
        }
        self.busy = true;
        self.error = None;
        let c = self.client.clone();
        let user = self.username.trim().to_string();
        cx.spawn("forgot", move || c.forgot_password(&user));
    }

    fn redeem(&mut self, cx: &mut Cx) {
        if self.busy || self.pin.trim().is_empty() {
            return;
        }
        self.busy = true;
        self.error = None;
        let c = self.client.clone();
        let pin = self.pin.trim().to_string();
        cx.spawn("pin", move || c.forgot_password_pin(&pin));
    }

    fn activate(&mut self, cx: &mut Cx) {
        match self.focus.group {
            F_USER => self.ask(cx),
            F_PIN => self.redeem(cx),
            _ if self.focus.index == 0 => self.ask(cx),
            _ => self.redeem(cx),
        }
    }
}

impl Screen for Forgot {
    fn name(&self) -> &'static str {
        "forgot"
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        match key {
            Key::Enter => {
                self.activate(cx);
                true
            }
            Key::Up | Key::Down | Key::Left | Key::Right => {
                if matches!(self.focus.group, F_USER | F_PIN) && matches!(key, Key::Left | Key::Right) {
                    return true;
                }
                self.focus = move_focus(&self.groups, self.focus, key);
                true
            }
            _ => false,
        }
    }

    fn on_done(&mut self, _cx: &mut Cx, tag: &'static str, payload: Box<dyn std::any::Any + Send>) {
        self.busy = false;
        match tag {
            "forgot" => {
                let Ok(r) = payload.downcast::<Result<ForgotPassword>>() else { return };
                match *r {
                    Ok(a) => {
                        let pin = a.action == ForgotAction::Pin;
                        self.answer = Some(a);
                        if pin {
                            self.focus = Focus { group: F_PIN, index: 0 };
                        }
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            "pin" => {
                let Ok(r) = payload.downcast::<Result<PinRedeem>>() else { return };
                match *r {
                    Ok(p) if p.success => {
                        self.done = Some(if p.users_reset.is_empty() {
                            "Password reset. Sign in without a password, then set a new one in Settings.".into()
                        } else {
                            format!(
                                "Password cleared for {}. Sign in without a password, then set a new one in Settings.",
                                p.users_reset.join(", ")
                            )
                        });
                    }
                    Ok(_) => self.error = Some("That PIN was not accepted.".into()),
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            _ => {}
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        let mut user_x = vec![];
        let mut pin_x = vec![];
        let mut button_xs = vec![];
        let mut act = false;
        ui.vertical_centered(|ui| {
            ui.add_space(t.margin * 2.0);
            parts::title(ui, t, "Forgot password");
            ui.add_space(t.gap);
            let focused = self.focus.group == F_USER;
            let r = parts::text_field(ui, t, &mut self.username, "User name", focused, false);
            user_x.push(r.rect.center().x);
            if r.clicked() {
                self.focus = Focus { group: F_USER, index: 0 };
            }
            if let Some(a) = &self.answer {
                ui.add_space(t.gap / 2.0);
                ui.scope(|ui| {
                    ui.set_max_width(t.field_w);
                    parts::muted(ui, t, &a.message());
                });
            }
            if self.pin_shown() {
                ui.add_space(t.gap / 2.0);
                let focused = self.focus.group == F_PIN;
                let r = parts::text_field(ui, t, &mut self.pin, "PIN", focused, false);
                pin_x.push(r.rect.center().x);
                if r.clicked() {
                    self.focus = Focus { group: F_PIN, index: 0 };
                }
            }
            ui.add_space(t.gap);
            parts::centered_row(ui, "forgot-buttons", |ui| {
                let label = if self.busy { "Working…" } else { "Ask the server" };
                let focused = self.focus == Focus { group: F_BUTTONS, index: 0 };
                let r = parts::button(ui, t, label, focused, self.answer.is_none());
                button_xs.push(r.rect.center().x);
                if r.hovered() {
                    self.focus = Focus { group: F_BUTTONS, index: 0 };
                }
                if r.clicked() {
                    self.focus = Focus { group: F_BUTTONS, index: 0 };
                    act = true;
                }
                if self.pin_shown() {
                    let focused = self.focus == Focus { group: F_BUTTONS, index: 1 };
                    let r = parts::button(ui, t, "Use PIN", focused, true);
                    button_xs.push(r.rect.center().x);
                    if r.hovered() {
                        self.focus = Focus { group: F_BUTTONS, index: 1 };
                    }
                    if r.clicked() {
                        self.focus = Focus { group: F_BUTTONS, index: 1 };
                        act = true;
                    }
                }
            });
            if let Some(e) = &self.error {
                ui.add_space(t.gap / 2.0);
                parts::error_line(ui, t, e);
            }
            if let Some(d) = &self.done {
                ui.add_space(t.gap / 2.0);
                parts::muted(ui, t, d);
            }
        });
        if act {
            self.activate(cx);
        }
        let n = button_xs.len();
        let pin_len = pin_x.len();
        self.groups = vec![
            Group { len: 1, cols: 1, xs: user_x },
            Group { len: pin_len, cols: 1, xs: pin_x },
            Group { len: n, cols: n.max(1), xs: button_xs },
        ];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: &str, name: &str) -> Session {
        Session { user_id: id.into(), user_name: name.into(), token: "t".into() }
    }

    fn public(id: &str, name: &str) -> PublicUser {
        PublicUser { id: id.into(), name: name.into(), has_password: true }
    }

    #[test]
    fn saved_users_come_first_then_public_ones() {
        let list = picker_entries(&[session("2", "tester"), session("1", "viewer")], &[public("3", "guest")]);
        let names: Vec<_> = list.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["tester", "viewer", "guest"]);
        assert!(list[0].saved.is_some() && list[1].saved.is_some() && list[2].saved.is_none());
    }

    #[test]
    fn a_user_who_is_both_saved_and_public_shows_once() {
        let list = picker_entries(&[session("1", "viewer")], &[public("1", "viewer"), public("2", "tester")]);
        assert_eq!(list.len(), 2);
        assert!(list[0].saved.is_some());
        assert_eq!(list[1].name, "tester");
    }

    #[test]
    fn nobody_known_means_an_empty_picker() {
        assert!(picker_entries(&[], &[]).is_empty());
    }
}
