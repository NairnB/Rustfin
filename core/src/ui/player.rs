//! Player screen (R-UI-10, R-PLAY-1 … R-PLAY-5, R-API-21/22). The shell draws the video under the UI
//! (`shows_video`); this screen only draws the OSD and the overlays on top of it. Built by packet P2.
use super::parts;
use super::widgets;
use crate::api::{
    secs_to_ticks, ticks_to_secs, ApiError, Chapter, Item, ItemKind, MediaStream, PlaybackInfo, PlaybackPlan,
    ProgressReport, Result, Segment, SegmentKind, StreamKind,
};
use crate::app::{Cx, Screen};
use crate::nav::{move_focus, Focus, Group};
use crate::platform::{Aspect, Key, PlayRequest, PlayerState, PlayerStats, SubColour, SubSize, SubStyle};
use crate::playreq::build_plan;
use crate::repeat::{end_action, EndAction, Repeat};
use crate::settings::SubPrefs;
use std::any::Any;

/// What the item page chose. `audio`/`subtitle` are Jellyfin `MediaStream.Index`es; `subtitle: Some(None)` = off,
/// `None` = use the server's default.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlayOptions {
    pub from_start: bool,
    pub audio: Option<i32>,
    pub subtitle: Option<Option<i32>>,
    /// G8 / P-ITEM-11: the version (media source id) chosen on the item page; `None` = the first source.
    pub media_source_id: Option<String>,
}

/// G4 / P-PLAY-10 + P-ITEM-17: a chapter list into `(start seconds, "time · name")` rows, sorted by start.
/// Missing names become "Chapter N" (1-based). Pure so it can be unit-tested without a server.
pub fn chapter_rows(chapters: &[Chapter]) -> Vec<(f64, String)> {
    let mut v: Vec<(f64, String)> = chapters
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let secs = ticks_to_secs(c.start_position_ticks);
            let name = c.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| format!("Chapter {}", i + 1));
            (secs, format!("{} · {}", parts::clock(secs), name))
        })
        .collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v
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
const G_VOL: usize = 2;
const G_LIST: usize = 3;
const G_SUB_OFFSET: usize = 4;
const G_SUB_SIZE: usize = 5;
const G_SUB_POS: usize = 6;
const G_SUB_COLOUR: usize = 7;
const G_SUB_BG: usize = 8;

const B_PLAY: usize = 1;
const B_VOL_MUTE: usize = 0;
const B_VOL_BAR: usize = 1;

/// G10 / P-PLAY-11: the segment (with a button to offer) that `pos` is inside, as an index into `segments`.
/// `Unknown` segments have nothing to offer and never count. The first match wins.
pub fn active_segment(segments: &[Segment], pos: f64) -> Option<usize> {
    segments.iter().position(|s| s.kind.button_label().is_some() && pos >= s.start && pos < s.end)
}

/// G10: a segment injected for render tours / manual checks: `JF_TEST_SEGMENT=intro:5:600` (kind:start:end).
fn fixture_segments() -> Option<Vec<Segment>> {
    let v = std::env::var("JF_TEST_SEGMENT").ok()?;
    let mut p = v.split(':');
    let kind = SegmentKind::from_server(&capitalise(p.next()?));
    Some(vec![Segment { kind, start: p.next()?.parse().ok()?, end: p.next()?.parse().ok()? }])
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// G6 / P-PLAY-13: the Quality menu, as max kbps. 0 = Auto / Original (no cap).
pub const QUALITY_KBPS: [u32; 6] = [0, 20000, 10000, 8000, 4000, 2000];
/// G6 / P-PLAY-12: the Speed menu.
pub const SPEED_STEPS: [f64; 6] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0];

/// The Quality button's label for `choice` (`None` = not chosen in the player, so the Settings cap applies).
pub fn quality_label(choice: Option<u32>, settings_kbps: u32) -> String {
    match choice.unwrap_or(settings_kbps) {
        0 => "Auto".to_string(),
        k if k % 1000 == 0 => format!("{} Mbit/s", k / 1000),
        k => format!("{:.1} Mbit/s", k as f64 / 1000.0),
    }
}

fn mbps(bps: f64) -> String {
    format!("{:.1} Mbit/s", bps / 1_000_000.0)
}

/// G6 / P-PLAY-20: the label/value rows of the stats overlay. Rows whose value is unknown are left out
/// (method is always there).
pub fn stats_rows(
    method: &str,
    container: Option<&str>,
    video_codec: Option<&str>,
    source_bitrate: Option<i64>,
    st: &PlayerStats,
) -> Vec<(&'static str, String)> {
    let mut v = vec![("Play method", method.to_string())];
    if let Some(c) = container {
        v.push(("Container", c.to_string()));
    }
    let res = st.width.zip(st.height).map(|(w, h)| format!("{w}×{h}"));
    let video = match (video_codec, res) {
        (Some(c), Some(r)) => Some(format!("{c} {r}")),
        (Some(c), None) => Some(c.to_string()),
        (None, Some(r)) => Some(r),
        (None, None) => None,
    };
    if let Some(x) = video {
        v.push(("Video", x));
    }
    if let Some(b) = source_bitrate {
        v.push(("Source bitrate", mbps(b as f64)));
    }
    if let Some(b) = st.video_bitrate_bps {
        v.push(("Bitrate", mbps(b)));
    }
    if let Some(d) = st.dropped_frames {
        v.push(("Dropped frames", d.to_string()));
    }
    if let Some(b) = st.buffer_secs {
        v.push(("Buffer", format!("{b:.0} s")));
    }
    v
}

#[derive(Clone)]
enum Action {
    Seek(f64),
    TogglePause,
    OpenAudio,
    OpenSubtitles,
    PickAudio(i32),
    PlayNext,
    PlayPrevious,
    OpenChapters,
    SeekChapter(f64),
    ToggleFullscreen,
    ToggleMute,
    OpenQuality,
    OpenSpeed,
    OpenAspect,
    OpenRepeat,
    ToggleStats,
    PickQuality(u32),
    PickSpeed(f64),
    PickAspect(Aspect),
    PickRepeat(Repeat),
}

pub struct PlayerScreen {
    item: Item,
    opts: PlayOptions,
    /// G8: the media source in use (the item page's version choice), for `source()` and progress reports.
    source_id: Option<String>,
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
    // G6: quality (max kbps chosen here; None = the Settings cap), speed, stats, transcoding state
    quality_open: bool,
    speed_open: bool,
    aspect_open: bool,
    repeat_open: bool,
    stats_open: bool,
    quality_kbps: Option<u32>,
    speed: f64,
    // P-PLAY-14 / P-PLAY-15: aspect and repeat choice (this session only)
    aspect: Aspect,
    repeat: Repeat,
    transcoding: bool,
    fell_back: bool,
    reload_pos: f64,
    source_bitrate: Option<i64>,
    // next episode
    next: Option<Item>,
    next_known: bool,
    next_loading: bool,
    // previous episode (G4 / P-PLAY-16 / P-TV-9)
    prev: Option<Item>,
    prev_known: bool,
    prev_loading: bool,
    /// G9: when set, next/previous come from this list (shuffle, play all) instead of the series order.
    queue: Option<Vec<Item>>,
    up_next: Option<Item>,
    up_next_deadline: Option<f64>,
    handling_end: bool,
    pending_end: bool,
    // G10 / P-PLAY-11: media segments of the playing item, and the ones already auto-skipped
    segments: Vec<Segment>,
    auto_skipped: Vec<usize>,
    // G4: volume (0.0..1.0), mute, fullscreen, chapters list, keys help
    volume: f32,
    muted: bool,
    fullscreen: bool,
    chapters_open: bool,
    keys_help: bool,
    // G12 / P-USER-4: OSD skip lengths (read from settings each frame; defaults until the first frame).
    skip_back: f64,
    skip_forward: f64,
    /// G6: the Settings max bitrate (kbps), read each frame, for the Quality button label.
    settings_kbps: u32,
    // G5: subtitle offset (seconds) and appearance (P-PLAY-8, P-PLAY-9)
    sub_delay: f64,
    sub_style: SubStyle,
    focus: Focus,
    groups: Vec<Group>,
    started: bool,
}

pub fn screen(item: Item, opts: PlayOptions) -> Box<dyn Screen> {
    Box::new(new_player(item, opts))
}

/// G9 (P-TV-10, P-LIB-23): plays the first of `items`, then continues through the rest in order instead of the
/// series' next episode. An empty list opens nothing, so callers check first.
pub fn screen_queue(items: Vec<Item>, opts: PlayOptions) -> Box<dyn Screen> {
    let mut p = new_player(items.first().cloned().unwrap_or_default(), opts);
    p.queue = Some(items);
    Box::new(p)
}

fn new_player(item: Item, opts: PlayOptions) -> PlayerScreen {
    PlayerScreen {
        item,
        opts,
        source_id: None,
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
        quality_open: false,
        speed_open: false,
        aspect_open: false,
        repeat_open: false,
        stats_open: false,
        quality_kbps: None,
        speed: 1.0,
        aspect: Aspect::default(),
        repeat: Repeat::default(),
        transcoding: false,
        fell_back: false,
        reload_pos: 0.0,
        source_bitrate: None,
        next: None,
        next_known: false,
        next_loading: false,
        prev: None,
        prev_known: false,
        prev_loading: false,
        queue: None,
        up_next: None,
        up_next_deadline: None,
        handling_end: false,
        pending_end: false,
        segments: vec![],
        auto_skipped: vec![],
        volume: 1.0,
        muted: false,
        fullscreen: false,
        chapters_open: false,
        keys_help: false,
        skip_back: 10.0,
        skip_forward: 30.0,
        settings_kbps: 0,
        sub_delay: 0.0,
        sub_style: SubStyle::default(),
        focus: Focus { group: G_BTN, index: B_PLAY },
        groups: vec![],
        started: false,
    }
}

impl PlayerScreen {
    /// The queue neighbour of the current item (`step` = 1 next, -1 previous); `None` outside a queue.
    fn queue_neighbour(&self, step: isize) -> Option<Option<Item>> {
        let q = self.queue.as_ref()?;
        let pos = q.iter().position(|i| i.id == self.item.id)? as isize + step;
        Some(usize::try_from(pos).ok().and_then(|p| q.get(p)).cloned())
    }

    fn source(&self) -> Option<&crate::api::MediaSource> {
        self.info.as_ref().and_then(|i| crate::playreq::pick_source(i, self.source_id.as_deref()))
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

    fn any_menu(&self) -> bool {
        self.audio_open
            || self.subtitle_open
            || self.chapters_open
            || self.quality_open
            || self.speed_open
            || self.aspect_open
            || self.repeat_open
    }

    fn close_menus(&mut self) {
        self.audio_open = false;
        self.subtitle_open = false;
        self.chapters_open = false;
        self.quality_open = false;
        self.speed_open = false;
        self.aspect_open = false;
        self.repeat_open = false;
    }

    fn quality_labels(&self) -> Vec<String> {
        QUALITY_KBPS
            .iter()
            .map(|&k| if k == 0 { "Auto / Original".to_string() } else { quality_label(Some(k), 0) })
            .collect()
    }

    fn speed_labels(&self) -> Vec<String> {
        SPEED_STEPS.iter().map(|s| format!("{s}×")).collect()
    }

    fn aspect_labels(&self) -> Vec<String> {
        Aspect::ALL.iter().map(|a| a.label().to_string()).collect()
    }

    fn repeat_labels(&self) -> Vec<String> {
        Repeat::ALL.iter().map(|r| r.label().to_string()).collect()
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
            (format!("⏪ {} s", self.skip_back as i64), Action::Seek(-self.skip_back)),
            (play_label.to_string(), Action::TogglePause),
            (format!("{} s ⏩", self.skip_forward as i64), Action::Seek(self.skip_forward)),
        ];
        if self.prev.is_some() {
            v.push(("Previous episode".to_string(), Action::PlayPrevious));
        }
        if self.next.is_some() {
            v.push(("Next episode".to_string(), Action::PlayNext));
        }
        v.push((format!("Audio: {}", self.audio_label()), Action::OpenAudio));
        v.push((format!("Subtitles: {}", self.subtitle_label()), Action::OpenSubtitles));
        v.push(("Chapters".to_string(), Action::OpenChapters));
        v.push((format!("Quality: {}", quality_label(self.quality_kbps, self.settings_kbps)), Action::OpenQuality));
        v.push((format!("Speed: {}×", self.speed), Action::OpenSpeed));
        v.push((format!("Aspect: {}", self.aspect.label()), Action::OpenAspect));
        v.push((format!("Repeat: {}", self.repeat.label()), Action::OpenRepeat));
        v.push(("Stats".to_string(), Action::ToggleStats));
        v.push(("Fullscreen".to_string(), Action::ToggleFullscreen));
        v
    }

    fn audio_button_index(&self) -> usize {
        self.buttons("").iter().position(|(_, a)| matches!(a, Action::OpenAudio)).unwrap_or(0)
    }

    /// Chapter entries for the OSD list (start seconds, label).
    fn chapter_entries(&self) -> Vec<(f64, String)> {
        chapter_rows(&self.item.chapters)
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
        self.fetch_plan(cx, "info", id, false);
    }

    /// G6: PlaybackInfo for `id` under the current quality choice (or the Settings cap); `force` = transcode.
    fn fetch_plan(&self, cx: &mut Cx, tag: &'static str, id: String, force: bool) {
        let cap = self.quality_kbps.unwrap_or(cx.settings.max_streaming_bitrate);
        let client = cx.api();
        cx.spawn(tag, move || client.playback_plan(&id, cap, force));
    }

    /// G6: load the same item again through a new plan, at the current position (quality change, or the
    /// fallback to transcoding when the direct stream would not open).
    fn reload(&mut self, cx: &mut Cx, force: bool) {
        self.reload_pos = cx.player.position();
        self.loading = true;
        let id = self.item.id.clone();
        self.fetch_plan(cx, "reload", id, force);
    }

    fn finish_reload(&mut self, cx: &mut Cx, plan: PlaybackPlan, forced: bool) {
        if forced && plan.transcoding_url.is_none() {
            self.loading = false;
            self.player_error = Some("The server could not transcode this video".to_string());
            return;
        }
        let client = cx.api();
        // Keep the version (media source) that was playing when the quality changes.
        let opts = PlayOptions {
            from_start: false,
            audio: self.audio,
            subtitle: self.subtitle,
            media_source_id: self.source_id.clone(),
        };
        let Some(mut req) = build_plan(&client, &self.item, &plan, &opts, &SubPrefs::default()) else {
            self.loading = false;
            self.build_error = Some("No playable source on the server".to_string());
            return;
        };
        req.start_secs = self.reload_pos;
        self.transcoding = plan.transcoding_url.is_some();
        self.source_bitrate = plan.source_bitrate;
        self.info = Some(plan.info);
        self.req = Some(req.clone());
        self.loading = false;
        self.osd_visible = true;
        self.osd_last_input = cx.now;
        cx.player.load(&req);
        cx.player.set_speed(self.speed);
        cx.player.set_aspect(self.aspect);
        self.report_now(cx);
    }

    fn ensure_next(&mut self, cx: &mut Cx) {
        if self.next_known || self.next_loading {
            return;
        }
        if let Some(n) = self.queue_neighbour(1) {
            self.next = n;
            self.next_known = true;
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

    fn ensure_prev(&mut self, cx: &mut Cx) {
        if self.prev_known || self.prev_loading {
            return;
        }
        if let Some(p) = self.queue_neighbour(-1) {
            self.prev = p;
            self.prev_known = true;
            return;
        }
        if self.item.kind != ItemKind::Episode || self.item.series_id.is_none() {
            self.prev_known = true;
            return;
        }
        let series_id = self.item.series_id.clone().unwrap();
        let id = self.item.id.clone();
        let client = cx.api();
        self.prev_loading = true;
        cx.spawn("prev", move || client.previous_episode(&series_id, &id));
    }

    fn start_playback(&mut self, cx: &mut Cx, item: Item, plan: PlaybackPlan, opts: PlayOptions) {
        let client = cx.api();
        let req = build_plan(&client, &item, &plan, &opts, &SubPrefs::default());
        let info = plan.info;
        match req {
            Some(req) => {
                self.source_id = opts.media_source_id.clone();
                self.loading = false;
                self.build_error = None;
                self.player_error = None;
                self.item = item;
                self.info = Some(info);
                self.transcoding = plan.transcoding_url.is_some();
                self.source_bitrate = plan.source_bitrate;
                self.fell_back = false;
                self.audio = req.audio;
                self.subtitle = Some(req.subtitle);
                self.req = Some(req.clone());
                self.reporter = Reporter::new();
                self.stop_sent = false;
                self.handling_end = false;
                self.pending_end = false;
                self.segments = vec![];
                self.auto_skipped = vec![];
                self.up_next = None;
                self.up_next_deadline = None;
                self.next = None;
                self.next_known = false;
                self.next_loading = false;
                self.prev = None;
                self.prev_known = false;
                self.prev_loading = false;
                self.close_menus();
                self.keys_help = false;
                self.volume = cx.player.volume();
                self.muted = cx.player.muted();
                self.sub_delay = 0.0;
                self.sub_style = cx.settings.subtitles.style.clone();
                self.osd_visible = false;
                self.osd_last_input = cx.now;
                self.focus = Focus { group: G_BTN, index: B_PLAY };
                cx.player.load(&req);
                cx.player.set_sub_style(&self.sub_style);
                cx.player.set_sub_delay(0.0);
                cx.player.set_speed(self.speed);
                cx.player.set_aspect(self.aspect);
                self.ensure_next(cx);
                self.ensure_prev(cx);
                match fixture_segments() {
                    Some(fixture) => self.segments = fixture,
                    None => {
                        let id = self.item.id.clone();
                        let client = cx.api();
                        cx.spawn("segments", move || client.media_segments(&id).map(|s| (id, s)));
                    }
                }
            }
            None => {
                self.loading = false;
                self.build_error = Some("No playable source on the server".to_string());
            }
        }
    }

    fn send(&self, cx: &Cx, kind: Report, pos: f64, paused: bool) {
        let (Some(_req), Some(info)) = (&self.req, &self.info) else { return };
        let media_source_id =
            crate::playreq::pick_source(info, self.source_id.as_deref()).map(|m| m.id.clone()).unwrap_or_default();
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
        let method = if self.transcoding { "Transcode" } else { "DirectPlay" };
        match kind {
            Report::Start => cx.tasks.run(move || {
                let _ = client.report_start_as(&r, method);
            }),
            Report::Progress => cx.tasks.run(move || {
                let _ = client.report_progress_as(&r, method);
            }),
            Report::Stopped => cx.tasks.run(move || {
                let _ = client.report_stopped_as(&r, method);
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
        let series = self.queue.is_some() || (self.item.kind == ItemKind::Episode && self.item.series_id.is_some());
        if series && self.repeat != Repeat::One && !self.next_known {
            self.pending_end = true;
            self.ensure_next(cx);
        } else {
            self.finish_item(cx);
        }
    }

    /// P-PLAY-14/15: the item ended and what comes next is known; act on the Repeat choice.
    fn finish_item(&mut self, cx: &mut Cx) {
        match end_action(self.repeat, self.next.is_some()) {
            EndAction::Close => cx.pop(),
            EndAction::UpNext => self.begin_up_next(cx),
            EndAction::Replay => self.replay(cx),
            EndAction::Restart => {
                let first = self.queue.as_ref().and_then(|q| q.first()).filter(|f| f.id != self.item.id).cloned();
                match first {
                    Some(first) => {
                        self.next = Some(first);
                        self.play_next(cx);
                    }
                    None => self.replay(cx),
                }
            }
        }
    }

    /// Play the current item again from the start, with no "Up next".
    fn replay(&mut self, cx: &mut Cx) {
        let Some(mut req) = self.req.clone() else {
            cx.pop();
            return;
        };
        req.start_secs = 0.0;
        self.reporter = Reporter::new();
        self.stop_sent = false;
        self.handling_end = false;
        self.pending_end = false;
        self.auto_skipped = vec![];
        self.up_next = None;
        self.up_next_deadline = None;
        cx.player.load(&req);
        cx.player.set_speed(self.speed);
        cx.player.set_aspect(self.aspect);
    }

    fn begin_up_next(&mut self, cx: &mut Cx) {
        self.up_next = self.next.clone();
        // G12 / P-USER-4: auto-play only when enabled; otherwise wait for "Play now" (no countdown).
        self.up_next_deadline = cx.settings.auto_play_next.then_some(cx.now + cx.theme.up_next_countdown as f64);
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
        self.fetch_plan(cx, "nextinfo", id, false);
    }

    fn play_previous(&mut self, cx: &mut Cx) {
        let Some(prev) = self.prev.clone() else { return };
        if !self.stop_sent {
            self.stop_sent = true;
            let pos = cx.player.position();
            let paused = matches!(cx.player.state(), PlayerState::Paused);
            self.send(cx, Report::Stopped, pos, paused);
        }
        self.up_next = None;
        self.up_next_deadline = None;
        self.loading = true;
        let id = prev.id.clone();
        self.fetch_plan(cx, "previnfo", id, false);
    }

    /// G10: skip the segment `i` — to its end; credits with a next episode go straight to it.
    fn skip_segment(&mut self, cx: &mut Cx, i: usize) {
        let Some(seg) = self.segments.get(i).copied() else { return };
        if seg.kind == SegmentKind::Outro && self.next.is_some() {
            self.play_next(cx);
            return;
        }
        let dur = cx.player.duration().unwrap_or(f64::INFINITY).max(0.0);
        cx.player.seek(seg.end.clamp(0.0, dur));
    }

    /// G10: skip a segment the user chose to auto-skip, once per segment.
    fn tick_auto_skip(&mut self, cx: &mut Cx) {
        let Some(i) = active_segment(&self.segments, cx.player.position()) else { return };
        if self.auto_skipped.contains(&i) || !cx.settings.auto_skip.contains(&self.segments[i].kind) {
            return;
        }
        self.auto_skipped.push(i);
        self.skip_segment(cx, i);
    }

    /// G10: the "Skip intro" style button, bottom right, while inside a segment. Clickable; Enter presses it
    /// while the OSD is hidden.
    fn draw_skip_button(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let Some(i) = active_segment(&self.segments, cx.player.position()) else { return };
        let t = cx.theme;
        let kind = self.segments[i].kind;
        let label = match kind {
            SegmentKind::Outro if self.next.is_some() => "Next episode",
            k => k.button_label().unwrap_or_default(),
        };
        let lift = if self.osd_visible { t.skip_lift } else { 0.0 };
        let focused = !self.osd_visible;
        let mut clicked = false;
        egui::Area::new(egui::Id::new("player-skip"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-t.margin, -(t.margin + lift)))
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                clicked = parts::button(ui, t, label, focused, true).clicked();
            });
        if clicked {
            self.skip_segment(cx, i);
        }
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

    fn toggle_fullscreen(&mut self, cx: &mut Cx) {
        self.fullscreen = !self.fullscreen;
        cx.shell.fullscreen = Some(self.fullscreen);
    }

    fn leave_fullscreen(&mut self, cx: &mut Cx) {
        if self.fullscreen {
            self.fullscreen = false;
            cx.shell.fullscreen = Some(false);
        }
    }

    fn toggle_mute(&mut self, cx: &mut Cx) {
        self.muted = !self.muted;
        cx.player.set_muted(self.muted);
    }

    fn adjust_volume(&mut self, cx: &mut Cx, delta: f32) {
        self.volume = (self.volume + delta).clamp(0.0, 1.0);
        cx.player.set_volume(self.volume);
    }

    fn volume_pct(&self) -> i32 {
        (self.volume * 100.0).round() as i32
    }

    fn pick_audio(&mut self, cx: &mut Cx, index: i32) {
        self.audio = Some(index);
        cx.player.set_audio(Some(index));
        self.audio_open = false;
        self.focus = Focus { group: G_BTN, index: self.audio_button_index() };
        self.report_now(cx);
    }

    fn pick_subtitle(&mut self, cx: &mut Cx, val: Option<i32>) {
        self.subtitle = Some(val);
        cx.player.set_subtitle(val);
        self.report_now(cx);
    }

    fn adj_sub_delay(&mut self, cx: &mut Cx, delta: f64) {
        self.sub_delay = (self.sub_delay + delta).clamp(-30.0, 30.0);
        cx.player.set_sub_delay(self.sub_delay);
    }

    fn adj_sub_pos(&mut self, cx: &mut Cx, delta: f32) {
        self.sub_style.position = (self.sub_style.position + delta).clamp(0.0, 150.0);
        cx.player.set_sub_style(&self.sub_style);
    }

    fn set_sub_size(&mut self, cx: &mut Cx, size: SubSize) {
        self.sub_style.size = size;
        cx.player.set_sub_style(&self.sub_style);
    }

    fn set_sub_colour(&mut self, cx: &mut Cx, colour: SubColour) {
        self.sub_style.colour = colour;
        cx.player.set_sub_style(&self.sub_style);
    }

    fn toggle_sub_bg(&mut self, cx: &mut Cx) {
        self.sub_style.background = !self.sub_style.background;
        cx.player.set_sub_style(&self.sub_style);
    }

    /// Enter in the subtitle panel acts on the focused control.
    fn sub_panel_enter(&mut self, cx: &mut Cx) {
        match self.focus.group {
            G_LIST => {
                if let Some(v) = self.subtitle_entries().get(self.focus.index).map(|(_, v)| *v) {
                    self.pick_subtitle(cx, v);
                }
            }
            G_SUB_OFFSET => {
                self.adj_sub_delay(cx, if self.focus.index == 0 { -0.1 } else { 0.1 });
            }
            G_SUB_SIZE => {
                let size = match self.focus.index {
                    0 => SubSize::Small,
                    1 => SubSize::Medium,
                    2 => SubSize::Large,
                    _ => SubSize::ExtraLarge,
                };
                self.set_sub_size(cx, size);
            }
            G_SUB_POS => {
                self.adj_sub_pos(cx, if self.focus.index == 0 { -5.0 } else { 5.0 });
            }
            G_SUB_COLOUR => {
                let colour = if self.focus.index == 0 { SubColour::White } else { SubColour::Yellow };
                self.set_sub_colour(cx, colour);
            }
            G_SUB_BG => self.toggle_sub_bg(cx),
            _ => {}
        }
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
                self.close_menus();
                self.audio_open = true;
                self.focus = Focus { group: G_LIST, index: 0 };
                self.osd_visible = true;
                self.osd_last_input = cx.now;
            }
            Action::OpenSubtitles => {
                self.close_menus();
                self.subtitle_open = true;
                self.focus = Focus { group: G_LIST, index: 0 };
                self.osd_visible = true;
                self.osd_last_input = cx.now;
            }
            Action::OpenChapters => {
                self.close_menus();
                self.chapters_open = true;
                self.focus = Focus { group: G_LIST, index: 0 };
                self.osd_visible = true;
                self.osd_last_input = cx.now;
            }
            Action::PickAudio(i) => self.pick_audio(cx, i),
            Action::PlayNext => self.play_next(cx),
            Action::PlayPrevious => self.play_previous(cx),
            Action::SeekChapter(secs) => {
                let dur = self.duration(cx).max(0.0);
                cx.player.seek(secs.clamp(0.0, dur));
                self.chapters_open = false;
                self.focus = Focus { group: G_BTN, index: B_PLAY };
                self.osd_visible = true;
                self.osd_last_input = cx.now;
            }
            Action::ToggleFullscreen => self.toggle_fullscreen(cx),
            Action::ToggleMute => self.toggle_mute(cx),
            Action::OpenQuality => {
                self.close_menus();
                self.quality_open = true;
                let now = self.quality_kbps.unwrap_or(cx.settings.max_streaming_bitrate);
                let at = QUALITY_KBPS.iter().position(|&k| k == now).unwrap_or(0);
                self.focus = Focus { group: G_LIST, index: at };
                self.osd_visible = true;
                self.osd_last_input = cx.now;
            }
            Action::OpenSpeed => {
                self.close_menus();
                self.speed_open = true;
                let at = SPEED_STEPS.iter().position(|&s| s == self.speed).unwrap_or(2);
                self.focus = Focus { group: G_LIST, index: at };
                self.osd_visible = true;
                self.osd_last_input = cx.now;
            }
            Action::OpenAspect => {
                self.close_menus();
                self.aspect_open = true;
                let at = Aspect::ALL.iter().position(|&a| a == self.aspect).unwrap_or(0);
                self.focus = Focus { group: G_LIST, index: at };
                self.osd_visible = true;
                self.osd_last_input = cx.now;
            }
            Action::OpenRepeat => {
                self.close_menus();
                self.repeat_open = true;
                let at = Repeat::ALL.iter().position(|&r| r == self.repeat).unwrap_or(0);
                self.focus = Focus { group: G_LIST, index: at };
                self.osd_visible = true;
                self.osd_last_input = cx.now;
            }
            Action::ToggleStats => self.stats_open = !self.stats_open,
            Action::PickQuality(kbps) => {
                self.close_menus();
                self.focus = Focus { group: G_BTN, index: B_PLAY };
                self.quality_kbps = Some(kbps);
                self.reload(cx, false);
            }
            Action::PickSpeed(speed) => {
                self.close_menus();
                self.focus = Focus { group: G_BTN, index: B_PLAY };
                self.speed = speed;
                cx.player.set_speed(speed);
            }
            Action::PickAspect(aspect) => {
                self.close_menus();
                self.focus = Focus { group: G_BTN, index: B_PLAY };
                self.aspect = aspect;
                cx.player.set_aspect(aspect);
            }
            Action::PickRepeat(mode) => {
                self.close_menus();
                self.focus = Focus { group: G_BTN, index: B_PLAY };
                self.repeat = mode;
            }
        }
    }

    fn enter_osd(&mut self, cx: &mut Cx) {
        if self.focus.group == G_BTN {
            let action = self.buttons("").get(self.focus.index).map(|(_, a)| a.clone());
            if let Some(a) = action {
                self.do_action(cx, a);
            }
        } else if self.focus.group == G_VOL && self.focus.index == B_VOL_MUTE {
            self.do_action(cx, Action::ToggleMute);
        }
    }

    fn pick_current(&mut self, cx: &mut Cx) {
        if self.audio_open {
            if let Some(idx) = self.audio_entries().get(self.focus.index).map(|(_, i)| *i) {
                self.pick_audio(cx, idx);
            }
        } else if self.subtitle_open {
            self.sub_panel_enter(cx);
        } else if self.chapters_open {
            if let Some((secs, _)) = self.chapter_entries().get(self.focus.index) {
                self.do_action(cx, Action::SeekChapter(*secs));
            }
        } else if self.quality_open {
            if let Some(&k) = QUALITY_KBPS.get(self.focus.index) {
                self.do_action(cx, Action::PickQuality(k));
            }
        } else if self.speed_open {
            if let Some(&s) = SPEED_STEPS.get(self.focus.index) {
                self.do_action(cx, Action::PickSpeed(s));
            }
        } else if self.aspect_open {
            if let Some(&a) = Aspect::ALL.get(self.focus.index) {
                self.do_action(cx, Action::PickAspect(a));
            }
        } else if self.repeat_open {
            if let Some(&r) = Repeat::ALL.get(self.focus.index) {
                self.do_action(cx, Action::PickRepeat(r));
            }
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
        let mut title = self.req.as_ref().map(|r| r.title.clone()).unwrap_or_default();
        if self.transcoding {
            title.push_str(" · Transcoding");
        }
        let play_label = if paused { "Play" } else { "Pause" };
        let buttons = self.buttons(play_label);
        let audio_open = self.audio_open;
        let subtitle_open = self.subtitle_open;
        let chapters_open =
            self.chapters_open || self.quality_open || self.speed_open || self.aspect_open || self.repeat_open;
        let focus_bar = self.focus.group == G_BAR;
        let focus_btn_index = (self.focus.group == G_BTN).then_some(self.focus.index);
        let focus_vol = self.focus.group == G_VOL;

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

        // Bottom bar: seek bar row, buttons row, then the volume row.
        let btn_h = t.button_h;
        let bottom_h = btn_h * 3.0 + t.gap * 2.0;
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
        let mut vol_xs = vec![];
        let mut vol_clicked: Option<usize> = None;
        let mut vol_hovered: Option<usize> = None;

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

            child.add_space(t.gap / 2.0);
            child.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = t.gap / 2.0;
                let mute_label = if self.muted { "Unmute" } else { "Mute" };
                let mute_focused = focus_vol && self.focus.index == B_VOL_MUTE;
                let r = parts::button(ui, t, mute_label, mute_focused, false);
                vol_xs.push(r.rect.center().x);
                if r.hovered() {
                    vol_hovered = Some(B_VOL_MUTE);
                }
                if r.clicked() {
                    vol_clicked = Some(B_VOL_MUTE);
                }
                let pct_label = format!("{}%", self.volume_pct());
                let pct_w = ui.fonts(|f| f.layout_no_wrap(pct_label.clone(), t.font(t.body_size), t.text).size().x);
                let vol_bar_w = t.volume_bar_w - pct_w - t.gap / 2.0;
                let (vrect, vresp) = ui.allocate_exact_size(egui::vec2(vol_bar_w, btn_h), egui::Sense::click());
                let vbar = egui::Rect::from_min_max(
                    egui::pos2(vrect.left(), vrect.center().y - t.progress_h),
                    egui::pos2(vrect.right(), vrect.center().y + t.progress_h),
                );
                ui.painter().rect_filled(vbar, 0.0, t.progress_track);
                let mut vdone = vbar;
                vdone.set_right(vbar.left() + vbar.width() * self.volume);
                ui.painter().rect_filled(vdone, 0.0, t.progress);
                let vol_bar_focused = focus_vol && self.focus.index == B_VOL_BAR;
                if vol_bar_focused {
                    parts::focus_ring(ui, t, vrect);
                }
                vol_xs.push(vrect.center().x);
                if vresp.hovered() {
                    vol_hovered = Some(B_VOL_BAR);
                }
                if vresp.clicked() {
                    if let Some(p) = vresp.interact_pointer_pos() {
                        let f = ((p.x - vrect.left()) / vrect.width()).clamp(0.0, 1.0);
                        self.adjust_volume(cx, f - self.volume);
                    }
                }
                ui.label(egui::RichText::new(pct_label).size(t.body_size).color(t.muted));
            });
        }

        if let Some(rel) = seek_click {
            cx.player.seek(rel as f64 * dur);
        }

        let mut groups = vec![Group::default(); 9];
        if subtitle_open {
            self.draw_sub_panel(ui, cx, &mut groups);
        } else if audio_open || chapters_open {
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
            groups[G_VOL] = Group { len: vol_xs.len(), cols: vol_xs.len(), xs: vol_xs };
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
        if let Some(i) = vol_hovered {
            self.focus = Focus { group: G_VOL, index: i };
        }
        if let Some(i) = vol_clicked {
            if i == B_VOL_MUTE {
                self.do_action(cx, Action::ToggleMute);
            }
        }
    }

    fn draw_picker(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> (Vec<f32>, Option<usize>, Option<Action>) {
        let t = cx.theme;
        let screen = ui.ctx().screen_rect();
        ui.painter().rect_filled(screen, 0.0, t.scrim);
        let labels: Vec<String> = if self.audio_open {
            self.audio_entries().into_iter().map(|(l, _)| l).collect()
        } else if self.quality_open {
            self.quality_labels()
        } else if self.speed_open {
            self.speed_labels()
        } else if self.aspect_open {
            self.aspect_labels()
        } else if self.repeat_open {
            self.repeat_labels()
        } else {
            let entries = self.chapter_entries();
            let mut v: Vec<String> = entries.into_iter().map(|(_, l)| l).collect();
            if v.is_empty() {
                v.push("No chapters".to_string());
            }
            v
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
        } else if self.quality_open {
            clicked.and_then(|i| QUALITY_KBPS.get(i).map(|&k| Action::PickQuality(k)))
        } else if self.speed_open {
            clicked.and_then(|i| SPEED_STEPS.get(i).map(|&s| Action::PickSpeed(s)))
        } else if self.aspect_open {
            clicked.and_then(|i| Aspect::ALL.get(i).map(|&a| Action::PickAspect(a)))
        } else if self.repeat_open {
            clicked.and_then(|i| Repeat::ALL.get(i).map(|&r| Action::PickRepeat(r)))
        } else {
            clicked.and_then(|i| self.chapter_entries().get(i).map(|(secs, _)| Action::SeekChapter(*secs)))
        };
        (xs, hovered, action)
    }

    /// The subtitle panel (P-PLAY-6/8/9): track list, sync offset, size, position, colour, background box.
    /// Fills the subtitle focus groups (G_LIST, G_SUB_OFFSET … G_SUB_BG) and applies hover/click.
    fn draw_sub_panel(&mut self, ui: &mut egui::Ui, cx: &mut Cx, groups: &mut [Group]) {
        let t = cx.theme;
        let screen = ui.ctx().screen_rect();
        ui.painter().rect_filled(screen, 0.0, t.scrim);

        let entries = self.subtitle_entries();
        let size_idx = match self.sub_style.size {
            SubSize::Small => 0,
            SubSize::Medium => 1,
            SubSize::Large => 2,
            SubSize::ExtraLarge => 3,
        };
        let colour_idx = match self.sub_style.colour {
            SubColour::White => 0,
            SubColour::Yellow => 1,
        };
        let focus = self.focus;
        let off_label = format!("{:.1} s", self.sub_delay);
        let pos_label = format!("{:.0}", self.sub_style.position);
        let bg_on = self.sub_style.background;

        let mut list_xs = vec![];
        let mut list_hover = None;
        let mut list_click = None;
        let mut off_xs = vec![];
        let mut off_hover = None;
        let mut off_click = None;
        let mut size_xs = vec![];
        let mut size_hover = None;
        let mut size_click = None;
        let mut pos_xs = vec![];
        let mut pos_hover = None;
        let mut pos_click = None;
        let mut colour_xs = vec![];
        let mut colour_hover = None;
        let mut colour_click = None;
        let mut bg_x = 0.0;
        let mut bg_hover = false;
        let mut bg_click = false;

        let area = egui::Area::new(egui::Id::new("player-sub-panel"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .order(egui::Order::Foreground);
        area.show(ui.ctx(), |ui| {
            ui.vertical_centered(|ui| {
                parts::heading(ui, t, "Subtitles");
                ui.add_space(t.gap / 3.0);
                for (i, (label, _)) in entries.iter().enumerate() {
                    let focused = focus.group == G_LIST && focus.index == i;
                    let r = parts::button_width(ui, t, label, focused, false, t.dropdown_w);
                    list_xs.push(r.rect.center().x);
                    if r.hovered() {
                        list_hover = Some(i);
                    }
                    if r.clicked() {
                        list_click = Some(i);
                    }
                    ui.add_space(t.gap / 3.0);
                }
                ui.add_space(t.gap / 3.0);
                ui.horizontal(|ui| {
                    parts::muted(ui, t, "Offset");
                    ui.add_space(t.gap);
                    let focused = focus.group == G_SUB_OFFSET && focus.index == 0;
                    let r = parts::button(ui, t, "-", focused, false);
                    off_xs.push(r.rect.center().x);
                    if r.hovered() {
                        off_hover = Some(0);
                    }
                    if r.clicked() {
                        off_click = Some(0);
                    }
                    ui.label(egui::RichText::new(&off_label).size(t.body_size).color(t.text));
                    let focused = focus.group == G_SUB_OFFSET && focus.index == 1;
                    let r = parts::button(ui, t, "+", focused, false);
                    off_xs.push(r.rect.center().x);
                    if r.hovered() {
                        off_hover = Some(1);
                    }
                    if r.clicked() {
                        off_click = Some(1);
                    }
                });
                ui.add_space(t.gap / 3.0);
                let labels = ["S".to_string(), "M".to_string(), "L".to_string(), "XL".to_string()];
                let selected = [size_idx == 0, size_idx == 1, size_idx == 2, size_idx == 3];
                let out = widgets::chips(ui, t, &labels, &selected, (focus.group == G_SUB_SIZE).then_some(focus.index));
                size_xs = out.xs;
                size_hover = out.hovered;
                size_click = out.clicked;
                ui.add_space(t.gap / 3.0);
                ui.horizontal(|ui| {
                    parts::muted(ui, t, "Position");
                    ui.add_space(t.gap);
                    let focused = focus.group == G_SUB_POS && focus.index == 0;
                    let r = parts::button(ui, t, "-", focused, false);
                    pos_xs.push(r.rect.center().x);
                    if r.hovered() {
                        pos_hover = Some(0);
                    }
                    if r.clicked() {
                        pos_click = Some(0);
                    }
                    ui.label(egui::RichText::new(&pos_label).size(t.body_size).color(t.text));
                    let focused = focus.group == G_SUB_POS && focus.index == 1;
                    let r = parts::button(ui, t, "+", focused, false);
                    pos_xs.push(r.rect.center().x);
                    if r.hovered() {
                        pos_hover = Some(1);
                    }
                    if r.clicked() {
                        pos_click = Some(1);
                    }
                });
                ui.add_space(t.gap / 3.0);
                let labels = ["White".to_string(), "Yellow".to_string()];
                let selected = [colour_idx == 0, colour_idx == 1];
                let out =
                    widgets::chips(ui, t, &labels, &selected, (focus.group == G_SUB_COLOUR).then_some(focus.index));
                colour_xs = out.xs;
                colour_hover = out.hovered;
                colour_click = out.clicked;
                ui.add_space(t.gap / 3.0);
                let out = widgets::toggle(ui, t, "Background box", bg_on, focus.group == G_SUB_BG);
                bg_x = out.x;
                bg_hover = out.hovered;
                bg_click = out.clicked;
            });
        });

        groups[G_LIST] = Group { len: list_xs.len(), cols: 1, xs: list_xs };
        groups[G_SUB_OFFSET] = Group { len: off_xs.len(), cols: off_xs.len(), xs: off_xs };
        groups[G_SUB_SIZE] = Group { len: size_xs.len(), cols: size_xs.len(), xs: size_xs };
        groups[G_SUB_POS] = Group { len: pos_xs.len(), cols: pos_xs.len(), xs: pos_xs };
        groups[G_SUB_COLOUR] = Group { len: colour_xs.len(), cols: colour_xs.len(), xs: colour_xs };
        groups[G_SUB_BG] = Group { len: 1, cols: 1, xs: vec![bg_x] };

        if let Some(i) = list_hover {
            self.focus = Focus { group: G_LIST, index: i };
        }
        if let Some(i) = off_hover {
            self.focus = Focus { group: G_SUB_OFFSET, index: i };
        }
        if let Some(i) = size_hover {
            self.focus = Focus { group: G_SUB_SIZE, index: i };
        }
        if let Some(i) = pos_hover {
            self.focus = Focus { group: G_SUB_POS, index: i };
        }
        if let Some(i) = colour_hover {
            self.focus = Focus { group: G_SUB_COLOUR, index: i };
        }
        if bg_hover {
            self.focus = Focus { group: G_SUB_BG, index: 0 };
        }

        if let Some(i) = list_click {
            if let Some(v) = entries.get(i).map(|(_, v)| *v) {
                self.pick_subtitle(cx, v);
            }
        }
        if let Some(i) = off_click {
            self.adj_sub_delay(cx, if i == 0 { -0.1 } else { 0.1 });
        }
        if let Some(i) = size_click {
            let size = match i {
                0 => SubSize::Small,
                1 => SubSize::Medium,
                2 => SubSize::Large,
                _ => SubSize::ExtraLarge,
            };
            self.set_sub_size(cx, size);
        }
        if let Some(i) = pos_click {
            self.adj_sub_pos(cx, if i == 0 { -5.0 } else { 5.0 });
        }
        if let Some(i) = colour_click {
            let colour = if i == 0 { SubColour::White } else { SubColour::Yellow };
            self.set_sub_colour(cx, colour);
        }
        if bg_click {
            self.toggle_sub_bg(cx);
        }
    }

    fn draw_keys_help(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        let screen = ui.ctx().screen_rect();
        ui.painter().rect_filled(screen, 0.0, t.scrim);
        let lines = [
            "Space  Pause",
            "← / →  Seek",
            "↑ / ↓  Volume",
            "F  Fullscreen",
            "M  Mute",
            "N  Next episode",
            "P  Previous episode",
            "C  Chapters",
            "S  Stats",
            "i  Keys",
        ];
        let area = egui::Area::new(egui::Id::new("player-keys"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .order(egui::Order::Foreground);
        area.show(ui.ctx(), |ui| {
            ui.vertical_centered(|ui| {
                parts::heading(ui, t, "Keys");
                ui.add_space(t.gap / 2.0);
                for line in lines {
                    parts::muted(ui, t, line);
                }
            });
        });
        self.groups = vec![];
    }

    /// G6 / P-PLAY-20: the stats overlay, top-right.
    fn draw_stats(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        let method = if self.transcoding { "Transcoding" } else { "Direct play" };
        let ms = self.source();
        let container =
            if self.transcoding { Some("ts (HLS)".to_string()) } else { ms.and_then(|m| m.container.clone()) };
        let codec =
            ms.and_then(|m| m.media_streams.iter().find(|s| s.kind == StreamKind::Video)).and_then(|s| s.codec.clone());
        let codec = if self.transcoding { Some("h264".to_string()) } else { codec };
        let rows = stats_rows(method, container.as_deref(), codec.as_deref(), self.source_bitrate, &cx.player.stats());
        egui::Area::new(egui::Id::new("player-stats"))
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-t.margin, t.margin * 3.0))
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::NONE.fill(t.scrim).corner_radius(t.radius).inner_margin(t.gap / 2.0).show(ui, |ui| {
                    for (label, value) in rows {
                        ui.label(egui::RichText::new(format!("{label}: {value}")).size(t.body_size).color(t.text));
                    }
                });
            });
    }

    fn draw_up_next(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        let screen = ui.ctx().screen_rect();
        ui.painter().rect_filled(screen, 0.0, t.scrim);
        let Some(next) = self.next.clone() else { return };
        let label = match self.up_next_deadline {
            Some(d) => format!("Up next: {} in {} s", self.next_label(&next), (d - cx.now).max(0.0).ceil() as i64),
            None => format!("Up next: {}", self.next_label(&next)),
        };
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
        if self.any_menu() {
            return match key {
                Key::Back => {
                    self.close_menus();
                    self.focus = Focus { group: G_BTN, index: B_PLAY };
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
        if self.keys_help {
            return match key {
                Key::Info | Key::Back => {
                    self.keys_help = false;
                    true
                }
                _ => true,
            };
        }
        if key == Key::Back {
            if self.fullscreen {
                self.leave_fullscreen(cx);
                return true;
            }
            return false;
        }

        let was_visible = self.osd_visible;
        let show = |p: &mut PlayerScreen, cx: &Cx| {
            p.osd_visible = true;
            p.osd_last_input = cx.now;
        };

        // Shortcuts that work even while the OSD is hidden.
        match key {
            Key::PlayPause => {
                self.toggle_pause(cx);
                show(self, cx);
                return true;
            }
            Key::Fullscreen => {
                self.toggle_fullscreen(cx);
                show(self, cx);
                return true;
            }
            Key::Mute => {
                self.toggle_mute(cx);
                show(self, cx);
                return true;
            }
            Key::Next => {
                if self.next.is_some() {
                    self.play_next(cx);
                }
                show(self, cx);
                return true;
            }
            Key::Previous => {
                if self.prev.is_some() {
                    self.play_previous(cx);
                }
                show(self, cx);
                return true;
            }
            Key::Chapters => {
                self.do_action(cx, Action::OpenChapters);
                show(self, cx);
                return true;
            }
            Key::Info => {
                self.keys_help = true;
                show(self, cx);
                return true;
            }
            Key::Stats => {
                self.stats_open = !self.stats_open;
                return true;
            }
            Key::VolumeUp => {
                self.adjust_volume(cx, 0.05);
                show(self, cx);
                return true;
            }
            Key::VolumeDown => {
                self.adjust_volume(cx, -0.05);
                show(self, cx);
                return true;
            }
            _ => {}
        }

        // ↑/↓ adjust volume while the OSD is hidden (G4 keyboard rule).
        if !was_visible && matches!(key, Key::Up | Key::Down) {
            self.adjust_volume(cx, if key == Key::Up { 0.05 } else { -0.05 });
            show(self, cx);
            return true;
        }

        show(self, cx);
        if !was_visible {
            if key == Key::Enter {
                if let Some(i) = active_segment(&self.segments, cx.player.position()) {
                    self.skip_segment(cx, i);
                }
            }
            return true;
        }

        match key {
            Key::Left | Key::Right => {
                if self.focus.group == G_VOL && self.focus.index == B_VOL_BAR {
                    self.adjust_volume(cx, if key == Key::Left { -0.05 } else { 0.05 });
                } else if self.focus.group == G_BAR {
                    self.seek_relative(cx, if key == Key::Left { -self.skip_back } else { self.skip_forward });
                } else {
                    self.focus = move_focus(&self.groups, self.focus, key);
                }
                true
            }
            Key::Up | Key::Down => {
                if self.focus.group == G_VOL && self.focus.index == B_VOL_BAR {
                    self.adjust_volume(cx, if key == Key::Up { 0.05 } else { -0.05 });
                } else {
                    self.focus = move_focus(&self.groups, self.focus, key);
                }
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
            "reload" => {
                let Ok(r) = payload.downcast::<Result<PlaybackPlan>>() else { return };
                match *r {
                    Ok(plan) => {
                        let forced = self.fell_back && !self.transcoding;
                        self.finish_reload(cx, plan, forced)
                    }
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(e) => {
                        self.loading = false;
                        self.player_error = Some(e.to_string());
                    }
                }
            }
            "info" => {
                let Ok(r) = payload.downcast::<Result<PlaybackPlan>>() else { return };
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
                    self.finish_item(cx);
                }
            }
            "nextinfo" => {
                let Ok(r) = payload.downcast::<Result<PlaybackPlan>>() else { return };
                match *r {
                    Ok(info) => {
                        let keep = self.audio.filter(|a| {
                            info.info
                                .media_sources
                                .first()
                                .map(|m| m.media_streams.iter().any(|s| s.kind == StreamKind::Audio && s.index == *a))
                                .unwrap_or(false)
                        });
                        let opts = PlayOptions { from_start: true, audio: keep, ..Default::default() };
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
            "segments" => {
                let Ok(r) = payload.downcast::<Result<(String, Vec<Segment>)>>() else { return };
                match *r {
                    Ok((id, segments)) if id == self.item.id => self.segments = segments,
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    _ => {}
                }
            }
            "prev" => {
                self.prev_loading = false;
                self.prev_known = true;
                let Ok(r) = payload.downcast::<Result<Option<Item>>>() else { return };
                match *r {
                    Ok(prev) => self.prev = prev,
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(_) => self.prev = None,
                }
            }
            "previnfo" => {
                let Ok(r) = payload.downcast::<Result<PlaybackPlan>>() else { return };
                match *r {
                    Ok(info) => {
                        let keep = self.audio.filter(|a| {
                            info.info
                                .media_sources
                                .first()
                                .map(|m| m.media_streams.iter().any(|s| s.kind == StreamKind::Audio && s.index == *a))
                                .unwrap_or(false)
                        });
                        let opts = PlayOptions { from_start: true, audio: keep, ..Default::default() };
                        if let Some(prev) = self.prev.clone() {
                            self.start_playback(cx, prev, info, opts);
                        } else {
                            self.loading = false;
                            self.build_error = Some("No previous episode".to_string());
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
        self.leave_fullscreen(cx);
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
        // G12 / P-USER-4: the OSD skip lengths come from Settings (changed while the player is open).
        self.skip_back = cx.settings.skip_back_seconds.max(1) as f64;
        self.skip_forward = cx.settings.skip_forward_seconds.max(1) as f64;
        self.settings_kbps = cx.settings.max_streaming_bitrate;
        let t = cx.theme;

        if ui.input(|i| i.pointer.is_moving() || i.pointer.any_pressed()) {
            self.osd_last_input = cx.now;
            self.osd_visible = true;
        }
        // G4 / P-PLAY-18: double-click on the video toggles fullscreen.
        if ui.input(|i| i.pointer.button_double_clicked(egui::PointerButton::Primary)) {
            self.toggle_fullscreen(cx);
        }

        // G6 / P-PLAY-13: a direct stream mpv could not open is retried once through the transcoding path.
        if let PlayerState::Failed(msg) = cx.player.state() {
            if !self.loading {
                if !self.transcoding && !self.fell_back && self.req.is_some() {
                    self.fell_back = true;
                    self.reload(cx, true);
                } else {
                    self.player_error = Some(msg);
                }
            }
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
        self.tick_auto_skip(cx);

        if self.up_next.is_some() {
            self.tick_up_next(cx);
            self.draw_up_next(ui, cx);
            return;
        }

        if self.keys_help {
            self.draw_keys_help(ui, cx);
            return;
        }

        let paused = matches!(cx.player.state(), PlayerState::Paused);
        if paused || self.any_menu() {
            self.osd_visible = true;
        } else if cx.now - self.osd_last_input > t.osd_hide_after as f64 {
            self.osd_visible = false;
        }

        if self.osd_visible {
            self.draw_osd(ui, cx);
        }
        self.draw_skip_button(ui, cx);
        if self.stats_open {
            self.draw_stats(ui, cx);
        }
    }
}
