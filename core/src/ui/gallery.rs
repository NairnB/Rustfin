//! W0 widget gallery: every widget in a labelled box, one focus group per widget. The demo screen
//! (`name() = "gallery"`) is opened with `--screen gallery` and shows each widget working by keyboard
//! and mouse. Key routing for the dropdown, chips, tabs, number field and list goes through the pure
//! functions in `widgets`.
use super::{parts, widgets};
use crate::app::{Cx, Screen};
use crate::nav::{move_focus, Focus, Group};
use crate::platform::Key;
use crate::theme::Theme;

const G_DROP: usize = 0;
const G_TOGGLE: usize = 1;
const G_CHIPS: usize = 2;
const G_TABS: usize = 3;
const G_DIALOG: usize = 4;
const G_TEXT: usize = 5;
const G_NUMBER: usize = 6;
const G_LIST: usize = 7;
const G_FORM: usize = 8;
const G_DIALOG_BTNS: usize = 9;
const N_GROUPS: usize = 10;

const NUM_MIN: i64 = 0;
const NUM_MAX: i64 = 100;
const NUM_STEP: i64 = 5;

pub struct Gallery {
    dropdown: widgets::DropdownState,
    dropdown_options: Vec<String>,
    toggle_on: bool,
    chip_labels: Vec<String>,
    chips_on: Vec<bool>,
    tab_labels: Vec<String>,
    tab_selected: usize,
    text: String,
    number: i64,
    dialog_open: bool,
    dialog_focus: usize,
    dialog_buttons: Vec<String>,
    form_fields: Vec<(String, String)>,
    focus: Focus,
    groups: Vec<Group>,
}

pub fn screen() -> Box<dyn Screen> {
    Box::new(Gallery {
        dropdown: widgets::DropdownState::default(),
        dropdown_options: vec!["A to Z".into(), "Newest first".into(), "Recently added".into()],
        toggle_on: true,
        chip_labels: vec!["Films".into(), "TV".into(), "Kids".into(), "Music".into()],
        chips_on: vec![true, false, true, false],
        tab_labels: vec!["General".into(), "Playback".into(), "Subtitles".into()],
        tab_selected: 0,
        text: String::new(),
        number: 50,
        dialog_open: false,
        dialog_focus: 0,
        dialog_buttons: vec!["Cancel".into(), "OK".into()],
        form_fields: vec![("Display name".into(), "Tester".into()), ("Language".into(), "English".into())],
        focus: Focus { group: G_DROP, index: 0 },
        groups: vec![],
    })
}

impl Gallery {
    fn list_sections() -> Vec<(&'static str, Vec<widgets::ListRow<'static>>)> {
        vec![
            (
                "Media",
                vec![
                    widgets::ListRow { label: "Films", secondary: Some("12 items"), value: Some("12") },
                    widgets::ListRow { label: "Series", secondary: Some("4 shows"), value: Some("4") },
                    widgets::ListRow { label: "Music videos", secondary: None, value: Some("2") },
                ],
            ),
            (
                "Playback",
                vec![
                    widgets::ListRow { label: "Audio", secondary: Some("Default"), value: Some("Auto") },
                    widgets::ListRow { label: "Subtitles", secondary: None, value: Some("Off") },
                ],
            ),
        ]
    }

    fn list_rows_len() -> usize {
        Self::list_sections().iter().map(|(_, rows)| rows.len()).sum()
    }
}

impl Screen for Gallery {
    fn name(&self) -> &'static str {
        "gallery"
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        let _ = cx;
        if self.dialog_open {
            return match key {
                Key::Back => {
                    self.dialog_open = false;
                    true
                }
                Key::Left => {
                    self.dialog_focus = self.dialog_focus.saturating_sub(1);
                    true
                }
                Key::Right => {
                    self.dialog_focus = (self.dialog_focus + 1).min(self.dialog_buttons.len().saturating_sub(1));
                    true
                }
                Key::Enter => {
                    self.dialog_open = false;
                    true
                }
                _ => false,
            };
        }
        if self.dropdown.open {
            let (s, ev) = widgets::dropdown_key(self.dropdown, key, self.dropdown_options.len());
            self.dropdown = s;
            self.focus.index = if self.dropdown.open { self.dropdown.highlighted } else { 0 };
            if matches!(ev, Some(widgets::DropdownEvent::Pick(_)) | Some(widgets::DropdownEvent::Close)) {
                self.focus.index = 0;
            }
            return matches!(key, Key::Up | Key::Down | Key::Enter | Key::Back);
        }
        match key {
            Key::Enter => {
                match self.focus.group {
                    G_DROP => {
                        let (s, _) = widgets::dropdown_key(self.dropdown, key, self.dropdown_options.len());
                        self.dropdown = s;
                        self.focus.index = if self.dropdown.open { self.dropdown.highlighted } else { 0 };
                    }
                    G_TOGGLE => self.toggle_on = !self.toggle_on,
                    G_CHIPS => {
                        let (f, ev) = widgets::chips_key(self.focus.index, self.chip_labels.len(), Key::Enter);
                        self.focus.index = f;
                        if let Some(i) = ev {
                            if let Some(v) = self.chips_on.get_mut(i) {
                                *v = !*v;
                            }
                        }
                    }
                    G_DIALOG => {
                        self.dialog_open = true;
                        self.dialog_focus = self.dialog_buttons.len().saturating_sub(1);
                    }
                    _ => {}
                }
                true
            }
            Key::Back => false,
            Key::Up | Key::Down => {
                match self.focus.group {
                    G_LIST => {
                        let (f, _) = widgets::list_key(self.focus.index, Self::list_rows_len(), key);
                        self.focus.index = f;
                    }
                    G_NUMBER => self.number = widgets::number_key(self.number, key, NUM_MIN, NUM_MAX, NUM_STEP),
                    _ => self.focus = move_focus(&self.groups, self.focus, key),
                }
                true
            }
            Key::Left | Key::Right => {
                match self.focus.group {
                    G_CHIPS => {
                        let (f, _) = widgets::chips_key(self.focus.index, self.chip_labels.len(), key);
                        self.focus.index = f;
                    }
                    G_TABS => self.tab_selected = widgets::tabs_key(self.tab_selected, self.tab_labels.len(), key),
                    _ => self.focus = move_focus(&self.groups, self.focus, key),
                }
                true
            }
            Key::Text(_) => {
                if self.focus.group == G_NUMBER {
                    self.number = widgets::number_key(self.number, key, NUM_MIN, NUM_MAX, NUM_STEP);
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        let mut groups = vec![Group::default(); N_GROUPS];

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(t.gap);
            parts::title(ui, t, "Widget gallery");
            ui.add_space(t.gap);

            // The hero buttons at rest (not in a focus group here; they are focusable on Home and details).
            boxed(ui, t, "Buttons", |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = t.gap / 2.0;
                    widgets::play_pill(ui, t, "Play", false);
                    widgets::pill_button(ui, t, "Trailer", false, false);
                    for i in [widgets::Icon::Plus, widgets::Icon::Tick, widgets::Icon::More] {
                        widgets::round_button(ui, t, i, false);
                    }
                });
            });

            boxed(ui, t, "Dropdown / menu", |ui| {
                let out = widgets::dropdown(
                    ui,
                    t,
                    &mut self.dropdown,
                    "Sort",
                    &self.dropdown_options,
                    self.focus.group == G_DROP,
                );
                if out.button_hovered && !self.dropdown.open {
                    self.focus = Focus { group: G_DROP, index: 0 };
                }
                if let Some(i) = out.item_hovered {
                    self.focus = Focus { group: G_DROP, index: i };
                }
                if let Some(i) = out.item_clicked {
                    self.dropdown.highlighted = i;
                    self.dropdown.open = false;
                }
                groups[G_DROP] = if self.dropdown.open {
                    Group { len: self.dropdown_options.len(), cols: 1, xs: out.item_xs }
                } else {
                    Group { len: 1, cols: 1, xs: vec![out.button_x] }
                };
            });

            boxed(ui, t, "Toggle", |ui| {
                let out = widgets::toggle(
                    ui,
                    t,
                    "Play next episode automatically",
                    self.toggle_on,
                    self.focus.group == G_TOGGLE,
                );
                if out.hovered {
                    self.focus = Focus { group: G_TOGGLE, index: 0 };
                }
                if out.clicked {
                    self.toggle_on = !self.toggle_on;
                }
                groups[G_TOGGLE] = Group { len: 1, cols: 1, xs: vec![out.x] };
            });

            boxed(ui, t, "Chip row (filters)", |ui| {
                let out = widgets::chips(
                    ui,
                    t,
                    &self.chip_labels,
                    &self.chips_on,
                    (self.focus.group == G_CHIPS).then_some(self.focus.index),
                );
                if let Some(i) = out.hovered {
                    self.focus = Focus { group: G_CHIPS, index: i };
                }
                if let Some(i) = out.clicked {
                    if let Some(v) = self.chips_on.get_mut(i) {
                        *v = !*v;
                    }
                }
                groups[G_CHIPS] = Group { len: self.chip_labels.len(), cols: self.chip_labels.len(), xs: out.xs };
            });

            boxed(ui, t, "Tabs", |ui| {
                let out = widgets::tabs(
                    ui,
                    t,
                    &self.tab_labels,
                    self.tab_selected,
                    (self.focus.group == G_TABS).then_some(self.focus.index),
                );
                if let Some(i) = out.hovered {
                    self.focus = Focus { group: G_TABS, index: i };
                }
                if let Some(i) = out.clicked {
                    self.tab_selected = i;
                }
                groups[G_TABS] = Group { len: self.tab_labels.len(), cols: self.tab_labels.len(), xs: out.xs };
            });

            boxed(ui, t, "Dialog / confirm", |ui| {
                let r = widgets::pill_button(ui, t, "Show dialog", self.focus.group == G_DIALOG, false);
                if r.hovered() {
                    self.focus = Focus { group: G_DIALOG, index: 0 };
                }
                if r.clicked() {
                    self.dialog_open = true;
                    self.dialog_focus = self.dialog_buttons.len().saturating_sub(1);
                }
                groups[G_DIALOG] = Group { len: 1, cols: 1, xs: vec![r.rect.center().x] };
            });

            boxed(ui, t, "Text field", |ui| {
                let r = widgets::text_field(ui, t, &mut self.text, "Type here", self.focus.group == G_TEXT, false);
                if r.hovered() {
                    self.focus = Focus { group: G_TEXT, index: 0 };
                }
                if r.clicked() {
                    self.focus = Focus { group: G_TEXT, index: 0 };
                }
                groups[G_TEXT] = Group { len: 1, cols: 1, xs: vec![r.rect.center().x] };
            });

            boxed(ui, t, "Number field (digits only, min/max, Up/Down step)", |ui| {
                let r = widgets::number_field(ui, t, self.number, self.focus.group == G_NUMBER);
                if r.hovered() {
                    self.focus = Focus { group: G_NUMBER, index: 0 };
                }
                if r.clicked() {
                    self.focus = Focus { group: G_NUMBER, index: 0 };
                }
                groups[G_NUMBER] = Group { len: 1, cols: 1, xs: vec![r.rect.center().x] };
            });

            boxed(ui, t, "List with sections", |ui| {
                let out = widgets::list(
                    ui,
                    t,
                    &Self::list_sections(),
                    (self.focus.group == G_LIST).then_some(self.focus.index),
                );
                if let Some(i) = out.hovered {
                    self.focus = Focus { group: G_LIST, index: i };
                }
                groups[G_LIST] = Group { len: Self::list_rows_len(), cols: 1, xs: out.xs };
            });

            boxed(ui, t, "Form layout", |ui| {
                let n = self.form_fields.len();
                let focus = if self.focus.group == G_FORM { Some(self.focus.index) } else { None };
                let out = widgets::form(ui, t, "Account", &mut self.form_fields, focus);
                if let Some(i) = out.field_hovered {
                    self.focus = Focus { group: G_FORM, index: i };
                }
                if let Some(i) = out.button_hovered {
                    self.focus = Focus { group: G_FORM, index: n + i };
                }
                let mut xs = out.field_xs;
                xs.extend(out.footer_xs);
                groups[G_FORM] = Group { len: n + 2, cols: 1, xs };
            });

            ui.add_space(t.gap * 2.0);
        });

        self.groups = groups;
        if !self.dialog_open {
            self.focus = move_focus(&self.groups, self.focus, Key::Enter);
        }

        if self.dialog_open {
            let out = widgets::dialog(
                ui,
                t,
                "Sign out?",
                "You will need to sign in again to use Rustfin.",
                &self.dialog_buttons,
                Some(self.dialog_focus),
            );
            if let Some(i) = out.hovered {
                self.dialog_focus = i;
            }
            if out.clicked.is_some() {
                self.dialog_open = false;
            }
            self.groups[G_DIALOG_BTNS] =
                Group { len: self.dialog_buttons.len(), cols: self.dialog_buttons.len(), xs: out.button_xs };
        }
    }
}

/// A labelled surface box: heading, then the content in a `surface`-filled frame.
fn boxed(ui: &mut egui::Ui, t: &Theme, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    parts::heading(ui, t, label);
    ui.add_space(t.gap / 3.0);
    let frame = egui::Frame::NONE.fill(t.surface).corner_radius(t.section_radius);
    frame.inner_margin(egui::Margin::same(t.gap as i8)).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        add(ui);
    });
    ui.add_space(t.gap);
}
