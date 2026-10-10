//! R-UI-2 / R-SET-1: sign in with a password, public-user tiles, and a Quick Connect entry point.
use super::parts;
use crate::api::{ApiError, Client, PublicInfo, PublicUser, Result, Session};
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
        focus: Focus { group: G_USERNAME, index: 0 },
        groups: vec![],
        started: false,
    })
}

impl SignIn {
    fn qc_shown(&self) -> bool {
        self.quick_connect_known && self.quick_connect
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
                let ci = if self.qc_shown() { 2 } else { 1 };
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
        });
        match act {
            Some(Act::Pick(i)) => {
                let name = self.users[i].name.clone();
                self.pick_user(name);
            }
            Some(Act::SignIn) => self.sign_in(cx),
            Some(Act::QuickConnect) => cx.push(super::quickconnect::screen(self.client.clone())),
            Some(Act::ChangeServer) => cx.reset(super::connect::screen(None)),
            None => {}
        }
        let buttons = if self.qc_shown() { 3 } else { 2 };
        self.groups = vec![
            Group { len: self.users.len(), cols: self.users.len().max(1), xs: user_xs },
            Group { len: 1, cols: 1, xs: username_x },
            Group { len: 1, cols: 1, xs: password_x },
            Group { len: buttons, cols: buttons, xs: button_xs },
        ];
    }
}
