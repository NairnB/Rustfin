//! `Player` (R-PLAT-1) on libmpv. mpv is created on the first `load`, on the render thread with the GL context
//! current. Track mapping (R-PLAY-6) and the remaining polish are packet P1.
use crate::mpv::{Mpv, END_FILE_REASON_EOF, END_FILE_REASON_ERROR};
use crate::tracks;
use jf_core::platform::{Aspect, PlayRequest, Player, PlayerState, PlayerStats, SubStyle, TrackRef};

pub type GetProcFactory = Box<dyn Fn() -> Box<dyn Fn(&std::ffi::CStr) -> *const std::ffi::c_void>>;

pub struct MpvPlayer {
    mpv: Option<Mpv>,
    make_get_proc: GetProcFactory,
    state: PlayerState,
    req: Option<PlayRequest>,
    null_audio: bool,
}

const EV_START_FILE: i32 = 6;
const EV_END_FILE: i32 = crate::mpv::EV_END_FILE;
const EV_FILE_LOADED: i32 = 8;

impl MpvPlayer {
    pub fn new(make_get_proc: GetProcFactory) -> MpvPlayer {
        MpvPlayer { mpv: None, make_get_proc, state: PlayerState::Idle, req: None, null_audio: false }
    }

    /// Headless test hook: `ao=null` so the audio track is still selected (aid / current-tracks/audio/lang
    /// are observable) even with no sound device.
    pub fn set_null_audio(&mut self, v: bool) {
        self.null_audio = v;
    }

    fn mpv(&mut self) -> Option<&Mpv> {
        if self.mpv.is_none() {
            let mut opts = vec![
                ("hwdec", "auto-safe"),
                ("keep-open", "yes"),
                ("ytdl", "no"),
                ("input-default-bindings", "no"),
                ("osc", "no"),
                ("cache", "yes"),
                ("demuxer-max-bytes", "150MiB"),
            ];
            if self.null_audio {
                opts.push(("ao", "null"));
            }
            match Mpv::new((self.make_get_proc)(), &opts) {
                Ok(m) => self.mpv = Some(m),
                Err(e) => {
                    self.state = PlayerState::Failed(e);
                    return None;
                }
            }
        }
        self.mpv.as_ref()
    }

    /// Poll mpv events; call once per frame.
    pub fn pump(&mut self) {
        let events = match &self.mpv {
            Some(m) => m.events(),
            None => return,
        };
        for ev in events {
            match ev.id {
                EV_START_FILE => self.state = PlayerState::Loading,
                EV_FILE_LOADED => {
                    self.state = PlayerState::Playing;
                    self.apply_tracks();
                }
                EV_END_FILE => {
                    if self.state == PlayerState::Idle {
                        continue;
                    }
                    match ev.end_file.map(|e| e.reason) {
                        Some(END_FILE_REASON_ERROR) => {
                            let code = ev.end_file.map(|e| e.error).unwrap_or(0);
                            let msg = self
                                .mpv
                                .as_ref()
                                .and_then(|m| m.error_string(code))
                                .unwrap_or_else(|| format!("mpv error {code}"));
                            self.state = PlayerState::Failed(msg);
                        }
                        Some(END_FILE_REASON_EOF) => self.state = PlayerState::Ended,
                        _ => self.state = PlayerState::Ended,
                    }
                }
                _ => {}
            }
        }
        if matches!(self.state, PlayerState::Playing | PlayerState::Paused) {
            let m = self.mpv.as_ref().unwrap();
            if m.get("eof-reached").as_deref() == Some("yes") {
                self.state = PlayerState::Ended;
            } else {
                let paused = m.get("pause").as_deref() == Some("yes");
                self.state = if paused { PlayerState::Paused } else { PlayerState::Playing };
            }
        }
    }

    /// R-PLAY-6: add external subs and select the request's audio/subtitle once the file is loaded.
    fn apply_tracks(&mut self) {
        let Some(m) = &self.mpv else { return };
        let Some(req) = &self.req else { return };
        for s in &req.external_subs {
            m.command(&["sub-add", &s.url, "auto", &s.title]);
        }
        let aid = tracks::aid(req, req.audio);
        let sid = tracks::sid(req, req.subtitle);
        m.command(&["set", "aid", &aid]);
        m.command(&["set", "sid", &sid]);
    }

    /// Read a raw mpv property (the playtest hook uses this for track state).
    pub fn get_prop(&self, name: &str) -> Option<String> {
        self.mpv.as_ref().and_then(|m| m.get(name))
    }
}

impl Player for MpvPlayer {
    fn load(&mut self, req: &PlayRequest) {
        self.req = Some(req.clone());
        let Some(m) = self.mpv() else { return };
        let start = format!("{:.3}", req.start_secs.max(0.0));
        // `start` as a property, not a loadfile argument: newer mpv reads the 3rd loadfile argument as an index.
        m.command(&["set", "start", &start]);
        // keep-open pauses mpv at the end of a file; a new load (e.g. the next episode) must play.
        m.command(&["set", "pause", "no"]);
        m.command(&["loadfile", &req.url, "replace"]);
        self.state = PlayerState::Loading;
    }
    fn set_paused(&mut self, paused: bool) {
        if let Some(m) = &self.mpv {
            m.command(&["set", "pause", if paused { "yes" } else { "no" }]);
        }
    }
    fn seek(&mut self, secs: f64) {
        if let Some(m) = &self.mpv {
            m.command(&["seek", &format!("{:.3}", secs.max(0.0)), "absolute"]);
        }
    }
    fn set_audio(&mut self, track: Option<TrackRef>) {
        if let Some(m) = &self.mpv {
            if let Some(req) = &self.req {
                let aid = tracks::aid(req, track);
                m.command(&["set", "aid", &aid]);
            }
        }
    }
    fn set_subtitle(&mut self, track: Option<TrackRef>) {
        if let Some(m) = &self.mpv {
            if let Some(req) = &self.req {
                let sid = tracks::sid(req, track);
                m.command(&["set", "sid", &sid]);
            }
        }
    }
    fn stop(&mut self) {
        if let Some(m) = &self.mpv {
            m.command(&["stop"]);
        }
        self.state = PlayerState::Idle;
    }
    fn state(&self) -> PlayerState {
        self.state.clone()
    }
    fn position(&self) -> f64 {
        self.mpv.as_ref().and_then(|m| m.get("time-pos")).and_then(|s| s.parse().ok()).unwrap_or(0.0)
    }
    fn duration(&self) -> Option<f64> {
        self.mpv.as_ref().and_then(|m| m.get("duration")).and_then(|s| s.parse().ok())
    }
    fn draw_video(&mut self, w: u32, h: u32) {
        if let Some(m) = &self.mpv {
            m.render(0, w as i32, h as i32);
        }
    }
    /// G4 / P-PLAY-19: mpv `volume` is 0..100; the trait speaks 0.0..1.0.
    fn set_volume(&mut self, volume: f32) {
        if let Some(m) = &self.mpv {
            m.command(&["set", "volume", &format!("{}", (volume.clamp(0.0, 1.0) * 100.0).round())]);
        }
    }
    fn volume(&self) -> f32 {
        self.mpv
            .as_ref()
            .and_then(|m| m.get("volume"))
            .and_then(|s| s.parse::<f32>().ok())
            .map(|v| v / 100.0)
            .unwrap_or(1.0)
    }
    fn set_muted(&mut self, muted: bool) {
        if let Some(m) = &self.mpv {
            m.command(&["set", "mute", if muted { "yes" } else { "no" }]);
        }
    }
    fn muted(&self) -> bool {
        self.mpv.as_ref().and_then(|m| m.get("mute")).map(|s| s == "yes").unwrap_or(false)
    }
    /// P-PLAY-8: mpv `sub-delay` in seconds.
    fn set_sub_delay(&mut self, secs: f64) {
        if let Some(m) = &self.mpv {
            m.command(&["set", "sub-delay", &format!("{secs:.3}")]);
        }
    }
    /// P-PLAY-9: set the mpv subtitle options from `SubStyle::mpv_options` (unit-tested in core). The default
    /// style leaves every option at its mpv default.
    fn set_sub_style(&mut self, style: &SubStyle) {
        if let Some(m) = &self.mpv {
            for (name, value) in style.mpv_options() {
                m.command(&["set", name, &value]);
            }
        }
    }
    /// P-PLAY-7: mpv `secondary-sid`, mapped like `sid` (R-PLAY-6).
    fn set_secondary_subtitle(&mut self, track: Option<TrackRef>) {
        if let Some(m) = &self.mpv {
            if let Some(req) = &self.req {
                let sid = tracks::sid(req, track);
                m.command(&["set", "secondary-sid", &sid]);
            }
        }
    }
    /// P-PLAY-12: mpv `speed`.
    fn set_speed(&mut self, speed: f64) {
        if let Some(m) = &self.mpv {
            m.command(&["set", "speed", &format!("{:.2}", speed.clamp(0.25, 4.0))]);
        }
    }
    /// P-PLAY-14: mpv `video-aspect-override` + `panscan` (Fill crops to cover the window).
    fn set_aspect(&mut self, aspect: Aspect) {
        if let Some(m) = &self.mpv {
            let (over, pan) = match aspect {
                Aspect::Auto | Aspect::Fit => ("no", "0"),
                Aspect::Fill => ("no", "1.0"),
                Aspect::Wide16x9 => ("16:9", "0"),
                Aspect::Standard4x3 => ("4:3", "0"),
            };
            m.command(&["set", "keepaspect", "yes"]);
            m.command(&["set", "video-aspect-override", over]);
            m.command(&["set", "panscan", pan]);
        }
    }
    /// P-PLAY-20: mpv's own counters.
    fn stats(&self) -> PlayerStats {
        let Some(m) = &self.mpv else { return PlayerStats::default() };
        let num = |name: &str| m.get(name).and_then(|s| s.parse::<f64>().ok());
        PlayerStats {
            width: num("width").map(|v| v as u32),
            height: num("height").map(|v| v as u32),
            video_bitrate_bps: num("video-bitrate"),
            dropped_frames: num("frame-drop-count").map(|v| v as i64),
            buffer_secs: num("demuxer-cache-duration"),
        }
    }
}
