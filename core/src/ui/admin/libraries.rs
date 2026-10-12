//! Admin libraries (P-ADM-11..15): the library list (scan, add), one library's page (rename, folders, options,
//! remove) and the Display, Metadata and NFO pages. Edits are held locally and saved with one button; Remove
//! confirms. The server config pages edit the config as JSON so every field they don't show is posted back as it was.
use super::sectioned_list;
use super::users::{fail, skeleton_rows, Ctl};
use crate::api::admin::{new_library_options, AvailableOptions, AvailableType, FetcherInfo};
use crate::api::gen_admin::{LibraryOptions, TypeOptions, VirtualFolderInfo};
use crate::api::Result;
use crate::app::{Cx, Screen};
use crate::nav::{move_focus, Focus, Group};
use crate::platform::Key;
use crate::ui::parts;
use crate::ui::widgets::{self, DropdownEvent, DropdownState, ListRow};
use serde_json::Value;

// ---------- pure helpers (unit-tested below) ----------

/// (label, server value) for the content types a library can have. Mixed content is sent as no type at all.
const CONTENT_TYPES: [(&str, &str); 7] = [
    ("Movies", "movies"),
    ("Shows", "tvshows"),
    ("Music", "music"),
    ("Music videos", "musicvideos"),
    ("Home videos and photos", "homevideos"),
    ("Books", "books"),
    ("Mixed content", ""),
];

fn content_label(ct: Option<&str>) -> String {
    let ct = ct.unwrap_or("");
    match ct {
        "boxsets" => return "Collections".to_string(),
        "playlists" => return "Playlists".to_string(),
        _ => {}
    }
    CONTENT_TYPES.iter().find(|(_, v)| *v == ct).map_or_else(|| ct.to_string(), |(l, _)| l.to_string())
}

/// The item types whose fetchers a library of this content type can switch.
fn types_for(ct: &str) -> &'static [&'static str] {
    match ct {
        "movies" => &["Movie"],
        "tvshows" => &["Series", "Season", "Episode"],
        "music" => &["MusicArtist", "MusicAlbum", "Audio"],
        "musicvideos" => &["MusicVideo"],
        "books" => &["Book"],
        _ => &[],
    }
}

fn type_label(ty: &str) -> String {
    match ty {
        "Movie" => "Movies",
        "Series" => "Shows",
        "Season" => "Seasons",
        "Episode" => "Episodes",
        "MusicArtist" => "Artists",
        "MusicAlbum" => "Albums",
        "Audio" => "Songs",
        "MusicVideo" => "Music videos",
        "Book" => "Books",
        other => other,
    }
    .to_string()
}

/// The fetchers switched on for `ty`: the library's own list when it has one, else the server's defaults.
fn effective_fetchers(lib: &LibraryOptions, ty: &str, images: bool, offered: &[FetcherInfo]) -> Vec<String> {
    let own = lib.type_options.iter().find(|t| t.type_.as_deref() == Some(ty)).and_then(|t| {
        if images {
            t.image_fetchers.clone()
        } else {
            t.metadata_fetchers.clone()
        }
    });
    own.unwrap_or_else(|| offered.iter().filter(|f| f.default_enabled).map(|f| f.name.clone()).collect())
}

/// Switch one fetcher on or off for `ty`. The library's entry for the type is created from the server defaults
/// first, so switching one fetcher keeps the rest as they were shown.
fn toggle_fetcher(lib: &mut LibraryOptions, offered: &AvailableType, images: bool, name: &str) {
    let ty = offered.type_.as_str();
    let meta = effective_fetchers(lib, ty, false, &offered.metadata_fetchers);
    let imgs = effective_fetchers(lib, ty, true, &offered.image_fetchers);
    let i = match lib.type_options.iter().position(|t| t.type_.as_deref() == Some(ty)) {
        Some(i) => i,
        None => {
            lib.type_options.push(TypeOptions {
                type_: Some(ty.to_string()),
                image_options: Some(offered.default_image_options.clone()),
                ..Default::default()
            });
            lib.type_options.len() - 1
        }
    };
    let entry = &mut lib.type_options[i];
    entry.metadata_fetchers = Some(meta);
    entry.image_fetchers = Some(imgs);
    let list = if images { entry.image_fetchers.as_mut() } else { entry.metadata_fetchers.as_mut() };
    if let Some(list) = list {
        match list.iter().position(|n| n == name) {
            Some(p) => {
                list.remove(p);
            }
            None => list.push(name.to_string()),
        }
    }
}

fn nfo_saver_on(lib: &LibraryOptions) -> bool {
    lib.metadata_savers.as_ref().is_some_and(|v| v.iter().any(|s| s == "Nfo"))
}

fn set_nfo_saver(lib: &mut LibraryOptions, on: bool) {
    let v = lib.metadata_savers.get_or_insert_with(Vec::new);
    v.retain(|s| s != "Nfo");
    if on {
        v.push("Nfo".to_string());
    }
}

fn nfo_reader_on(lib: &LibraryOptions) -> bool {
    !lib.disabled_local_metadata_readers.iter().any(|s| s == "Nfo")
}

fn set_nfo_reader(lib: &mut LibraryOptions, on: bool) {
    lib.disabled_local_metadata_readers.retain(|s| s != "Nfo");
    if !on {
        lib.disabled_local_metadata_readers.push("Nfo".to_string());
    }
}

const REFRESH_DAYS: [i32; 4] = [0, 30, 60, 90];

fn refresh_labels() -> Vec<String> {
    REFRESH_DAYS.iter().map(|d| if *d == 0 { "Never".to_string() } else { format!("Every {d} days") }).collect()
}

/// Index into `REFRESH_DAYS`; a value the server holds that is not listed shows as "Never".
fn refresh_index(days: i32) -> usize {
    REFRESH_DAYS.iter().position(|d| *d == days).unwrap_or(0)
}

/// "Scanning 42%" while the server is refreshing the library; empty when it is idle.
fn scan_status(f: &VirtualFolderInfo) -> String {
    match f.refresh_status.as_deref() {
        None | Some("") | Some("Idle") => String::new(),
        Some(_) => match f.refresh_progress {
            Some(p) => format!("Scanning {}%", p.round() as i64),
            None => "Scanning".to_string(),
        },
    }
}

fn is_scanning(f: &VirtualFolderInfo) -> bool {
    !scan_status(f).is_empty()
}

/// A folder path typed by the user: trimmed, must not be empty.
fn clean_path(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

/// A text field's value with surrounding spaces removed; empty means "not set".
fn opt_text(s: &str) -> Option<String> {
    clean_path(s)
}

fn cfg_bool(v: &Value, key: &str) -> bool {
    v.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn cfg_str(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

fn cfg_set(v: &mut Value, key: &str, new: Value) {
    if let Some(o) = v.as_object_mut() {
        o.insert(key.to_string(), new);
    }
}

// ---------- shared form state ----------

/// Focus, keys and dropdowns of a one-column form of controls (`Ctl` draws them).
pub(super) struct FormState {
    focus: usize,
    groups: Vec<Group>,
    enter: bool,
    pub(super) dd: [DropdownState; 6],
    cur_dd: Option<(usize, usize)>,
    on_field: bool,
}

impl FormState {
    pub(super) fn new() -> FormState {
        FormState {
            focus: 0,
            groups: vec![],
            enter: false,
            dd: [DropdownState::default(); 6],
            cur_dd: None,
            on_field: false,
        }
    }

    /// Returns (handled, dropdown pick as (dropdown, option)).
    pub(super) fn key(&mut self, key: Key) -> (bool, Option<(usize, usize)>) {
        if let Some((w, len)) = self.cur_dd {
            let st = self.dd[w];
            if matches!(key, Key::Enter | Key::Up | Key::Down | Key::Back) && (st.open || key == Key::Enter) {
                let (ns, ev) = widgets::dropdown_key(st, key, len);
                self.dd[w] = ns;
                return (true, if let Some(DropdownEvent::Pick(i)) = ev { Some((w, i)) } else { None });
            }
        }
        match key {
            Key::Enter => {
                self.enter = true;
                (true, None)
            }
            Key::Left | Key::Right if self.on_field => (true, None),
            Key::Up | Key::Down | Key::Left | Key::Right => {
                self.focus = move_focus(&self.groups, Focus { group: 0, index: self.focus }, key).index;
                (true, None)
            }
            _ => (false, None),
        }
    }

    pub(super) fn ctl(&mut self, moving: bool, active: bool) -> Ctl {
        let enter = std::mem::take(&mut self.enter) && active;
        Ctl::new(active.then_some(self.focus), enter, moving)
    }

    pub(super) fn finish(&mut self, ctl: Ctl) {
        self.cur_dd = ctl.dd;
        self.on_field = ctl.on_field;
        self.groups = vec![Group { len: ctl.idx, cols: 1, xs: ctl.xs.clone() }];
        self.focus = move_focus(&self.groups, Focus { group: 0, index: self.focus }, Key::Enter).index;
        if let Some(i) = ctl.hover.or(ctl.clicked) {
            self.focus = i;
        }
        if ctl.advance {
            self.focus = move_focus(&self.groups, Focus { group: 0, index: self.focus }, Key::Down).index;
        }
    }
}

// ---------- the list ----------

const L_ACTIONS: usize = 0;
const L_LIST: usize = 1;

pub struct Libraries {
    folders: Option<Vec<VirtualFolderInfo>>,
    error: Option<String>,
    notice: Option<String>,
    started: bool,
    last_load: f64,
    /// FIX-1 #18: a scan was started at this time (s) and is being watched until it ends; `scan_seen` once any
    /// library reported scanning.
    scan_watch: Option<f64>,
    scan_seen: bool,
    focus: Focus,
    groups: Vec<Group>,
}

/// FIX-1 #18: after "Scan all", when to say it has finished: once a library was seen scanning and none is now,
/// or after `quiet` seconds with none ever seen (a scan quicker than the first poll).
fn scan_finished(seen: bool, scanning_now: bool, waited: f64, quiet: f64) -> bool {
    !scanning_now && (seen || waited > quiet)
}

pub fn screen() -> Box<dyn Screen> {
    Box::new(Libraries {
        folders: None,
        error: None,
        notice: None,
        started: false,
        last_load: 0.0,
        scan_watch: None,
        scan_seen: false,
        focus: Focus { group: L_LIST, index: 0 },
        groups: vec![],
    })
}

impl Libraries {
    fn load(&mut self, cx: &Cx) {
        self.error = None;
        let c = cx.api();
        cx.spawn("folders", move || c.virtual_folders());
    }

    fn open(&self, idx: usize, cx: &mut Cx) {
        if let Some(f) = self.folders.as_ref().and_then(|v| v.get(idx)) {
            cx.push(library_page(f.name.clone().unwrap_or_default()));
        }
    }

    fn act(&mut self, idx: usize, cx: &mut Cx) {
        if idx == 0 {
            cx.push(add_library());
        } else {
            self.notice = None;
            let c = cx.api();
            cx.spawn("scanall", move || c.scan_all_libraries());
        }
    }
}

impl Screen for Libraries {
    fn name(&self) -> &'static str {
        "libraries"
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        match key {
            Key::Enter => {
                match self.focus.group {
                    L_ACTIONS => self.act(self.focus.index, cx),
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
        match tag {
            "folders" => {
                let Ok(r) = payload.downcast::<Result<Vec<VirtualFolderInfo>>>() else { return };
                match *r {
                    Ok(mut v) => {
                        v.sort_by_key(|f| f.name.clone().unwrap_or_default().to_lowercase());
                        let scanning = v.iter().any(is_scanning);
                        self.scan_seen |= scanning;
                        if let Some(start) = self.scan_watch {
                            if scan_finished(self.scan_seen, scanning, cx.now - start, 30.0) {
                                self.scan_watch = None;
                                self.notice = Some("Scan finished.".into());
                            }
                        }
                        self.folders = Some(v);
                    }
                    Err(e) => self.error = fail(cx, e),
                }
            }
            "scanall" => {
                let Ok(r) = payload.downcast::<Result<()>>() else { return };
                match *r {
                    Ok(()) => {
                        self.notice = Some("Scanning all libraries…".into());
                        self.scan_watch = Some(cx.now);
                        self.scan_seen = false;
                        self.load(cx);
                    }
                    Err(e) => self.error = fail(cx, e),
                }
            }
            _ => {}
        }
    }

    fn on_resume(&mut self, cx: &mut Cx) {
        self.load(cx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        let now = ui.input(|i| i.time);
        if !self.started {
            self.started = true;
            self.last_load = now;
            self.load(cx);
        }
        // While the server is scanning, ask again every few seconds so the progress moves.
        if self.scan_watch.is_some() || self.folders.as_ref().is_some_and(|v| v.iter().any(is_scanning)) {
            if now - self.last_load > 3.0 {
                self.last_load = now;
                self.load(cx);
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
        }
        let moving = ui.input(|i| i.pointer.is_moving());
        ui.add_space(t.margin);
        parts::title(ui, t, "Libraries");
        ui.add_space(t.gap / 2.0);

        let mut action_xs = vec![];
        let mut action_clicked = None;
        ui.horizontal(|ui| {
            for (i, label) in ["Add library", "Scan all libraries"].iter().enumerate() {
                let focused = self.focus.group == L_ACTIONS && self.focus.index == i;
                let r = parts::button(ui, t, label, focused, i == 0);
                action_xs.push(r.rect.center().x);
                if r.hovered() && moving {
                    self.focus = Focus { group: L_ACTIONS, index: i };
                }
                if r.clicked() {
                    action_clicked = Some(i);
                }
            }
        });
        if let Some(n) = &self.notice {
            parts::muted(ui, t, n);
        }
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
        } else if let Some(folders) = &self.folders {
            let labels: Vec<String> = folders.iter().map(|f| f.name.clone().unwrap_or_default()).collect();
            let seconds: Vec<String> = folders
                .iter()
                .map(|f| {
                    let paths = f.locations.clone().unwrap_or_default().join(", ");
                    format!("{} · {}", content_label(f.collection_type.as_deref()), paths)
                })
                .collect();
            let status: Vec<String> = folders.iter().map(scan_status).collect();
            let rows: Vec<ListRow<'_>> = (0..folders.len())
                .map(|i| ListRow { label: &labels[i], secondary: Some(&seconds[i]), value: Some(&status[i]) })
                .collect();
            let sections: Vec<(&str, Vec<ListRow<'_>>)> = vec![("", rows)];
            egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
                let out = sectioned_list(ui, t, &sections, (self.focus.group == L_LIST).then_some(self.focus.index));
                list_xs = out.xs;
                hovered = out.hovered;
                clicked = out.clicked;
            });
            count = folders.len();
        } else {
            skeleton_rows(ui, t, 4);
        }

        self.groups = vec![Group { len: 2, cols: 2, xs: action_xs }, Group { len: count, cols: 1, xs: list_xs }];
        self.focus = move_focus(&self.groups, self.focus, Key::Enter);
        if moving {
            if let Some(i) = hovered {
                self.focus = Focus { group: L_LIST, index: i };
            }
        }
        if let Some(i) = action_clicked {
            self.focus = Focus { group: L_ACTIONS, index: i };
            self.act(i, cx);
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

// ---------- add library ----------

const DD_TYPE: usize = 0;

struct AddLibrary {
    name: String,
    path_input: String,
    paths: Vec<String>,
    language: String,
    country: String,
    realtime: bool,
    busy: bool,
    error: Option<String>,
    form: FormState,
}

fn add_library() -> Box<dyn Screen> {
    Box::new(AddLibrary {
        name: String::new(),
        path_input: String::new(),
        paths: vec![],
        language: "en".into(),
        country: "GB".into(),
        realtime: true,
        busy: false,
        error: None,
        form: FormState::new(),
    })
}

enum AddAct {
    AddPath,
    RemovePath(usize),
    Create,
}

impl AddLibrary {
    fn add_path(&mut self) {
        if let Some(p) = clean_path(&self.path_input) {
            if !self.paths.contains(&p) {
                self.paths.push(p);
            }
            self.path_input.clear();
        }
    }

    fn create(&mut self, cx: &Cx) {
        if self.busy {
            return;
        }
        let name = self.name.trim().to_string();
        if name.is_empty() {
            self.error = Some("Give the library a name.".into());
            return;
        }
        self.add_path();
        if self.paths.is_empty() {
            self.error = Some("Add at least one folder (a path on the server).".into());
            return;
        }
        self.busy = true;
        self.error = None;
        let ct = CONTENT_TYPES[self.form.dd[DD_TYPE].highlighted.min(CONTENT_TYPES.len() - 1)].1;
        let mut opts = new_library_options(&self.paths);
        opts.enable_realtime_monitor = self.realtime;
        opts.preferred_metadata_language = opt_text(&self.language);
        opts.metadata_country_code = opt_text(&self.country);
        let paths = self.paths.clone();
        let c = cx.api();
        cx.spawn("create", move || c.add_virtual_folder(&name, (!ct.is_empty()).then_some(ct), &paths, &opts, true));
    }
}

impl Screen for AddLibrary {
    fn name(&self) -> &'static str {
        "libraryadd"
    }

    fn on_key(&mut self, _cx: &mut Cx, key: Key) -> bool {
        self.form.key(key).0
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn std::any::Any + Send>) {
        if tag != "create" {
            return;
        }
        self.busy = false;
        let Ok(r) = payload.downcast::<Result<()>>() else { return };
        match *r {
            Ok(()) => cx.pop(),
            Err(crate::api::ApiError::Server(400, _)) => {
                self.error = Some("The server refused that name or folder. Is the folder a path on the server?".into())
            }
            Err(e) => self.error = fail(cx, e),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        let moving = ui.input(|i| i.pointer.is_moving());
        let mut ctl = self.form.ctl(moving, true);
        let mut acts = vec![];
        ui.add_space(t.margin);
        parts::title(ui, t, "Add library");
        ui.add_space(t.gap / 2.0);
        egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
            ctl.field(ui, t, &mut self.name, "Library name", false);
            ui.add_space(t.gap / 2.0);
            parts::muted(ui, t, "Content type");
            let types: Vec<String> = CONTENT_TYPES.iter().map(|(l, _)| l.to_string()).collect();
            ctl.dropdown(ui, t, &mut self.form.dd[DD_TYPE], DD_TYPE, "", &types);
            ui.add_space(t.gap / 2.0);
            parts::muted(ui, t, "Folders (paths on the server)");
            for (i, p) in self.paths.iter().enumerate() {
                if ctl.button(ui, t, &format!("Remove  {p}"), false) {
                    acts.push(AddAct::RemovePath(i));
                }
            }
            ctl.field(ui, t, &mut self.path_input, "e.g. /media/Movies", false);
            if ctl.button(ui, t, "Add folder", false) {
                acts.push(AddAct::AddPath);
            }
            ui.add_space(t.gap / 2.0);
            parts::muted(ui, t, "Metadata language and country");
            ctl.field(ui, t, &mut self.language, "Language code, e.g. en", false);
            ui.add_space(t.gap / 3.0);
            ctl.field(ui, t, &mut self.country, "Country code, e.g. GB", false);
            ui.add_space(t.gap / 2.0);
            if ctl.toggle(ui, t, "Watch folders for changes (real-time monitor)", self.realtime) {
                self.realtime ^= true;
            }
            ui.add_space(t.gap / 2.0);
            let label = if self.busy { "Creating…" } else { "Create library" };
            if ctl.button(ui, t, label, true) {
                acts.push(AddAct::Create);
            }
            if let Some(e) = &self.error {
                ui.add_space(t.gap / 2.0);
                parts::error_line(ui, t, e);
            }
        });
        self.form.finish(ctl);
        for a in acts {
            match a {
                AddAct::AddPath => self.add_path(),
                AddAct::RemovePath(i) => {
                    if i < self.paths.len() {
                        self.paths.remove(i);
                    }
                }
                AddAct::Create => self.create(cx),
            }
        }
    }
}

// ---------- one library ----------

const DD_REFRESH: usize = 0;

fn library_page(name: String) -> Box<dyn Screen> {
    Box::new(LibraryPage {
        title: name.clone(),
        name: name.clone(),
        new_name: name,
        folder: None,
        opts: LibraryOptions::default(),
        avail: None,
        language: String::new(),
        country: String::new(),
        path_input: String::new(),
        busy: false,
        error: None,
        notice: None,
        started: false,
        confirm: false,
        confirm_focus: 0,
        form: FormState::new(),
    })
}

struct LibraryPage {
    title: String,
    /// The library's current name on the server (what the calls use).
    name: String,
    new_name: String,
    folder: Option<VirtualFolderInfo>,
    opts: LibraryOptions,
    avail: Option<AvailableOptions>,
    language: String,
    country: String,
    path_input: String,
    busy: bool,
    error: Option<String>,
    notice: Option<String>,
    started: bool,
    confirm: bool,
    confirm_focus: usize,
    form: FormState,
}

enum Act {
    Scan,
    Rename,
    AddPath,
    RemovePath(String),
    SaveOptions,
    Remove,
    Fetcher(usize, bool, String),
}

impl LibraryPage {
    fn load(&mut self, cx: &Cx) {
        self.error = None;
        let (c, name) = (cx.api(), self.name.clone());
        cx.spawn("folder", move || -> Result<Option<VirtualFolderInfo>> {
            Ok(c.virtual_folders()?.into_iter().find(|f| f.name.as_deref() == Some(name.as_str())))
        });
        let c = cx.api();
        cx.spawn("avail", move || c.library_available_options());
    }

    /// The item types this library's fetchers can be switched for, with what the server offers for each.
    fn offered(&self) -> Vec<AvailableType> {
        let (Some(avail), Some(f)) = (&self.avail, &self.folder) else { return vec![] };
        let ct = f.collection_type.as_deref().unwrap_or("");
        types_for(ct).iter().filter_map(|ty| avail.type_options.iter().find(|t| t.type_ == *ty).cloned()).collect()
    }

    fn call(&mut self, cx: &Cx, tag: &'static str, f: impl FnOnce(crate::api::Client) -> Result<()> + Send + 'static) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        self.notice = None;
        let c = cx.api();
        cx.spawn(tag, move || f(c));
    }

    fn run(&mut self, act: Act, cx: &Cx) {
        let name = self.name.clone();
        match act {
            Act::Scan => {
                if let Some(id) = self.folder.as_ref().and_then(|f| f.item_id.clone()) {
                    self.call(cx, "scan", move |c| c.scan_library(&id));
                }
            }
            Act::Rename => {
                let new = self.new_name.trim().to_string();
                if new.is_empty() {
                    self.error = Some("The name can't be empty.".into());
                } else if new != name {
                    self.call(cx, "rename", move |c| c.rename_virtual_folder(&name, &new));
                }
            }
            Act::AddPath => {
                if let Some(p) = clean_path(&self.path_input) {
                    self.call(cx, "path", move |c| c.add_media_path(&name, &p));
                }
            }
            Act::RemovePath(p) => self.call(cx, "path", move |c| c.remove_media_path(&name, &p)),
            Act::SaveOptions => {
                let Some(id) = self.folder.as_ref().and_then(|f| f.item_id.clone()) else { return };
                self.opts.preferred_metadata_language = opt_text(&self.language);
                self.opts.metadata_country_code = opt_text(&self.country);
                let opts = self.opts.clone();
                self.call(cx, "options", move |c| c.set_library_options(&id, &opts));
            }
            Act::Remove => {
                self.confirm = true;
                self.confirm_focus = 0;
            }
            Act::Fetcher(i, images, fetcher) => {
                if let Some(ty) = self.offered().get(i) {
                    toggle_fetcher(&mut self.opts, ty, images, &fetcher);
                }
            }
        }
    }

    fn remove(&mut self, cx: &Cx) {
        let name = self.name.clone();
        self.call(cx, "remove", move |c| c.remove_virtual_folder(&name));
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
                    self.remove(cx);
                }
            }
            _ => {}
        }
    }
}

impl Screen for LibraryPage {
    fn name(&self) -> &'static str {
        "adminlibrary"
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        if self.confirm {
            self.confirm_key(cx, key);
            return true;
        }
        let (handled, pick) = self.form.key(key);
        if let Some((DD_REFRESH, i)) = pick {
            self.opts.automatic_refresh_interval_days = REFRESH_DAYS[i.min(REFRESH_DAYS.len() - 1)];
        }
        handled
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn std::any::Any + Send>) {
        match tag {
            "folder" => {
                let Ok(r) = payload.downcast::<Result<Option<VirtualFolderInfo>>>() else { return };
                match *r {
                    Ok(Some(f)) => {
                        self.opts = f.library_options.clone().unwrap_or_default();
                        self.language = self.opts.preferred_metadata_language.clone().unwrap_or_default();
                        self.country = self.opts.metadata_country_code.clone().unwrap_or_default();
                        self.folder = Some(f);
                    }
                    Ok(None) => self.error = Some("That library is gone.".into()),
                    Err(e) => self.error = fail(cx, e),
                }
            }
            "avail" => {
                let Ok(r) = payload.downcast::<Result<AvailableOptions>>() else { return };
                match *r {
                    Ok(a) => self.avail = Some(a),
                    Err(e) => self.error = fail(cx, e),
                }
            }
            "scan" | "rename" | "path" | "options" | "remove" => {
                self.busy = false;
                let Ok(r) = payload.downcast::<Result<()>>() else { return };
                match *r {
                    Ok(()) => match tag {
                        "remove" => cx.pop(),
                        "rename" => {
                            self.name = self.new_name.trim().to_string();
                            self.title = self.name.clone();
                            self.folder = None;
                            self.notice = Some("Renamed. Scan the library to finish; options open after that.".into());
                            self.load(cx);
                        }
                        "path" => {
                            self.path_input.clear();
                            self.notice = Some("Folders changed. Scan the library to pick up the files.".into());
                            self.load(cx);
                        }
                        "options" => self.notice = Some("Saved.".into()),
                        _ => self.notice = Some("Scanning.".into()),
                    },
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
        let mut ctl = self.form.ctl(moving, !self.confirm);
        let mut acts: Vec<Act> = vec![];
        let mut retry = false;

        if let Some(e) = self.error.clone() {
            parts::error_line(ui, t, &e);
            if self.folder.is_none() {
                retry = ctl.button(ui, t, "Retry", false);
            }
        }
        if let Some(n) = &self.notice {
            parts::muted(ui, t, n);
        }
        if self.folder.is_none() {
            if self.error.is_none() {
                skeleton_rows(ui, t, 4);
            }
        } else {
            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .show(ui, |ui| self.body(ui, t, &mut ctl, &mut acts));
        }

        self.form.finish(ctl);
        if retry {
            self.load(cx);
        }
        for a in acts {
            self.run(a, cx);
        }

        if self.confirm {
            let buttons = vec!["Cancel".to_string(), "Remove library".to_string()];
            let body = format!("Remove {} from the server? Your media files are not deleted.", self.name);
            let out = widgets::dialog(ui, t, "Remove library", &body, &buttons, Some(self.confirm_focus));
            if let Some(i) = out.hovered.filter(|_| moving) {
                self.confirm_focus = i;
            }
            if let Some(i) = out.clicked {
                self.confirm = false;
                if i == 1 {
                    self.remove(cx);
                }
            }
        }
    }
}

impl LibraryPage {
    fn body(&mut self, ui: &mut egui::Ui, t: &crate::theme::Theme, ctl: &mut Ctl, acts: &mut Vec<Act>) {
        let Some(folder) = self.folder.clone() else { return };
        let status = scan_status(&folder);
        let line = format!(
            "{}{}",
            content_label(folder.collection_type.as_deref()),
            if status.is_empty() { String::new() } else { format!(" · {status}") }
        );
        parts::muted(ui, t, &line);
        ui.add_space(t.gap / 2.0);
        let has_item = folder.item_id.is_some();
        if has_item && ctl.button(ui, t, "Scan this library", true) {
            acts.push(Act::Scan);
        }

        ui.add_space(t.gap);
        parts::muted(ui, t, "Name");
        ctl.field(ui, t, &mut self.new_name, "Library name", false);
        if ctl.button(ui, t, "Rename", false) {
            acts.push(Act::Rename);
        }

        ui.add_space(t.gap);
        parts::muted(ui, t, "Folders");
        let paths = folder.locations.clone().unwrap_or_default();
        for p in &paths {
            if paths.len() > 1 {
                if ctl.button(ui, t, &format!("Remove  {p}"), false) {
                    acts.push(Act::RemovePath(p.clone()));
                }
            } else {
                parts::muted(ui, t, p);
            }
        }
        ctl.field(ui, t, &mut self.path_input, "Add a folder, e.g. /media/Movies", false);
        if ctl.button(ui, t, "Add folder", false) {
            acts.push(Act::AddPath);
        }

        ui.add_space(t.gap);
        parts::heading(ui, t, "Options");
        if !has_item {
            parts::muted(
                ui,
                t,
                "Options open once the library has been scanned. Scan all libraries from the list first.",
            );
        } else {
            self.options(ui, t, ctl, acts);
        }

        ui.add_space(t.gap);
        if ctl.button(ui, t, "Remove library", false) {
            acts.push(Act::Remove);
        }
    }

    fn options(&mut self, ui: &mut egui::Ui, t: &crate::theme::Theme, ctl: &mut Ctl, acts: &mut Vec<Act>) {
        let o = &mut self.opts;
        if ctl.toggle(ui, t, "Watch folders for changes (real-time monitor)", o.enable_realtime_monitor) {
            o.enable_realtime_monitor ^= true;
        }
        ui.add_space(t.gap / 3.0);
        if ctl.toggle(ui, t, "Save artwork into media folders", o.save_local_metadata) {
            o.save_local_metadata ^= true;
        }
        ui.add_space(t.gap / 3.0);
        let nfo_save = nfo_saver_on(o);
        if ctl.toggle(ui, t, "Save metadata as NFO files", nfo_save) {
            set_nfo_saver(o, !nfo_save);
        }
        ui.add_space(t.gap / 3.0);
        let nfo_read = nfo_reader_on(o);
        if ctl.toggle(ui, t, "Read NFO files", nfo_read) {
            set_nfo_reader(o, !nfo_read);
        }
        ui.add_space(t.gap / 2.0);
        parts::muted(ui, t, "Refresh metadata automatically");
        if !self.form.dd[DD_REFRESH].open {
            self.form.dd[DD_REFRESH].highlighted = refresh_index(o.automatic_refresh_interval_days);
        }
        let labels = refresh_labels();
        if let Some(i) = ctl.dropdown(ui, t, &mut self.form.dd[DD_REFRESH], DD_REFRESH, "", &labels) {
            o.automatic_refresh_interval_days = REFRESH_DAYS[i.min(REFRESH_DAYS.len() - 1)];
        }
        ui.add_space(t.gap / 2.0);
        parts::muted(ui, t, "Metadata language and country");
        ctl.field(ui, t, &mut self.language, "Language code, e.g. en", false);
        ui.add_space(t.gap / 3.0);
        ctl.field(ui, t, &mut self.country, "Country code, e.g. GB", false);

        let offered = self.offered();
        for (i, ty) in offered.iter().enumerate() {
            ui.add_space(t.gap / 2.0);
            parts::muted(ui, t, &format!("{}: metadata fetchers", type_label(&ty.type_)));
            let on = effective_fetchers(&self.opts, &ty.type_, false, &ty.metadata_fetchers);
            for f in &ty.metadata_fetchers {
                if ctl.toggle(ui, t, &f.name, on.contains(&f.name)) {
                    acts.push(Act::Fetcher(i, false, f.name.clone()));
                }
                ui.add_space(t.gap / 4.0);
            }
            parts::muted(ui, t, &format!("{}: image fetchers", type_label(&ty.type_)));
            let on = effective_fetchers(&self.opts, &ty.type_, true, &ty.image_fetchers);
            for f in &ty.image_fetchers {
                if ctl.toggle(ui, t, &f.name, on.contains(&f.name)) {
                    acts.push(Act::Fetcher(i, true, f.name.clone()));
                }
                ui.add_space(t.gap / 4.0);
            }
        }
        ui.add_space(t.gap / 2.0);
        let label = if self.busy { "Saving…" } else { "Save options" };
        if ctl.button(ui, t, label, true) {
            acts.push(Act::SaveOptions);
        }
    }
}

// ---------- server config pages: Display, Metadata, NFO ----------

#[derive(Clone, Copy)]
enum Field {
    Toggle(&'static str, &'static str),
    Text(&'static str, &'static str, &'static str),
    Choice(&'static str, &'static str, &'static [&'static str]),
}

impl Field {
    fn key(&self) -> &'static str {
        match self {
            Field::Toggle(k, _) | Field::Text(k, _, _) | Field::Choice(k, _, _) => k,
        }
    }
}

/// One config page: which screen name, where the config lives (`None` = `/System/Configuration`) and its fields.
#[derive(Clone, Copy)]
struct Kind {
    screen: &'static str,
    title: &'static str,
    section: Option<&'static str>,
    fields: &'static [Field],
}

const DISPLAY: Kind = Kind {
    screen: "librarydisplay",
    title: "Library display",
    section: None,
    fields: &[
        Field::Toggle("EnableFolderView", "Show a folder view"),
        Field::Toggle("DisplaySpecialsWithinSeasons", "Show special episodes within their seasons"),
        Field::Toggle("EnableGroupingIntoCollections", "Group movies into collections"),
        Field::Toggle("EnableExternalContentInSuggestions", "Allow external content in suggestions"),
    ],
};

const METADATA: Kind = Kind {
    screen: "librarymetadata",
    title: "Metadata",
    section: None,
    fields: &[
        Field::Text("PreferredMetadataLanguage", "Preferred metadata language", "Language code, e.g. en"),
        Field::Text("MetadataCountryCode", "Country", "Country code, e.g. GB"),
        Field::Choice("ImageSavingConvention", "Image file names", &["Legacy", "Compatible"]),
        Field::Toggle("SaveMetadataHidden", "Hide the metadata files it saves"),
    ],
};

const NFO: Kind = Kind {
    screen: "librarynfo",
    title: "NFO settings",
    section: Some("xbmcmetadata"),
    fields: &[
        Field::Text("ReleaseDateFormat", "Release date format", "e.g. yyyy-MM-dd"),
        Field::Toggle("SaveImagePathsInNfo", "Save image paths in NFO files"),
        Field::Toggle("EnablePathSubstitution", "Use path substitution"),
        Field::Toggle("EnableExtraThumbsDuplication", "Copy the main image to extra thumbs"),
    ],
};

pub fn display() -> Box<dyn Screen> {
    config_page(DISPLAY)
}
pub fn metadata() -> Box<dyn Screen> {
    config_page(METADATA)
}
pub fn nfo() -> Box<dyn Screen> {
    config_page(NFO)
}

fn config_page(kind: Kind) -> Box<dyn Screen> {
    Box::new(ConfigPage {
        kind,
        config: None,
        texts: vec![],
        busy: false,
        error: None,
        notice: None,
        started: false,
        form: FormState::new(),
    })
}

struct ConfigPage {
    kind: Kind,
    config: Option<Value>,
    /// One buffer per field (used by the text fields).
    texts: Vec<String>,
    busy: bool,
    error: Option<String>,
    notice: Option<String>,
    started: bool,
    form: FormState,
}

impl ConfigPage {
    fn load(&mut self, cx: &Cx) {
        self.error = None;
        let (c, section) = (cx.api(), self.kind.section);
        cx.spawn("config", move || match section {
            Some(s) => c.named_configuration(s),
            None => c.system_configuration(),
        });
    }

    fn save(&mut self, cx: &Cx) {
        let Some(mut cfg) = self.config.clone() else { return };
        if self.busy {
            return;
        }
        for (i, f) in self.kind.fields.iter().enumerate() {
            match f {
                Field::Text(k, _, _) => cfg_set(&mut cfg, k, Value::String(self.texts[i].trim().to_string())),
                Field::Choice(k, _, opts) => {
                    let idx = self.form.dd[0].highlighted.min(opts.len() - 1);
                    cfg_set(&mut cfg, k, Value::String(opts[idx].to_string()));
                }
                Field::Toggle(..) => {}
            }
        }
        self.busy = true;
        self.error = None;
        self.notice = None;
        let (c, section) = (cx.api(), self.kind.section);
        cx.spawn("save", move || -> Result<Value> {
            match section {
                Some(s) => c.set_named_configuration(s, &cfg)?,
                None => c.set_system_configuration(&cfg)?,
            }
            Ok(cfg)
        });
    }

    fn choice_index(&self, cfg: &Value, f: &Field) -> usize {
        match f {
            Field::Choice(k, _, opts) => opts.iter().position(|o| *o == cfg_str(cfg, k)).unwrap_or(0),
            _ => 0,
        }
    }
}

impl Screen for ConfigPage {
    fn name(&self) -> &'static str {
        self.kind.screen
    }

    fn on_key(&mut self, _cx: &mut Cx, key: Key) -> bool {
        let (handled, pick) = self.form.key(key);
        if let Some((_, i)) = pick {
            // The pick is kept in the dropdown's highlight; Save reads it.
            self.form.dd[0].highlighted = i;
        }
        handled
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn std::any::Any + Send>) {
        match tag {
            "config" => {
                let Ok(r) = payload.downcast::<Result<Value>>() else { return };
                match *r {
                    Ok(v) => {
                        self.texts = self.kind.fields.iter().map(|f| cfg_str(&v, f.key())).collect();
                        if let Some(f) = self.kind.fields.iter().find(|f| matches!(f, Field::Choice(..))) {
                            self.form.dd[0].highlighted = self.choice_index(&v, f);
                        }
                        self.config = Some(v);
                    }
                    Err(e) => self.error = fail(cx, e),
                }
            }
            "save" => {
                self.busy = false;
                let Ok(r) = payload.downcast::<Result<Value>>() else { return };
                match *r {
                    Ok(v) => {
                        self.config = Some(v);
                        self.notice = Some("Saved.".into());
                    }
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
        parts::title(ui, t, self.kind.title);
        ui.add_space(t.gap / 2.0);
        let mut ctl = self.form.ctl(moving, true);
        let mut save = false;
        let mut retry = false;
        if let Some(e) = self.error.clone() {
            parts::error_line(ui, t, &e);
            if self.config.is_none() {
                retry = ctl.button(ui, t, "Retry", false);
            }
        }
        if let Some(n) = &self.notice {
            parts::muted(ui, t, n);
        }
        if self.config.is_none() {
            if self.error.is_none() {
                skeleton_rows(ui, t, 4);
            }
        } else {
            let kind = self.kind;
            egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
                for (i, f) in kind.fields.iter().enumerate() {
                    ui.add_space(t.gap / 3.0);
                    match f {
                        Field::Toggle(k, label) => {
                            let on = self.config.as_ref().is_some_and(|c| cfg_bool(c, k));
                            if ctl.toggle(ui, t, label, on) {
                                if let Some(c) = self.config.as_mut() {
                                    cfg_set(c, k, Value::Bool(!on));
                                }
                            }
                        }
                        Field::Text(_, label, hint) => {
                            parts::muted(ui, t, label);
                            ctl.field(ui, t, &mut self.texts[i], hint, false);
                        }
                        Field::Choice(_, label, opts) => {
                            parts::muted(ui, t, label);
                            let labels: Vec<String> = opts.iter().map(|s| s.to_string()).collect();
                            if let Some(p) = ctl.dropdown(ui, t, &mut self.form.dd[0], 0, "", &labels) {
                                self.form.dd[0].highlighted = p;
                            }
                        }
                    }
                }
                ui.add_space(t.gap);
                let label = if self.busy { "Saving…" } else { "Save" };
                save = ctl.button(ui, t, label, true);
            });
        }
        self.form.finish(ctl);
        if retry {
            self.load(cx);
        }
        if save {
            self.save(cx);
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_scan_finishes_when_nothing_is_scanning_any_more() {
        assert!(!scan_finished(false, false, 2.0, 30.0), "too early to tell");
        assert!(!scan_finished(true, true, 40.0, 30.0), "still scanning");
        assert!(scan_finished(true, false, 5.0, 30.0));
        assert!(scan_finished(false, false, 31.0, 30.0), "a quick scan never seen");
    }

    use super::*;

    fn offered() -> AvailableType {
        AvailableType {
            type_: "Movie".into(),
            metadata_fetchers: vec![
                FetcherInfo { name: "TheMovieDb".into(), default_enabled: true },
                FetcherInfo { name: "OMDb".into(), default_enabled: false },
            ],
            image_fetchers: vec![FetcherInfo { name: "TheMovieDb".into(), default_enabled: true }],
            default_image_options: vec![],
        }
    }

    #[test]
    fn content_labels() {
        assert_eq!(content_label(Some("tvshows")), "Shows");
        assert_eq!(content_label(None), "Mixed content");
        assert_eq!(content_label(Some("")), "Mixed content");
        assert_eq!(content_label(Some("boxsets")), "Collections");
        assert_eq!(content_label(Some("odd")), "odd");
    }

    #[test]
    fn fetchers_default_then_toggle_keeps_the_rest() {
        let o = offered();
        let mut lib = LibraryOptions::default();
        assert_eq!(effective_fetchers(&lib, "Movie", false, &o.metadata_fetchers), vec!["TheMovieDb".to_string()]);
        toggle_fetcher(&mut lib, &o, false, "OMDb");
        assert_eq!(lib.type_options.len(), 1);
        assert_eq!(
            effective_fetchers(&lib, "Movie", false, &o.metadata_fetchers),
            vec!["TheMovieDb".to_string(), "OMDb".to_string()]
        );
        // The image list was filled from the defaults too, so it did not turn into "none".
        assert_eq!(effective_fetchers(&lib, "Movie", true, &o.image_fetchers), vec!["TheMovieDb".to_string()]);
        toggle_fetcher(&mut lib, &o, false, "TheMovieDb");
        assert_eq!(effective_fetchers(&lib, "Movie", false, &o.metadata_fetchers), vec!["OMDb".to_string()]);
        toggle_fetcher(&mut lib, &o, true, "TheMovieDb");
        assert!(effective_fetchers(&lib, "Movie", true, &o.image_fetchers).is_empty());
        assert_eq!(lib.type_options.len(), 1);
    }

    #[test]
    fn nfo_switches() {
        let mut lib = LibraryOptions::default();
        assert!(!nfo_saver_on(&lib) && nfo_reader_on(&lib));
        set_nfo_saver(&mut lib, true);
        set_nfo_saver(&mut lib, true);
        assert_eq!(lib.metadata_savers, Some(vec!["Nfo".to_string()]));
        set_nfo_saver(&mut lib, false);
        assert!(!nfo_saver_on(&lib));
        set_nfo_reader(&mut lib, false);
        assert!(!nfo_reader_on(&lib));
        set_nfo_reader(&mut lib, true);
        assert!(nfo_reader_on(&lib));
    }

    #[test]
    fn refresh_choices() {
        assert_eq!(refresh_index(30), 1);
        assert_eq!(refresh_index(7), 0);
        assert_eq!(refresh_labels()[0], "Never");
        assert_eq!(refresh_labels()[3], "Every 90 days");
    }

    #[test]
    fn scan_status_text() {
        let mut f = VirtualFolderInfo::default();
        assert_eq!(scan_status(&f), "");
        f.refresh_status = Some("Idle".into());
        assert!(!is_scanning(&f));
        f.refresh_status = Some("Active".into());
        assert_eq!(scan_status(&f), "Scanning");
        f.refresh_progress = Some(41.6);
        assert_eq!(scan_status(&f), "Scanning 42%");
    }

    #[test]
    fn paths_and_config_values() {
        assert_eq!(clean_path("  /media/Movies "), Some("/media/Movies".to_string()));
        assert_eq!(clean_path("   "), None);
        let mut v = serde_json::json!({"A": true, "B": "x", "Keep": [1, 2]});
        assert!(cfg_bool(&v, "A") && !cfg_bool(&v, "Missing"));
        assert_eq!(cfg_str(&v, "B"), "x");
        cfg_set(&mut v, "A", Value::Bool(false));
        cfg_set(&mut v, "New", Value::String("y".into()));
        assert!(!cfg_bool(&v, "A"));
        assert_eq!(cfg_str(&v, "New"), "y");
        assert_eq!(v["Keep"], serde_json::json!([1, 2]));
    }

    #[test]
    fn types_follow_the_content_type() {
        assert_eq!(types_for("tvshows"), &["Series", "Season", "Episode"]);
        assert!(types_for("").is_empty());
        assert_eq!(type_label("MusicAlbum"), "Albums");
    }
}
