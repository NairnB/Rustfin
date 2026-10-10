//! What the shell (desktop now, TV later) provides to the core (SPEC §2, R-PLAT-*).
use std::path::PathBuf;

/// R-PLAT-3: remote-style input. The shell maps its keys to these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Back,
    PlayPause,
    Info,
    Backspace,
    Text(char),
}

/// R-PLAT-1: a Jellyfin `MediaStream.Index`.
pub type TrackRef = i32;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ExternalSub {
    pub index: TrackRef,
    pub url: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PlayRequest {
    pub url: String,
    pub title: String,
    pub start_secs: f64,
    /// Embedded streams of the media source in Jellyfin order: (Index, kind is audio?)
    pub audio_indexes: Vec<TrackRef>,
    pub subtitle_indexes: Vec<TrackRef>,
    pub external_subs: Vec<ExternalSub>,
    pub audio: Option<TrackRef>,
    pub subtitle: Option<TrackRef>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum PlayerState {
    #[default]
    Idle,
    Loading,
    Playing,
    Paused,
    Ended,
    Failed(String),
}

/// R-PLAT-1
pub trait Player {
    fn load(&mut self, req: &PlayRequest);
    fn set_paused(&mut self, paused: bool);
    fn seek(&mut self, secs: f64);
    fn set_audio(&mut self, track: Option<TrackRef>);
    fn set_subtitle(&mut self, track: Option<TrackRef>);
    fn stop(&mut self);
    fn state(&self) -> PlayerState;
    fn position(&self) -> f64;
    fn duration(&self) -> Option<f64>;
    /// Draw the current frame into the bound framebuffer (size in physical pixels).
    fn draw_video(&mut self, w: u32, h: u32);
}

/// A player that does nothing (tests, render shots).
#[derive(Default)]
pub struct NullPlayer {
    pub state: PlayerState,
    pub pos: f64,
    pub last: Option<PlayRequest>,
}
impl Player for NullPlayer {
    fn load(&mut self, req: &PlayRequest) {
        self.last = Some(req.clone());
        self.pos = req.start_secs;
        self.state = PlayerState::Playing;
    }
    fn set_paused(&mut self, p: bool) {
        self.state = if p { PlayerState::Paused } else { PlayerState::Playing };
    }
    fn seek(&mut self, secs: f64) {
        self.pos = secs;
    }
    fn set_audio(&mut self, _: Option<TrackRef>) {}
    fn set_subtitle(&mut self, _: Option<TrackRef>) {}
    fn stop(&mut self) {
        self.state = PlayerState::Idle;
    }
    fn state(&self) -> PlayerState {
        self.state.clone()
    }
    fn position(&self) -> f64 {
        self.pos
    }
    fn duration(&self) -> Option<f64> {
        None
    }
    fn draw_video(&mut self, _: u32, _: u32) {}
}

/// R-PLAT-2: small key/value files + a cache directory. Must be `Send + Sync` (used from workers).
pub trait Storage: Send + Sync {
    fn read(&self, key: &str) -> Option<Vec<u8>>;
    /// Atomic replace (temp file + rename for files).
    fn write(&self, key: &str, data: &[u8]) -> std::io::Result<()>;
    fn cache_dir(&self) -> PathBuf;
}
