//! Scheduled tasks (P-ADM-23): list by category, Run now / Stop, and a detail with its triggers (add / remove).
use super::sectioned_list;
use super::users::Ctl;
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

const TICKS_PER_SEC: i64 = 10_000_000;
const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const KINDS: [&str; 4] = ["Daily", "Every N hours", "On app start", "Weekly"];
const K_DAILY: usize = 0;
const K_INTERVAL: usize = 1;
const K_STARTUP: usize = 2;
const K_WEEKLY: usize = 3;

/// "03:30" → ticks since midnight; `None` for anything that isn't a 24-hour time.
fn parse_time(text: &str) -> Option<i64> {
    let (h, m) = text.trim().split_once(':')?;
    let (h, m): (i64, i64) = (h.trim().parse().ok()?, m.trim().parse().ok()?);
    ((0..24).contains(&h) && (0..60).contains(&m)).then_some((h * 3600 + m * 60) * TICKS_PER_SEC)
}

/// "6" → ticks; whole hours from 1 to 168 (a week).
fn parse_hours(text: &str) -> Option<i64> {
    let h: i64 = text.trim().parse().ok()?;
    (1..=168).contains(&h).then_some(h * 3600 * TICKS_PER_SEC)
}

/// The trigger the form describes, or the reason it can't be added.
fn build_trigger(
    kind: usize,
    time: &str,
    hours: &str,
    day: usize,
) -> std::result::Result<TaskTriggerInfo, &'static str> {
    let at = || parse_time(time).ok_or("Enter the time as HH:MM, like 03:30.");
    let mut tr = TaskTriggerInfo::default();
    match kind {
        K_DAILY => {
            tr.type_ = Some("DailyTrigger".into());
            tr.time_of_day_ticks = Some(at()?);
        }
        K_INTERVAL => {
            tr.type_ = Some("IntervalTrigger".into());
            tr.interval_ticks = Some(parse_hours(hours).ok_or("Enter whole hours, from 1 to 168.")?);
        }
        K_STARTUP => tr.type_ = Some("StartupTrigger".into()),
        K_WEEKLY => {
            tr.type_ = Some("WeeklyTrigger".into());
            tr.day_of_week = Some(DAYS[day % 7].into());
            tr.time_of_day_ticks = Some(at()?);
        }
        _ => return Err("Pick a trigger type."),
    }
    Ok(tr)
}

fn with_trigger(list: &[TaskTriggerInfo], new: TaskTriggerInfo) -> Vec<TaskTriggerInfo> {
    let mut out = list.to_vec();
    out.push(new);
    out
}

fn without_trigger(list: &[TaskTriggerInfo], i: usize) -> Vec<TaskTriggerInfo> {
    let mut out = list.to_vec();
    if i < out.len() {
        out.remove(i);
    }
    out
}

struct Form {
    kind: usize,
    time: String,
    hours: String,
    day: usize,
}

pub struct TaskDetail {
    task: TaskInfo,
    form: Option<Form>,
    focus: Focus,
    groups: Vec<Group>,
    enter: bool,
    on_field: bool,
    busy: bool,
    error: Option<String>,
    notice: Option<String>,
}

pub fn detail_screen(task: TaskInfo) -> Box<dyn Screen> {
    // Start on "Add trigger" (after the Remove buttons), so a stray Enter never removes a trigger.
    let first = task.triggers.as_ref().map_or(0, |t| t.len());
    Box::new(TaskDetail {
        task,
        form: None,
        focus: Focus { group: 0, index: first },
        groups: vec![],
        enter: false,
        on_field: false,
        busy: false,
        error: None,
        notice: None,
    })
}

enum DetAct {
    OpenForm,
    Kind(usize),
    Day,
    Add,
    Cancel,
    Remove(usize),
}

impl TaskDetail {
    fn triggers(&self) -> Vec<TaskTriggerInfo> {
        self.task.triggers.clone().unwrap_or_default()
    }

    fn save(&mut self, cx: &Cx, list: Vec<TaskTriggerInfo>) {
        let Some(id) = self.task.id.clone() else { return };
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        self.notice = None;
        let c = cx.api();
        cx.spawn("save", move || c.set_task_triggers(&id, &list));
    }

    fn run(&mut self, a: DetAct, cx: &Cx) {
        match a {
            DetAct::OpenForm => {
                self.form = Some(Form { kind: K_DAILY, time: "03:00".into(), hours: "6".into(), day: 1 });
                self.error = None;
            }
            DetAct::Kind(k) => {
                if let Some(f) = &mut self.form {
                    f.kind = k;
                }
                self.error = None;
            }
            DetAct::Day => {
                if let Some(f) = &mut self.form {
                    f.day = (f.day + 1) % 7;
                }
            }
            DetAct::Cancel => {
                self.form = None;
                self.error = None;
            }
            DetAct::Add => {
                let Some(f) = &self.form else { return };
                match build_trigger(f.kind, &f.time, &f.hours, f.day) {
                    Ok(tr) => {
                        let list = with_trigger(&self.triggers(), tr);
                        self.form = None;
                        self.save(cx, list);
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            DetAct::Remove(i) => {
                let list = without_trigger(&self.triggers(), i);
                self.save(cx, list);
            }
        }
    }
}

impl Screen for TaskDetail {
    fn name(&self) -> &'static str {
        "taskdetail"
    }

    fn on_key(&mut self, _cx: &mut Cx, key: Key) -> bool {
        match key {
            Key::Back if self.form.is_some() => {
                self.form = None;
                self.error = None;
                true
            }
            Key::Enter => {
                self.enter = true;
                true
            }
            Key::Left | Key::Right if self.on_field => true,
            Key::Left | Key::Up => {
                self.focus = move_focus(&self.groups, self.focus, Key::Up);
                true
            }
            Key::Right | Key::Down => {
                self.focus = move_focus(&self.groups, self.focus, Key::Down);
                true
            }
            _ => false,
        }
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn std::any::Any + Send>) {
        match tag {
            "save" => {
                let Ok(r) = payload.downcast::<Result<()>>() else { return };
                match *r {
                    Ok(()) => {
                        self.notice = Some("Saved.".into());
                        let (c, id) = (cx.api(), self.task.id.clone().unwrap_or_default());
                        cx.spawn("task", move || c.scheduled_task(&id));
                    }
                    Err(ApiError::Unauthorized) => {
                        self.busy = false;
                        cx.signed_out()
                    }
                    Err(e) => {
                        self.busy = false;
                        self.error = Some(e.to_string());
                    }
                }
            }
            "task" => {
                self.busy = false;
                let Ok(r) = payload.downcast::<Result<TaskInfo>>() else { return };
                match *r {
                    Ok(t) => self.task = t,
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            _ => {}
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        let moving = ui.input(|i| i.pointer.is_moving());
        let enter = std::mem::take(&mut self.enter);
        let mut ctl = Ctl::new(Some(self.focus.index), enter, moving);
        let mut acts: Vec<DetAct> = vec![];
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
                for (i, trigger) in triggers.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = t.gap / 2.0;
                        parts::muted(ui, t, &trigger_label(trigger));
                        if ctl.button(ui, t, "Remove", false) {
                            acts.push(DetAct::Remove(i));
                        }
                    });
                }
            }
            _ => parts::muted(ui, t, "None"),
        }
        ui.add_space(t.gap / 2.0);

        if let Some(e) = self.error.clone() {
            parts::error_line(ui, t, &e);
        }
        if let Some(n) = &self.notice {
            parts::muted(ui, t, n);
        }
        match &mut self.form {
            None => {
                let label = if self.busy { "Saving…" } else { "Add trigger" };
                if ctl.button(ui, t, label, true) {
                    acts.push(DetAct::OpenForm);
                }
            }
            Some(f) => {
                parts::heading(ui, t, "New trigger");
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = t.gap / 2.0;
                    for (k, name) in KINDS.iter().enumerate() {
                        if ctl.button(ui, t, name, f.kind == k) {
                            acts.push(DetAct::Kind(k));
                        }
                    }
                });
                if f.kind == K_WEEKLY && ctl.button(ui, t, DAYS[f.day % 7], false) {
                    acts.push(DetAct::Day);
                }
                match f.kind {
                    K_DAILY | K_WEEKLY => ctl.field(ui, t, &mut f.time, "Time, 24-hour, like 03:30", false),
                    K_INTERVAL => ctl.field(ui, t, &mut f.hours, "Hours between runs", false),
                    _ => parts::muted(ui, t, "Runs each time the server starts."),
                }
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = t.gap / 2.0;
                    if ctl.button(ui, t, "Add", true) {
                        acts.push(DetAct::Add);
                    }
                    if ctl.button(ui, t, "Cancel", false) {
                        acts.push(DetAct::Cancel);
                    }
                });
            }
        }

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

#[cfg(test)]
mod tests {
    use super::*;

    const H: i64 = 3600 * TICKS_PER_SEC;

    #[test]
    fn times_parse_as_24_hour_clock() {
        assert_eq!(parse_time("03:30"), Some(3 * H + 30 * 60 * TICKS_PER_SEC));
        assert_eq!(parse_time(" 0:00 "), Some(0));
        assert_eq!(parse_time("23:59"), Some(23 * H + 59 * 60 * TICKS_PER_SEC));
        for bad in ["24:00", "12:60", "3", "ab:cd", "", "-1:00"] {
            assert_eq!(parse_time(bad), None, "{bad}");
        }
    }

    #[test]
    fn hours_are_whole_and_bounded() {
        assert_eq!(parse_hours("6"), Some(6 * H));
        assert_eq!(parse_hours("168"), Some(168 * H));
        for bad in ["0", "169", "-2", "1.5", "x", ""] {
            assert_eq!(parse_hours(bad), None, "{bad}");
        }
    }

    #[test]
    fn each_kind_builds_its_trigger() {
        let d = build_trigger(K_DAILY, "04:15", "", 0).unwrap();
        assert_eq!(d.type_.as_deref(), Some("DailyTrigger"));
        assert_eq!(d.time_of_day_ticks, Some(4 * H + 15 * 60 * TICKS_PER_SEC));
        let i = build_trigger(K_INTERVAL, "", "12", 0).unwrap();
        assert_eq!((i.type_.as_deref(), i.interval_ticks), (Some("IntervalTrigger"), Some(12 * H)));
        let s = build_trigger(K_STARTUP, "", "", 0).unwrap();
        assert_eq!(s.type_.as_deref(), Some("StartupTrigger"));
        let w = build_trigger(K_WEEKLY, "01:00", "", 3).unwrap();
        assert_eq!((w.type_.as_deref(), w.day_of_week.as_deref()), (Some("WeeklyTrigger"), Some("Wednesday")));
        assert_eq!(w.time_of_day_ticks, Some(H));
    }

    #[test]
    fn bad_input_builds_nothing() {
        assert!(build_trigger(K_DAILY, "late", "", 0).is_err());
        assert!(build_trigger(K_INTERVAL, "", "0", 0).is_err());
        assert!(build_trigger(K_WEEKLY, "99:00", "", 1).is_err());
        assert!(build_trigger(9, "", "", 0).is_err());
    }

    #[test]
    fn add_and_remove_edit_the_list() {
        let a = build_trigger(K_STARTUP, "", "", 0).unwrap();
        let b = build_trigger(K_DAILY, "02:00", "", 0).unwrap();
        let both = with_trigger(std::slice::from_ref(&a), b.clone());
        assert_eq!(both.len(), 2);
        let left = without_trigger(&both, 0);
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].type_, b.type_);
        assert_eq!(without_trigger(&both, 7).len(), 2);
    }
}
