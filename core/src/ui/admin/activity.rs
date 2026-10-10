//! Activity log (P-ADM-22): newest first, paged. Each row shows the name, severity and date.
use super::sectioned_list;
use crate::api::admin::ActivityLogPage;
use crate::api::{ApiError, Result};
use crate::app::{Cx, Screen};
use crate::nav::{move_focus, Focus, Group};
use crate::platform::Key;
use crate::ui::parts;
use crate::ui::widgets::ListRow;

const PAGE: usize = 50;
const G_LIST: usize = 0;
const G_MORE: usize = 1;

pub struct Activity {
    entries: Vec<crate::api::gen_admin::ActivityLogEntry>,
    total: i32,
    next_start: usize,
    in_flight: bool,
    error: Option<String>,
    started: bool,
    focus: Focus,
    groups: Vec<Group>,
}

pub fn screen() -> Box<dyn Screen> {
    Box::new(Activity {
        entries: vec![],
        total: 0,
        next_start: 0,
        in_flight: false,
        error: None,
        started: false,
        focus: Focus { group: G_LIST, index: 0 },
        groups: vec![],
    })
}

impl Activity {
    fn load_more(&mut self, cx: &Cx) {
        if self.in_flight {
            return;
        }
        self.in_flight = true;
        let start = self.next_start;
        let c = cx.api();
        cx.spawn("page", move || c.activity_log(start, PAGE));
    }
}

impl Screen for Activity {
    fn name(&self) -> &'static str {
        "activity"
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        match key {
            Key::Enter => {
                if self.focus.group == G_MORE {
                    self.load_more(cx);
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
        if tag != "page" {
            return;
        }
        self.in_flight = false;
        let Ok(r) = payload.downcast::<Result<ActivityLogPage>>() else { return };
        match *r {
            Ok(page) => {
                self.total = page.total_record_count;
                self.entries.extend(page.items);
                self.next_start = self.entries.len();
            }
            Err(ApiError::Unauthorized) => cx.signed_out(),
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        if !self.started {
            self.started = true;
            self.load_more(cx);
        }
        let pointer_moved = ui.input(|i| i.pointer.is_moving());

        ui.add_space(t.margin);
        parts::title(ui, t, "Activity log");
        parts::muted(ui, t, &format!("{total} entries", total = self.total));
        ui.add_space(t.gap / 2.0);

        if let Some(e) = self.error.clone() {
            parts::error_line(ui, t, &e);
            let r = parts::button(ui, t, "Retry", false, false);
            if r.clicked() {
                self.error = None;
                self.load_more(cx);
            }
            self.groups = vec![];
            return;
        }

        let mut labels: Vec<String> = vec![];
        let mut sevs: Vec<String> = vec![];
        let mut dates: Vec<String> = vec![];
        for e in &self.entries {
            labels.push(if e.name.is_empty() { "Activity".to_string() } else { e.name.clone() });
            sevs.push(e.severity.clone());
            dates.push(short_date(&e.date));
        }
        let mut rows: Vec<ListRow<'_>> = vec![];
        for i in 0..self.entries.len() {
            rows.push(ListRow { label: &labels[i], secondary: Some(&sevs[i]), value: Some(&dates[i]) });
        }
        let sections: Vec<(&str, Vec<ListRow<'_>>)> = vec![("", rows)];

        let mut list_xs = vec![];
        let mut hovered = None;
        let mut clicked = None;
        let list_h = (ui.available_height() - t.button_h - t.gap).max(t.list_row_h);
        egui::ScrollArea::vertical().max_height(list_h).auto_shrink([false, true]).show(ui, |ui| {
            let out = sectioned_list(ui, t, &sections, (self.focus.group == G_LIST).then_some(self.focus.index));
            list_xs = out.xs;
            hovered = out.hovered;
            clicked = out.clicked;
        });

        let more_x = if self.next_start < self.total as usize {
            ui.add_space(t.gap / 2.0);
            let label = if self.in_flight { "Loading…" } else { "More" };
            let r = parts::button(ui, t, label, self.focus.group == G_MORE, false);
            let x = r.rect.center().x;
            if r.hovered() {
                self.focus = Focus { group: G_MORE, index: 0 };
            }
            if r.clicked() {
                self.load_more(cx);
            }
            vec![x]
        } else {
            vec![]
        };

        self.groups = vec![
            Group { len: self.entries.len(), cols: 1, xs: list_xs },
            Group { len: more_x.len(), cols: 1, xs: more_x },
        ];
        self.focus = move_focus(&self.groups, self.focus, Key::Enter);

        if pointer_moved {
            if let Some(i) = hovered {
                self.focus = Focus { group: G_LIST, index: i };
            }
        }
        let _ = clicked;
    }
}

/// "2026-10-10T17:00:00.0000000Z" → "2026-10-10 17:00".
fn short_date(iso: &str) -> String {
    let mut s: String = iso.chars().take(16).collect();
    if let Some(pos) = s.find('T') {
        s.replace_range(pos..pos + 1, " ");
    }
    s
}
