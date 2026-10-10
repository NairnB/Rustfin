//! R-UI-1 Connect: servers found on the LAN + a typed address. The worked example of a screen:
//! focus groups rebuilt every frame → `nav::move_focus`; data loads through `cx.spawn` → `on_done`.
use super::parts;
use crate::api::{Client, PublicInfo, Result};
use crate::app::{Cx, Screen};
use crate::discovery::{discover, DiscoveredServer};
use crate::nav::{move_focus, Focus, Group};
use crate::platform::Key;
use crate::settings::ServerEntry;
use std::time::Duration;

const G_FOUND: usize = 0;
const G_FIELD: usize = 1;
const G_BUTTON: usize = 2;

pub struct Connect {
    address: String,
    found: Vec<DiscoveredServer>,
    searching: bool,
    busy: bool,
    error: Option<String>,
    focus: Focus,
    groups: Vec<Group>,
    started: bool,
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
    })
}

impl Connect {
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
            Key::Back => true, // first screen: nothing to go back to
            _ => false,
        }
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn std::any::Any + Send>) {
        match tag {
            "discover" => {
                self.searching = false;
                if let Ok(v) = payload.downcast::<Vec<DiscoveredServer>>() {
                    self.found = *v;
                    if !self.found.is_empty() && self.address.is_empty() {
                        self.focus = Focus { group: G_FOUND, index: 0 };
                    }
                }
            }
            "connect" => {
                self.busy = false;
                let Ok(r) = payload.downcast::<Result<(Client, PublicInfo)>>() else { return };
                match *r {
                    Ok((client, info)) => {
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
            cx.spawn("discover", || discover(Duration::from_secs(2)));
        }
        let t = cx.theme;
        let mut found_xs = vec![];
        let mut field_x = vec![];
        let mut button_x = vec![];
        let mut clicked = None;
        ui.vertical_centered(|ui| {
            ui.add_space(t.margin * 2.0);
            parts::title(ui, t, "Connect to Jellyfin");
            ui.add_space(t.gap);
            if self.searching {
                parts::muted(ui, t, "Looking for servers on your network…");
            } else if self.found.is_empty() {
                parts::muted(ui, t, "No servers found on your network. Type the address below.");
            } else {
                parts::muted(ui, t, "Servers on your network");
            }
            ui.add_space(t.gap / 2.0);
            for (i, s) in self.found.iter().enumerate() {
                let focused = self.focus == Focus { group: G_FOUND, index: i };
                let r = parts::button(ui, t, &format!("{}  ·  {}", s.name, s.address), focused, false);
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
            Group { len: self.found.len(), cols: 1, xs: found_xs },
            Group { len: 1, cols: 1, xs: field_x },
            Group { len: 1, cols: 1, xs: button_x },
        ];
    }
}
