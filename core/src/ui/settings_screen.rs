//! R-UI-9 / R-SET-4 Settings: server + user info, Sign out, Change server, app version.
use super::parts;
use crate::api::Client;
use crate::app::{Cx, Screen};
use crate::nav::{move_focus, Focus, Group};
use crate::platform::Key;

const G_SIGNOUT: usize = 0;
const G_CHANGE: usize = 1;

pub struct SettingsScreen {
    busy: bool,
    error: Option<String>,
    focus: Focus,
    groups: Vec<Group>,
}

pub fn screen() -> Box<dyn Screen> {
    Box::new(SettingsScreen { busy: false, error: None, focus: Focus { group: G_SIGNOUT, index: 0 }, groups: vec![] })
}

impl SettingsScreen {
    fn sign_out(&mut self, cx: &mut Cx) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        let mut client = cx.api();
        cx.spawn("signout", move || {
            let _ = client.sign_out();
            client
        });
    }
}

enum Act {
    SignOut,
    ChangeServer,
}

impl Screen for SettingsScreen {
    fn name(&self) -> &'static str {
        "settings"
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        match key {
            Key::Enter => {
                match self.focus.group {
                    G_SIGNOUT => self.sign_out(cx),
                    _ => cx.reset(super::connect::screen(None)),
                }
                true
            }
            Key::Up | Key::Down | Key::Left | Key::Right => {
                self.focus = move_focus(&self.groups, self.focus, key);
                true
            }
            Key::Back => false,
            _ => false,
        }
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn std::any::Any + Send>) {
        if tag == "signout" {
            self.busy = false;
            let Ok(c) = payload.downcast::<Client>() else { return };
            let client = *c;
            if let Some(id) = cx.settings.last_server.clone() {
                cx.settings.forget_session(&id);
            }
            cx.save_settings();
            cx.reset(super::signin::screen(client, None));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        let (server, user) = {
            let entry = cx.settings.last();
            let server = entry.map(|e| format!("{}  ·  {}", e.name, e.address)).unwrap_or_default();
            let user = entry
                .and_then(|e| e.session.as_ref())
                .map(|s| s.user_name.clone())
                .unwrap_or_else(|| "Signed out".to_string());
            (server, user)
        };
        let mut signout_x = vec![];
        let mut change_x = vec![];
        let mut act: Option<Act> = None;
        ui.vertical_centered(|ui| {
            ui.add_space(t.margin * 2.0);
            parts::title(ui, t, "Settings");
            ui.add_space(t.gap);
            info_row(ui, t, "Server", &server);
            info_row(ui, t, "Signed in as", &user);
            info_row(ui, t, "Rustfin version", env!("CARGO_PKG_VERSION"));
            ui.add_space(t.gap);
            parts::centered_row(ui, "buttons", |ui| {
                let focused = self.focus.group == G_SIGNOUT;
                let label = if self.busy { "Signing out…" } else { "Sign out" };
                let r = parts::button(ui, t, label, focused, true);
                signout_x.push(r.rect.center().x);
                if r.hovered() {
                    self.focus = Focus { group: G_SIGNOUT, index: 0 };
                }
                if r.clicked() {
                    act = Some(Act::SignOut);
                }
                let focused = self.focus.group == G_CHANGE;
                let r = parts::button(ui, t, "Change server", focused, false);
                change_x.push(r.rect.center().x);
                if r.hovered() {
                    self.focus = Focus { group: G_CHANGE, index: 0 };
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
            Some(Act::SignOut) => self.sign_out(cx),
            Some(Act::ChangeServer) => cx.reset(super::connect::screen(None)),
            None => {}
        }
        self.groups = vec![Group { len: 1, cols: 1, xs: signout_x }, Group { len: 1, cols: 1, xs: change_x }];
    }
}

fn info_row(ui: &mut egui::Ui, t: &crate::theme::Theme, label: &str, value: &str) {
    parts::muted(ui, t, label);
    parts::heading(ui, t, value);
}
