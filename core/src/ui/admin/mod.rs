//! Admin area (packets A1..A5). The home screen (`name() = "admin"`) is a list with sections;
//! every row opens its own screen.
pub mod activity;
pub mod dashboard;
pub mod item_more;
pub mod libraries;
pub mod server;
pub mod system;
pub mod tasks;
pub mod users;

use super::parts;
use super::widgets::ListRow;
use crate::app::{Cx, Screen};
use crate::nav::{move_focus, Focus, Group};
use crate::platform::Key;
use crate::theme::Theme;

/// One admin-home row and what it opens.
#[derive(Clone, Copy)]
enum Target {
    Dashboard,
    Tasks,
    Activity,
    Users,
    Libraries,
    LibraryDisplay,
    LibraryMetadata,
    LibraryNfo,
    General,
    Branding,
    Networking,
    Transcoding,
    Resume,
    Streaming,
    Trickplay,
    Devices,
    Plugins,
    Catalogue,
    Repositories,
    Logs,
    Keys,
}

/// The single source of the admin home: section heading → rows (label, target). The flattened row order
/// across all sections is the focus order and the index `activate` is given.
const MENU: [(&str, &[(&str, Target)]); 10] = [
    (
        "Server",
        &[
            ("Dashboard", Target::Dashboard),
            ("General", Target::General),
            ("Branding", Target::Branding),
            ("Networking", Target::Networking),
        ],
    ),
    ("Users", &[("Users", Target::Users)]),
    (
        "Libraries",
        &[
            ("Libraries", Target::Libraries),
            ("Display", Target::LibraryDisplay),
            ("Metadata", Target::LibraryMetadata),
            ("NFO settings", Target::LibraryNfo),
        ],
    ),
    (
        "Playback",
        &[
            ("Transcoding", Target::Transcoding),
            ("Resume", Target::Resume),
            ("Streaming", Target::Streaming),
            ("Trickplay", Target::Trickplay),
        ],
    ),
    ("Devices", &[("Devices", Target::Devices)]),
    ("Activity", &[("Activity", Target::Activity)]),
    ("Scheduled tasks", &[("Scheduled tasks", Target::Tasks)]),
    (
        "Plugins",
        &[("Plugins", Target::Plugins), ("Catalogue", Target::Catalogue), ("Repositories", Target::Repositories)],
    ),
    ("Logs", &[("Logs", Target::Logs)]),
    ("API keys", &[("API keys", Target::Keys)]),
];

pub struct AdminHome {
    focus: Focus,
    groups: Vec<Group>,
}

pub fn screen() -> Box<dyn Screen> {
    Box::new(AdminHome { focus: Focus { group: 0, index: 0 }, groups: vec![] })
}

impl AdminHome {
    fn sections() -> Vec<(&'static str, Vec<ListRow<'static>>)> {
        MENU.iter()
            .map(|(heading, rows)| {
                let rows = rows.iter().map(|(label, _)| ListRow { label, secondary: None, value: None }).collect();
                (*heading, rows)
            })
            .collect()
    }

    fn flat() -> Vec<(&'static str, Target)> {
        MENU.iter().flat_map(|(_, rows)| rows.iter().copied()).collect()
    }

    fn activate(&self, idx: usize, cx: &mut Cx) {
        let flat = Self::flat();
        let Some((_, target)) = flat.get(idx) else { return };
        match target {
            Target::Dashboard => cx.push(dashboard::screen()),
            Target::Tasks => cx.push(tasks::screen()),
            Target::Activity => cx.push(activity::screen()),
            Target::Users => cx.push(users::screen()),
            Target::Libraries => cx.push(libraries::screen()),
            Target::LibraryDisplay => cx.push(libraries::display()),
            Target::LibraryMetadata => cx.push(libraries::metadata()),
            Target::LibraryNfo => cx.push(libraries::nfo()),
            Target::General => cx.push(server::general()),
            Target::Branding => cx.push(server::branding()),
            Target::Networking => cx.push(server::networking()),
            Target::Transcoding => cx.push(server::transcoding()),
            Target::Resume => cx.push(server::resume()),
            Target::Streaming => cx.push(server::streaming()),
            Target::Trickplay => cx.push(server::trickplay()),
            Target::Devices => cx.push(system::devices()),
            Target::Plugins => cx.push(system::plugins()),
            Target::Catalogue => cx.push(system::catalogue()),
            Target::Repositories => cx.push(system::repositories()),
            Target::Logs => cx.push(system::logs()),
            Target::Keys => cx.push(system::keys()),
        }
    }
}

impl Screen for AdminHome {
    fn name(&self) -> &'static str {
        "admin"
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        match key {
            Key::Enter => {
                self.activate(self.focus.index, cx);
                true
            }
            Key::Up | Key::Down | Key::Left | Key::Right => {
                self.focus = move_focus(&self.groups, self.focus, key);
                true
            }
            _ => false,
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        let pointer_moved = ui.input(|i| i.pointer.is_moving());
        ui.add_space(t.margin);
        parts::title(ui, t, "Server admin");
        ui.add_space(t.gap / 2.0);

        let sections = Self::sections();
        let mut xs = vec![];
        let mut hovered = None;
        let mut clicked = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            let out = sectioned_list(ui, t, &sections, Some(self.focus.index));
            xs = out.xs;
            hovered = out.hovered;
            clicked = out.clicked;
        });

        self.groups = vec![Group { len: Self::flat().len(), cols: 1, xs }];
        self.focus = move_focus(&self.groups, self.focus, Key::Enter);
        if pointer_moved {
            if let Some(i) = hovered {
                self.focus.index = i;
            }
        }
        if let Some(i) = clicked {
            self.focus.index = i;
            self.activate(i, cx);
        }
    }
}

pub(super) struct SectionsOut {
    pub xs: Vec<f32>,
    pub hovered: Option<usize>,
    pub clicked: Option<usize>,
}

/// A list with sections drawn like `widgets::list` (grouped cards), but it also scrolls the focused row into
/// view (the admin lists are longer than one screen). Focus indexes the flattened rows across all sections.
pub(super) fn sectioned_list(
    ui: &mut egui::Ui,
    t: &Theme,
    sections: &[(&str, Vec<ListRow<'_>>)],
    focus: Option<usize>,
) -> SectionsOut {
    let o = crate::ui::widgets::list_scrolling(ui, t, sections, focus);
    SectionsOut { xs: o.xs, hovered: o.hovered, clicked: o.clicked }
}
