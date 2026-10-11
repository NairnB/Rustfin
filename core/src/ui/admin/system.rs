//! Admin devices, API keys, plugins, plugin catalogue, repositories and logs (P-ADM-21, 24, 25, 26).
//! The first five are lists of entries with a few buttons each, built on `Ctl` (focus = draw order, Up/Down).
//! Anything that signs a device out, revokes a key, removes a plugin or repository, or installs code asks first.
//! Keys are only ever drawn masked (`mask_key`) and never logged.
use super::sectioned_list;
use super::users::{fail, short_date, skeleton_rows, Ctl};
use crate::api::admin::{mask_key, newest_first, page_for_plugin, plugin_settings_url, ConfigPage};
use crate::api::gen_admin::{AuthenticationInfo, DeviceInfoDto, LogFile, PackageInfo, PluginInfo, RepositoryInfo};
use crate::api::Result;
use crate::app::{Cx, Screen};
use crate::nav::{move_focus, Focus, Group};
use crate::platform::Key;
use crate::theme::Theme;
use crate::ui::parts;
use crate::ui::widgets::{self, ListRow};
use std::any::Any;

/// How many lines of a log the viewer keeps (the newest ones).
const LOG_LINES: usize = 2000;

// ---------- pure helpers (unit-tested below) ----------

/// What a device is called: its custom name, else the name it gave itself.
fn device_name(d: &DeviceInfoDto) -> String {
    d.custom_name
        .clone()
        .filter(|n| !n.is_empty())
        .or_else(|| d.name.clone())
        .unwrap_or_else(|| "Unknown device".into())
}

fn device_meta(d: &DeviceInfoDto) -> String {
    let mut bits = vec![];
    if let Some(a) = d.app_name.as_deref().filter(|a| !a.is_empty()) {
        match d.app_version.as_deref().filter(|v| !v.is_empty()) {
            Some(v) => bits.push(format!("{a} {v}")),
            None => bits.push(a.to_string()),
        }
    }
    if let Some(u) = d.last_user_name.as_deref().filter(|u| !u.is_empty()) {
        bits.push(u.to_string());
    }
    if let Some(when) = d.date_last_activity.as_deref().filter(|w| !w.is_empty()) {
        bits.push(format!("last seen {}", short_date(when)));
    }
    bits.join(" · ")
}

/// Newest activity first.
fn sort_devices(v: &mut [DeviceInfoDto]) {
    v.sort_by(|a, b| b.date_last_activity.cmp(&a.date_last_activity));
}

/// 1234 → "1.2 KB".
fn size_label(bytes: i64) -> String {
    let b = bytes.max(0) as f64;
    if b >= 1_048_576.0 {
        format!("{:.1} MB", b / 1_048_576.0)
    } else if b >= 1024.0 {
        format!("{:.1} KB", b / 1024.0)
    } else {
        format!("{} B", bytes.max(0))
    }
}

/// Newest log file first.
fn sort_logs(v: &mut [LogFile]) {
    v.sort_by(|a, b| b.date_modified.cmp(&a.date_modified));
}

fn plugin_disabled(p: &PluginInfo) -> bool {
    p.status.eq_ignore_ascii_case("disabled")
}

/// A repository list with `new` added; `None` when the name or address is missing or the address isn't http(s).
fn with_repository(list: &[RepositoryInfo], name: &str, url: &str) -> Option<Vec<RepositoryInfo>> {
    let (name, url) = (name.trim(), url.trim());
    if name.is_empty() || !(url.starts_with("http://") || url.starts_with("https://")) {
        return None;
    }
    let mut out = list.to_vec();
    out.push(RepositoryInfo { name: Some(name.to_string()), url: Some(url.to_string()), enabled: true });
    Some(out)
}

fn without_repository(list: &[RepositoryInfo], i: usize) -> Vec<RepositoryInfo> {
    let mut out = list.to_vec();
    if i < out.len() {
        out.remove(i);
    }
    out
}

fn take<T: 'static>(payload: Box<dyn Any + Send>) -> Option<Result<T>> {
    payload.downcast::<Result<T>>().ok().map(|b| *b)
}

// ---------- shared page state ----------

struct Confirm {
    title: &'static str,
    body: String,
    yes: &'static str,
    act: usize,
}

enum KeyOut {
    Done,
    Confirmed(usize),
    Pass,
}

/// Focus, messages and the confirm dialog every list page shares.
#[derive(Default)]
struct Base {
    focus: Focus,
    groups: Vec<Group>,
    enter: bool,
    error: Option<String>,
    notice: Option<String>,
    started: bool,
    busy: bool,
    confirm: Option<Confirm>,
    confirm_focus: usize,
    on_field: bool,
}

impl Base {
    fn key(&mut self, key: Key) -> KeyOut {
        if self.confirm.is_some() {
            match key {
                Key::Back => self.confirm = None,
                Key::Left => self.confirm_focus = self.confirm_focus.saturating_sub(1),
                Key::Right => self.confirm_focus = (self.confirm_focus + 1).min(1),
                Key::Enter => {
                    let c = self.confirm.take();
                    if let (Some(c), 1) = (c, self.confirm_focus) {
                        return KeyOut::Confirmed(c.act);
                    }
                }
                _ => {}
            }
            return KeyOut::Done;
        }
        match key {
            Key::Enter => {
                self.enter = true;
                KeyOut::Done
            }
            Key::Left | Key::Right if self.on_field => KeyOut::Done,
            Key::Left | Key::Up => {
                self.focus = move_focus(&self.groups, self.focus, Key::Up);
                KeyOut::Done
            }
            Key::Right | Key::Down => {
                self.focus = move_focus(&self.groups, self.focus, Key::Down);
                KeyOut::Done
            }
            _ => KeyOut::Pass,
        }
    }

    fn ask(&mut self, title: &'static str, body: String, yes: &'static str, act: usize) {
        self.confirm = Some(Confirm { title, body, yes, act });
        self.confirm_focus = 0;
    }

    fn ctl(&mut self, moving: bool) -> Ctl {
        let enter = std::mem::take(&mut self.enter) && self.confirm.is_none();
        Ctl::new((self.confirm.is_none()).then_some(self.focus.index), enter, moving)
    }

    fn end_frame(&mut self, ctl: &Ctl) {
        self.on_field = ctl.on_field;
        self.groups = vec![Group { len: ctl.idx, cols: 1, xs: ctl.xs.clone() }];
        self.focus = move_focus(&self.groups, self.focus, Key::Enter);
        if let Some(i) = ctl.hover.or(ctl.clicked) {
            self.focus = Focus { group: 0, index: i };
        }
        if ctl.advance {
            self.focus = move_focus(&self.groups, self.focus, Key::Down);
        }
    }

    /// Draws the dialog if one is open; returns the act when "yes" was clicked.
    fn confirm_ui(&mut self, ui: &mut egui::Ui, t: &Theme, moving: bool) -> Option<usize> {
        let (title, body, yes, act) = {
            let c = self.confirm.as_ref()?;
            (c.title, c.body.clone(), c.yes, c.act)
        };
        let buttons = vec!["Cancel".to_string(), yes.to_string()];
        let out = widgets::dialog(ui, t, title, &body, &buttons, Some(self.confirm_focus));
        if let Some(i) = out.hovered.filter(|_| moving) {
            self.confirm_focus = i;
        }
        let i = out.clicked?;
        self.confirm = None;
        (i == 1).then_some(act)
    }

    /// Title, then the error (with Retry) and notice lines. Returns true when Retry was pressed.
    fn head(&mut self, ui: &mut egui::Ui, t: &Theme, ctl: &mut Ctl, title: &str, retry: bool) -> bool {
        ui.add_space(t.margin);
        parts::title(ui, t, title);
        ui.add_space(t.gap / 4.0);
        let mut again = false;
        if let Some(e) = self.error.clone() {
            parts::error_line(ui, t, &e);
            if retry {
                again = ctl.button(ui, t, "Retry", false);
            }
        }
        if let Some(n) = &self.notice {
            parts::muted(ui, t, n);
        }
        again
    }

    /// An action finished: unlock, show the notice or the error.
    fn finished(&mut self, cx: &mut Cx, r: Result<()>, notice: &str) -> bool {
        self.busy = false;
        match r {
            Ok(()) => {
                self.error = None;
                self.notice = Some(notice.to_string());
                true
            }
            Err(e) => {
                self.error = fail(cx, e);
                false
            }
        }
    }
}

fn body_line(ui: &mut egui::Ui, t: &Theme, text: &str) {
    ui.label(egui::RichText::new(text).size(t.body_size).color(t.text));
}

fn small_line(ui: &mut egui::Ui, t: &Theme, text: &str) {
    ui.label(egui::RichText::new(text).size(t.small_size).color(t.muted));
}

// ---------- devices ----------

const D_LIST: &str = "list";
const D_DONE: &str = "done";

pub struct Devices {
    b: Base,
    list: Option<Vec<DeviceInfoDto>>,
    me: String,
    editing: Option<usize>,
    edit: String,
}

pub fn devices() -> Box<dyn Screen> {
    Box::new(Devices { b: Base::default(), list: None, me: String::new(), editing: None, edit: String::new() })
}

enum DevAct {
    Rename(usize),
    Save(usize),
    Cancel,
    Delete(usize),
}

impl Devices {
    fn load(&mut self, cx: &Cx) {
        self.b.error = None;
        let c = cx.api();
        cx.spawn(D_LIST, move || c.devices());
    }

    fn id(&self, i: usize) -> Option<String> {
        self.list.as_ref().and_then(|l| l.get(i)).and_then(|d| d.id.clone())
    }

    fn run(&mut self, a: DevAct, cx: &Cx) {
        match a {
            DevAct::Rename(i) => {
                self.editing = Some(i);
                self.edit = self
                    .list
                    .as_ref()
                    .and_then(|l| l.get(i))
                    .and_then(|d| d.custom_name.clone().filter(|n| !n.is_empty()).or_else(|| d.name.clone()))
                    .unwrap_or_default();
            }
            DevAct::Cancel => self.editing = None,
            DevAct::Save(i) => {
                let Some(id) = self.id(i) else { return };
                if self.b.busy {
                    return;
                }
                self.b.busy = true;
                self.b.notice = None;
                let (c, name) = (cx.api(), self.edit.trim().to_string());
                self.editing = None;
                cx.spawn(D_DONE, move || c.rename_device(&id, &name));
            }
            DevAct::Delete(i) => {
                let name = self.list.as_ref().and_then(|l| l.get(i)).map(device_name).unwrap_or_default();
                self.b.ask(
                    "Delete device",
                    format!("Delete {name}? It is signed out and will be listed again if it connects."),
                    "Delete device",
                    i,
                );
            }
        }
    }

    fn delete(&mut self, i: usize, cx: &Cx) {
        let Some(id) = self.id(i) else { return };
        if id == self.me {
            return;
        }
        self.b.busy = true;
        self.b.notice = None;
        let c = cx.api();
        cx.spawn(D_DONE, move || c.delete_device(&id));
    }
}

impl Screen for Devices {
    fn name(&self) -> &'static str {
        "devices"
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        match self.b.key(key) {
            KeyOut::Confirmed(i) => {
                self.delete(i, cx);
                true
            }
            KeyOut::Done => true,
            KeyOut::Pass => false,
        }
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn Any + Send>) {
        match tag {
            D_LIST => {
                if let Some(r) = take::<Vec<DeviceInfoDto>>(payload) {
                    match r {
                        Ok(mut v) => {
                            sort_devices(&mut v);
                            self.list = Some(v);
                        }
                        Err(e) => self.b.error = fail(cx, e),
                    }
                }
            }
            D_DONE => {
                if let Some(r) = take::<()>(payload) {
                    if self.b.finished(cx, r, "Done.") {
                        self.load(cx);
                    }
                }
            }
            _ => {}
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        if !self.b.started {
            self.b.started = true;
            self.me = cx.api().device.id.clone();
            self.load(cx);
        }
        let moving = ui.input(|i| i.pointer.is_moving());
        let mut ctl = self.b.ctl(moving);
        let mut acts: Vec<DevAct> = vec![];
        let retry = self.b.head(ui, t, &mut ctl, "Devices", self.list.is_none());
        match &self.list {
            None => {
                if self.b.error.is_none() {
                    skeleton_rows(ui, t, 4);
                }
            }
            Some(list) if list.is_empty() => parts::muted(ui, t, "No devices yet."),
            Some(list) => {
                let (editing, me) = (self.editing, self.me.clone());
                egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
                    for (i, d) in list.iter().enumerate() {
                        ui.add_space(t.gap / 3.0);
                        let mine = d.id.as_deref() == Some(me.as_str());
                        let name = if mine { format!("{} (this device)", device_name(d)) } else { device_name(d) };
                        body_line(ui, t, &name);
                        small_line(ui, t, &device_meta(d));
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = t.gap / 2.0;
                            if editing == Some(i) {
                                ctl.field(ui, t, &mut self.edit, "Device name", false);
                                if ctl.button(ui, t, "Save", true) {
                                    acts.push(DevAct::Save(i));
                                }
                                if ctl.button(ui, t, "Cancel", false) {
                                    acts.push(DevAct::Cancel);
                                }
                            } else {
                                if ctl.button(ui, t, "Rename", false) {
                                    acts.push(DevAct::Rename(i));
                                }
                                if !mine && ctl.button(ui, t, "Delete", false) {
                                    acts.push(DevAct::Delete(i));
                                }
                            }
                        });
                    }
                });
            }
        }
        self.b.end_frame(&ctl);
        if retry {
            self.load(cx);
        }
        for a in acts {
            self.run(a, cx);
        }
        if let Some(i) = self.b.confirm_ui(ui, t, moving) {
            self.delete(i, cx);
        }
    }
}

// ---------- API keys ----------

pub struct Keys {
    b: Base,
    list: Option<Vec<AuthenticationInfo>>,
    app: String,
}

pub fn keys() -> Box<dyn Screen> {
    Box::new(Keys { b: Base::default(), list: None, app: String::new() })
}

enum KeyAct {
    Create,
    Revoke(usize),
}

impl Keys {
    fn load(&mut self, cx: &Cx) {
        self.b.error = None;
        let c = cx.api();
        cx.spawn(D_LIST, move || c.api_keys());
    }

    fn run(&mut self, a: KeyAct, cx: &Cx) {
        match a {
            KeyAct::Create => {
                let app = self.app.trim().to_string();
                if app.is_empty() {
                    self.b.error = Some("Type a name for the app first.".into());
                    return;
                }
                if self.b.busy {
                    return;
                }
                self.b.busy = true;
                self.b.error = None;
                self.b.notice = None;
                self.app.clear();
                let c = cx.api();
                cx.spawn(D_DONE, move || c.create_api_key(&app));
            }
            KeyAct::Revoke(i) => {
                let app =
                    self.list.as_ref().and_then(|l| l.get(i)).and_then(|k| k.app_name.clone()).unwrap_or_default();
                self.b.ask(
                    "Revoke key",
                    format!("Revoke the key for {app}? Anything using it stops working."),
                    "Revoke key",
                    i,
                );
            }
        }
    }

    fn revoke(&mut self, i: usize, cx: &Cx) {
        let Some(token) = self.list.as_ref().and_then(|l| l.get(i)).and_then(|k| k.access_token.clone()) else {
            return;
        };
        self.b.busy = true;
        self.b.notice = None;
        let c = cx.api();
        cx.spawn(D_DONE, move || c.revoke_api_key(&token));
    }
}

impl Screen for Keys {
    fn name(&self) -> &'static str {
        "keys"
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        match self.b.key(key) {
            KeyOut::Confirmed(i) => {
                self.revoke(i, cx);
                true
            }
            KeyOut::Done => true,
            KeyOut::Pass => false,
        }
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn Any + Send>) {
        match tag {
            D_LIST => {
                if let Some(r) = take::<Vec<AuthenticationInfo>>(payload) {
                    match r {
                        Ok(mut v) => {
                            v.sort_by(|a, b| b.date_created.cmp(&a.date_created));
                            self.list = Some(v);
                        }
                        Err(e) => self.b.error = fail(cx, e),
                    }
                }
            }
            D_DONE => {
                if let Some(r) = take::<()>(payload) {
                    if self.b.finished(cx, r, "Done.") {
                        self.load(cx);
                    }
                }
            }
            _ => {}
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        if !self.b.started {
            self.b.started = true;
            self.load(cx);
        }
        let moving = ui.input(|i| i.pointer.is_moving());
        let mut ctl = self.b.ctl(moving);
        let mut acts: Vec<KeyAct> = vec![];
        let retry = self.b.head(ui, t, &mut ctl, "API keys", self.list.is_none());
        small_line(ui, t, "Keys show only their last four characters.");
        ui.add_space(t.gap / 3.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = t.gap / 2.0;
            ctl.field(ui, t, &mut self.app, "App name", false);
            let label = if self.b.busy { "Working…" } else { "Create key" };
            if ctl.button(ui, t, label, true) {
                acts.push(KeyAct::Create);
            }
        });
        match &self.list {
            None => {
                if self.b.error.is_none() {
                    skeleton_rows(ui, t, 3);
                }
            }
            Some(list) if list.is_empty() => parts::muted(ui, t, "No API keys yet."),
            Some(list) => {
                egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
                    for (i, k) in list.iter().enumerate() {
                        ui.add_space(t.gap / 3.0);
                        body_line(ui, t, k.app_name.as_deref().filter(|a| !a.is_empty()).unwrap_or("Unnamed app"));
                        let masked = mask_key(k.access_token.as_deref().unwrap_or(""));
                        small_line(ui, t, &format!("{masked} · made {}", short_date(&k.date_created)));
                        if ctl.button(ui, t, "Revoke", false) {
                            acts.push(KeyAct::Revoke(i));
                        }
                    }
                });
            }
        }
        self.b.end_frame(&ctl);
        if retry {
            self.load(cx);
        }
        for a in acts {
            self.run(a, cx);
        }
        if let Some(i) = self.b.confirm_ui(ui, t, moving) {
            self.revoke(i, cx);
        }
    }
}

// ---------- installed plugins ----------

pub struct Plugins {
    b: Base,
    list: Option<Vec<PluginInfo>>,
    /// Settings pages the server's web app knows (P-ADM-24); empty until loaded, or when none is registered.
    pages: Vec<ConfigPage>,
    open: Option<String>,
}

pub fn plugins() -> Box<dyn Screen> {
    Box::new(Plugins { b: Base::default(), list: None, pages: vec![], open: None })
}

const P_PAGES: &str = "pages";

enum PlugAct {
    Settings(usize),
    Toggle(usize),
    Uninstall(usize),
}

impl Plugins {
    fn load(&mut self, cx: &Cx) {
        self.b.error = None;
        let c = cx.api();
        cx.spawn(D_LIST, move || c.plugins());
        let c = cx.api();
        cx.spawn(P_PAGES, move || c.configuration_pages());
    }

    fn run(&mut self, a: PlugAct, cx: &Cx) {
        let Some(p) = (match a {
            PlugAct::Settings(i) | PlugAct::Toggle(i) | PlugAct::Uninstall(i) => {
                self.list.as_ref().and_then(|l| l.get(i)).cloned()
            }
        }) else {
            return;
        };
        match a {
            PlugAct::Settings(_) => {
                self.open = page_for_plugin(&self.pages, &p.id).map(|pg| plugin_settings_url(&cx.api().base, &pg.name));
            }
            PlugAct::Toggle(_) => {
                if self.b.busy {
                    return;
                }
                self.b.busy = true;
                self.b.notice = None;
                let (c, off) = (cx.api(), plugin_disabled(&p));
                cx.spawn(D_DONE, move || {
                    if off {
                        c.enable_plugin(&p.id, &p.version)
                    } else {
                        c.disable_plugin(&p.id, &p.version)
                    }
                });
            }
            PlugAct::Uninstall(i) => self.b.ask(
                "Uninstall plugin",
                format!("Uninstall {} {}? The server needs a restart to finish.", p.name, p.version),
                "Uninstall",
                i,
            ),
        }
    }

    fn uninstall(&mut self, i: usize, cx: &Cx) {
        let Some(p) = self.list.as_ref().and_then(|l| l.get(i)).cloned() else { return };
        self.b.busy = true;
        self.b.notice = None;
        let c = cx.api();
        cx.spawn(D_DONE, move || c.uninstall_plugin(&p.id, &p.version));
    }
}

impl Screen for Plugins {
    fn name(&self) -> &'static str {
        "plugins"
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        match self.b.key(key) {
            KeyOut::Confirmed(i) => {
                self.uninstall(i, cx);
                true
            }
            KeyOut::Done => true,
            KeyOut::Pass => false,
        }
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn Any + Send>) {
        match tag {
            D_LIST => {
                if let Some(r) = take::<Vec<PluginInfo>>(payload) {
                    match r {
                        Ok(v) => self.list = Some(v),
                        Err(e) => self.b.error = fail(cx, e),
                    }
                }
            }
            P_PAGES => {
                // Settings buttons are a convenience: a failed lookup just hides them.
                if let Some(Ok(v)) = take::<Vec<ConfigPage>>(payload) {
                    self.pages = v;
                }
            }
            D_DONE => {
                if let Some(r) = take::<()>(payload) {
                    if self.b.finished(cx, r, "Done. Restart the server for this to take effect.") {
                        self.load(cx);
                    }
                }
            }
            _ => {}
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        if !self.b.started {
            self.b.started = true;
            self.load(cx);
        }
        let moving = ui.input(|i| i.pointer.is_moving());
        let mut ctl = self.b.ctl(moving);
        let mut acts: Vec<PlugAct> = vec![];
        let retry = self.b.head(ui, t, &mut ctl, "Plugins", self.list.is_none());
        match &self.list {
            None => {
                if self.b.error.is_none() {
                    skeleton_rows(ui, t, 4);
                }
            }
            Some(list) if list.is_empty() => parts::muted(ui, t, "No plugins installed."),
            Some(list) => {
                egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
                    for (i, p) in list.iter().enumerate() {
                        ui.add_space(t.gap / 3.0);
                        body_line(ui, t, &format!("{}  {}", p.name, p.version));
                        small_line(ui, t, &format!("{} · {}", p.status, p.description));
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = t.gap / 2.0;
                            if page_for_plugin(&self.pages, &p.id).is_some() && ctl.button(ui, t, "Settings", false) {
                                acts.push(PlugAct::Settings(i));
                            }
                            let label = if plugin_disabled(p) { "Enable" } else { "Disable" };
                            if ctl.button(ui, t, label, false) {
                                acts.push(PlugAct::Toggle(i));
                            }
                            if p.can_uninstall && ctl.button(ui, t, "Uninstall", false) {
                                acts.push(PlugAct::Uninstall(i));
                            }
                        });
                    }
                });
            }
        }
        self.b.end_frame(&ctl);
        if retry {
            self.load(cx);
        }
        for a in acts {
            self.run(a, cx);
        }
        if let Some(url) = self.open.take() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
        }
        if let Some(i) = self.b.confirm_ui(ui, t, moving) {
            self.uninstall(i, cx);
        }
    }
}

// ---------- plugin catalogue ----------

const C_PLUGINS: &str = "plugins";

pub struct Catalogue {
    b: Base,
    list: Option<Vec<PackageInfo>>,
    installed: Vec<String>,
}

pub fn catalogue() -> Box<dyn Screen> {
    Box::new(Catalogue { b: Base::default(), list: None, installed: vec![] })
}

impl Catalogue {
    fn load(&mut self, cx: &Cx) {
        self.b.error = None;
        let c = cx.api();
        cx.spawn(D_LIST, move || c.packages());
        let c = cx.api();
        cx.spawn(C_PLUGINS, move || c.plugins());
    }

    fn install(&mut self, i: usize, cx: &Cx) {
        let Some(p) = self.list.as_ref().and_then(|l| l.get(i)).cloned() else { return };
        let Some(v) = p.versions.first().cloned() else { return };
        self.b.busy = true;
        self.b.notice = None;
        let c = cx.api();
        cx.spawn(D_DONE, move || c.install_package(&p.name, &p.guid, &v.version, &v.repository_url));
    }
}

impl Screen for Catalogue {
    fn name(&self) -> &'static str {
        "catalogue"
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        match self.b.key(key) {
            KeyOut::Confirmed(i) => {
                self.install(i, cx);
                true
            }
            KeyOut::Done => true,
            KeyOut::Pass => false,
        }
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn Any + Send>) {
        match tag {
            D_LIST => {
                if let Some(r) = take::<Vec<PackageInfo>>(payload) {
                    match r {
                        Ok(mut v) => {
                            v.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
                            self.list = Some(v);
                        }
                        Err(e) => self.b.error = fail(cx, e),
                    }
                }
            }
            C_PLUGINS => {
                if let Some(Ok(v)) = take::<Vec<PluginInfo>>(payload) {
                    self.installed = v.into_iter().map(|p| p.name.to_lowercase()).collect();
                }
            }
            D_DONE => {
                if let Some(r) = take::<()>(payload) {
                    if self.b.finished(cx, r, "Install started. Restart the server to finish.") {
                        self.load(cx);
                    }
                }
            }
            _ => {}
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        if !self.b.started {
            self.b.started = true;
            self.load(cx);
        }
        let moving = ui.input(|i| i.pointer.is_moving());
        let mut ctl = self.b.ctl(moving);
        let mut asks: Vec<usize> = vec![];
        let retry = self.b.head(ui, t, &mut ctl, "Plugin catalogue", self.list.is_none());
        match &self.list {
            None => {
                if self.b.error.is_none() {
                    skeleton_rows(ui, t, 4);
                }
            }
            Some(list) if list.is_empty() => {
                parts::muted(ui, t, "Nothing in the catalogue. Check the repositories and the server's internet.")
            }
            Some(list) => {
                egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
                    for (i, p) in list.iter().enumerate() {
                        ui.add_space(t.gap / 3.0);
                        let latest = p.versions.first().map(|v| v.version.as_str()).unwrap_or("");
                        body_line(ui, t, &format!("{}  {}", p.name, latest));
                        small_line(ui, t, &format!("{} · {}", p.category, p.description));
                        if self.installed.contains(&p.name.to_lowercase()) {
                            small_line(ui, t, "Installed");
                        } else if !p.versions.is_empty() && ctl.button(ui, t, "Install", true) {
                            asks.push(i);
                        }
                    }
                });
            }
        }
        self.b.end_frame(&ctl);
        if retry {
            self.load(cx);
        }
        for i in asks {
            if let Some(p) = self.list.as_ref().and_then(|l| l.get(i)) {
                let v = p.versions.first().map(|v| v.version.clone()).unwrap_or_default();
                let body = format!("Install {} {} from {}? The server needs a restart to finish.", p.name, v, p.owner);
                self.b.ask("Install plugin", body, "Install", i);
            }
        }
        if let Some(i) = self.b.confirm_ui(ui, t, moving) {
            self.install(i, cx);
        }
    }
}

// ---------- repositories ----------

pub struct Repositories {
    b: Base,
    list: Option<Vec<RepositoryInfo>>,
    name: String,
    url: String,
}

pub fn repositories() -> Box<dyn Screen> {
    Box::new(Repositories { b: Base::default(), list: None, name: String::new(), url: String::new() })
}

enum RepoAct {
    Add,
    Remove(usize),
}

impl Repositories {
    fn load(&mut self, cx: &Cx) {
        self.b.error = None;
        let c = cx.api();
        cx.spawn(D_LIST, move || c.repositories());
    }

    fn save(&mut self, cx: &Cx, list: Vec<RepositoryInfo>) {
        self.b.busy = true;
        self.b.notice = None;
        let c = cx.api();
        cx.spawn(D_DONE, move || c.set_repositories(&list));
    }

    fn run(&mut self, a: RepoAct, cx: &Cx) {
        let Some(list) = self.list.clone() else { return };
        match a {
            RepoAct::Add => {
                if self.b.busy {
                    return;
                }
                match with_repository(&list, &self.name, &self.url) {
                    Some(new) => {
                        self.b.error = None;
                        self.name.clear();
                        self.url.clear();
                        self.save(cx, new);
                    }
                    None => self.b.error = Some("Give it a name and an address starting http:// or https://.".into()),
                }
            }
            RepoAct::Remove(i) => {
                let name = list.get(i).and_then(|r| r.name.clone()).unwrap_or_default();
                self.b.ask("Remove repository", format!("Remove {name} from the plugin repositories?"), "Remove", i);
            }
        }
    }

    fn remove(&mut self, i: usize, cx: &Cx) {
        if let Some(list) = self.list.clone() {
            self.save(cx, without_repository(&list, i));
        }
    }
}

impl Screen for Repositories {
    fn name(&self) -> &'static str {
        "repositories"
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        match self.b.key(key) {
            KeyOut::Confirmed(i) => {
                self.remove(i, cx);
                true
            }
            KeyOut::Done => true,
            KeyOut::Pass => false,
        }
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn Any + Send>) {
        match tag {
            D_LIST => {
                if let Some(r) = take::<Vec<RepositoryInfo>>(payload) {
                    match r {
                        Ok(v) => self.list = Some(v),
                        Err(e) => self.b.error = fail(cx, e),
                    }
                }
            }
            D_DONE => {
                if let Some(r) = take::<()>(payload) {
                    if self.b.finished(cx, r, "Saved.") {
                        self.load(cx);
                    }
                }
            }
            _ => {}
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        if !self.b.started {
            self.b.started = true;
            self.load(cx);
        }
        let moving = ui.input(|i| i.pointer.is_moving());
        let mut ctl = self.b.ctl(moving);
        let mut acts: Vec<RepoAct> = vec![];
        let retry = self.b.head(ui, t, &mut ctl, "Plugin repositories", self.list.is_none());
        ui.add_space(t.gap / 3.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = t.gap / 2.0;
            ctl.field(ui, t, &mut self.name, "Name", false);
            ctl.field(ui, t, &mut self.url, "Address (https://…)", false);
            let label = if self.b.busy { "Working…" } else { "Add" };
            if ctl.button(ui, t, label, true) {
                acts.push(RepoAct::Add);
            }
        });
        match &self.list {
            None => {
                if self.b.error.is_none() {
                    skeleton_rows(ui, t, 3);
                }
            }
            Some(list) if list.is_empty() => parts::muted(ui, t, "No repositories."),
            Some(list) => {
                egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
                    for (i, r) in list.iter().enumerate() {
                        ui.add_space(t.gap / 3.0);
                        let off = if r.enabled { "" } else { " (off)" };
                        body_line(ui, t, &format!("{}{off}", r.name.as_deref().unwrap_or("Unnamed")));
                        small_line(ui, t, r.url.as_deref().unwrap_or(""));
                        if ctl.button(ui, t, "Remove", false) {
                            acts.push(RepoAct::Remove(i));
                        }
                    }
                });
            }
        }
        self.b.end_frame(&ctl);
        if retry {
            self.load(cx);
        }
        for a in acts {
            self.run(a, cx);
        }
        if let Some(i) = self.b.confirm_ui(ui, t, moving) {
            self.remove(i, cx);
        }
    }
}

// ---------- logs ----------

pub struct Logs {
    files: Option<Vec<LogFile>>,
    error: Option<String>,
    started: bool,
    focus: Focus,
    groups: Vec<Group>,
}

pub fn logs() -> Box<dyn Screen> {
    Box::new(Logs { files: None, error: None, started: false, focus: Focus::default(), groups: vec![] })
}

impl Logs {
    fn load(&mut self, cx: &Cx) {
        self.error = None;
        let c = cx.api();
        cx.spawn(D_LIST, move || c.log_files());
    }

    fn open(&self, i: usize, cx: &mut Cx) {
        if let Some(f) = self.files.as_ref().and_then(|l| l.get(i)) {
            cx.push(log_view(f.name.clone()));
        }
    }
}

impl Screen for Logs {
    fn name(&self) -> &'static str {
        "logs"
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        match key {
            Key::Enter => {
                self.open(self.focus.index, cx);
                true
            }
            Key::Up | Key::Down | Key::Left | Key::Right => {
                self.focus = move_focus(&self.groups, self.focus, key);
                true
            }
            _ => false,
        }
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn Any + Send>) {
        if tag != D_LIST {
            return;
        }
        if let Some(r) = take::<Vec<LogFile>>(payload) {
            match r {
                Ok(mut v) => {
                    sort_logs(&mut v);
                    self.files = Some(v);
                }
                Err(e) => self.error = fail(cx, e),
            }
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
        parts::title(ui, t, "Logs");
        ui.add_space(t.gap / 2.0);
        if let Some(e) = self.error.clone() {
            parts::error_line(ui, t, &e);
            if parts::button(ui, t, "Retry", true, false).clicked() {
                self.load(cx);
            }
            self.groups = vec![];
            return;
        }
        let Some(files) = &self.files else {
            skeleton_rows(ui, t, 5);
            self.groups = vec![];
            return;
        };
        if files.is_empty() {
            parts::muted(ui, t, "No log files.");
            self.groups = vec![];
            return;
        }
        let secondary: Vec<String> =
            files.iter().map(|f| format!("{} · {}", size_label(f.size), short_date(&f.date_modified))).collect();
        let rows: Vec<ListRow<'_>> = files
            .iter()
            .zip(&secondary)
            .map(|(f, s)| ListRow { label: &f.name, secondary: Some(s), value: None })
            .collect();
        let sections = vec![("", rows)];
        let mut out = None;
        egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
            out = Some(sectioned_list(ui, t, &sections, Some(self.focus.index)));
        });
        let Some(out) = out else { return };
        self.groups = vec![Group { len: files.len(), cols: 1, xs: out.xs }];
        self.focus = move_focus(&self.groups, self.focus, Key::Enter);
        if let Some(i) = out.hovered.filter(|_| moving) {
            self.focus.index = i;
        }
        if let Some(i) = out.clicked {
            self.focus.index = i;
            self.open(i, cx);
        }
    }
}

// ---------- one log file ----------

pub struct LogView {
    name: String,
    lines: Option<Vec<String>>,
    error: Option<String>,
    started: bool,
    scroll: f32,
}

fn log_view(name: String) -> Box<dyn Screen> {
    Box::new(LogView { name, lines: None, error: None, started: false, scroll: 0.0 })
}

impl Screen for LogView {
    fn name(&self) -> &'static str {
        "logview"
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        let line = cx.theme.small_size * 1.5;
        match key {
            Key::Up => self.scroll = (self.scroll - 3.0 * line).max(0.0),
            Key::Down => self.scroll += 3.0 * line,
            Key::Left => self.scroll = (self.scroll - 30.0 * line).max(0.0),
            Key::Right => self.scroll += 30.0 * line,
            _ => return false,
        }
        true
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn Any + Send>) {
        if tag != "text" {
            return;
        }
        if let Some(r) = take::<String>(payload) {
            match r {
                Ok(text) => self.lines = Some(newest_first(&text, LOG_LINES)),
                Err(e) => self.error = fail(cx, e),
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        if !self.started {
            self.started = true;
            let (c, name) = (cx.api(), self.name.clone());
            cx.spawn("text", move || c.log_text(&name));
        }
        ui.add_space(t.margin);
        parts::title(ui, t, &self.name);
        ui.add_space(t.gap / 4.0);
        if let Some(e) = &self.error {
            parts::error_line(ui, t, e);
            return;
        }
        let Some(lines) = &self.lines else {
            skeleton_rows(ui, t, 6);
            return;
        };
        small_line(ui, t, &format!("Newest first · last {} lines", lines.len()));
        if lines.is_empty() {
            parts::muted(ui, t, "This file is empty.");
            return;
        }
        let font = egui::FontId::monospace(t.small_size);
        let row_h = t.small_size * 1.5;
        let out = egui::ScrollArea::both().auto_shrink([false, false]).vertical_scroll_offset(self.scroll).show_rows(
            ui,
            row_h,
            lines.len(),
            |ui, range| {
                for l in &lines[range] {
                    ui.add(egui::Label::new(egui::RichText::new(l).font(font.clone()).color(t.text)).extend());
                }
            },
        );
        self.scroll = out.state.offset.y;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(name: &str, custom: Option<&str>) -> DeviceInfoDto {
        DeviceInfoDto { name: Some(name.into()), custom_name: custom.map(str::to_string), ..Default::default() }
    }

    #[test]
    fn device_name_prefers_custom() {
        assert_eq!(device_name(&dev("TV", Some("Lounge"))), "Lounge");
        assert_eq!(device_name(&dev("TV", Some(""))), "TV");
        assert_eq!(device_name(&dev("TV", None)), "TV");
        assert_eq!(device_name(&DeviceInfoDto::default()), "Unknown device");
    }

    #[test]
    fn device_meta_joins_what_is_known() {
        let mut d = dev("TV", None);
        d.app_name = Some("Jellyfin".into());
        d.app_version = Some("10.9".into());
        d.last_user_name = Some("tester".into());
        d.date_last_activity = Some("2026-10-10T17:00:00Z".into());
        assert_eq!(device_meta(&d), "Jellyfin 10.9 · tester · last seen 2026-10-10 17:00");
        assert_eq!(device_meta(&dev("x", None)), "");
    }

    #[test]
    fn devices_sort_newest_first() {
        let mut a = dev("a", None);
        a.date_last_activity = Some("2026-01-01T00:00:00Z".into());
        let mut b = dev("b", None);
        b.date_last_activity = Some("2026-02-01T00:00:00Z".into());
        let mut v = vec![a, b];
        sort_devices(&mut v);
        assert_eq!(v[0].name.as_deref(), Some("b"));
    }

    #[test]
    fn sizes() {
        assert_eq!(size_label(10), "10 B");
        assert_eq!(size_label(2048), "2.0 KB");
        assert_eq!(size_label(3 * 1_048_576), "3.0 MB");
    }

    #[test]
    fn repositories_add_and_remove() {
        let list = vec![RepositoryInfo { name: Some("a".into()), url: Some("https://a".into()), enabled: true }];
        assert!(with_repository(&list, "", "https://b").is_none());
        assert!(with_repository(&list, "b", "ftp://b").is_none());
        let added = with_repository(&list, " b ", " https://b ").unwrap();
        assert_eq!(added.len(), 2);
        assert_eq!(added[1].name.as_deref(), Some("b"));
        assert_eq!(added[1].url.as_deref(), Some("https://b"));
        assert_eq!(without_repository(&added, 0).len(), 1);
        assert_eq!(without_repository(&added, 9).len(), 2);
    }

    #[test]
    fn disabled_status() {
        let p = PluginInfo { status: "Disabled".into(), ..Default::default() };
        assert!(plugin_disabled(&p));
        assert!(!plugin_disabled(&PluginInfo { status: "Active".into(), ..Default::default() }));
    }
}
