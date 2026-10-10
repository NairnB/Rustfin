//! Dashboard (P-ADM-1, P-ADM-2): server info, active sessions, running tasks, and the admin actions —
//! Scan all libraries, Restart, Shut down (the last two behind a confirm dialog).
use crate::api::gen_admin::{SessionInfoDto, SystemInfo, TaskInfo};
use crate::api::{ApiError, Result};
use crate::app::{Cx, Screen};
use crate::nav::{move_focus, Focus, Group};
use crate::platform::Key;
use crate::ui::parts;
use crate::ui::widgets;
use egui::{vec2, Sense};

const G_BUTTONS: usize = 0;
const BUTTONS: [&str; 3] = ["Scan all libraries", "Restart", "Shut down"];

#[derive(Clone, Copy, PartialEq)]
enum Confirm {
    Restart,
    Shutdown,
}

impl Confirm {
    fn title(&self) -> &'static str {
        match self {
            Confirm::Restart => "Restart server?",
            Confirm::Shutdown => "Shut down server?",
        }
    }
    fn body(&self) -> &'static str {
        match self {
            Confirm::Restart => "The server will restart. Everyone watching will be disconnected.",
            Confirm::Shutdown => "The server will shut down. It will need to be started again by hand.",
        }
    }
    fn confirm_label(&self) -> &'static str {
        match self {
            Confirm::Restart => "Restart",
            Confirm::Shutdown => "Shut down",
        }
    }
}

pub struct Dashboard {
    info: Option<SystemInfo>,
    sessions: Option<Vec<SessionInfoDto>>,
    tasks: Option<Vec<TaskInfo>>,
    error: Option<String>,
    notice: Option<String>,
    busy: bool,
    confirm: Option<Confirm>,
    confirm_focus: usize,
    started: bool,
    focus: Focus,
    groups: Vec<Group>,
}

pub fn screen() -> Box<dyn Screen> {
    Box::new(Dashboard {
        info: None,
        sessions: None,
        tasks: None,
        error: None,
        notice: None,
        busy: false,
        confirm: None,
        confirm_focus: 0,
        started: false,
        focus: Focus { group: G_BUTTONS, index: 0 },
        groups: vec![],
    })
}

impl Dashboard {
    fn load(&mut self, cx: &Cx) {
        if self.started {
            return;
        }
        self.started = true;
        let c = cx.api();
        cx.spawn("info", move || c.system_info());
        let c = cx.api();
        cx.spawn("sessions", move || c.sessions());
        let c = cx.api();
        cx.spawn("tasks", move || c.scheduled_tasks());
    }

    fn scan(&mut self, cx: &mut Cx) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.notice = None;
        self.error = None;
        let c = cx.api();
        cx.spawn("action", move || c.scan_all_libraries());
    }

    fn restart(&mut self, cx: &mut Cx) {
        let c = cx.api();
        cx.spawn("action", move || c.restart_server());
    }

    fn shutdown(&mut self, cx: &mut Cx) {
        let c = cx.api();
        cx.spawn("action", move || c.shutdown_server());
    }

    fn confirm_buttons(&self) -> Vec<String> {
        let c = self.confirm.unwrap_or(Confirm::Restart);
        vec!["Cancel".to_string(), c.confirm_label().to_string()]
    }
}

impl Screen for Dashboard {
    fn name(&self) -> &'static str {
        "dashboard"
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        if let Some(c) = self.confirm {
            return match key {
                Key::Back => {
                    self.confirm = None;
                    true
                }
                Key::Left => {
                    self.confirm_focus = self.confirm_focus.saturating_sub(1);
                    true
                }
                Key::Right => {
                    self.confirm_focus = (self.confirm_focus + 1).min(1);
                    true
                }
                Key::Enter => {
                    let do_it = self.confirm_focus == 1;
                    self.confirm = None;
                    if do_it {
                        match c {
                            Confirm::Restart => self.restart(cx),
                            Confirm::Shutdown => self.shutdown(cx),
                        }
                    }
                    true
                }
                _ => false,
            };
        }
        match key {
            Key::Enter => {
                match self.focus.index {
                    0 => self.scan(cx),
                    1 => {
                        self.confirm = Some(Confirm::Restart);
                        self.confirm_focus = 0;
                    }
                    2 => {
                        self.confirm = Some(Confirm::Shutdown);
                        self.confirm_focus = 0;
                    }
                    _ => {}
                }
                true
            }
            Key::Left | Key::Right | Key::Up | Key::Down => {
                self.focus = move_focus(&self.groups, self.focus, key);
                true
            }
            Key::Back => false,
            _ => false,
        }
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn std::any::Any + Send>) {
        match tag {
            "info" => {
                let Ok(r) = payload.downcast::<Result<SystemInfo>>() else { return };
                match *r {
                    Ok(v) => self.info = Some(v),
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            "sessions" => {
                let Ok(r) = payload.downcast::<Result<Vec<SessionInfoDto>>>() else { return };
                match *r {
                    Ok(v) => self.sessions = Some(v),
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            "tasks" => {
                let Ok(r) = payload.downcast::<Result<Vec<TaskInfo>>>() else { return };
                match *r {
                    Ok(v) => self.tasks = Some(v),
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            "action" => {
                self.busy = false;
                let Ok(r) = payload.downcast::<Result<()>>() else { return };
                match *r {
                    Ok(()) => self.notice = Some("Done.".to_string()),
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            _ => {}
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        self.load(cx);
        let pointer_moved = ui.input(|i| i.pointer.is_moving());
        let mut button_xs = vec![];
        let mut clicked = None;
        let mut hovered = None;

        ui.add_space(t.margin);
        parts::title(ui, t, "Dashboard");
        ui.add_space(t.gap / 2.0);

        if let Some(e) = self.error.clone() {
            parts::error_line(ui, t, &e);
            let r = parts::button(ui, t, "Retry", false, false);
            if r.clicked() {
                self.error = None;
                self.started = false;
            }
            self.groups = vec![];
            return;
        }

        // Server info
        parts::heading(ui, t, "Server");
        match &self.info {
            Some(info) => {
                info_row(ui, t, "Name", info.server_name.as_deref().unwrap_or(""));
                info_row(ui, t, "Version", info.version.as_deref().unwrap_or(""));
                info_row(ui, t, "System", info.operating_system.as_deref().unwrap_or(""));
            }
            None => parts::muted(ui, t, "Loading…"),
        }
        ui.add_space(t.gap / 2.0);

        // Active sessions
        parts::heading(ui, t, "Active sessions");
        match &self.sessions {
            Some(list) if list.is_empty() => parts::muted(ui, t, "No active sessions"),
            Some(list) => {
                for s in list.iter().take(10) {
                    parts::muted(ui, t, &session_line(s));
                }
                if list.len() > 10 {
                    parts::muted(ui, t, &format!("+{} more", list.len() - 10));
                }
            }
            None => parts::muted(ui, t, "Loading…"),
        }
        ui.add_space(t.gap / 2.0);

        // Running tasks
        parts::heading(ui, t, "Scheduled tasks");
        match &self.tasks {
            Some(list) => {
                let running: Vec<&TaskInfo> = list.iter().filter(|task| task.state == "Running").collect();
                if running.is_empty() {
                    parts::muted(ui, t, "No tasks running");
                } else {
                    for task in running {
                        let name = task.name.clone().unwrap_or_else(|| "Task".to_string());
                        parts::muted(ui, t, &name);
                        if let Some(pct) = task.current_progress_percentage {
                            let (bar, _) =
                                ui.allocate_exact_size(vec2(ui.available_width(), t.progress_h), Sense::hover());
                            parts::progress_bar(ui, t, bar, (pct as f32 / 100.0).clamp(0.0, 1.0));
                        }
                    }
                }
            }
            None => parts::muted(ui, t, "Loading…"),
        }

        ui.add_space(t.gap);
        let scan_label = if self.busy { "Scanning…" } else { BUTTONS[0] };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = t.gap / 2.0;
            for (i, label) in [scan_label, BUTTONS[1], BUTTONS[2]].iter().enumerate() {
                let focused = self.focus.group == G_BUTTONS && self.focus.index == i;
                let r = parts::button(ui, t, label, focused, i == 0);
                button_xs.push(r.rect.center().x);
                if r.hovered() {
                    hovered = Some(i);
                }
                if r.clicked() {
                    clicked = Some(i);
                }
            }
        });
        if let Some(n) = &self.notice {
            ui.add_space(t.gap / 2.0);
            parts::muted(ui, t, n);
        }

        self.groups = vec![Group { len: 3, cols: 3, xs: button_xs }];
        self.focus = move_focus(&self.groups, self.focus, Key::Enter);
        if pointer_moved {
            if let Some(i) = hovered {
                self.focus = Focus { group: G_BUTTONS, index: i };
            }
        }
        if let Some(i) = clicked {
            match i {
                0 => self.scan(cx),
                1 => {
                    self.confirm = Some(Confirm::Restart);
                    self.confirm_focus = 0;
                }
                2 => {
                    self.confirm = Some(Confirm::Shutdown);
                    self.confirm_focus = 0;
                }
                _ => {}
            }
        }

        if let Some(c) = self.confirm {
            let out = widgets::dialog(ui, t, c.title(), c.body(), &self.confirm_buttons(), Some(self.confirm_focus));
            if let Some(i) = out.hovered {
                self.confirm_focus = i;
            }
            if let Some(i) = out.clicked {
                let do_it = i == 1;
                self.confirm = None;
                if do_it {
                    match c {
                        Confirm::Restart => self.restart(cx),
                        Confirm::Shutdown => self.shutdown(cx),
                    }
                }
            }
        }
    }
}

fn info_row(ui: &mut egui::Ui, t: &crate::theme::Theme, label: &str, value: &str) {
    parts::muted(ui, t, label);
    parts::heading(ui, t, value);
}

fn session_line(s: &SessionInfoDto) -> String {
    let mut bits: Vec<String> = vec![];
    if let Some(u) = &s.user_name {
        if !u.is_empty() {
            bits.push(u.clone());
        }
    }
    if let Some(c) = &s.client {
        if !c.is_empty() {
            bits.push(c.clone());
        }
    }
    if let Some(d) = &s.device_name {
        if !d.is_empty() {
            bits.push(d.clone());
        }
    }
    if let Some(now) = s.now_playing_item.as_ref().and_then(|i| i.name.clone()) {
        bits.push(format!("playing {now}"));
    }
    if bits.is_empty() {
        "Session".to_string()
    } else {
        bits.join(" · ")
    }
}
