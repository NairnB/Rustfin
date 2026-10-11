//! R-UI-3 / R-API-11 Quick Connect: show the code, poll every 2 s, finish on approval.
use super::parts;
use crate::api::{ApiError, Client, PublicInfo, QuickConnectState, Result, Session};
use crate::app::{Cx, Screen};
use crate::nav::{Focus, Group};
use crate::platform::Key;
use crate::settings::ServerEntry;

const G_RETRY: usize = 0;

pub struct QuickConnect {
    client: Client,
    info: Option<PublicInfo>,
    secret: String,
    code: String,
    error: Option<String>,
    busy: bool,
    polling: bool,
    /// `cx.now` before which no new poll starts (2 s apart; a sleeping job would panic on the TV web route).
    next_poll: f64,
    started: bool,
    focus: Focus,
    groups: Vec<Group>,
    /// P-AUTH-5: follows the sign-in screen's Remember me toggle.
    remember: bool,
}

pub fn screen(client: Client, remember: bool) -> Box<dyn Screen> {
    Box::new(QuickConnect {
        client,
        info: None,
        secret: String::new(),
        code: String::new(),
        error: None,
        busy: false,
        polling: false,
        next_poll: 0.0,
        started: false,
        focus: Focus { group: G_RETRY, index: 0 },
        groups: vec![],
        remember,
    })
}

impl QuickConnect {
    fn start_poll(&mut self, cx: &mut Cx) {
        if self.polling || self.busy || self.secret.is_empty() || self.error.is_some() || cx.now < self.next_poll {
            return;
        }
        self.polling = true;
        self.next_poll = cx.now + 2.0;
        let client = self.client.clone();
        let secret = self.secret.clone();
        cx.spawn("poll", move || client.quick_connect_poll(&secret));
    }

    fn finish(&mut self, cx: &mut Cx) {
        if self.busy {
            return;
        }
        self.busy = true;
        let mut client = self.client.clone();
        let secret = self.secret.clone();
        cx.spawn("finish", move || match client.quick_connect_finish(&secret) {
            Ok(session) => Ok((client, session)),
            Err(e) => Err(e),
        });
    }

    fn retry(&mut self, cx: &mut Cx) {
        self.error = None;
        self.secret.clear();
        self.code.clear();
        self.polling = false;
        let client = self.client.clone();
        cx.spawn("init", move || client.quick_connect_initiate());
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

impl Screen for QuickConnect {
    fn name(&self) -> &'static str {
        "quickconnect"
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        match key {
            Key::Enter => {
                if self.error.is_some() {
                    self.retry(cx);
                }
                true
            }
            Key::Back => false,
            _ => false,
        }
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn std::any::Any + Send>) {
        match tag {
            "init" => {
                if let Ok(v) = payload.downcast::<Result<QuickConnectState>>() {
                    match *v {
                        Ok(st) => {
                            self.secret = st.secret;
                            self.code = st.code;
                        }
                        Err(e) => self.error = Some(e.to_string()),
                    }
                }
            }
            "info" => {
                if let Ok(v) = payload.downcast::<Result<PublicInfo>>() {
                    if let Ok(info) = *v {
                        self.info = Some(info);
                    }
                }
            }
            "poll" => {
                self.polling = false;
                let Ok(r) = payload.downcast::<Result<bool>>() else { return };
                match *r {
                    Ok(true) => self.finish(cx),
                    Ok(false) => {}
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            "finish" => {
                self.busy = false;
                let Ok(r) = payload.downcast::<Result<(Client, Session)>>() else { return };
                match *r {
                    Ok((client, session)) => self.signed_in(cx, client, session),
                    Err(ApiError::Unauthorized) => cx.signed_out(),
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
            cx.spawn("init", move || c.quick_connect_initiate());
            let c = self.client.clone();
            cx.spawn("info", move || c.public_info());
        }
        self.start_poll(cx);
        let t = cx.theme;
        let mut retry_x = vec![];
        let mut clicked = false;
        ui.vertical_centered(|ui| {
            ui.add_space(t.margin * 2.0);
            parts::title(ui, t, "Quick Connect");
            ui.add_space(t.gap);
            if self.error.is_some() {
                parts::muted(ui, t, "Quick Connect could not be set up.");
            } else if self.code.is_empty() {
                parts::muted(ui, t, "Requesting a code…");
            } else {
                let spaced = self.code.chars().map(|c| c.to_string()).collect::<Vec<_>>().join("  ");
                ui.label(egui::RichText::new(spaced).size(t.code_size).color(t.text).strong());
                ui.add_space(t.gap);
                parts::muted(ui, t, "Enter this code in Jellyfin on another device (Settings, then Quick Connect).");
            }
            if let Some(e) = &self.error {
                ui.add_space(t.gap);
                parts::error_line(ui, t, e);
                ui.add_space(t.gap / 2.0);
                let focused = self.focus.group == G_RETRY;
                let r = parts::button(ui, t, "Retry", focused, true);
                retry_x.push(r.rect.center().x);
                if r.hovered() {
                    self.focus = Focus { group: G_RETRY, index: 0 };
                }
                if r.clicked() {
                    clicked = true;
                }
            }
        });
        if clicked {
            self.retry(cx);
        }
        let len = usize::from(self.error.is_some());
        self.groups = vec![Group { len, cols: 1, xs: retry_x }];
    }
}
