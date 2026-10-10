//! Player screen (R-UI-10, R-PLAY-1 … R-PLAY-5, R-API-21/22). The shell draws the video under the UI
//! (`shows_video`); this screen only draws the OSD and the overlays on top of it. Built by packet P2.
use super::parts;
use crate::api::{
    secs_to_ticks, ticks_to_secs, ApiError, Item, ItemKind, MediaStream, PlaybackInfo, ProgressReport, Result,
    StreamKind,
};
use crate::app::{Cx, Screen};
use crate::nav::{move_focus, Focus, Group};
use crate::platform::{Key, PlayRequest, PlayerState};
use crate::playreq::build;
use std::any::Any;

/// What the item page chose. `audio`/`subtitle` are Jellyfin `MediaStream.Index`es; `subtitle: Some(None)` = off,
/// `None` = use the server's default.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlayOptions {
    pub from_start: bool,
    pub audio: Option<i32>,
    pub subtitle: Option<Option<i32>>,
}

/// Which report the `Reporter` wants sent this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Report {
    None,
    Start,
    Progress,
    Stopped,
}

/// Pure reporting state machine (R-PLAY-2): fed `(now, state, position, paused)` each frame, it returns the
/// report to send. Start once on the first `Playing`; Progress every 10 s of playing and on any pause/unpause;
/// Stopped once on `Ended`. The caller fills in positions and stream indexes at send time.
#[derive(Debug, Default)]
pub struct Reporter {
    started: bool,
    stopped: bool,
    last_progress: f64,
    last_paused: bool,
}

impl Reporter {
    pub fn new() -> Reporter {
        Reporter::default()
    }

    pub fn step(&mut self, now: f64, state: &PlayerState, _position: f64, paused: bool) -> Report {
        if matches!(state, PlayerState::Ended) {
            if !self.stopped {
                self.stopped = true;
                return Report::Stopped;
            }
            return Report::None;
        }
        if !self.started {
            if matches!(state, PlayerState::Playing) {
                self.started = true;
                self.last_progress = now;
                self.last_paused = paused;
                return Report::Start;
            }
            return Report::None;
        }
        if paused != self.last_paused {
            self.last_paused = paused;
            self.last_progress = now;
            return Report::Progress;
        }
        if matches!(state, PlayerState::Playing) && now - self.last_progress >= 10.0 {
            self.last_progress = now;
            return Report::Progress;
        }
        Report::None
    }
}

const G_BAR: usize = 0;
const G_BTN: usize = 1;
const G_LIST: usize = 2;

const B_PLAY: usize = 1;
const B_AUDIO: usize = 3;
const B_SUBTITLE: usize = 4;

#[derive(Clone)]
enum Action {
    Seek(f64),
    TogglePause,
    OpenAudio,
    OpenSubtitles,
    PickAudio(i32),
    PickSubtitle(Option<i32>),
    PlayNext,
}

pub struct PlayerScreen {
    item: Item,
    opts: PlayOptions,
    req: Option<PlayRequest>,
    info: Option<PlaybackInfo>,
    loading: bool,
    build_error: Option<String>,
    player_error: Option<String>,
    reporter: Reporter,
    stop_sent: bool,
    // OSD
    osd_visible: bool,
    osd_last_input: f64,
    audio: Option<i32>,
    subtitle: Option<Option<i32>>,
    audio_open: bool,
    subtitle_open: bool,
    // next episode
    next: Option<Item>,
    next_known: bool,
    next_loading: bool,
    up_next: Option<Item>,
    up_next_deadline: Option<f64>,
    handling_end: bool,
    pending_end: bool,
    focus: Focus,
    groups: Vec<Group>,
    started: bool,
}

pub fn screen(item: Item, opts: PlayOptions) -> Box<dyn Screen> {
    Box::new(PlayerScreen {
        item,
        opts,
        req: None,
        info: None,
        loading: true,
        build_error: None,
        player_error: None,
        reporter: Reporter::new(),
        stop_sent: false,
        osd_visible: false,
        osd_last_input: 0.0,
        audio: None,
        subtitle: None,
        audio_open: false,
        subtitle_open: false,
        next: None,
        next_known: false,
        next_loading: false,
        up_next: None,
        up_next_deadline: None,
        handling_end: false,
        pending_end: false,
        focus: Focus { group: G_BTN, index: B_PLAY },
        groups: vec![],
        started: false,
    })
}

impl PlayerScreen {
    fn source(&self) -> Option<&crate::api::MediaSource> {
        self.info.as_ref().and_then(|i| i.media_sources.first())
    }

    fn stream_label(&self, s: &MediaStream) -> String {
        let mut l =
            s.display_title.clone().or_else(|| s.language.clone()).unwrap_or_else(|| format!("Track {}", s.index));
        if s.is_external && !l.to_lowercase().contains("external") {
            l.push_str(" · External");
        }
        l
    }

    fn audio_entries(&self) -> Vec<(String, i32)> {
        let mut v = vec![];
        if let Some(ms) = self.source() {
            for s in ms.media_streams.iter().filter(|s| s.kind == StreamKind::Audio) {
                v.push((self.stream_label(s), s.index));
            }
        }
        v
    }

    fn subtitle_entries(&self) -> Vec<(String, Option<i32>)> {
        let mut v = vec![("Off".to_string(), None)];
        if let Some(ms) = self.source() {
            for s in ms.media_streams.iter().filter(|s| s.kind == StreamKind::Subtitle) {
                v.push((self.stream_label(s), Some(s.index)));
            }
        }
        v
    }

    fn audio_label(&self) -> String {
        match self.audio {
            Some(i) => self
                .source()
                .and_then(|ms| ms.media_streams.iter().find(|s| s.kind == StreamKind::Audio && s.index == i))
                .map(|s| self.stream_label(s))
                .unwrap_or_else(|| "Default".to_string()),
            None => "Default".to_string(),
        }
    }

    fn subtitle_label(&self) -> String {
        match self.subtitle {
            Some(Some(i)) => self
                .source()
                .and_then(|ms| ms.media_streams.iter().find(|s| s.kind == StreamKind::Subtitle && s.index == i))
                .map(|s| self.stream_label(s))
                .unwrap_or_else(|| "Default".to_string()),
            Some(None) => "Off".to_string(),
            None => "Default".to_string(),
        }
    }

    fn buttons(&self, play_label: &str) -> Vec<(String, Action)> {
        let mut v = vec![
            ("⏪ 10 s".to_string(), Action::Seek(-10.0)),
            (play_label.to_string(), Action::TogglePause),
            ("30 s ⏩".to_string(), Action::Seek(30.0)),
            (format!("Audio: {}", self.audio_label()), Action::OpenAudio),
            (format!("Subtitles: {}", self.subtitle_label()), Action::OpenSubtitles),
        ];
        if self.next.is_some() {
            v.push(("Next episode".to_string(), Action::PlayNext));
        }
        v
    }

    fn duration(&self, cx: &Cx) -> f64 {
        cx.player.duration().or_else(|| self.item.run_time_ticks.map(ticks_to_secs)).unwrap_or(0.0)
    }

    fn next_label(&self, next: &Item) -> String {
        let s = next.parent_index_number.unwrap_or(0);
        let e = next.index_number.unwrap_or(0);
        format!("S{s}E{e} · {}", next.name)
    }

    fn begin(&mut self, cx: &mut Cx) {
        self.loading = true;
        let id = self.item.id.clone();
        let client = cx.api();
        cx.spawn("info", move || client.playback_info(&id));
    }

    fn ensure_next(&mut self, cx: &mut Cx) {
        if self.next_known || self.next_loading {
            return;
        }
        if self.item.kind != ItemKind::Episode || self.item.series_id.is_none() {
            self.next_known = true;
            return;
        }
        let series_id = self.item.series_id.clone().unwrap();
        let id = self.item.id.clone();
        let client = cx.api();
        self.next_loading = true;
        cx.spawn("next", move || client.next_episode(&series_id, &id));
    }

    fn start_playback(&mut self, cx: &mut Cx, item: Item, info: PlaybackInfo, opts: PlayOptions) {
        let client = cx.api();
        let req = build(&client, &item, &info, &opts);
        match req {
            Some(req) => {
                self.loading = false;
                self.build_error = None;
                self.player_error = None;
                self.item = item;
                self.info = Some(info);
                self.audio = req.audio;
                self.subtitle = Some(req.subtitle);
                self.req = Some(req.clone());
                self.reporter = Reporter::new();
                self.stop_sent = false;
                self.handling_end = false;
                self.pending_end = false;
                self.up_next = None;
                self.up_next_deadline = None;
                self.next = None;
                self.next_known = false;
                self.next_loading = false;
                self.audio_open = false;
                self.subtitle_open = false;
                self.osd_visible = false;
                self.osd_last_input = cx.now;
                self.focus = Focus { group: G_BTN, index: B_PLAY };
                cx.player.load(&req);
                self.ensure_next(cx);
            }
            None => {
                self.loading = false;
                self.build_error = Some("No playable source on the server".to_string());
            }
        }
    }

    fn send(&self, cx: &Cx, kind: Report, pos: f64, paused: bool) {
        let (Some(_req), Some(info)) = (&self.req, &self.info) else { return };
        let media_source_id = info.media_sources.first().map(|m| m.id.clone()).unwrap_or_default();
        let r = ProgressReport {
            item_id: self.item.id.clone(),
            media_source_id,
            play_session_id: info.play_session_id.clone(),
            position_ticks: secs_to_ticks(pos),
            is_paused: paused,
            audio_stream_index: self.audio,
            subtitle_stream_index: self.subtitle.flatten(),
        };
        let client = cx.api();
        match kind {
            Report::Start => cx.tasks.run(move || {
                let _ = client.report_start(&r);
            }),
            Report::Progress => cx.tasks.run(move || {
                let _ = client.report_progress(&r);
            }),
            Report::Stopped => cx.tasks.run(move || {
                let _ = client.report_stopped(&r);
            }),
            Report::None => {}
        }
    }

    fn tick_reports(&mut self, cx: &mut Cx) {
        let state = cx.player.state();
        let pos = cx.player.position();
        let paused = matches!(state, PlayerState::Paused);
        let d = self.reporter.step(cx.now, &state, pos, paused);
        if d == Report::Stopped {
            self.stop_sent = true;
        }
        if d != Report::None {
            self.send(cx, d, pos, paused);
        }
    }

    fn tick_end(&mut self, cx: &mut Cx) {
        if self.handling_end || !matches!(cx.player.state(), PlayerState::Ended) {
            return;
        }
        self.handling_end = true;
        if self.item.kind == ItemKind::Episode && self.item.series_id.is_some() {
            if self.next_known {
                if self.next.is_some() {
                    self.begin_up_next(cx);
                } else {
                    cx.pop();
                }
            } else {
                self.pending_end = true;
                self.ensure_next(cx);
            }
        } else {
            cx.pop();
        }
    }

    fn begin_up_next(&mut self, cx: &mut Cx) {
        self.up_next = self.next.clone();
        self.up_next_deadline = Some(cx.now + cx.theme.up_next_countdown as f64);
        self.focus = Focus { group: 0, index: 0 };
    }

    fn tick_up_next(&mut self, cx: &mut Cx) {
        if let Some(deadline) = self.up_next_deadline {
            if cx.now >= deadline {
                self.up_next_deadline = None;
                self.play_next(cx);
            }
        }
    }

    fn play_next(&mut self, cx: &mut Cx) {
        let Some(next) = self.next.clone() else { return };
        if !self.stop_sent {
            self.stop_sent = true;
            let pos = cx.player.position();
            let paused = matches!(cx.player.state(), PlayerState::Paused);
            self.send(cx, Report::Stopped, pos, paused);
        }
        self.up_next = None;
        self.up_next_deadline = None;
        self.loading = true;
        let id = next.id.clone();
        let client = cx.api();
        cx.spawn("nextinfo", move || client.playback_info(&id));
    }

    fn seek_relative(&mut self, cx: &mut Cx, delta: f64) {
        let pos = cx.player.position();
        let dur = cx.player.duration().unwrap_or(f64::INFINITY).max(0.0);
        cx.player.seek((pos + delta).clamp(0.0, dur));
    }

    fn toggle_pause(&mut self, cx: &mut Cx) {
        let paused = matches!(cx.player.state(), PlayerState::Paused);
        cx.player.set_paused(!paused);
    }

    fn pick_audio(&mut self, cx: &mut Cx, index: i32) {
        self.audio = Some(index);
        cx.player.set_audio(Some(index));
        self.audio_open = false;
        self.focus = Focus { group: G_BTN, index: B_AUDIO };
        self.report_now(cx);
    }

    fn pick_subtitle(&mut self, cx: &mut Cx, val: Option<i32>) {
        self.subtitle = Some(val);
        cx.player.set_subtitle(val);
        self.subtitle_open = false;
        self.focus = Focus { group: G_BTN, index: B_SUBTITLE };
        self.report_now(cx);
    }

    fn report_now(&mut self, cx: &mut Cx) {
        let pos = cx.player.position();
        let paused = matches!(cx.player.state(), PlayerState::Paused);
        self.send(cx, Report::Progress, pos, paused);
    }

    fn do_action(&mut self, cx: &mut Cx, a: Action) {
        match a {
            Action::Seek(d) => self.seek_relative(cx, d),
            Action::TogglePause => self.toggle_pause(cx),
            Action::OpenAudio => {
                self.audio_open = true;
                self.subtitle_open = false;
                self.focus = Focus { group: G_LIST, index: 0 };
                self.osd_visible = true;
                self.osd_last_input = cx.now;
            }
            Action::OpenSubtitles => {
                self.subtitle_open = true;
                self.audio_open = false;
                self.focus = Focus { group: G_LIST, index: 0 };
                self.osd_visible = true;
                self.osd_last_input = cx.now;
            }
            Action::PickAudio(i) => self.pick_audio(cx, i),
            Action::PickSubtitle(v) => self.pick_subtitle(cx, v),
            Action::PlayNext => self.play_next(cx),
        }
    }

    fn enter_osd(&mut self, cx: &mut Cx) {
        if self.focus.group == G_BTN {
            let action = self.buttons("").get(self.focus.index).map(|(_, a)| a.clone());
            if let Some(a) = action {
                self.do_action(cx, a);
            }
        }
    }

    fn pick_current(&mut self, cx: &mut Cx) {
        if self.audio_open {
            if let Some(idx) = self.audio_entries().get(self.focus.index).map(|(_, i)| *i) {
                self.pick_audio(cx, idx);
            }
        } else if let Some(v) = self.subtitle_entries().get(self.focus.index).map(|(_, v)| *v) {
            self.pick_subtitle(cx, v);
        }
    }

    fn draw_error(&mut self, ui: &mut egui::Ui, cx: &mut Cx, err: &str) {
        let t = cx.theme;
        let screen = ui.ctx().screen_rect();
        ui.painter().rect_filled(screen, 0.0, t.scrim);
        let mut xs = vec![];
        let mut clicked = false;
        ui.vertical_centered(|ui| {
            ui.add_space(t.margin * 2.0);
            parts::error_line(ui, t, err);
            ui.add_space(t.gap);
            let r = parts::button(ui, t, "Back", self.focus.group == G_BAR, true);
            xs.push(r.rect.center().x);
            if r.clicked() {
                clicked = true;
            }
        });
        self.groups = vec![Group { len: 1, cols: 1, xs }];
        if clicked {
            cx.pop();
        }
    }

    fn draw_loading(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        let screen = ui.ctx().screen_rect();
        ui.painter().rect_filled(screen, 0.0, t.scrim);
        ui.painter().text(screen.center(), egui::Align2::CENTER_CENTER, "Loading…", t.font(t.body_size), t.muted);
    }

    fn draw_osd(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        let screen = ui.ctx().screen_rect();
        let content_w = (screen.width() - 2.0 * t.margin).max(0.0);
        let pos = cx.player.position();
        let dur = self.duration(cx);
        let paused = matches!(cx.player.state(), PlayerState::Paused);
        let frac = if dur > 0.0 { (pos / dur).clamp(0.0, 1.0) as f32 } else { 0.0 };
        let title = self.req.as_ref().map(|r| r.title.clone()).unwrap_or_default();
        let play_label = if paused { "Play" } else { "Pause" };
        let buttons = self.buttons(play_label);
        let audio_open = self.audio_open;
        let subtitle_open = self.subtitle_open;
        let focus_bar = self.focus.group == G_BAR;
        let focus_btn_index = (self.focus.group == G_BTN).then_some(self.focus.index);

        // Title, top-left.
        let title_rect = egui::Rect::from_min_size(
            egui::pos2(screen.left() + t.margin, screen.top() + t.margin),
            egui::vec2(content_w, t.title_size + t.gap / 2.0),
        );
        ui.painter().rect_filled(title_rect, t.radius, t.scrim);
        ui.painter().text(
            title_rect.min + egui::vec2(t.gap / 2.0, t.gap / 4.0),
            egui::Align2::LEFT_TOP,
            title,
            t.font(t.title_size),
            t.text,
        );

        // Bottom bar: seek bar row, then buttons.
        let btn_h = t.button_h;
        let bottom_h = btn_h * 2.0 + t.gap;
        let bottom_rect = egui::Rect::from_min_size(
            egui::pos2(screen.left() + t.margin, screen.bottom() - t.margin - bottom_h),
            egui::vec2(content_w, bottom_h),
        );
        ui.painter().rect_filled(bottom_rect, t.radius, t.scrim);

        let mut bar_xs = vec![];
        let mut seek_click: Option<f32> = None;
        let mut btn_xs = vec![];
        let mut btn_clicked: Option<usize> = None;
        let mut btn_hovered: Option<usize> = None;

        {
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bottom_rect.shrink(t.gap / 2.0)));
            let pos_label = parts::clock(pos);
            let dur_label = parts::clock(dur);
            let pos_w = child.fonts(|f| f.layout_no_wrap(pos_label.clone(), t.font(t.body_size), t.text).size().x);
            let dur_w = child.fonts(|f| f.layout_no_wrap(dur_label.clone(), t.font(t.body_size), t.text).size().x);
            let bar_w = (bottom_rect.width() - t.gap - pos_w - dur_w - t.gap).max(0.0);

            child.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0; // bar_w accounts for every gap itself
                ui.label(egui::RichText::new(pos_label).size(t.body_size).color(t.text));
                ui.add_space(t.gap / 2.0);
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(bar_w, btn_h), egui::Sense::click());
                let bar = egui::Rect::from_min_max(
                    egui::pos2(rect.left(), rect.center().y - t.progress_h),
                    egui::pos2(rect.right(), rect.center().y + t.progress_h),
                );
                ui.painter().rect_filled(bar, 0.0, t.progress_track);
                let mut done = bar;
                done.set_right(bar.left() + bar.width() * frac);
                ui.painter().rect_filled(done, 0.0, t.progress);
                if focus_bar {
                    parts::focus_ring(ui, t, rect);
                }
                bar_xs.push(rect.center().x);
                if resp.clicked() {
                    if let Some(p) = resp.interact_pointer_pos() {
                        seek_click = Some(((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0));
                    }
                }
                ui.add_space(t.gap / 2.0);
                ui.label(egui::RichText::new(dur_label).size(t.body_size).color(t.muted));
            });

            child.add_space(t.gap / 2.0);
            child.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = t.gap / 2.0;
                for (i, (label, _)) in buttons.iter().enumerate() {
                    let focused = focus_btn_index == Some(i);
                    let r = parts::button(ui, t, label, focused, i == B_PLAY);
                    btn_xs.push(r.rect.center().x);
                    if r.hovered() {
                        btn_hovered = Some(i);
                    }
                    if r.clicked() {
                        btn_clicked = Some(i);
                    }
                }
            });
        }

        if let Some(rel) = seek_click {
            cx.player.seek(rel as f64 * dur);
        }

        let mut groups = vec![Group::default(); 3];
        if audio_open || subtitle_open {
            let (list_xs, list_hovered, list_action) = self.draw_picker(ui, cx);
            groups[G_LIST] = Group { len: list_xs.len(), cols: 1, xs: list_xs };
            if let Some(i) = list_hovered {
                self.focus = Focus { group: G_LIST, index: i };
            }
            if let Some(a) = list_action {
                self.do_action(cx, a);
            }
        } else {
            groups[G_BAR] = Group { len: 1, cols: 1, xs: bar_xs };
            groups[G_BTN] = Group { len: btn_xs.len(), cols: btn_xs.len(), xs: btn_xs };
        }
        self.groups = groups;

        if let Some(i) = btn_hovered {
            self.focus = Focus { group: G_BTN, index: i };
        }
        if let Some(i) = btn_clicked {
            let action = buttons.get(i).map(|(_, a)| a.clone());
            if let Some(a) = action {
                self.do_action(cx, a);
            }
        }
    }

    fn draw_picker(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> (Vec<f32>, Option<usize>, Option<Action>) {
        let t = cx.theme;
        let screen = ui.ctx().screen_rect();
        ui.painter().rect_filled(screen, 0.0, t.scrim);
        let labels: Vec<String> = if self.audio_open {
            self.audio_entries().into_iter().map(|(l, _)| l).collect()
        } else {
            self.subtitle_entries().into_iter().map(|(l, _)| l).collect()
        };
        let focus_list_index = (self.focus.group == G_LIST).then_some(self.focus.index);
        let mut xs = vec![];
        let mut hovered = None;
        let mut clicked = None;
        let area = egui::Area::new(egui::Id::new("player-picker"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .order(egui::Order::Foreground);
        area.show(ui.ctx(), |ui| {
            ui.vertical_centered(|ui| {
                for (i, label) in labels.iter().enumerate() {
                    let focused = focus_list_index == Some(i);
                    let r = parts::button(ui, t, label, focused, false);
                    xs.push(r.rect.center().x);
                    if r.hovered() {
                        hovered = Some(i);
                    }
                    if r.clicked() {
                        clicked = Some(i);
                    }
                    ui.add_space(t.gap / 3.0);
                }
            });
        });
        let action = if self.audio_open {
            clicked.and_then(|i| self.audio_entries().get(i).map(|(_, idx)| Action::PickAudio(*idx)))
        } else {
            clicked.and_then(|i| self.subtitle_entries().get(i).map(|(_, v)| Action::PickSubtitle(*v)))
        };
        (xs, hovered, action)
    }

    fn draw_up_next(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        let screen = ui.ctx().screen_rect();
        ui.painter().rect_filled(screen, 0.0, t.scrim);
        let Some(next) = self.next.clone() else { return };
        let remaining = self.up_next_deadline.map(|d| (d - cx.now).max(0.0).ceil() as i64).unwrap_or(0);
        let label = format!("Up next: {} in {remaining} s", self.next_label(&next));
        let focus_index = self.focus.index;

        let mut xs = vec![];
        let mut clicked = None;
        let area = egui::Area::new(egui::Id::new("player-upnext"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .order(egui::Order::Foreground);
        area.show(ui.ctx(), |ui| {
            ui.vertical_centered(|ui| {
                parts::heading(ui, t, &label);
                ui.add_space(t.gap);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = t.gap / 2.0;
                    for (i, l) in ["Play now", "Cancel"].iter().enumerate() {
                        let r = parts::button(ui, t, l, focus_index == i, i == 0);
                        xs.push(r.rect.center().x);
                        if r.clicked() {
                            clicked = Some(i);
                        }
                    }
                });
            });
        });
        self.groups = vec![Group { len: 2, cols: 2, xs }];
        if let Some(i) = clicked {
            if i == 0 {
                self.play_next(cx);
            } else {
                cx.pop();
            }
        }
    }
}

impl Screen for PlayerScreen {
    fn name(&self) -> &'static str {
        "player"
    }

    fn shows_video(&self) -> bool {
        true
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        if self.loading {
            return false;
        }
        if self.build_error.is_some() || self.player_error.is_some() {
            return match key {
                Key::Enter => {
                    cx.pop();
                    true
                }
                _ => false,
            };
        }
        if self.up_next.is_some() {
            return match key {
                Key::Enter => {
                    if self.focus.index == 0 {
                        self.play_next(cx);
                    } else {
                        cx.pop();
                    }
                    true
                }
                Key::Left | Key::Right => {
                    self.focus = move_focus(&self.groups, self.focus, key);
                    true
                }
                _ => false,
            };
        }
        if self.audio_open || self.subtitle_open {
            return match key {
                Key::Back => {
                    self.audio_open = false;
                    self.subtitle_open = false;
                    true
                }
                Key::Enter => {
                    self.pick_current(cx);
                    true
                }
                Key::Up | Key::Down | Key::Left | Key::Right => {
                    self.focus = move_focus(&self.groups, self.focus, key);
                    true
                }
                _ => false,
            };
        }
        if key == Key::Back {
            return false;
        }
        let was_visible = self.osd_visible;
        self.osd_visible = true;
        self.osd_last_input = cx.now;
        if key == Key::PlayPause {
            self.toggle_pause(cx);
            return true;
        }
        if !was_visible {
            return true;
        }
        match key {
            Key::Left | Key::Right => {
                if self.focus.group == G_BAR {
                    self.seek_relative(cx, if key == Key::Left { -10.0 } else { 30.0 });
                } else {
                    self.focus = move_focus(&self.groups, self.focus, key);
                }
                true
            }
            Key::Up | Key::Down => {
                self.focus = move_focus(&self.groups, self.focus, key);
                true
            }
            Key::Enter => {
                self.enter_osd(cx);
                true
            }
            _ => false,
        }
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn Any + Send>) {
        match tag {
            "info" => {
                let Ok(r) = payload.downcast::<Result<PlaybackInfo>>() else { return };
                match *r {
                    Ok(info) => self.start_playback(cx, self.item.clone(), info, self.opts.clone()),
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(e) => {
                        self.loading = false;
                        self.build_error = Some(e.to_string());
                    }
                }
            }
            "next" => {
                self.next_loading = false;
                self.next_known = true;
                let Ok(r) = payload.downcast::<Result<Option<Item>>>() else { return };
                match *r {
                    Ok(next) => self.next = next,
                    Err(ApiError::Unauthorized) => {
                        cx.signed_out();
                        return;
                    }
                    Err(_) => self.next = None,
                }
                if self.pending_end {
                    self.pending_end = false;
                    if self.next.is_some() {
                        self.begin_up_next(cx);
                    } else {
                        cx.pop();
                    }
                }
            }
            "nextinfo" => {
                let Ok(r) = payload.downcast::<Result<PlaybackInfo>>() else { return };
                match *r {
                    Ok(info) => {
                        let keep = self.audio.filter(|a| {
                            info.media_sources
                                .first()
                                .map(|m| m.media_streams.iter().any(|s| s.kind == StreamKind::Audio && s.index == *a))
                                .unwrap_or(false)
                        });
                        let opts = PlayOptions { from_start: true, audio: keep, subtitle: None };
                        if let Some(next) = self.next.clone() {
                            self.start_playback(cx, next, info, opts);
                        } else {
                            self.loading = false;
                            self.build_error = Some("No next episode".to_string());
                        }
                    }
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(e) => {
                        self.loading = false;
                        self.build_error = Some(e.to_string());
                    }
                }
            }
            _ => {}
        }
    }

    fn on_leave(&mut self, cx: &mut Cx) {
        if !self.stop_sent {
            self.stop_sent = true;
            let pos = cx.player.position();
            let paused = matches!(cx.player.state(), PlayerState::Paused);
            self.send(cx, Report::Stopped, pos, paused);
        }
        cx.player.stop();
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        if !self.started {
            self.started = true;
            self.begin(cx);
        }
        let t = cx.theme;

        if ui.input(|i| i.pointer.is_moving() || i.pointer.any_pressed()) {
            self.osd_last_input = cx.now;
            self.osd_visible = true;
        }

        if let PlayerState::Failed(msg) = cx.player.state() {
            self.player_error = Some(msg);
        }

        if let Some(e) = self.player_error.clone().or_else(|| self.build_error.clone()) {
            self.draw_error(ui, cx, &e);
            return;
        }

        if self.loading {
            self.draw_loading(ui, cx);
            return;
        }

        self.tick_reports(cx);
        self.tick_end(cx);

        if self.up_next.is_some() {
            self.tick_up_next(cx);
            self.draw_up_next(ui, cx);
            return;
        }

        let paused = matches!(cx.player.state(), PlayerState::Paused);
        if paused || self.audio_open || self.subtitle_open {
            self.osd_visible = true;
        } else if cx.now - self.osd_last_input > t.osd_hide_after as f64 {
            self.osd_visible = false;
        }

        if self.osd_visible {
            self.draw_osd(ui, cx);
        }
    }
}
