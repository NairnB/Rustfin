//! R-UI-2 / R-SET-1: sign in with a password, public-user tiles, and a Quick Connect entry point.
use super::parts;
use crate::api::{
    ApiError, Branding, Client, ForgotAction, ForgotPassword, PinRedeem, PublicInfo, PublicUser, Result, Session,
};
use crate::app::{Cx, Screen};
use crate::nav::{move_focus, Focus, Group};
use crate::platform::Key;
use crate::settings::ServerEntry;

const G_USERS: usize = 0;
const G_USERNAME: usize = 1;
const G_PASSWORD: usize = 2;
const G_BUTTONS: usize = 3;

pub struct SignIn {
    client: Client,
    info: Option<PublicInfo>,
    users: Vec<PublicUser>,
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
}

pub fn screen(client: Client, message: Option<String>) -> Box<dyn Screen> {
    Box::new(SignIn {
        client,
        info: None,
        users: vec![],
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
    })
}

impl SignIn {
    fn qc_shown(&self) -> bool {
        self.quick_connect_known && self.quick_connect
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

    fn pick_user(&mut self, name: String) {
        self.username = name;
        self.focus = Focus { group: G_PASSWORD, index: 0 };
    }

    fn sign_in(&mut self, cx: &mut Cx) {
        if self.busy {
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
        cx.settings.remember(ServerEntry { id, name, address: client.base.clone(), session: Some(session) });
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
}

impl Screen for SignIn {
    fn name(&self) -> &'static str {
        "signin"
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        match key {
            Key::Enter => {
                match self.focus.group {
                    G_USERS => {
                        if let Some(u) = self.users.get(self.focus.index) {
                            let name = u.name.clone();
                            self.pick_user(name);
                        }
                    }
                    G_USERNAME => self.focus = Focus { group: G_PASSWORD, index: 0 },
                    G_PASSWORD => self.sign_in(cx),
                    G_BUTTONS => match self.focus.index {
                        0 => self.sign_in(cx),
                        1 if self.qc_shown() => cx.push(super::quickconnect::screen(self.client.clone())),
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
                    }
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
            Some(i) => format!("{}  ·  {}", i.server_name, self.client.base),
            None => self.client.base.clone(),
        };
        let mut user_xs = vec![];
        let mut username_x = vec![];
        let mut password_x = vec![];
        let mut button_xs = vec![];
        let mut act: Option<Act> = None;
        ui.vertical_centered(|ui| {
            ui.add_space(t.margin * 2.0);
            parts::title(ui, t, "Sign in");
            ui.add_space(t.gap / 2.0);
            parts::muted(ui, t, &server);
            ui.add_space(t.gap);
            if !self.users.is_empty() {
                parts::centered_row(ui, "users", |ui| {
                    for (i, u) in self.users.iter().enumerate() {
                        let focused = self.focus == Focus { group: G_USERS, index: i };
                        let r = parts::button(ui, t, &u.name, focused, false);
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
            Some(Act::Pick(i)) => {
                let name = self.users[i].name.clone();
                self.pick_user(name);
            }
            Some(Act::SignIn) => self.sign_in(cx),
            Some(Act::QuickConnect) => cx.push(super::quickconnect::screen(self.client.clone())),
            Some(Act::Forgot) => self.open_forgot(cx),
            Some(Act::ChangeServer) => cx.reset(super::connect::screen(None)),
            None => {}
        }
        let buttons = if self.qc_shown() { 4 } else { 3 };
        self.groups = vec![
            Group { len: self.users.len(), cols: self.users.len().max(1), xs: user_xs },
            Group { len: 1, cols: 1, xs: username_x },
            Group { len: 1, cols: 1, xs: password_x },
            Group { len: buttons, cols: buttons, xs: button_xs },
        ];
    }
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
