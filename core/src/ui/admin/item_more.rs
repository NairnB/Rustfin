//! A6 "More…" on an item page (P-ITEM-20, P-ADM-27, P-ITEM-12): Edit metadata, Identify, Refresh metadata,
//! Merge / Split versions, Copy link, Delete. A non-admin only gets Copy link. One screen with a few modes;
//! Back inside a mode returns to the menu, Back on the menu leaves.
use super::users::{fail, skeleton_rows, Ctl};
use crate::api::item_edit::{
    join_list, parse_person, parse_rating, parse_year, person_line, split_list, web_link, ItemEdit, PersonEdit,
    RefreshMode, RemoteMatch,
};
use crate::api::{ApiError, Item, Result};
use crate::app::{Cx, Screen};
use crate::nav::{move_focus, Focus, Group};
use crate::platform::Key;
use crate::ui::admin_actions::{identify_for, more_entries, MoreAction};
use crate::ui::parts;
use crate::ui::widgets;
use serde_json::Value;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Menu,
    Edit,
    Refresh,
    Identify,
    Merge,
}

/// Editor text, one string per field.
#[derive(Default)]
struct Form {
    base: ItemEdit,
    name: String,
    sort: String,
    year: String,
    overview: String,
    genres: String,
    tags: String,
    official: String,
    rating: String,
    people: Vec<String>,
}

impl Form {
    fn from_edit(e: ItemEdit) -> Form {
        Form {
            name: e.name.clone(),
            sort: e.sort_name.clone(),
            year: e.year.map(|y| y.to_string()).unwrap_or_default(),
            overview: e.overview.clone(),
            genres: join_list(&e.genres),
            tags: join_list(&e.tags),
            official: e.official_rating.clone(),
            rating: e.community_rating.map(|r| r.to_string()).unwrap_or_default(),
            people: e.people.iter().map(person_line).collect(),
            base: e,
        }
    }

    /// The edit the fields describe, or what is wrong with them.
    fn to_edit(&self) -> std::result::Result<ItemEdit, String> {
        if self.name.trim().is_empty() {
            return Err("The title can't be empty.".into());
        }
        let people: Vec<PersonEdit> = self.people.iter().filter_map(|l| parse_person(l)).collect();
        Ok(ItemEdit {
            name: self.name.trim().to_string(),
            sort_name: self.sort.trim().to_string(),
            year: parse_year(&self.year)?,
            overview: self.overview.clone(),
            genres: split_list(&self.genres),
            tags: split_list(&self.tags),
            rf_tags: self.base.rf_tags.clone(),
            official_rating: self.official.trim().to_string(),
            community_rating: parse_rating(&self.rating)?,
            people,
        })
    }
}

enum Act {
    Do(MoreAction),
    SaveEdit,
    AddPerson,
    Refresh(RefreshMode),
    Search,
    Apply(usize),
    DoMerge,
}

pub fn screen(item: Item, is_admin: bool) -> Box<dyn Screen> {
    Box::new(More {
        item,
        is_admin,
        mode: Mode::Menu,
        focus: Focus { group: 0, index: 0 },
        groups: vec![],
        enter: false,
        on_field: false,
        busy: false,
        error: None,
        notice: None,
        confirm: None,
        confirm_focus: 0,
        form: None,
        replace_images: false,
        id_name: String::new(),
        id_year: String::new(),
        matches: None,
        cands: None,
        picked: vec![],
        copy: None,
    })
}

struct More {
    item: Item,
    is_admin: bool,
    mode: Mode,
    focus: Focus,
    groups: Vec<Group>,
    enter: bool,
    on_field: bool,
    busy: bool,
    error: Option<String>,
    notice: Option<String>,
    /// Delete / Split waiting for a yes.
    confirm: Option<MoreAction>,
    confirm_focus: usize,
    form: Option<Form>,
    replace_images: bool,
    id_name: String,
    id_year: String,
    matches: Option<Vec<RemoteMatch>>,
    cands: Option<Vec<Item>>,
    picked: Vec<bool>,
    /// Link to put on the clipboard at the next frame.
    copy: Option<String>,
}

impl More {
    fn go(&mut self, mode: Mode) {
        self.mode = mode;
        self.focus = Focus { group: 0, index: 0 };
        self.error = None;
        self.notice = None;
    }

    fn start(&mut self, a: MoreAction, cx: &mut Cx) {
        let (c, id) = (cx.api(), self.item.id.clone());
        match a {
            MoreAction::Edit => {
                self.form = None;
                self.go(Mode::Edit);
                cx.spawn("load", move || c.item_json(&id));
            }
            MoreAction::Identify => {
                self.go(Mode::Identify);
                self.id_name = self.item.name.clone();
                self.id_year = self.item.production_year.map(|y| y.to_string()).unwrap_or_default();
                self.matches = None;
            }
            MoreAction::Refresh => self.go(Mode::Refresh),
            MoreAction::Merge => {
                self.cands = None;
                self.picked.clear();
                self.go(Mode::Merge);
                let name = self.item.name.clone();
                cx.spawn("cands", move || c.merge_candidates(&id, &name));
            }
            MoreAction::Split | MoreAction::Delete => {
                self.confirm = Some(a);
                self.confirm_focus = 0;
            }
            MoreAction::CopyLink => {
                let link = web_link(&c.base, &id, None);
                self.error = None;
                self.notice = Some(format!("Link copied: {link}"));
                self.copy = Some(link);
            }
        }
    }

    fn run(&mut self, act: Act, cx: &mut Cx) {
        let (c, id) = (cx.api(), self.item.id.clone());
        match act {
            Act::Do(a) => self.start(a, cx),
            Act::SaveEdit => {
                let Some(form) = &self.form else { return };
                match form.to_edit() {
                    Ok(edit) => {
                        self.busy = true;
                        self.error = None;
                        cx.spawn("saved", move || c.save_item_edit(&id, &edit));
                    }
                    Err(e) => self.error = Some(e),
                }
            }
            Act::AddPerson => {
                if let Some(f) = &mut self.form {
                    f.people.push(String::new());
                }
            }
            Act::Refresh(mode) => {
                self.busy = true;
                let images = self.replace_images;
                cx.spawn("refreshed", move || c.refresh_item(&id, mode, images));
            }
            Act::Search => {
                let Some(kind) = identify_for(&self.item.kind) else { return };
                let year = match parse_year(&self.id_year) {
                    Ok(y) => y,
                    Err(e) => {
                        self.error = Some(e);
                        return;
                    }
                };
                self.busy = true;
                self.error = None;
                self.matches = None;
                let name = self.id_name.trim().to_string();
                cx.spawn("search", move || c.remote_search(kind, &id, &name, year));
            }
            Act::Apply(i) => {
                let Some(m) = self.matches.as_ref().and_then(|v| v.get(i)).cloned() else { return };
                self.busy = true;
                cx.spawn("applied", move || c.apply_remote_match(&id, &m, true));
            }
            Act::DoMerge => {
                let Some(cands) = &self.cands else { return };
                let mut ids = vec![id];
                ids.extend(cands.iter().zip(&self.picked).filter(|(_, p)| **p).map(|(i, _)| i.id.clone()));
                if ids.len() < 2 {
                    return;
                }
                self.busy = true;
                cx.spawn("merged", move || c.merge_versions(&ids));
            }
        }
    }

    fn confirm_key(&mut self, cx: &mut Cx, key: Key) {
        match key {
            Key::Back => self.confirm = None,
            Key::Left => self.confirm_focus = self.confirm_focus.saturating_sub(1),
            Key::Right => self.confirm_focus = (self.confirm_focus + 1).min(1),
            Key::Enter => {
                let (what, go) = (self.confirm.take(), self.confirm_focus == 1);
                if go {
                    self.confirmed(what, cx);
                }
            }
            _ => {}
        }
    }

    fn confirmed(&mut self, what: Option<MoreAction>, cx: &mut Cx) {
        let (c, id) = (cx.api(), self.item.id.clone());
        self.busy = true;
        match what {
            Some(MoreAction::Delete) => cx.spawn("deleted", move || c.delete_item(&id)),
            Some(MoreAction::Split) => cx.spawn("split", move || c.split_versions(&id)),
            _ => self.busy = false,
        }
    }

    fn menu(&self, ui: &mut egui::Ui, t: &crate::theme::Theme, ctl: &mut Ctl, acts: &mut Vec<Act>) {
        let sources = self.item.media_sources.len();
        for (label, a) in more_entries(&self.item.kind, self.is_admin, sources) {
            ui.add_space(t.gap / 3.0);
            if ctl.button(ui, t, label, false) {
                acts.push(Act::Do(a));
            }
        }
    }

    fn edit(&mut self, ui: &mut egui::Ui, t: &crate::theme::Theme, ctl: &mut Ctl, acts: &mut Vec<Act>) {
        let Some(f) = &mut self.form else {
            skeleton_rows(ui, t, 6);
            return;
        };
        let rows: [(&str, &mut String); 8] = [
            ("Title", &mut f.name),
            ("Sort title", &mut f.sort),
            ("Year", &mut f.year),
            ("Overview", &mut f.overview),
            ("Genres (comma separated)", &mut f.genres),
            ("Tags (comma separated)", &mut f.tags),
            ("Official rating", &mut f.official),
            ("Community rating (0–10)", &mut f.rating),
        ];
        for (label, text) in rows {
            parts::muted(ui, t, label);
            ctl.field(ui, t, text, label, false);
            ui.add_space(t.gap / 3.0);
        }
        if !f.base.rf_tags.is_empty() {
            parts::muted(ui, t, &format!("Rustfin tags (read-only): {}", f.base.rf_tags.join(", ")));
            ui.add_space(t.gap / 3.0);
        }
        parts::muted(ui, t, "People (Name | Type | Role — clear a line to remove)");
        for p in &mut f.people {
            ctl.field(ui, t, p, "Name | Type | Role", false);
            ui.add_space(t.gap / 4.0);
        }
        if ctl.button(ui, t, "Add person", false) {
            acts.push(Act::AddPerson);
        }
        ui.add_space(t.gap / 2.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = t.gap / 2.0;
            let label = if self.busy { "Saving…" } else { "Save" };
            if ctl.button(ui, t, label, true) {
                acts.push(Act::SaveEdit);
            }
            if ctl.button(ui, t, "Cancel", false) {
                self.mode = Mode::Menu;
            }
        });
    }

    fn refresh(&mut self, ui: &mut egui::Ui, t: &crate::theme::Theme, ctl: &mut Ctl, acts: &mut Vec<Act>) {
        parts::muted(ui, t, "The server does the work in the background; changes show when it finishes.");
        ui.add_space(t.gap / 2.0);
        if ctl.toggle(ui, t, "Replace all images", self.replace_images) {
            self.replace_images ^= true;
        }
        for (label, mode) in [
            ("Scan for new and updated files", RefreshMode::Scan),
            ("Search for missing metadata", RefreshMode::Missing),
            ("Replace all metadata", RefreshMode::Replace),
        ] {
            ui.add_space(t.gap / 3.0);
            if ctl.button(ui, t, label, false) {
                acts.push(Act::Refresh(mode));
            }
        }
    }

    fn identify(&mut self, ui: &mut egui::Ui, t: &crate::theme::Theme, ctl: &mut Ctl, acts: &mut Vec<Act>) {
        parts::muted(ui, t, "Name");
        ctl.field(ui, t, &mut self.id_name, "Name", false);
        ui.add_space(t.gap / 3.0);
        parts::muted(ui, t, "Year");
        ctl.field(ui, t, &mut self.id_year, "Year", false);
        ui.add_space(t.gap / 2.0);
        let label = if self.busy { "Searching…" } else { "Search" };
        if ctl.button(ui, t, label, true) {
            acts.push(Act::Search);
        }
        ui.add_space(t.gap / 2.0);
        match &self.matches {
            None => {}
            Some(m) if m.is_empty() => {
                parts::muted(ui, t, "No matches.");
                if let Some(n) = &self.notice {
                    parts::muted(ui, t, n);
                }
            }
            Some(m) => {
                parts::muted(ui, t, "Pick the right one to apply it (images are replaced too).");
                for (i, r) in m.iter().enumerate() {
                    ui.add_space(t.gap / 3.0);
                    if ctl.button(ui, t, &r.label(), false) {
                        acts.push(Act::Apply(i));
                    }
                }
            }
        }
    }

    fn merge(&mut self, ui: &mut egui::Ui, t: &crate::theme::Theme, ctl: &mut Ctl, acts: &mut Vec<Act>) {
        let Some(cands) = &self.cands else {
            skeleton_rows(ui, t, 3);
            return;
        };
        if cands.is_empty() {
            parts::muted(ui, t, "No other films with this title.");
            return;
        }
        parts::muted(ui, t, "Tick the films to merge into this one as extra versions.");
        for (i, c) in cands.iter().enumerate() {
            let year = c.production_year.map(|y| format!(" ({y})")).unwrap_or_default();
            ui.add_space(t.gap / 3.0);
            if ctl.toggle(ui, t, &format!("{}{year}", c.name), self.picked[i]) {
                self.picked[i] ^= true;
            }
        }
        ui.add_space(t.gap / 2.0);
        if self.picked.iter().any(|p| *p) {
            let label = if self.busy { "Merging…" } else { "Merge" };
            if ctl.button(ui, t, label, true) {
                acts.push(Act::DoMerge);
            }
        }
    }
}

impl Screen for More {
    fn name(&self) -> &'static str {
        match self.mode {
            Mode::Menu => "more",
            Mode::Edit => "metadata-edit",
            Mode::Refresh => "metadata-refresh",
            Mode::Identify => "identify",
            Mode::Merge => "merge-versions",
        }
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        if self.confirm.is_some() {
            self.confirm_key(cx, key);
            return true;
        }
        match key {
            Key::Enter => {
                self.enter = true;
                true
            }
            Key::Left | Key::Right if self.on_field => true,
            Key::Up | Key::Down | Key::Left | Key::Right => {
                self.focus = move_focus(&self.groups, self.focus, key);
                true
            }
            Key::Back if self.mode != Mode::Menu => {
                self.go(Mode::Menu);
                true
            }
            _ => false,
        }
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn std::any::Any + Send>) {
        self.busy = false;
        match tag {
            "load" => {
                let Ok(r) = payload.downcast::<Result<Value>>() else { return };
                match *r {
                    Ok(v) => self.form = Some(Form::from_edit(ItemEdit::from_json(&v))),
                    Err(e) => self.error = fail(cx, e),
                }
            }
            "cands" => {
                let Ok(r) = payload.downcast::<Result<Vec<Item>>>() else { return };
                match *r {
                    Ok(v) => {
                        self.picked = vec![false; v.len()];
                        self.cands = Some(v);
                    }
                    Err(e) => self.error = fail(cx, e),
                }
            }
            "search" => {
                let Ok(r) = payload.downcast::<Result<Vec<RemoteMatch>>>() else { return };
                match *r {
                    Ok(v) => self.matches = Some(v),
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    // A server with no internet can't search: say "No matches", with the reason underneath.
                    Err(e) => {
                        self.matches = Some(vec![]);
                        self.notice = Some(format!("The server could not search its metadata providers ({e})."));
                    }
                }
            }
            "refreshed" => {
                let Ok(r) = payload.downcast::<Result<()>>() else { return };
                match *r {
                    Ok(()) => {
                        self.go(Mode::Menu);
                        self.notice = Some("Refresh started. It runs on the server.".into());
                    }
                    Err(e) => self.error = fail(cx, e),
                }
            }
            "saved" | "applied" | "merged" | "split" | "deleted" => {
                let Ok(r) = payload.downcast::<Result<()>>() else { return };
                match *r {
                    Ok(()) => {
                        cx.pop();
                        // A deleted item has no page to go back to.
                        if tag == "deleted" {
                            cx.pop();
                        }
                    }
                    Err(e) => self.error = fail(cx, e),
                }
            }
            _ => {}
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        let moving = ui.input(|i| i.pointer.is_moving());
        let enter = std::mem::take(&mut self.enter) && self.confirm.is_none();
        let mut ctl = Ctl::new(Some(self.focus.index), enter, moving);
        let mut acts: Vec<Act> = vec![];
        if let Some(link) = self.copy.take() {
            ui.ctx().copy_text(link);
        }

        ui.add_space(t.margin);
        let title = match self.mode {
            Mode::Menu => "More",
            Mode::Edit => "Edit metadata",
            Mode::Refresh => "Refresh metadata",
            Mode::Identify => "Identify",
            Mode::Merge => "Merge versions",
        };
        parts::title(ui, t, title);
        parts::muted(ui, t, &self.item.name.clone());
        ui.add_space(t.gap / 2.0);
        if let Some(e) = &self.error {
            parts::error_line(ui, t, e);
            ui.add_space(t.gap / 3.0);
        }
        if let (Some(n), true) = (&self.notice, self.mode != Mode::Identify) {
            parts::muted(ui, t, n);
            ui.add_space(t.gap / 3.0);
        }
        egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| match self.mode {
            Mode::Menu => self.menu(ui, t, &mut ctl, &mut acts),
            Mode::Edit => self.edit(ui, t, &mut ctl, &mut acts),
            Mode::Refresh => self.refresh(ui, t, &mut ctl, &mut acts),
            Mode::Identify => self.identify(ui, t, &mut ctl, &mut acts),
            Mode::Merge => self.merge(ui, t, &mut ctl, &mut acts),
        });

        self.on_field = ctl.on_field;
        self.groups = vec![Group { len: ctl.idx, cols: 1, xs: ctl.xs.clone() }];
        self.focus = move_focus(&self.groups, self.focus, Key::Enter);
        if let Some(i) = ctl.hover.or(ctl.clicked) {
            self.focus = Focus { group: 0, index: i };
        }
        if ctl.advance {
            self.focus = move_focus(&self.groups, self.focus, Key::Down);
        }
        for a in acts {
            self.run(a, cx);
        }

        if let Some(what) = self.confirm {
            let (title, body, yes) = match what {
                MoreAction::Delete => {
                    ("Delete", format!("Delete {} from the library? This can't be undone.", self.item.name), "Delete")
                }
                _ => ("Split versions", format!("Split {} back into separate films?", self.item.name), "Split"),
            };
            let buttons = vec!["Cancel".to_string(), yes.to_string()];
            let out = widgets::dialog(ui, t, title, &body, &buttons, Some(self.confirm_focus));
            if let Some(i) = out.hovered.filter(|_| moving) {
                self.confirm_focus = i;
            }
            if let Some(i) = out.clicked {
                self.confirm = None;
                if i == 1 {
                    self.confirmed(Some(what), cx);
                }
            }
        }
    }
}
