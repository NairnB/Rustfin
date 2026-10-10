//! What the shell (desktop now, TV later) provides to the core (SPEC §2, R-PLAT-*).
use serde::{Deserialize, Serialize};
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
    /// Player shortcuts (G4): F = fullscreen, M = mute, N = next, P = previous, C = chapters.
    Fullscreen,
    Mute,
    Next,
    Previous,
    Chapters,
    VolumeUp,
    VolumeDown,
    /// G6: S = playback stats overlay.
    Stats,
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

/// Subtitle size tier (P-PLAY-9 / P-USER-5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SubSize {
    Small,
    #[default]
    Medium,
    Large,
    ExtraLarge,
}

/// Subtitle text colour (P-PLAY-9 / P-USER-5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SubColour {
    #[default]
    White,
    Yellow,
}

/// Subtitle appearance (P-PLAY-9 / P-USER-5). `position` is the mpv `sub-pos` value (0..150, 100 = bottom of
/// the screen, the mpv default). The default `SubStyle` leaves every mpv subtitle option at its default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubStyle {
    pub size: SubSize,
    pub position: f32,
    pub colour: SubColour,
    pub background: bool,
}

impl Default for SubStyle {
    fn default() -> SubStyle {
        SubStyle { size: SubSize::Medium, position: 100.0, colour: SubColour::White, background: false }
    }
}

/// G6 / P-PLAY-20: what the player knows about the running stream (every field optional).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PlayerStats {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub video_bitrate_bps: Option<f64>,
    pub dropped_frames: Option<i64>,
    pub buffer_secs: Option<f64>,
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
    /// 0.0–1.0. Default: no volume control.
    fn set_volume(&mut self, volume: f32) {
        let _ = volume;
    }
    fn volume(&self) -> f32 {
        1.0
    }
    fn set_muted(&mut self, muted: bool) {
        let _ = muted;
    }
    fn muted(&self) -> bool {
        false
    }
    /// Subtitle sync offset in seconds (P-PLAY-8). Default: no offset.
    fn set_sub_delay(&mut self, secs: f64) {
        let _ = secs;
    }
    /// Subtitle appearance (P-PLAY-9). Default: no style change.
    fn set_sub_style(&mut self, style: &SubStyle) {
        let _ = style;
    }
    /// Playback speed, 1.0 = normal (P-PLAY-12). Default: no speed control.
    fn set_speed(&mut self, speed: f64) {
        let _ = speed;
    }
    /// Stream facts for the stats overlay (P-PLAY-20). Default: nothing known.
    fn stats(&self) -> PlayerStats {
        PlayerStats::default()
    }
}

/// Things a screen asks of the window (read by the shell after each frame).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShellRequests {
    pub toggle_fullscreen: bool,
    /// Set while a screen wants fullscreen (the player); None = leave as it is.
    pub fullscreen: Option<bool>,
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
