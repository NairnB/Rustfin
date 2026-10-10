//! Scheduled tasks (P-ADM-23): list by category, Run now / Stop, and a read-only detail with its triggers.
//! Editing triggers is a later packet, so P-ADM-23 is marked partial in PARITY.md.
use super::sectioned_list;
use crate::api::gen_admin::{TaskInfo, TaskTriggerInfo};
use crate::api::{ApiError, Result};
use crate::app::{Cx, Screen};
use crate::nav::{move_focus, Focus, Group};
use crate::platform::Key;
use crate::ui::parts;
use crate::ui::widgets::ListRow;

const G_LIST: usize = 0;
const G_RUN: usize = 1;
const G_STOP: usize = 2;

pub struct Tasks {
    tasks: Vec<TaskInfo>,
    /// Flattened row → index into `tasks` (grouped by category in draw order).
    flat: Vec<usize>,
    selected: usize,
    loaded: bool,
    in_flight: bool,
    busy: bool,
    error: Option<String>,
    notice: Option<String>,
    started: bool,
    focus: Focus,
    groups: Vec<Group>,
}

pub fn screen() -> Box<dyn Screen> {
    Box::new(Tasks {
        tasks: vec![],
        flat: vec![],
        selected: 0,
        loaded: false,
        in_flight: false,
        busy: false,
        error: None,
        notice: None,
        started: false,
        focus: Focus { group: G_LIST, index: 0 },
        groups: vec![],
    })
}

impl Tasks {
    fn load(&mut self, cx: &Cx) {
        if self.in_flight {
            return;
        }
        self.in_flight = true;
        let c = cx.api();
        cx.spawn("tasks", move || c.scheduled_tasks());
    }

    fn focused_id(&self) -> Option<String> {
        self.flat.get(self.selected).and_then(|&i| self.tasks.get(i)).and_then(|t| t.id.clone())
    }

    fn run(&mut self, cx: &mut Cx) {
        if self.busy {
            return;
        }
        let Some(id) = self.focused_id() else { return };
        self.busy = true;
        self.notice = None;
        self.error = None;
        let c = cx.api();
        cx.spawn("run", move || c.run_task(&id));
    }

    fn stop(&mut self, cx: &mut Cx) {
        if self.busy {
            return;
        }
        let Some(id) = self.focused_id() else { return };
        self.busy = true;
        self.notice = None;
        self.error = None;
        let c = cx.api();
        cx.spawn("stop", move || c.stop_task(&id));
    }

    /// (category, task indices) in first-appearance order.
    fn grouped(&self) -> Vec<(String, Vec<usize>)> {
        let mut out: Vec<(String, Vec<usize>)> = vec![];
        for (i, task) in self.tasks.iter().enumerate() {
            let cat = task.category.clone().filter(|c| !c.is_empty()).unwrap_or_else(|| "General".to_string());
            match out.iter_mut().find(|(c, _)| *c == cat) {
                Some((_, idxs)) => idxs.push(i),
                None => out.push((cat, vec![i])),
            }
        }
        out
    }
}

impl Screen for Tasks {
    fn name(&self) -> &'static str {
        "tasks"
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        match key {
            Key::Enter => {
                match self.focus.group {
                    G_LIST => {
                        self.selected = self.focus.index;
                        if let Some(&i) = self.flat.get(self.focus.index) {
                            if let Some(task) = self.tasks.get(i) {
                                cx.push(detail_screen(task.clone()));
                            }
                        }
                    }
                    G_RUN => self.run(cx),
                    G_STOP => self.stop(cx),
                    _ => {}
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
        match tag {
            "tasks" => {
                self.in_flight = false;
                let Ok(r) = payload.downcast::<Result<Vec<TaskInfo>>>() else { return };
                match *r {
                    Ok(v) => {
                        self.tasks = v;
                        self.loaded = true;
                    }
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            "run" | "stop" => {
                self.busy = false;
                let Ok(r) = payload.downcast::<Result<()>>() else { return };
                match *r {
                    Ok(()) => {
                        self.notice = Some("Done.".to_string());
                        self.in_flight = false;
                        self.loaded = false;
                        self.load(cx);
                    }
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(e) => self.error = Some(e.to_string()),
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
        let pointer_moved = ui.input(|i| i.pointer.is_moving());

        ui.add_space(t.margin);
        parts::title(ui, t, "Scheduled tasks");
        ui.add_space(t.gap / 2.0);

        if let Some(e) = self.error.clone() {
            parts::error_line(ui, t, &e);
            let r = parts::button(ui, t, "Retry", false, false);
            if r.clicked() {
                self.error = None;
                self.load(cx);
            }
            self.groups = vec![];
            return;
        }

        if !self.loaded {
            parts::muted(ui, t, "Loading…");
            self.groups = vec![];
            return;
        }

        let grouped = self.grouped();
        let mut flat: Vec<usize> = vec![];
        let mut headings: Vec<String> = vec![];
        let mut labels: Vec<String> = vec![];
        let mut secondaries: Vec<String> = vec![];
        let mut values: Vec<String> = vec![];
        let mut counts: Vec<usize> = vec![];
        for (cat, idxs) in &grouped {
            headings.push(cat.clone());
            counts.push(idxs.len());
            for &i in idxs {
                flat.push(i);
                let task = &self.tasks[i];
                labels.push(task.name.clone().unwrap_or_default());
                secondaries.push(task_state(task));
                values.push(next_run(task));
            }
        }
        self.flat = flat;

        let mut sections: Vec<(&str, Vec<ListRow<'_>>)> = vec![];
        let mut li = 0;
        for (hi, &count) in counts.iter().enumerate() {
            let mut rows = vec![];
            for _ in 0..count {
                rows.push(ListRow { label: &labels[li], secondary: Some(&secondaries[li]), value: Some(&values[li]) });
                li += 1;
            }
            sections.push((headings[hi].as_str(), rows));
        }

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

        ui.add_space(t.gap / 2.0);
        let mut run_x = vec![];
        let mut stop_x = vec![];
        let mut act_clicked: Option<usize> = None;
        let mut act_hovered: Option<usize> = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = t.gap / 2.0;
            let label = if self.busy { "Working…" } else { "Run now" };
            let r = parts::button(ui, t, label, self.focus.group == G_RUN, true);
            run_x.push(r.rect.center().x);
            if r.hovered() {
                act_hovered = Some(0);
            }
            if r.clicked() {
                act_clicked = Some(0);
            }
            let r = parts::button(ui, t, "Stop", self.focus.group == G_STOP, false);
            stop_x.push(r.rect.center().x);
            if r.hovered() {
                act_hovered = Some(1);
            }
            if r.clicked() {
                act_clicked = Some(1);
            }
        });
        if let Some(n) = &self.notice {
            ui.add_space(t.gap / 2.0);
            parts::muted(ui, t, n);
        }

        self.groups = vec![
            Group { len: self.flat.len(), cols: 1, xs: list_xs },
            Group { len: 1, cols: 1, xs: run_x },
            Group { len: 1, cols: 1, xs: stop_x },
        ];
        self.focus = move_focus(&self.groups, self.focus, Key::Enter);

        if pointer_moved {
            if let Some(i) = hovered {
                self.focus = Focus { group: G_LIST, index: i };
            }
            if let Some(i) = act_hovered {
                self.focus = Focus { group: if i == 0 { G_RUN } else { G_STOP }, index: 0 };
            }
        }
        if let Some(i) = clicked {
            self.focus = Focus { group: G_LIST, index: i };
            self.selected = i;
            if let Some(&ti) = self.flat.get(i) {
                if let Some(task) = self.tasks.get(ti) {
                    cx.push(detail_screen(task.clone()));
                }
            }
        }
        if let Some(i) = act_clicked {
            match i {
                0 => self.run(cx),
                1 => self.stop(cx),
                _ => {}
            }
        }
    }
}

/// "Idle · Completed", "Running · 42%", … — the state plus the last result / progress.
fn task_state(task: &TaskInfo) -> String {
    let mut bits = vec![task.state.clone()];
    if task.state == "Running" {
        if let Some(p) = task.current_progress_percentage {
            bits.push(format!("{p:.0}%"));
        }
    } else if let Some(res) = &task.last_execution_result {
        if !res.status.is_empty() {
            bits.push(res.status.clone());
        }
    }
    bits.join(" · ")
}

/// The task's schedule (first trigger) as a short "next run" summary.
fn next_run(task: &TaskInfo) -> String {
    task.triggers.as_ref().and_then(|t| t.first()).map(trigger_label).unwrap_or_default()
}

fn trigger_label(trigger: &TaskTriggerInfo) -> String {
    match trigger.type_.as_deref().unwrap_or("") {
        "DailyTrigger" => format!("Daily at {}", time_of_day(trigger.time_of_day_ticks)),
        "WeeklyTrigger" => {
            format!("{} at {}", day_of_week(trigger.day_of_week.as_deref()), time_of_day(trigger.time_of_day_ticks))
        }
        "IntervalTrigger" => format!("Every {}", interval(trigger.interval_ticks)),
        "StartupTrigger" => "On startup".to_string(),
        _ => "On demand".to_string(),
    }
}

fn time_of_day(ticks: Option<i64>) -> String {
    let Some(ticks) = ticks else { return String::new() };
    let secs = ticks / 10_000_000;
    format!("{:02}:{:02}", secs / 3600, (secs / 60) % 60)
}

fn day_of_week(day: Option<&str>) -> String {
    day.map(str::to_string).unwrap_or_default()
}

fn interval(ticks: Option<i64>) -> String {
    let Some(ticks) = ticks else { return String::new() };
    let secs = ticks / 10_000_000;
    if secs % 3600 == 0 {
        format!("{}h", secs / 3600)
    } else if secs % 60 == 0 {
        format!("{} min", secs / 60)
    } else {
        format!("{secs}s")
    }
}

// ---------- detail ----------

pub struct TaskDetail {
    task: TaskInfo,
}

pub fn detail_screen(task: TaskInfo) -> Box<dyn Screen> {
    Box::new(TaskDetail { task })
}

impl Screen for TaskDetail {
    fn name(&self) -> &'static str {
        "taskdetail"
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        ui.add_space(t.margin);
        parts::title(ui, t, self.task.name.as_deref().unwrap_or("Task"));
        ui.add_space(t.gap / 2.0);
        parts::muted(ui, t, &format!("State: {}", self.task.state));
        ui.add_space(t.gap / 2.0);

        parts::heading(ui, t, "Last result");
        match &self.task.last_execution_result {
            Some(res) => {
                parts::muted(
                    ui,
                    t,
                    &format!(
                        "{} · {} → {}",
                        res.status,
                        short_time(&res.start_time_utc),
                        short_time(&res.end_time_utc)
                    ),
                );
                if let Some(msg) = res.error_message.as_deref().filter(|m| !m.is_empty()) {
                    parts::muted(ui, t, msg);
                }
            }
            None => parts::muted(ui, t, "Never run"),
        }
        ui.add_space(t.gap / 2.0);

        parts::heading(ui, t, "Triggers");
        match self.task.triggers.as_deref() {
            Some(triggers) if !triggers.is_empty() => {
                for trigger in triggers {
                    parts::muted(ui, t, &trigger_label(trigger));
                }
            }
            _ => parts::muted(ui, t, "None"),
        }
        parts::muted(ui, t, "Editing triggers is a later packet.");
    }
}

/// "2026-10-10T17:00:00.0000000Z" → "17:00".
fn short_time(iso: &str) -> String {
    let mut chars = iso.chars();
    // skip "YYYY-MM-DDT"
    let time: String = chars.by_ref().skip(11).take(5).collect();
    if time.len() == 5 {
        time
    } else {
        iso.chars().take(16).collect()
    }
}
