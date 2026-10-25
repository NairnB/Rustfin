//! R-UI-1 Connect: servers found on the LAN + a typed address. The worked example of a screen:
//! focus groups rebuilt every frame → `nav::move_focus`; data loads through `cx.spawn` → `on_done`.
use super::parts;
use crate::api::{Client, PublicInfo, Result};
use crate::app::{Cx, Screen};
use crate::discovery::{search_lan, DiscoveredServer, Report};
use crate::nav::{move_focus, Focus, Group};
use crate::platform::Key;
use crate::settings::ServerEntry;

const G_SAVED: usize = 0;
const G_FOUND: usize = 1;
const G_FIELD: usize = 2;
const G_BUTTON: usize = 3;

pub struct Connect {
    address: String,
    found: Vec<DiscoveredServer>,
    searching: bool,
    busy: bool,
    error: Option<String>,
    focus: Focus,
    groups: Vec<Group>,
    started: bool,
    /// C3: the saved server (Id) that didn't answer at its saved address. Discovery focuses it in the list; nothing is
    /// sent to a new address until the user presses it, and then its public Id is checked before the saved session
    /// (token) is used there (a discovery reply's Id is public, so anyone could answer with it).
    follow: Option<String>,
}

pub fn screen(default_address: Option<String>) -> Box<dyn Screen> {
    Box::new(Connect {
        address: default_address.unwrap_or_default(),
        found: vec![],
        searching: true,
        busy: false,
        error: None,
        focus: Focus { group: G_FIELD, index: 0 },
        groups: vec![],
        started: false,
        follow: None,
    })
}

/// C3: the last server didn't answer: look for it on the network and focus it when it is found (one press
/// continues; see `Connect::follow`).
pub fn refind(server_id: String) -> Box<dyn Screen> {
    Box::new(Connect {
        address: String::new(),
        found: vec![],
        searching: true,
        busy: false,
        error: None,
        focus: Focus { group: G_FIELD, index: 0 },
        groups: vec![],
        started: false,
        follow: Some(server_id),
    })
}

/// C3: same, with servers a search already found (no second search).
pub fn refind_with(server_id: String, found: Vec<DiscoveredServer>) -> Box<dyn Screen> {
    let mut c = Connect {
        address: String::new(),
        found,
        searching: false,
        busy: false,
        error: None,
        focus: Focus { group: G_FIELD, index: 0 },
        groups: vec![],
        started: true,
        follow: Some(server_id),
    };
    c.focus_followed();
    Box::new(c)
}

impl Connect {
    /// C3: focus the followed server in the found list, if it is there.
    fn focus_followed(&mut self) {
        let Some(id) = &self.follow else { return };
        if let Some(i) = self.found.iter().position(|s| &s.id == id) {
            self.focus = Focus { group: G_FOUND, index: i };
        }
    }

    fn connect(&mut self, cx: &mut Cx, address: String) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        let device = cx.device();
        cx.spawn("connect", move || Client::connect(&address, device));
    }
}

impl Screen for Connect {
    fn name(&self) -> &'static str {
        "connect"
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        match key {
            Key::Enter => {
                match self.focus.group {
                    G_SAVED => {
                        if let Some(s) = cx.settings.servers.get(self.focus.index) {
                            let a = s.address.clone();
                            self.connect(cx, a);
                        }
                    }
                    G_FOUND => {
                        if let Some(s) = self.found.get(self.focus.index) {
                            let a = s.address.clone();
                            self.connect(cx, a);
                        }
                    }
                    _ => {
                        let a = self.address.clone();
                        self.connect(cx, a);
                    }
                }
                true
            }
            Key::Up | Key::Down | Key::Left | Key::Right => {
                // Left/Right stay inside the text field (cursor movement).
                if self.focus.group == G_FIELD && matches!(key, Key::Left | Key::Right) {
                    return true;
                }
                self.focus = move_focus(&self.groups, self.focus, key);
                true
            }
            // G13 / P-AUTH-8: Info forgets the focused saved server.
            Key::Info if self.focus.group == G_SAVED => {
                if self.focus.index < cx.settings.servers.len() {
                    let gone = cx.settings.servers.remove(self.focus.index);
                    if cx.settings.last_server.as_deref() == Some(gone.id.as_str()) {
                        cx.settings.last_server = None;
                    }
                    cx.save_settings();
                }
                true
            }
            Key::Back => true, // first screen: nothing to go back to
            _ => false,
        }
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn std::any::Any + Send>) {
        match tag {
            "discover" => {
                self.searching = false;
                if let Ok(r) = payload.downcast::<Report>() {
                    self.found = r.servers();
                    if !self.found.is_empty() && self.address.is_empty() {
                        self.focus = Focus { group: G_FOUND, index: 0 };
                    }
                    self.focus_followed();
                }
            }
            "connect" => {
                self.busy = false;
                let Ok(r) = payload.downcast::<Result<(Client, PublicInfo)>>() else { return };
                match *r {
                    Ok((client, info)) => {
                        // C3: the user picked the server we were looking for and its public info has the same Id:
                        // carry on with the saved session at its new address (Home verifies it).
                        let saved = cx.settings.servers.iter().find(|e| e.id == info.id).cloned();
                        if let Some(mut entry) = saved.filter(|_| self.follow.as_deref() == Some(info.id.as_str())) {
                            if let Some(session) = entry.session.clone() {
                                entry.address = client.base.clone();
                                cx.settings.remember(entry);
                                cx.save_settings();
                                *cx.client = Some(Client::with_session(&client.base, cx.device(), session));
                                cx.reset(super::home::screen(true));
                                return;
                            }
                        }
                        cx.settings.remember(ServerEntry {
                            id: info.id.clone(),
                            name: info.server_name.clone(),
                            address: client.base.clone(),
                            session: None,
                        });
                        cx.save_settings();
                        *cx.client = Some(client.clone());
                        cx.push(super::signin::screen(client, None));
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            _ => {}
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        if !self.started {
            self.started = true;
            let last = cx.settings.last().map(|e| e.address.clone());
            cx.spawn("discover", move || search_lan(last.as_deref()));
        }
        let t = cx.theme;
        let mut saved_xs = vec![];
        let mut found_xs = vec![];
        let mut field_x = vec![];
        let mut button_x = vec![];
        let mut clicked = None;
        ui.vertical_centered(|ui| {
            ui.add_space(t.margin * 2.0);
            parts::title(ui, t, "Connect to Jellyfin");
            ui.add_space(t.gap);
            if !cx.settings.servers.is_empty() {
                parts::muted(ui, t, "Saved servers  ·  Info to forget one");
                ui.add_space(t.gap / 2.0);
                for (i, s) in cx.settings.servers.iter().enumerate() {
                    let focused = self.focus == Focus { group: G_SAVED, index: i };
                    let r = parts::button(ui, t, &parts::server_label(&s.name, &s.address), focused, false);
                    saved_xs.push(r.rect.center().x);
                    if r.hovered() {
                        self.focus = Focus { group: G_SAVED, index: i };
                    }
                    if r.clicked() {
                        clicked = Some(s.address.clone());
                    }
                    ui.add_space(t.gap / 3.0);
                }
                ui.add_space(t.gap / 2.0);
            }
            if self.searching && self.follow.is_some() {
                let name = cx.settings.last().map(|e| e.name.clone()).filter(|n| !n.is_empty());
                let name = name.unwrap_or_else(|| "the server".into());
                parts::muted(ui, t, &format!("Can't reach {name}. Looking for it on your network…"));
            } else if self.searching {
                parts::muted(ui, t, "Looking for servers on your network…");
            } else if self.found.is_empty() {
                parts::muted(ui, t, "No servers found on your network. Type the address below.");
            } else {
                parts::muted(ui, t, "Servers on your network");
            }
            ui.add_space(t.gap / 2.0);
            for (i, s) in self.found.iter().enumerate() {
                let focused = self.focus == Focus { group: G_FOUND, index: i };
                let r = parts::button(ui, t, &parts::server_label(&s.name, &s.address), focused, false);
                found_xs.push(r.rect.center().x);
                if r.hovered() {
                    self.focus = Focus { group: G_FOUND, index: i };
                }
                if r.clicked() {
                    clicked = Some(s.address.clone());
                }
                ui.add_space(t.gap / 3.0);
            }
            ui.add_space(t.gap);
            let focused = self.focus.group == G_FIELD;
            let r = parts::text_field(ui, t, &mut self.address, "Server address, e.g. 10.0.0.20:8096", focused, false);
            field_x.push(r.rect.center().x);
            if r.clicked() {
                self.focus = Focus { group: G_FIELD, index: 0 };
            }
            ui.add_space(t.gap);
            let focused = self.focus.group == G_BUTTON;
            let label = if self.busy { "Connecting…" } else { "Connect" };
            let r = parts::button(ui, t, label, focused, true);
            button_x.push(r.rect.center().x);
            if r.hovered() {
                self.focus = Focus { group: G_BUTTON, index: 0 };
            }
            if r.clicked() {
                let a = self.address.clone();
                self.connect(cx, a);
            }
            if let Some(e) = &self.error {
                ui.add_space(t.gap / 2.0);
                parts::error_line(ui, t, e);
            }
        });
        if let Some(a) = clicked {
            self.connect(cx, a);
        }
        self.groups = vec![
            Group { len: saved_xs.len(), cols: 1, xs: saved_xs },
            Group { len: self.found.len(), cols: 1, xs: found_xs },
            Group { len: 1, cols: 1, xs: field_x },
            Group { len: 1, cols: 1, xs: button_x },
        ];
    }
}
