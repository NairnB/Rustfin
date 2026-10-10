//! R-UI-5 Library: title + count, then a paged poster grid that loads the next page as focus nears the end.
use super::parts::{self, RowOut};
use crate::api::{ApiError, Item, Page, Result};
use crate::app::{Cx, Screen};
use crate::nav::{self, Focus, Group};
use crate::platform::Key;

const PAGE: usize = 100;

pub struct Library {
    view: Item,
    items: Vec<Option<Item>>,
    total: usize,
    next_start: usize,
    in_flight: bool,
    error: Option<String>,
    started: bool,
    focus: Focus,
    groups: Vec<Group>,
    cols: usize,
}

pub fn screen(view: Item) -> Box<dyn Screen> {
    Box::new(Library {
        view,
        items: vec![],
        total: 0,
        next_start: 0,
        in_flight: false,
        error: None,
        started: false,
        focus: Focus { group: 0, index: 0 },
        groups: vec![],
        cols: 1,
    })
}

impl Library {
    fn request_page(&mut self, cx: &Cx) {
        if self.in_flight {
            return;
        }
        let client = cx.api();
        let view_id = self.view.id.clone();
        let start = self.next_start;
        self.in_flight = true;
        cx.spawn("page", move || client.library_page(&view_id, start, PAGE));
    }
}

impl Screen for Library {
    fn name(&self) -> &'static str {
        "library"
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        match key {
            Key::Enter => {
                if let Some(Some(item)) = self.items.get(self.focus.index) {
                    cx.push(super::item::screen(item.clone()));
                }
                true
            }
            Key::Up | Key::Down | Key::Left | Key::Right => {
                self.focus = nav::move_focus(&self.groups, self.focus, key);
                true
            }
            _ => false,
        }
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn std::any::Any + Send>) {
        if tag != "page" {
            return;
        }
        self.in_flight = false;
        match *payload.downcast::<Result<Page>>().unwrap() {
            Ok(page) => {
                self.total = page.total_record_count;
                if self.items.len() < self.total {
                    self.items.resize(self.total, None);
                }
                let start = self.next_start;
                let n = page.items.len();
                for (i, item) in page.items.into_iter().enumerate() {
                    if start + i < self.items.len() {
                        self.items[start + i] = Some(item);
                    }
                }
                self.next_start = if n == 0 { self.total } else { start + n };
            }
            Err(ApiError::Unauthorized) => cx.signed_out(),
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        if !self.started {
            self.started = true;
            self.request_page(cx);
        }
        let t = cx.theme;
        let pointer_moved = ui.input(|i| i.pointer.is_moving());

        ui.add_space(t.margin);
        parts::title(ui, t, &self.view.name);
        parts::muted(ui, t, &format!("{} items", self.total));
        ui.add_space(t.gap / 2.0);

        if let Some(e) = self.error.clone() {
            parts::error_line(ui, t, &e);
            let r = parts::button(ui, t, "Retry", false, false);
            if r.clicked() {
                self.error = None;
                self.total = 0;
                self.next_start = 0;
                self.items.clear();
                self.request_page(cx);
            }
            return;
        }

        let cols = nav::grid_cols(ui.available_width(), t.margin, t.poster_w, t.gap);
        self.cols = cols;
        let mut out = RowOut { clicked: None, hovered: None, xs: vec![] };
        egui::ScrollArea::vertical().show(ui, |ui| {
            out = parts::grid(ui, cx, &self.items, cols, Some(self.focus.index), self.total);
        });
        self.groups = vec![Group { len: self.total, cols, xs: out.xs }];
        if pointer_moved {
            if let Some(i) = out.hovered {
                self.focus.index = i;
            }
        }
        if let Some(i) = out.clicked {
            if let Some(Some(item)) = self.items.get(i) {
                cx.push(super::item::screen(item.clone()));
            }
        }

        // R-UI-5 / R-IMG-2: fetch the next page when focus is within 2 rows of the last loaded item.
        if !self.in_flight && self.next_start < self.total && self.focus.index + 2 * self.cols >= self.next_start {
            self.request_page(cx);
        }
    }
}
