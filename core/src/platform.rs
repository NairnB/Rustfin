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
    /// NAV: re-open the screen Back left (mouse button 5, Alt+Right, Cmd+]).
    Forward,
    /// NAV: jump to Search (Cmd/Ctrl+F).
    Search,
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

/// Subtitle font family (P-PLAY-9): the mpv `sub-font` choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SubFont {
    #[default]
    Default,
    Sans,
    Serif,
    Mono,
}

impl SubFont {
    pub const ALL: [SubFont; 4] = [SubFont::Default, SubFont::Sans, SubFont::Serif, SubFont::Mono];

    pub fn label(self) -> &'static str {
        match self {
            SubFont::Default => "Default",
            SubFont::Sans => "Sans",
            SubFont::Serif => "Serif",
            SubFont::Mono => "Mono",
        }
    }
}

/// Subtitle text weight (P-PLAY-9): mpv `sub-bold`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SubWeight {
    #[default]
    Normal,
    Bold,
}

impl SubWeight {
    pub const ALL: [SubWeight; 2] = [SubWeight::Normal, SubWeight::Bold];

    pub fn label(self) -> &'static str {
        match self {
            SubWeight::Normal => "Normal",
            SubWeight::Bold => "Bold",
        }
    }
}

/// Subtitle drop shadow (P-PLAY-9): mpv `sub-shadow-offset` 0 / 1.5 / 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SubShadow {
    #[default]
    Off,
    Light,
    Heavy,
}

impl SubShadow {
    pub const ALL: [SubShadow; 3] = [SubShadow::Off, SubShadow::Light, SubShadow::Heavy];

    pub fn label(self) -> &'static str {
        match self {
            SubShadow::Off => "Off",
            SubShadow::Light => "Light",
            SubShadow::Heavy => "Heavy",
        }
    }
}

/// The eight named subtitle colours (name, `#RRGGBB`) offered for text and background (P-PLAY-9).
pub const SUB_COLOURS: [(&str, &str); 8] = [
    ("White", "#FFFFFF"),
    ("Yellow", "#FFFF00"),
    ("Lime", "#00FF00"),
    ("Cyan", "#00FFFF"),
    ("Blue", "#0000FF"),
    ("Red", "#FF0000"),
    ("Black", "#000000"),
    ("Grey", "#808080"),
];

/// Index into `SUB_COLOURS` of a `#RRGGBB` value (case-insensitive); None for a custom colour.
pub fn sub_colour_index(hex: &str) -> Option<usize> {
    SUB_COLOURS.iter().position(|(_, h)| h.eq_ignore_ascii_case(hex.trim()))
}

/// Parse a colour typed by the user: `#RRGGBB` or `RRGGBB`, any case, surrounding spaces ignored.
/// Returns the normalised upper-case `#RRGGBB`, or None when it is not six hex digits.
pub fn parse_hex_colour(s: &str) -> Option<String> {
    let s = s.trim();
    let digits = s.strip_prefix('#').unwrap_or(s);
    if digits.len() == 6 && digits.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(format!("#{}", digits.to_ascii_uppercase()))
    } else {
        None
    }
}

/// Subtitle appearance (P-PLAY-9 / P-USER-5). `position` is the mpv `sub-pos` value (0..150, 100 = bottom of
/// the screen, the mpv default). `back_colour` None = no background box. The default `SubStyle` leaves every
/// mpv subtitle option at its default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "SavedSubStyle")]
pub struct SubStyle {
    pub size: SubSize,
    pub position: f32,
    pub font: SubFont,
    pub weight: SubWeight,
    pub shadow: SubShadow,
    /// `#RRGGBB`.
    pub text_colour: String,
    /// `#RRGGBB`; None = no background box.
    pub back_colour: Option<String>,
}

impl Default for SubStyle {
    fn default() -> SubStyle {
        SubStyle {
            size: SubSize::Medium,
            position: 100.0,
            font: SubFont::Default,
            weight: SubWeight::Normal,
            shadow: SubShadow::Off,
            text_colour: "#FFFFFF".to_string(),
            back_colour: None,
        }
    }
}

/// A saved `SubStyle`, any field missing. Also reads the keys saved before P-PLAY-9 grew (`colour` White/Yellow,
/// `background` on/off), so an old preference carries over instead of resetting to white / no box.
#[derive(Deserialize, Default)]
#[serde(default)]
struct SavedSubStyle {
    size: Option<SubSize>,
    position: Option<f32>,
    font: Option<SubFont>,
    weight: Option<SubWeight>,
    shadow: Option<SubShadow>,
    text_colour: Option<String>,
    back_colour: Option<String>,
    colour: Option<String>,
    background: Option<bool>,
}

impl From<SavedSubStyle> for SubStyle {
    fn from(s: SavedSubStyle) -> SubStyle {
        let d = SubStyle::default();
        let old_text = s.colour.map(|c| if c == "Yellow" { "#FFFF00" } else { "#FFFFFF" }.to_string());
        let old_back = s.background.filter(|on| *on).map(|_| "#000000".to_string());
        SubStyle {
            size: s.size.unwrap_or(d.size),
            position: s.position.unwrap_or(d.position),
            font: s.font.unwrap_or(d.font),
            weight: s.weight.unwrap_or(d.weight),
            shadow: s.shadow.unwrap_or(d.shadow),
            text_colour: s.text_colour.or(old_text).unwrap_or(d.text_colour),
            back_colour: s.back_colour.or(old_back),
        }
    }
}

impl SubStyle {
    /// P-PLAY-9: the mpv options (name, value) for this style, in the order they are set. An unparseable
    /// colour falls back to white text / no background box.
    pub fn mpv_options(&self) -> Vec<(&'static str, String)> {
        let font_size = match self.size {
            SubSize::Small => "40",
            SubSize::Medium => "55",
            SubSize::Large => "70",
            SubSize::ExtraLarge => "85",
        };
        let font = match self.font {
            SubFont::Default => "sans-serif",
            SubFont::Sans => "sans-serif",
            SubFont::Serif => "serif",
            SubFont::Mono => "monospace",
        };
        let shadow = match self.shadow {
            SubShadow::Off => "0",
            SubShadow::Light => "1.5",
            SubShadow::Heavy => "3",
        };
        let text = parse_hex_colour(&self.text_colour).unwrap_or_else(|| "#FFFFFF".to_string());
        let mut v = vec![
            ("sub-font-size", font_size.to_string()),
            ("sub-pos", format!("{}", self.position.clamp(0.0, 150.0))),
            ("sub-font", font.to_string()),
            ("sub-bold", if self.weight == SubWeight::Bold { "yes" } else { "no" }.to_string()),
            ("sub-shadow-offset", shadow.to_string()),
            ("sub-shadow-color", "#000000".to_string()),
            ("sub-color", text),
        ];
        match self.back_colour.as_deref().and_then(parse_hex_colour) {
            Some(back) => {
                v.push(("sub-back-color", back));
                v.push(("sub-border-style", "background-box".to_string()));
            }
            None => {
                v.push(("sub-back-color", "#00000000".to_string()));
                v.push(("sub-border-style", "outline-and-shadow".to_string()));
            }
        }
        v
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

/// Video aspect (P-PLAY-14): the Aspect menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Aspect {
    #[default]
    Auto,
    Fit,
    Fill,
    Wide16x9,
    Standard4x3,
}

impl Aspect {
    pub const ALL: [Aspect; 5] = [Aspect::Auto, Aspect::Fit, Aspect::Fill, Aspect::Wide16x9, Aspect::Standard4x3];

    pub fn label(self) -> &'static str {
        match self {
            Aspect::Auto => "Auto",
            Aspect::Fit => "Fit",
            Aspect::Fill => "Fill",
            Aspect::Wide16x9 => "16:9",
            Aspect::Standard4x3 => "4:3",
        }
    }
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
    /// Second subtitle track shown above the primary one (P-PLAY-7). Default: no secondary subtitles.
    fn set_secondary_subtitle(&mut self, track: Option<TrackRef>) {
        let _ = track;
    }
    /// Playback speed, 1.0 = normal (P-PLAY-12). Default: no speed control.
    fn set_speed(&mut self, speed: f64) {
        let _ = speed;
    }
    /// Video aspect (P-PLAY-14). Default: no aspect control.
    fn set_aspect(&mut self, aspect: Aspect) {
        let _ = aspect;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn opt(v: &[(&'static str, String)], name: &str) -> String {
        v.iter().find(|(n, _)| *n == name).map(|(_, x)| x.clone()).unwrap()
    }

    #[test]
    fn hex_colours_parse() {
        assert_eq!(parse_hex_colour("#ff8000").as_deref(), Some("#FF8000"));
        assert_eq!(parse_hex_colour(" 00aaBB ").as_deref(), Some("#00AABB"));
        assert_eq!(parse_hex_colour("#FFF"), None);
        assert_eq!(parse_hex_colour("#GGGGGG"), None);
        assert_eq!(parse_hex_colour("#1234567"), None);
        assert_eq!(parse_hex_colour(""), None);
    }

    #[test]
    fn named_colours_are_valid_and_found() {
        for (_, h) in SUB_COLOURS {
            assert_eq!(parse_hex_colour(h).as_deref(), Some(h));
        }
        assert_eq!(sub_colour_index("#ffff00"), Some(1));
        assert_eq!(sub_colour_index("#123456"), None);
    }

    #[test]
    fn default_style_maps_to_mpv_defaults() {
        let v = SubStyle::default().mpv_options();
        assert_eq!(opt(&v, "sub-font"), "sans-serif");
        assert_eq!(opt(&v, "sub-bold"), "no");
        assert_eq!(opt(&v, "sub-shadow-offset"), "0");
        assert_eq!(opt(&v, "sub-color"), "#FFFFFF");
        assert_eq!(opt(&v, "sub-border-style"), "outline-and-shadow");
        assert_eq!(opt(&v, "sub-pos"), "100");
    }

    #[test]
    fn full_style_maps_every_option() {
        let s = SubStyle {
            size: SubSize::Large,
            position: 90.0,
            font: SubFont::Mono,
            weight: SubWeight::Bold,
            shadow: SubShadow::Heavy,
            text_colour: "#ffff00".into(),
            back_colour: Some("#202020".into()),
        };
        let v = s.mpv_options();
        assert_eq!(opt(&v, "sub-font-size"), "70");
        assert_eq!(opt(&v, "sub-font"), "monospace");
        assert_eq!(opt(&v, "sub-bold"), "yes");
        assert_eq!(opt(&v, "sub-shadow-offset"), "3");
        assert_eq!(opt(&v, "sub-shadow-color"), "#000000");
        assert_eq!(opt(&v, "sub-color"), "#FFFF00");
        assert_eq!(opt(&v, "sub-back-color"), "#202020");
        assert_eq!(opt(&v, "sub-border-style"), "background-box");
        let light = SubStyle { shadow: SubShadow::Light, font: SubFont::Serif, ..SubStyle::default() }.mpv_options();
        assert_eq!(opt(&light, "sub-shadow-offset"), "1.5");
        assert_eq!(opt(&light, "sub-font"), "serif");
    }

    #[test]
    fn bad_colours_fall_back() {
        let s = SubStyle { text_colour: "nope".into(), back_colour: Some("zzz".into()), ..SubStyle::default() };
        let v = s.mpv_options();
        assert_eq!(opt(&v, "sub-color"), "#FFFFFF");
        assert_eq!(opt(&v, "sub-border-style"), "outline-and-shadow");
    }

    #[test]
    fn old_saved_style_still_loads() {
        let s: SubStyle =
            serde_json::from_str(r#"{"size":"Large","position":90.0,"colour":"Yellow","background":true}"#).unwrap();
        assert_eq!(s.size, SubSize::Large);
        assert_eq!(s.font, SubFont::Default);
        assert_eq!(s.text_colour, "#FFFF00");
        assert_eq!(s.back_colour.as_deref(), Some("#000000"));
        let s: SubStyle = serde_json::from_str(r#"{"colour":"White","background":false}"#).unwrap();
        assert_eq!((s.text_colour.as_str(), s.back_colour), ("#FFFFFF", None));
        let new = SubStyle { back_colour: Some("#336699".into()), ..SubStyle::default() };
        assert_eq!(serde_json::from_str::<SubStyle>(&serde_json::to_string(&new).unwrap()).unwrap(), new);
    }
}
