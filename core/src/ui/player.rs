//! Player screen (R-UI-10, R-PLAY-1 … R-PLAY-5, R-API-21/22). The shell draws the video under the UI
//! (`shows_video`); this screen only draws the OSD and the overlays on top of it. Built by packet P2.
use super::parts;
use super::widgets;
use crate::api::{
    image_width_bucket, secs_to_ticks, ticks_to_secs, ApiError, Chapter, ImageKind, Item, ItemKind, MediaStream,
    PlaybackInfo, PlaybackPlan, ProgressReport, Result, Segment, SegmentKind, StreamKind, TrickplayInfo,
};
use crate::app::{Cx, Screen};
use crate::nav::{move_focus, Focus, Group};
use crate::platform::{
    sub_colour_index, Aspect, Key, PlayRequest, PlayerState, PlayerStats, SubFont, SubShadow, SubSize, SubStyle,
    SubWeight, SUB_COLOURS,
};
use crate::playreq::{build_plan_burnt_on, must_transcode, request_indexes_on};
use crate::queue::Queue;
use crate::repeat::{end_action, EndAction, Repeat};
use crate::settings::Settings;
use crate::theme::{Theme, Weight};
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
    /// FIX-4: the viewer chose `subtitle` themselves, so an audio change must not re-apply the default one.
    pub subtitle_picked: bool,
    /// FIX-4: the viewer's remembered subtitle for this item, used when `subtitle` is None; the player fills it in
    /// from the settings (`Cx::remembered_subtitle`).
    pub remembered_subtitle: Option<crate::settings::SubPick>,
}

/// C3-SUBS / R-SUBS-3: while transcoding the audio track and any burnt-in subtitle are part of the stream, so a
/// track change asks the server for a new stream (at the current position); direct play switches the track in place.
pub fn track_change_replans(transcoding: bool) -> bool {
    transcoding
}

/// R-SUBS-3: a subtitle change re-plans only when the burnt-in stream changes (`before` / `after` = the subtitle
/// index each request names): switching between Off and an external subtitle leaves the HLS stream as it is.
pub fn subtitle_change_replans(transcoding: bool, before: Option<i32>, after: Option<i32>) -> bool {
    transcoding && before != after
}

/// FIX-3 T3: as `subtitle_change_replans` for a backend that may not draw subtitles itself (`renders_subtitles`):
/// there a chosen subtitle needs a burnt-in stream, so a change re-plans whether the stream transcodes or not,
/// whenever the subtitle the request names changes.
pub fn subtitle_change_replans_on(
    renders_subtitles: bool,
    transcoding: bool,
    before: Option<i32>,
    after: Option<i32>,
) -> bool {
    if renders_subtitles {
        subtitle_change_replans(transcoding, before, after)
    } else {
        before != after
    }
}

/// The `force` a track-change re-plan asks the server with: a stream that only transcodes because direct play
/// failed (`fell_back`) must keep transcoding, or the server answers with the direct url mpv already failed on.
pub fn track_change_force(transcoding: bool, fell_back: bool) -> bool {
    transcoding && fell_back
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
/// FIX-1 #5: the Audio | Subtitles segmented control at the top of the sheet.
const G_TABS: usize = 3;
/// UXP2: the audio list of the Audio & Subtitles sheet (its Audio tab).
const G_AUDIO: usize = 4;
const G_LIST: usize = 5;
/// FIX-1 #5: the second-subtitles list (Subtitle options…).
const G_SEC: usize = 6;
const G_SUB_OFFSET: usize = 7;
const G_SUB_POS: usize = 8;
const G_SUB_SIZE: usize = 9;
const G_SUB_FONT: usize = 10;
const G_SUB_WEIGHT: usize = 11;
const G_SUB_SHADOW: usize = 12;
const G_SUB_TEXT: usize = 13;
const G_SUB_BACK: usize = 14;
const GROUP_COUNT: usize = 15;

/// FIX-1 #5: which view of the Audio & Subtitles sheet is showing.
const SUB_TAB_AUDIO: u8 = 0;
const SUB_TAB_SUBS: u8 = 1;
const SUB_TAB_OPTIONS: u8 = 2;

/// FIX-1 #5: a row of the Subtitles list — a track (index into the subtitle entries, "Off" first), or the
/// "Subtitle options…" row after the last track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetRow {
    Track(usize),
    Options,
}

/// FIX-1 #5: what row `i` of the Subtitles list is, with `tracks` entries before the options row.
pub fn sheet_row(i: usize, tracks: usize) -> Option<SheetRow> {
    match i.cmp(&tracks) {
        std::cmp::Ordering::Less => Some(SheetRow::Track(i)),
        std::cmp::Ordering::Equal => Some(SheetRow::Options),
        std::cmp::Ordering::Greater => None,
    }
}

/// One subtitle-panel focus group as drawn: (group, x of each entry, hovered entry, clicked entry).
type PanelGroup = (usize, Vec<f32>, Option<usize>, Option<usize>);

const B_PLAY: usize = 1;
const B_VOL_MUTE: usize = 0;
const B_VOL_BAR: usize = 1;
const B_VOL_QUEUE: usize = 2;

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

/// FIX-1 #6: an audio track's channel layout in words ("Mono", "Stereo", "5.1 Surround", "7.1 Surround").
pub fn channel_words(s: &MediaStream) -> Option<String> {
    let from_layout = s.channel_layout.as_deref().map(|l| l.to_lowercase()).and_then(|l| {
        if l.starts_with("7.1") {
            Some(8)
        } else if l.starts_with("5.1") {
            Some(6)
        } else if l.contains("stereo") {
            Some(2)
        } else if l.contains("mono") {
            Some(1)
        } else {
            None
        }
    });
    Some(match s.channels.or(from_layout)? {
        1 => "Mono".to_string(),
        2 => "Stereo".to_string(),
        6 => "5.1 Surround".to_string(),
        8 => "7.1 Surround".to_string(),
        n => format!("{n} channels"),
    })
}

/// FIX-1 #6: the codec a person may know by name ("Dolby Digital", "DTS"), or its short name in capitals.
pub fn codec_words(s: &MediaStream) -> Option<String> {
    let c = s.codec.as_deref()?.to_lowercase();
    Some(
        match c.as_str() {
            "ac3" => "Dolby Digital",
            "eac3" => "Dolby Digital Plus",
            "truehd" => "Dolby TrueHD",
            "dts" => "DTS",
            "aac" => "AAC",
            "mp3" => "MP3",
            "flac" => "FLAC",
            "opus" => "Opus",
            "vorbis" => "Vorbis",
            _ => return Some(c.to_uppercase()),
        }
        .to_string(),
    )
}

/// FIX-1 #1: seconds the start cover takes to fade into the first frame.
const COVER_FADE: f64 = 0.4;

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

/// RR3: the buttons of the queue panel.
#[derive(Clone, Copy)]
enum QueueAct {
    MoveUp,
    MoveDown,
    Remove,
    Close,
}

/// RR3: how a queue row reads — "S1E3 · Name" for an episode, the name otherwise.
pub fn queue_row_label(item: &Item) -> String {
    match (&item.kind, item.parent_index_number, item.index_number) {
        (ItemKind::Episode, Some(s), Some(e)) => format!("S{s}E{e} · {}", item.name),
        _ => item.name.clone(),
    }
}

/// UXP2 (D4, R1): which kind of control holds the OSD focus, for the key model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsdFocus {
    Timeline,
    VolumeBar,
    Other,
}

/// UXP2: what a key does in the player (before any menu or panel is open).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OsdAct {
    /// Seek by this many seconds (negative = back).
    Seek(f64),
    /// Show the OSD, focus on the timeline.
    Wake,
    /// FIX-1 #16: show the OSD with the button row focused (Down while hidden).
    WakeButtons,
    HideOsd,
    Leave,
    /// Arrow keys move the focus.
    Move,
    Activate,
    /// Change the volume by this fraction.
    Volume(f32),
    None,
}

/// UXP2: the remote-first key model. OSD hidden: Left/Right seek, Up/Down/Enter wake it. OSD shown: the timeline
/// is focused first and Left/Right scrub it, Down reaches the buttons; Back hides the OSD, then leaves.
pub fn osd_key(visible: bool, focus: OsdFocus, key: Key, skip_back: f64, skip_forward: f64) -> OsdAct {
    let seek = |k: Key| OsdAct::Seek(if k == Key::Left { -skip_back } else { skip_forward });
    match (visible, key) {
        (false, Key::Left | Key::Right) => seek(key),
        (false, Key::Down) => OsdAct::WakeButtons,
        (false, Key::Up | Key::Enter) => OsdAct::Wake,
        (false, Key::Back) => OsdAct::Leave,
        (true, Key::Back) => OsdAct::HideOsd,
        (true, Key::Left | Key::Right | Key::Up | Key::Down) if focus == OsdFocus::VolumeBar => {
            OsdAct::Volume(if matches!(key, Key::Left | Key::Down) { -0.05 } else { 0.05 })
        }
        (true, Key::Left | Key::Right) if focus == OsdFocus::Timeline => seek(key),
        (true, Key::Left | Key::Right | Key::Up | Key::Down) => OsdAct::Move,
        (true, Key::Enter) => OsdAct::Activate,
        _ => OsdAct::None,
    }
}

/// FIX-1 #3: seconds the OSD stays up when playback starts, so the title and tracks are seen.
pub const OSD_START_SECS: f64 = 5.0;
/// FIX-1 #16: seconds without input before the OSD hides while a button (not the timeline) has focus.
pub const OSD_BUTTON_HOLD_SECS: f64 = 10.0;

/// FIX-2 #17: seconds the trickplay thumbnail stays up after a seek from the remote/keyboard.
pub const TRICKPLAY_HOLD_SECS: f64 = 2.0;

/// FIX-2 #17: the trickplay thumbnail shows while the pointer is over the bar, or while seeking with the bar
/// focused (up to TRICKPLAY_HOLD_SECS after the last seek) — not merely because the bar has focus (start of play).
pub fn show_trickplay(hovering: bool, focus_bar: bool, now: f64, last_seek: Option<f64>) -> bool {
    hovering || (focus_bar && last_seek.is_some_and(|t| now - t <= TRICKPLAY_HOLD_SECS))
}

/// FIX-1 #3: the "what's playing" line under the title: the audio track, plus the subtitles when they are on.
pub fn now_playing_line(audio: &str, subtitle: Option<&str>) -> String {
    match subtitle {
        Some(s) => format!("{audio} · {s} subtitles"),
        None => audio.to_string(),
    }
}

/// UXP2: how far (in mpv `sub-pos` percent) subtitles are lifted while the OSD is up.
pub const OSD_SUB_LIFT: f32 = 25.0;

/// UXP2: the `sub-pos` to use: the chosen position, but no lower than the top of the OSD while it is up.
pub fn lifted_sub_position(position: f32, osd_up: bool) -> f32 {
    if osd_up {
        position.min(100.0 - OSD_SUB_LIFT)
    } else {
        position
    }
}

/// FIX-4: a click or Enter on entry `i` of subtitle-panel group `group` that is one of the Subtitle style rows
/// (position, size, font, weight, shadow, text and background colour) changes `style` and returns true. Anything
/// else (offset, second subtitles, tracks) leaves it alone and returns false.
fn pick_sub_style(style: &mut SubStyle, group: usize, i: usize) -> bool {
    let delta = if i == 0 { -1.0 } else { 1.0 };
    match group {
        G_SUB_SIZE => {
            style.size = match i {
                0 => SubSize::Small,
                1 => SubSize::Medium,
                2 => SubSize::Large,
                _ => SubSize::ExtraLarge,
            }
        }
        G_SUB_POS => style.position = (style.position + 5.0 * delta).clamp(0.0, 150.0),
        G_SUB_FONT => style.font = SubFont::ALL[i.min(SubFont::ALL.len() - 1)],
        G_SUB_WEIGHT => style.weight = SubWeight::ALL[i.min(SubWeight::ALL.len() - 1)],
        G_SUB_SHADOW => style.shadow = SubShadow::ALL[i.min(SubShadow::ALL.len() - 1)],
        G_SUB_TEXT => style.text_colour = SUB_COLOURS[i.min(SUB_COLOURS.len() - 1)].1.to_string(),
        G_SUB_BACK => {
            style.back_colour = i.checked_sub(1).map(|c| SUB_COLOURS[c.min(SUB_COLOURS.len() - 1)].1.to_string())
        }
        _ => return false,
    }
    true
}

/// FIX-4: `pick_sub_style`, and a style row that changed is also the saved default for every later playback.
fn save_sub_pick(settings: &mut Settings, style: &mut SubStyle, group: usize, i: usize) -> bool {
    let changed = pick_sub_style(style, group, i);
    if changed {
        settings.subtitles.style = style.clone();
    }
    changed
}

/// UXP2 (D14): English name of an ISO 639 language code (2- or 3-letter), `None` when unknown.
fn language_name(code: &str) -> Option<&'static str> {
    Some(match code.to_lowercase().as_str() {
        "eng" | "en" => "English",
        "ger" | "deu" | "de" => "German",
        "fre" | "fra" | "fr" => "French",
        "spa" | "es" => "Spanish",
        "ita" | "it" => "Italian",
        "por" | "pt" => "Portuguese",
        "dut" | "nld" | "nl" => "Dutch",
        "swe" | "sv" => "Swedish",
        "nor" | "no" | "nob" | "nb" => "Norwegian",
        "dan" | "da" => "Danish",
        "fin" | "fi" => "Finnish",
        "pol" | "pl" => "Polish",
        "rus" | "ru" => "Russian",
        "ukr" | "uk" => "Ukrainian",
        "cze" | "ces" | "cs" => "Czech",
        "slo" | "slk" | "sk" => "Slovak",
        "hun" | "hu" => "Hungarian",
        "rum" | "ron" | "ro" => "Romanian",
        "bul" | "bg" => "Bulgarian",
        "gre" | "ell" | "el" => "Greek",
        "tur" | "tr" => "Turkish",
        "heb" | "he" => "Hebrew",
        "ara" | "ar" => "Arabic",
        "hin" | "hi" => "Hindi",
        "jpn" | "ja" => "Japanese",
        "kor" | "ko" => "Korean",
        "chi" | "zho" | "zh" => "Chinese",
        "tha" | "th" => "Thai",
        "vie" | "vi" => "Vietnamese",
        "ind" | "id" => "Indonesian",
        "gla" | "gd" => "Gaelic",
        "wel" | "cym" | "cy" => "Welsh",
        "und" | "zxx" | "mul" => return None,
        _ => return None,
    })
}

/// UXP2 (D14): names a person reads for one kind of stream ("German", "English (embedded)", "English — Forced"),
/// in the order given. The server's display title is jargon ("Ger - SUBRIP - External") and is not used. A
/// language that appears twice is told apart as embedded / external, then numbered.
pub fn track_labels(streams: &[MediaStream]) -> Vec<String> {
    let base: Vec<String> = streams
        .iter()
        .map(|s| {
            let name = s
                .language
                .as_deref()
                .and_then(|l| {
                    language_name(l).map(String::from).or_else(|| (!l.is_empty() && l.len() > 3).then(|| capitalise(l)))
                })
                .or_else(|| s.title.clone().filter(|t| !t.trim().is_empty()))
                .unwrap_or_else(|| format!("Track {}", s.index));
            if s.is_forced {
                format!("{name} — Forced")
            } else {
                name
            }
        })
        .collect();
    let count = |v: &[String], l: &str| v.iter().filter(|x| x.as_str() == l).count();
    let mut out: Vec<String> = streams
        .iter()
        .zip(&base)
        .map(|(s, b)| {
            let twins: Vec<&MediaStream> = streams.iter().zip(&base).filter(|(_, o)| *o == b).map(|(t, _)| t).collect();
            // FIX-1 #6: tell twins apart by what really differs, in plain words: the channel layout first
            // ("English — 5.1 Surround" / "English — Stereo"), then the codec ("Dolby Digital"), then where it lives.
            let differ = |f: &dyn Fn(&MediaStream) -> Option<String>| twins.iter().any(|t| f(t) != f(s));
            if twins.len() > 1 && differ(&channel_words) && channel_words(s).is_some() {
                format!("{b} — {}", channel_words(s).unwrap_or_default())
            } else if twins.len() > 1 && differ(&codec_words) && codec_words(s).is_some() {
                format!("{b} — {}", codec_words(s).unwrap_or_default())
            } else if twins.len() > 1 && twins.iter().any(|t| t.is_external != s.is_external) {
                format!("{b} ({})", if s.is_external { "external" } else { "embedded" })
            } else {
                b.clone()
            }
        })
        .collect();
    let snapshot = out.clone();
    let mut seen: Vec<(String, usize)> = vec![];
    for l in out.iter_mut() {
        if count(&snapshot, l) > 1 {
            let n = match seen.iter_mut().find(|(k, _)| k == l) {
                Some((_, n)) => {
                    *n += 1;
                    *n
                }
                None => {
                    seen.push((l.clone(), 1));
                    1
                }
            };
            *l = format!("{l} {n}");
        }
    }
    out
}

/// UXP2: the rows of the Info panel — what is playing, how far, and the chosen tracks.
pub fn now_playing_rows(title: &str, pos: f64, dur: f64, audio: &str, subtitles: &str) -> Vec<(&'static str, String)> {
    vec![
        ("Playing", title.to_string()),
        ("Time", format!("{} / {}", parts::clock(pos), parts::clock(dur))),
        ("Audio", audio.to_string()),
        ("Subtitles", subtitles.to_string()),
    ]
}

#[derive(Clone)]
enum Action {
    Seek(f64),
    TogglePause,
    /// UXP2: the one Audio & Subtitles panel.
    OpenAudioSubs,
    /// UXP2: the "…" More panel (Quality, Speed, Aspect, Repeat, Chapters, Stats, Queue, Previous episode).
    OpenMore,
    /// UXP2: ✖ — leave the player.
    Leave,
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
    OpenQueue,
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
    /// FIX-4: the viewer chose the subtitle (on the page or here); an audio change leaves it alone.
    sub_picked: bool,
    /// The Audio & Subtitles panel (UXP2: audio and subtitle lists in one panel).
    subtitle_open: bool,
    /// UXP2: the "…" More panel.
    more_open: bool,
    /// UXP2: subtitles are lifted above the OSD (the `sub-pos` last sent was the lifted one).
    sub_lifted: bool,
    /// UXP2: Back hid the OSD while paused (a paused player otherwise keeps it up).
    osd_dismissed: bool,
    /// FIX-1 #3: the OSD is up only because playback just started; keys act as if it were hidden.
    osd_auto: bool,
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
    /// FIX-3 T1: the item + options of a start whose plan came back direct but needs a burnt-in subtitle.
    sub_retry: Option<(Item, PlayOptions)>,
    source_bitrate: Option<i64>,
    // next episode
    next: Option<Item>,
    next_known: bool,
    next_loading: bool,
    // previous episode (G4 / P-PLAY-16 / P-TV-9)
    prev: Option<Item>,
    prev_known: bool,
    prev_loading: bool,
    /// G9 / RR3 (P-PLAY-22): the play queue. Explicit (shuffle, play all) it decides next/previous; for a single
    /// episode it is the season, and the server's next episode still takes over past its end.
    queue: Option<Queue<Item>>,
    queue_explicit: bool,
    queue_open: bool,
    /// The queue row the Move / Remove buttons act on (the last focused row).
    queue_sel: usize,
    /// Scroll the focused queue row into view on the next frame (set by keys and on open).
    queue_scroll: bool,
    /// FIX-1 #1: when the first video frame showed (the start cover fades out from then).
    first_frame_at: Option<f64>,
    /// D4: the focus the Audio & Subtitles card last scrolled to (scroll again when it moves).
    panel_seen: Option<Focus>,
    /// FIX-1 #5: the sheet's view (SUB_TAB_*).
    sub_tab: u8,
    up_next: Option<Item>,
    up_next_deadline: Option<f64>,
    handling_end: bool,
    pending_end: bool,
    // G10 / P-PLAY-11: media segments of the playing item, and the ones already auto-skipped
    segments: Vec<Segment>,
    auto_skipped: Vec<usize>,
    // RR4 / P-PLAY-21: trickplay info of the playing item (None = no thumbnails)
    trickplay: Option<TrickplayInfo>,
    /// FIX-2 #17: when the last seek happened (cx.now), for the trickplay thumbnail.
    last_seek: Option<f64>,
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
    /// P-PLAY-7: the secondary subtitle (a `MediaStream.Index`), None = off.
    sec_subtitle: Option<i32>,
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
    screen_queue_at(items, 0, opts)
}

/// RR6 (P-ITEM-16): like `screen_queue` but starts at `start` (a multi-part film opened at its second part still
/// has part 1 before it). Out of range → the first item.
pub fn screen_queue_at(items: Vec<Item>, start: usize, opts: PlayOptions) -> Box<dyn Screen> {
    let start = if start < items.len() { start } else { 0 };
    let mut p = new_player(items.get(start).cloned().unwrap_or_default(), opts);
    p.queue = Some(Queue::new(items, start));
    p.queue_explicit = true;
    Box::new(p)
}

fn new_player(item: Item, opts: PlayOptions) -> PlayerScreen {
    PlayerScreen {
        sub_picked: opts.subtitle_picked,
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
        subtitle_open: false,
        more_open: false,
        sub_lifted: false,
        osd_dismissed: false,
        osd_auto: false,
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
        sub_retry: None,
        source_bitrate: None,
        next: None,
        next_known: false,
        next_loading: false,
        prev: None,
        prev_known: false,
        prev_loading: false,
        queue: None,
        queue_explicit: false,
        queue_open: false,
        queue_sel: 0,
        queue_scroll: false,
        first_frame_at: None,
        panel_seen: None,
        sub_tab: SUB_TAB_SUBS,
        up_next: None,
        up_next_deadline: None,
        handling_end: false,
        pending_end: false,
        segments: vec![],
        auto_skipped: vec![],
        trickplay: None,
        last_seek: None,
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
        sec_subtitle: None,
        focus: Focus { group: G_BAR, index: 0 },
        groups: vec![],
        started: false,
    }
}

impl PlayerScreen {
    /// The queue neighbour of the current item (`step` = 1 next, -1 previous); `None` outside a queue.
    fn queue_neighbour(&self, step: isize) -> Option<Option<Item>> {
        let q = self.queue.as_ref()?;
        let at = if step > 0 { q.next(Repeat::Off) } else { q.previous() };
        Some(at.and_then(|i| q.get(i)).cloned())
    }

    fn source(&self) -> Option<&crate::api::MediaSource> {
        self.info.as_ref().and_then(|i| crate::playreq::pick_source(i, self.source_id.as_deref()))
    }

    /// UXP2 (D14): (label, stream index) for each stream of `kind`, labels as `track_labels` reads them.
    fn track_entries(&self, kind: StreamKind) -> Vec<(String, i32)> {
        let Some(ms) = self.source() else { return vec![] };
        let streams: Vec<MediaStream> = ms.media_streams.iter().filter(|s| s.kind == kind).cloned().collect();
        track_labels(&streams).into_iter().zip(streams.iter().map(|s| s.index)).collect()
    }

    fn audio_entries(&self) -> Vec<(String, i32)> {
        self.track_entries(StreamKind::Audio)
    }

    fn subtitle_entries(&self) -> Vec<(String, Option<i32>)> {
        let mut v = vec![("Off".to_string(), None)];
        v.extend(self.track_entries(StreamKind::Subtitle).into_iter().map(|(l, i)| (l, Some(i))));
        v
    }

    /// UXP2: the audio stream playing now (the choice, else the source's default, else the first).
    fn current_audio(&self) -> Option<i32> {
        let entries = self.audio_entries();
        let known = |i: &i32| entries.iter().any(|(_, x)| x == i);
        self.audio
            .filter(known)
            .or_else(|| self.source().and_then(|ms| ms.default_audio_stream_index).filter(known))
            .or_else(|| entries.first().map(|(_, i)| *i))
    }

    fn any_menu(&self) -> bool {
        self.more_open
            || self.subtitle_open
            || self.chapters_open
            || self.quality_open
            || self.speed_open
            || self.aspect_open
            || self.repeat_open
            || self.queue_open
    }

    fn close_menus(&mut self) {
        self.more_open = false;
        self.subtitle_open = false;
        self.chapters_open = false;
        self.quality_open = false;
        self.speed_open = false;
        self.aspect_open = false;
        self.repeat_open = false;
        self.queue_open = false;
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

    /// FIX-1 #16: the OSD waits longer before hiding while a button has focus.
    fn osd_hide_secs(&self, t: &Theme) -> f64 {
        if self.focus.group == G_BTN {
            OSD_BUTTON_HOLD_SECS
        } else {
            t.osd_hide_after as f64
        }
    }

    /// FIX-1 #3: subtitles by name when they are on.
    fn subtitles_on(&self) -> Option<String> {
        match self.subtitle {
            Some(Some(_)) => Some(self.subtitle_label()),
            _ => None,
        }
    }

    /// UXP2: the audio track playing now, by name (the choice, else the source's default).
    fn audio_label(&self) -> String {
        self.current_audio()
            .and_then(|i| self.audio_entries().into_iter().find(|(_, x)| *x == i).map(|(l, _)| l))
            .unwrap_or_else(|| "Default".to_string())
    }

    fn subtitle_label(&self) -> String {
        match self.subtitle {
            Some(Some(i)) => self
                .subtitle_entries()
                .into_iter()
                .find(|(_, x)| *x == Some(i))
                .map(|(l, _)| l)
                .unwrap_or_else(|| "Default".to_string()),
            Some(None) => "Off".to_string(),
            None => "Default".to_string(),
        }
    }

    /// UXP2: the OSD button row — short, so it fits at 1440 and 1920. The Next episode slot is always there for an
    /// episode (it never moves between episodes); everything else lives in the More panel.
    fn buttons(&self, play_label: &str) -> Vec<(String, Action)> {
        let mut v = vec![
            (format!("⏪ {} s", self.skip_back as i64), Action::Seek(-self.skip_back)),
            (play_label.to_string(), Action::TogglePause),
            (format!("{} s ⏩", self.skip_forward as i64), Action::Seek(self.skip_forward)),
            ("Audio & Subtitles".to_string(), Action::OpenAudioSubs),
        ];
        if self.item.kind == ItemKind::Episode || self.next.is_some() {
            v.push(("Next episode".to_string(), Action::PlayNext));
        }
        v.push(("More…".to_string(), Action::OpenMore));
        v.push(("Fullscreen".to_string(), Action::ToggleFullscreen));
        v.push(("✖".to_string(), Action::Leave));
        v
    }

    /// UXP2: the More panel's entries — what the old button row held besides the buttons above.
    fn more_entries(&self) -> Vec<(String, Action)> {
        let mut v = vec![];
        if self.prev.is_some() {
            v.push(("Previous episode".to_string(), Action::PlayPrevious));
        }
        v.push((format!("Quality: {}", quality_label(self.quality_kbps, self.settings_kbps)), Action::OpenQuality));
        v.push((format!("Speed: {}×", self.speed), Action::OpenSpeed));
        v.push((format!("Aspect: {}", self.aspect.label()), Action::OpenAspect));
        v.push((format!("Repeat: {}", self.repeat.label()), Action::OpenRepeat));
        if !self.item.chapters.is_empty() {
            v.push(("Chapters".to_string(), Action::OpenChapters));
        }
        v.push((if self.stats_open { "Hide stats" } else { "Stats" }.to_string(), Action::ToggleStats));
        if self.queue.as_ref().is_some_and(|q| q.len() > 1) {
            v.push(("Queue".to_string(), Action::OpenQueue));
        }
        v
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
        let renders = cx.player.renders_subtitles();
        let (audio, sub) = request_indexes_on(renders, None, None, self.opts.audio, self.opts.subtitle);
        let force = must_transcode(renders, self.opts.subtitle.flatten());
        self.fetch_plan(cx, "info", id, force, audio, sub);
    }

    /// G6: PlaybackInfo for `id` under the current quality choice (or the Settings cap); `force` = transcode.
    /// `audio` / `sub` = the chosen stream indexes the request names (R-SUBS-1), `None` = the server's default.
    fn fetch_plan(
        &self,
        cx: &mut Cx,
        tag: &'static str,
        id: String,
        force: bool,
        audio: Option<i32>,
        sub: Option<i32>,
    ) {
        let cap = self.quality_kbps.unwrap_or(cx.settings.max_streaming_bitrate);
        let fmp4 = cx.settings.prefer_fmp4;
        let client = cx.api();
        cx.spawn(tag, move || client.playback_plan_for(&id, cap, force, fmp4, audio, sub));
    }

    /// G6: load the same item again through a new plan, at the current position (quality change, or the
    /// fallback to transcoding when the direct stream would not open).
    fn reload(&mut self, cx: &mut Cx, force: bool) {
        self.reload_pos = cx.player.position();
        self.loading = true;
        let id = self.item.id.clone();
        let renders = cx.player.renders_subtitles();
        let (audio, sub) =
            request_indexes_on(renders, self.info.as_ref(), self.source_id.as_deref(), self.audio, self.subtitle);
        // FIX-3 T1: a backend that cannot draw the chosen subtitle needs the transcode that burns it in.
        let force = force || must_transcode(renders, self.subtitle.flatten());
        self.fetch_plan(cx, "reload", id, force, audio, sub);
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
            subtitle_picked: self.sub_picked,
            remembered_subtitle: cx.remembered_subtitle(&self.item),
        };
        let renders = cx.player.renders_subtitles();
        let Some((mut req, burnt)) =
            build_plan_burnt_on(renders, &client, &self.item, &plan, &opts, &cx.settings.subtitles)
        else {
            self.loading = false;
            self.build_error = Some("No playable source on the server".to_string());
            return;
        };
        req.start_secs = self.reload_pos;
        // The burnt-in subtitle stays the chosen one (header, menu, next re-plan).
        self.subtitle = Some(burnt.or(req.subtitle));
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
            if n.is_some() || self.queue_explicit {
                self.next = n;
                self.next_known = true;
                return;
            }
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
            if p.is_some() || self.queue_explicit {
                self.prev = p;
                self.prev_known = true;
                return;
            }
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
        let opts = PlayOptions { remembered_subtitle: cx.remembered_subtitle(&item), ..opts };
        self.start_playback_inner(cx, item, plan, opts, false);
    }

    /// `retried`: this plan is already the forced answer to a `sub_retry`, so it is played as it is.
    fn start_playback_inner(&mut self, cx: &mut Cx, item: Item, plan: PlaybackPlan, opts: PlayOptions, retried: bool) {
        let client = cx.api();
        let renders = cx.player.renders_subtitles();
        let req = build_plan_burnt_on(renders, &client, &item, &plan, &opts, &cx.settings.subtitles);
        if let (false, false, Some((r, _))) = (retried, plan.transcoding_url.is_some(), &req) {
            // FIX-3 T1: the subtitle chosen by the user's preferences came after the request; a backend that cannot
            // draw it asks again for the transcode that burns it in.
            if must_transcode(renders, r.subtitle) {
                let (audio, sub) = request_indexes_on(
                    renders,
                    Some(&plan.info),
                    opts.media_source_id.as_deref(),
                    r.audio,
                    Some(r.subtitle),
                );
                let retry_opts = PlayOptions { audio: r.audio, subtitle: Some(r.subtitle), ..opts };
                let id = item.id.clone();
                self.sub_retry = Some((item, retry_opts));
                self.loading = true;
                self.fetch_plan(cx, "subretry", id, true, audio, sub);
                return;
            }
        }
        let info = plan.info;
        match req {
            Some((req, burnt)) => {
                self.source_id = opts.media_source_id.clone();
                self.sub_picked = opts.subtitle_picked;
                self.loading = false;
                self.build_error = None;
                self.player_error = None;
                self.item = item;
                self.info = Some(info);
                self.transcoding = plan.transcoding_url.is_some();
                self.source_bitrate = plan.source_bitrate;
                self.fell_back = false;
                self.audio = req.audio;
                self.subtitle = Some(burnt.or(req.subtitle));
                self.req = Some(req.clone());
                self.reporter = Reporter::new();
                self.stop_sent = false;
                self.handling_end = false;
                self.pending_end = false;
                self.segments = vec![];
                self.auto_skipped = vec![];
                self.trickplay = None;
                self.last_seek = None;
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
                self.sec_subtitle = None;
                // FIX-1 #3: the OSD shows what's playing for OSD_START_SECS, timeline focused.
                self.osd_visible = true;
                self.osd_last_input = cx.now + OSD_START_SECS - cx.theme.osd_hide_after as f64;
                self.osd_auto = true;
                self.focus = Focus { group: G_BAR, index: 0 };
                cx.player.load(&req);
                cx.player.set_sub_style(&self.sub_style);
                cx.player.set_sub_delay(0.0);
                cx.player.set_secondary_subtitle(None);
                cx.player.set_speed(self.speed);
                cx.player.set_aspect(self.aspect);
                self.sync_queue(cx);
                self.ensure_next(cx);
                self.ensure_prev(cx);
                {
                    let id = self.item.id.clone();
                    let client = cx.api();
                    cx.spawn("trickplay", move || client.trickplay_info(&id).map(|t| (id, t)));
                }
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
                let first = self
                    .queue
                    .as_ref()
                    .and_then(|q| q.next(Repeat::All).and_then(|i| q.get(i)))
                    .filter(|f| f.id != self.item.id)
                    .cloned();
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
        self.fetch_plan(cx, "nextinfo", id, false, None, None);
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
        self.fetch_plan(cx, "previnfo", id, false, None, None);
    }

    /// RR3: make the queue's current item the one now playing; a single episode with no queue (or one from another
    /// season) gets its season as the default queue.
    fn sync_queue(&mut self, cx: &mut Cx) {
        let id = self.item.id.clone();
        if let Some(q) = &mut self.queue {
            if let Some(i) = q.items().iter().position(|x| x.id == id) {
                q.jump(i);
                return;
            }
            if self.queue_explicit {
                return;
            }
            self.queue = None;
        }
        if let (ItemKind::Episode, Some(series), Some(season)) =
            (&self.item.kind, self.item.series_id.clone(), self.item.season_id.clone())
        {
            let client = cx.api();
            cx.spawn("season", move || client.episodes(&series, Some(&season)).map(|v| (id, v)));
        }
    }

    /// RR3: the queue changed under the current item; work out next again.
    fn queue_changed(&mut self, cx: &mut Cx) {
        self.next = None;
        self.next_known = false;
        self.ensure_next(cx);
    }

    /// RR3: jump to queue item `i` — report stopped, then start it.
    fn queue_jump(&mut self, cx: &mut Cx, i: usize) {
        let Some(q) = &self.queue else { return };
        if i == q.current_index() {
            self.close_menus();
            self.focus = Focus { group: G_BTN, index: B_PLAY };
            return;
        }
        let Some(item) = q.get(i).cloned() else { return };
        self.close_menus();
        self.focus = Focus { group: G_BTN, index: B_PLAY };
        self.next = Some(item);
        self.play_next(cx);
    }

    /// The Move / Remove / Close buttons of the queue panel, for the selected row.
    fn queue_actions(&self) -> Vec<(&'static str, QueueAct)> {
        let mut v = vec![];
        if let Some(q) = &self.queue {
            if q.can_move_up(self.queue_sel) {
                v.push(("Move up", QueueAct::MoveUp));
            }
            if q.can_move_down(self.queue_sel) {
                v.push(("Move down", QueueAct::MoveDown));
            }
            if q.is_upcoming(self.queue_sel) {
                v.push(("Remove", QueueAct::Remove));
            }
        }
        v.push(("Close", QueueAct::Close));
        v
    }

    fn queue_act(&mut self, cx: &mut Cx, a: QueueAct) {
        let sel = self.queue_sel;
        let from_list = self.focus.group == G_LIST;
        let Some(q) = &mut self.queue else { return };
        match a {
            QueueAct::MoveUp => {
                if q.move_up(sel) {
                    self.queue_sel = sel - 1;
                    if from_list {
                        self.focus.index = sel - 1;
                    }
                    self.queue_changed(cx);
                }
            }
            QueueAct::MoveDown => {
                if q.move_down(sel) {
                    self.queue_sel = sel + 1;
                    if from_list {
                        self.focus.index = sel + 1;
                    }
                    self.queue_changed(cx);
                }
            }
            QueueAct::Remove => {
                if q.remove(sel) {
                    self.queue_sel = sel.min(q.len() - 1);
                    if from_list {
                        self.focus.index = self.queue_sel;
                    }
                    self.queue_changed(cx);
                }
            }
            QueueAct::Close => {
                self.close_menus();
                self.focus = Focus { group: G_BTN, index: B_PLAY };
            }
        }
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
        self.last_seek = Some(cx.now);
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

    /// FIX-4 R5: a new audio track re-applies the default subtitle unless the viewer picked one.
    fn pick_audio(&mut self, cx: &mut Cx, index: i32) {
        self.audio = Some(index);
        let remembered = cx.remembered_subtitle(&self.item);
        let sub = self.source().and_then(|ms| {
            crate::playreq::subtitle_after_audio_pick(
                &cx.settings.subtitles,
                ms,
                index,
                remembered.as_ref(),
                self.sub_picked,
                self.subtitle,
            )
        });
        let sub = sub.filter(|v| Some(*v) != self.subtitle);
        let sub_replans = sub.is_some_and(|v| self.change_subtitle(cx, v));
        if sub_replans || track_change_replans(self.transcoding) {
            return self.reload(cx, track_change_force(self.transcoding, self.fell_back));
        }
        cx.player.set_audio(Some(index));
        if let Some(v) = sub {
            cx.player.set_subtitle(v);
        }
        self.report_now(cx);
    }

    /// Records the subtitle `val` as the current one; true when the stream has to be re-planned for it.
    fn change_subtitle(&mut self, cx: &mut Cx, val: Option<i32>) -> bool {
        let renders = cx.player.renders_subtitles();
        let sub_of = |s: &Self, v| request_indexes_on(renders, s.info.as_ref(), s.source_id.as_deref(), None, v).1;
        let replan =
            subtitle_change_replans_on(renders, self.transcoding, sub_of(self, self.subtitle), sub_of(self, Some(val)));
        self.subtitle = Some(val);
        replan
    }

    fn pick_subtitle(&mut self, cx: &mut Cx, val: Option<i32>) {
        self.sub_picked = true;
        cx.remember_subtitle(&self.item, self.source(), val);
        let replan = self.change_subtitle(cx, val);
        if replan {
            return self.reload(cx, track_change_force(self.transcoding, self.fell_back));
        }
        cx.player.set_subtitle(val);
        self.report_now(cx);
    }

    fn adj_sub_delay(&mut self, cx: &mut Cx, delta: f64) {
        self.sub_delay = (self.sub_delay + delta).clamp(-30.0, 30.0);
        cx.player.set_sub_delay(self.sub_delay);
    }

    /// UXP2: send the subtitle style to the player, lifted above the OSD while it is up (and the panel is not).
    fn apply_sub_style(&mut self, cx: &mut Cx) {
        let mut style = self.sub_style.clone();
        self.sub_lifted = self.osd_visible && !self.subtitle_open && self.up_next.is_none();
        style.position = lifted_sub_position(style.position, self.sub_lifted);
        cx.player.set_sub_style(&style);
    }

    fn pick_secondary(&mut self, cx: &mut Cx, val: Option<i32>) {
        self.sec_subtitle = val;
        cx.player.set_secondary_subtitle(val);
    }

    /// Enter or a click on entry `i` of subtitle-panel group `group`.
    fn sub_choice(&mut self, cx: &mut Cx, group: usize, i: usize) {
        match group {
            G_AUDIO => {
                if let Some(idx) = self.audio_entries().get(i).map(|(_, x)| *x) {
                    self.pick_audio(cx, idx);
                }
                return;
            }
            G_TABS => {
                self.sub_tab = if i == 0 { SUB_TAB_AUDIO } else { SUB_TAB_SUBS };
                return;
            }
            G_LIST => {
                let entries = self.subtitle_entries();
                match sheet_row(i, entries.len()) {
                    Some(SheetRow::Track(k)) => self.pick_subtitle(cx, entries[k].1),
                    Some(SheetRow::Options) => {
                        self.sub_tab = SUB_TAB_OPTIONS;
                        let at = entries.iter().position(|(_, v)| *v == self.sec_subtitle).unwrap_or(0);
                        self.focus = Focus { group: G_SEC, index: at };
                    }
                    None => {}
                }
                return;
            }
            G_SEC => {
                if let Some(v) = self.subtitle_entries().get(i).map(|(_, v)| *v) {
                    self.pick_secondary(cx, v);
                }
                return;
            }
            G_SUB_OFFSET => {
                self.adj_sub_delay(cx, if i == 0 { -0.1 } else { 0.1 });
                return;
            }
            // The TV's server burn-in never sees the style, so its rows are not shown there and do nothing.
            _ if !cx.player.renders_subtitles() => return,
            _ => {
                if !save_sub_pick(cx.settings, &mut self.sub_style, group, i) {
                    return;
                }
                cx.save_settings();
            }
        }
        self.apply_sub_style(cx);
    }

    /// Enter in the subtitle panel acts on the focused control.
    fn sub_panel_enter(&mut self, cx: &mut Cx) {
        self.sub_choice(cx, self.focus.group, self.focus.index);
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
            Action::OpenAudioSubs => {
                self.close_menus();
                self.subtitle_open = true;
                self.sub_tab = SUB_TAB_SUBS;
                let at = self.subtitle_entries().iter().position(|(_, v)| Some(*v) == self.subtitle).unwrap_or(0);
                self.focus = Focus { group: G_LIST, index: at };
                self.osd_visible = true;
                self.osd_last_input = cx.now;
            }
            Action::OpenMore => {
                self.close_menus();
                self.more_open = true;
                self.focus = Focus { group: G_LIST, index: 0 };
                self.osd_visible = true;
                self.osd_last_input = cx.now;
            }
            Action::Leave => cx.pop(),
            Action::OpenChapters => {
                self.close_menus();
                self.chapters_open = true;
                self.focus = Focus { group: G_LIST, index: 0 };
                self.osd_visible = true;
                self.osd_last_input = cx.now;
            }
            Action::PlayNext => {
                if self.next.is_some() {
                    self.play_next(cx);
                }
            }
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
            Action::OpenQueue => {
                if let Some(at) = self.queue.as_ref().map(|q| q.current_index()) {
                    self.close_menus();
                    self.queue_open = true;
                    self.queue_sel = at;
                    self.queue_scroll = true;
                    self.focus = Focus { group: G_LIST, index: at };
                    self.osd_visible = true;
                    self.osd_last_input = cx.now;
                }
            }
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
        } else if self.focus.group == G_VOL && self.focus.index == B_VOL_QUEUE {
            self.do_action(cx, Action::OpenQueue);
        }
    }

    fn pick_current(&mut self, cx: &mut Cx) {
        if self.queue_open {
            if self.focus.group == G_LIST {
                self.queue_jump(cx, self.focus.index);
            } else if let Some((_, a)) = self.queue_actions().get(self.focus.index).copied() {
                self.queue_act(cx, a);
            }
        } else if self.more_open {
            if let Some((_, a)) = self.more_entries().get(self.focus.index).cloned() {
                self.close_menus();
                self.focus = Focus { group: G_BTN, index: B_PLAY };
                self.do_action(cx, a);
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
        self.draw_cover(ui, cx, 1.0);
    }

    /// FIX-1 #1: the start cover — the item's backdrop dimmed full screen, its title centred and a spinner under
    /// it, at opacity `a` (it cross-fades into the video once the first frame shows).
    fn draw_cover(&mut self, ui: &mut egui::Ui, cx: &mut Cx, a: f32) {
        let t = cx.theme;
        let screen = ui.ctx().screen_rect();
        let p = ui.painter();
        p.rect_filled(screen, 0.0, t.bg.gamma_multiply(a));
        let ppp = ui.ctx().pixels_per_point();
        let url = cx
            .client
            .as_ref()
            .and_then(|c| c.image_url(&self.item, ImageKind::Backdrop, image_width_bucket(screen.width(), ppp)));
        if let Some(tex) = url.and_then(|u| cx.images.get(&u)) {
            let tint = egui::Color32::WHITE.gamma_multiply(a * 0.45);
            p.image(tex, screen, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), tint);
        }
        let title = self.req.as_ref().map(|r| r.title.clone()).unwrap_or_else(|| self.item.name.clone());
        let g = ui.fonts(|f| f.layout_no_wrap(title, t.font_w(t.title_size, Weight::Bold), t.text));
        let g = if g.size().x > screen.width() - 2.0 * t.margin {
            let mut job = (*g.job).clone();
            job.wrap = egui::text::TextWrapping::truncate_at_width(screen.width() - 2.0 * t.margin);
            ui.fonts(|f| f.layout_job(job))
        } else {
            g
        };
        let at = screen.center() - egui::vec2(g.size().x / 2.0, g.size().y);
        p.galley(at, g, t.text.gamma_multiply(a));
        if a > 0.5 {
            let sr =
                egui::Rect::from_center_size(screen.center() + egui::vec2(0.0, t.gap * 1.5), egui::vec2(t.gap, t.gap));
            ui.put(sr, egui::Spinner::new().size(t.gap).color(t.text));
        }
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
        let subtitle_open = self.subtitle_open;
        let queue_open = self.queue_open;
        let has_queue = self.queue.as_ref().is_some_and(|q| q.len() > 1);
        let chapters_open = self.chapters_open
            || self.more_open
            || self.quality_open
            || self.speed_open
            || self.aspect_open
            || self.repeat_open;
        let focus_bar = self.focus.group == G_BAR;
        let focus_btn_index = (self.focus.group == G_BTN).then_some(self.focus.index);
        let focus_vol = self.focus.group == G_VOL;

        // D4 (Apple TV look): no boxes — the picture darkens towards the top and bottom edges instead.
        let shade = t.hero_fade_h * 0.75;
        shade_rect(ui, egui::Rect::from_min_size(screen.min, egui::vec2(screen.width(), shade)), t.scrim, true);
        // Title, top-left.
        let title_g = ui.fonts(|f| f.layout_no_wrap(title, t.font_w(t.title_size, Weight::Bold), t.text));
        let title_h = title_g.size().y;
        let clip = ui.painter().with_clip_rect(screen.shrink(t.margin));
        clip.galley(egui::pos2(screen.left() + t.margin, screen.top() + t.margin), title_g, t.text);
        // FIX-1 #3: what's playing — audio track, and the subtitles when on.
        let line = now_playing_line(&self.audio_label(), self.subtitles_on().as_deref());
        clip.text(
            egui::pos2(screen.left() + t.margin, screen.top() + t.margin + title_h + t.gap / 4.0),
            egui::Align2::LEFT_TOP,
            line,
            t.font_w(t.body_size, Weight::Medium),
            t.muted,
        );

        // Bottom: scrubber with times under it, the button row, then the volume row.
        let btn_h = t.button_h;
        let bottom_h = btn_h * 3.0 + t.gap * 2.0;
        let bottom_rect = egui::Rect::from_min_size(
            egui::pos2(screen.left() + t.margin, screen.bottom() - t.margin - bottom_h),
            egui::vec2(content_w, bottom_h),
        );
        shade_rect(
            ui,
            egui::Rect::from_min_max(egui::pos2(screen.left(), bottom_rect.top() - shade), screen.max),
            t.scrim,
            false,
        );

        let mut bar_xs = vec![];
        let mut seek_click: Option<f32> = None;
        let mut bar_hover: Option<f32> = None;
        let mut bar_rect = egui::Rect::NOTHING;
        let mut btn_xs = vec![];
        let mut btn_clicked: Option<usize> = None;
        let mut btn_hovered: Option<usize> = None;
        let mut vol_xs = vec![];
        let mut vol_clicked: Option<usize> = None;
        let mut vol_hovered: Option<usize> = None;

        {
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bottom_rect));
            // Elapsed on the left, time left on the right, both under the bar (as on Apple TV).
            let pos_label = parts::clock(pos);
            let dur_label = format!("-{}", parts::clock((dur - pos).max(0.0)));
            let bar_w = bottom_rect.width();

            child.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(bar_w, btn_h * 0.6), egui::Sense::click());
                // A rounded track that thickens and grows a knob when focused.
                let a = widgets::focus_amount(ui, t, ui.id().with("d4-scrub"), focus_bar);
                let half = t.progress_h * (1.0 + a);
                let bar = egui::Rect::from_min_max(
                    egui::pos2(rect.left(), rect.center().y - half),
                    egui::pos2(rect.right(), rect.center().y + half),
                );
                ui.painter().rect_filled(bar, half, t.progress_track);
                let mut done = bar;
                done.set_right(bar.left() + bar.width() * frac);
                ui.painter().rect_filled(done, half, t.progress);
                if a > 0.01 {
                    let knob = egui::pos2(done.right(), bar.center().y);
                    ui.painter().add(
                        t.shadow(a).as_shape(
                            egui::Rect::from_center_size(knob, egui::vec2(half * 4.0, half * 4.0)),
                            half * 2.0,
                        ),
                    );
                    ui.painter().circle_filled(knob, half * 2.0 * a, t.focus_fill);
                }
                bar_xs.push(rect.center().x);
                bar_rect = rect;
                if resp.hovered() {
                    bar_hover = resp.hover_pos().map(|p| ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0));
                }
                if resp.clicked() {
                    if let Some(p) = resp.interact_pointer_pos() {
                        seek_click = Some(((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0));
                    }
                }
                let times = ui.allocate_exact_size(egui::vec2(bar_w, t.body_size * 1.4), egui::Sense::hover()).0;
                let font = t.font_w(t.small_size, Weight::Medium);
                ui.painter().text(times.left_center(), egui::Align2::LEFT_CENTER, pos_label, font.clone(), t.text);
                ui.painter().text(times.right_center(), egui::Align2::RIGHT_CENTER, dur_label, font, t.muted);
            });

            child.add_space(t.gap / 2.0);
            child.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = t.gap / 2.0;
                for (i, (label, _)) in buttons.iter().enumerate() {
                    let focused = focus_btn_index == Some(i);
                    let r = widgets::pill_button(ui, t, label, focused, false);
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
                let r = widgets::pill_button(ui, t, mute_label, mute_focused, false);
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
                let vol_bar_focused = focus_vol && self.focus.index == B_VOL_BAR;
                let va = widgets::focus_amount(ui, t, ui.id().with("d4-vol"), vol_bar_focused);
                let vbar = vbar.expand2(egui::vec2(0.0, t.progress_h * va));
                ui.painter().rect_filled(vbar, vbar.height() / 2.0, t.progress_track);
                let mut vdone = vbar;
                vdone.set_right(vbar.left() + vbar.width() * self.volume);
                ui.painter().rect_filled(vdone, vbar.height() / 2.0, t.progress);
                if va > 0.01 {
                    ui.painter().circle_filled(
                        egui::pos2(vdone.right(), vbar.center().y),
                        vbar.height() * va,
                        t.focus_fill,
                    );
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
                if has_queue {
                    let r = widgets::pill_button(ui, t, "Queue", focus_vol && self.focus.index == B_VOL_QUEUE, false);
                    vol_xs.push(r.rect.center().x);
                    if r.hovered() {
                        vol_hovered = Some(B_VOL_QUEUE);
                    }
                    if r.clicked() {
                        vol_clicked = Some(B_VOL_QUEUE);
                    }
                }
            });
        }

        if let Some(rel) = seek_click {
            cx.player.seek(rel as f64 * dur);
        }
        if show_trickplay(bar_hover.is_some(), focus_bar, cx.now, self.last_seek) {
            self.draw_trickplay(ui, cx, bar_rect, bottom_rect.top(), bar_hover.unwrap_or(frac), dur);
        }

        let mut groups = vec![Group::default(); GROUP_COUNT];
        if subtitle_open {
            self.draw_sub_panel(ui, cx, &mut groups);
        } else if queue_open {
            self.draw_queue(ui, cx, &mut groups);
        } else if chapters_open {
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
            } else if i == B_VOL_QUEUE {
                self.do_action(cx, Action::OpenQueue);
            }
        }
    }

    /// RR4 / P-PLAY-21: the trickplay thumbnail for the position `frac` along the seek bar, above the bar. Nothing
    /// when the item has no trickplay or its tile sheet has not loaded yet.
    fn draw_trickplay(&self, ui: &mut egui::Ui, cx: &mut Cx, bar: egui::Rect, bottom: f32, frac: f32, dur: f64) {
        let (Some(info), Some(client)) = (&self.trickplay, cx.client.as_ref()) else { return };
        let secs = frac as f64 * dur;
        let Some(cell) = info.locate(secs) else { return };
        let url = client.trickplay_sheet_url(&self.item.id, info.width, cell.sheet);
        let Some(tex) = cx.images.get(&url) else { return };
        let t = cx.theme;
        let (sheet_w, sheet_h) = info.sheet_size();
        let uv = egui::Rect::from_min_max(
            egui::pos2(cell.x as f32 / sheet_w as f32, cell.y as f32 / sheet_h as f32),
            egui::pos2((cell.x + info.width) as f32 / sheet_w as f32, (cell.y + info.height) as f32 / sheet_h as f32),
        );
        let size = egui::vec2(t.trickplay_w, t.trickplay_w * info.height as f32 / info.width as f32);
        let screen = ui.ctx().screen_rect();
        let x = (bar.left() + bar.width() * frac - size.x / 2.0)
            .clamp(screen.left() + t.margin, (screen.right() - t.margin - size.x).max(screen.left() + t.margin));
        let rect = egui::Rect::from_min_size(egui::pos2(x, bottom - t.gap - size.y), size);
        ui.painter().add(t.shadow(1.0).as_shape(rect, t.card_radius));
        egui::Image::new((tex, size)).uv(uv).corner_radius(t.card_radius).paint_at(ui, rect);
        let g = ui.fonts(|f| f.layout_no_wrap(parts::clock(secs), t.font_w(t.small_size, Weight::SemiBold), t.text));
        let chip = egui::Rect::from_center_size(
            egui::pos2(rect.center().x, rect.bottom() - g.size().y),
            g.size() + egui::vec2(t.gap / 2.0, t.gap / 4.0),
        );
        ui.painter().rect_filled(chip, chip.height() / 2.0, t.bar_fill);
        ui.painter().galley(chip.center() - g.size() / 2.0, g, t.text);
    }

    fn draw_picker(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> (Vec<f32>, Option<usize>, Option<Action>) {
        let t = cx.theme;
        let screen = ui.ctx().screen_rect();
        ui.painter().rect_filled(screen, 0.0, t.scrim);
        let labels: Vec<String> = if self.more_open {
            self.more_entries().into_iter().map(|(l, _)| l).collect()
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
        let heading = if self.more_open {
            "More"
        } else if self.quality_open {
            "Quality"
        } else if self.speed_open {
            "Speed"
        } else if self.aspect_open {
            "Aspect ratio"
        } else if self.repeat_open {
            "Repeat"
        } else {
            "Chapters"
        };
        let max_h = screen.height() - 2.0 * t.margin;
        card(ui.ctx(), t, "player-picker", heading, t.dropdown_w + 2.0 * t.dialog_pad, max_h, |ui| {
            egui::ScrollArea::vertical().max_height(max_h - t.title_size * 2.0).show(ui, |ui| {
                for (i, label) in labels.iter().enumerate() {
                    let focused = focus_list_index == Some(i);
                    let r = widgets::pill_button_width(ui, t, label, focused, false, t.dropdown_w);
                    if focused {
                        ui.scroll_to_rect(r.rect, None);
                    }
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
        let action = if self.more_open {
            clicked.and_then(|i| self.more_entries().get(i).map(|(_, a)| a.clone()))
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

    /// The queue side panel (P-PLAY-22): played items greyed at the top, the current one marked, upcoming below.
    /// Fills G_BTN (Move / Remove / Close for the selected row) and G_LIST (the rows); applies hover and click.
    fn draw_queue(&mut self, ui: &mut egui::Ui, cx: &mut Cx, groups: &mut [Group]) {
        let t = cx.theme;
        let Some(q) = self.queue.clone() else { return };
        if self.focus.group == G_LIST {
            self.queue_sel = self.focus.index;
        }
        self.queue_sel = self.queue_sel.min(q.len().saturating_sub(1));
        let acts = self.queue_actions();
        if self.focus.group == G_BTN {
            self.focus.index = self.focus.index.min(acts.len() - 1);
        }
        let screen = ui.ctx().screen_rect();
        let bottom_h = t.button_h * 3.0 + t.gap * 2.0;
        let panel_h = (screen.height() - 3.0 * t.margin - bottom_h).max(0.0);
        let list_h = (panel_h - t.button_h * 2.0 - t.gap * 4.0).max(0.0);
        let focus = self.focus;
        let scroll = self.queue_scroll;
        let mut act_xs = vec![];
        let mut act_hover = None;
        let mut act_click = None;
        let mut row_xs = vec![];
        let mut row_hover = None;
        let mut row_click = None;
        egui::Area::new(egui::Id::new("player-queue"))
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-t.margin, t.margin))
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                card_frame(t).show(ui, |ui| {
                    ui.set_width(t.queue_w);
                    ui.set_height(panel_h);
                    parts::heading(ui, t, &format!("Queue · {} of {}", q.current_index() + 1, q.len()));
                    ui.add_space(t.gap / 2.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = t.gap / 2.0;
                        for (i, (label, _)) in acts.iter().enumerate() {
                            let r = widgets::pill_button(ui, t, label, focus.group == G_BTN && focus.index == i, false);
                            act_xs.push(r.rect.center().x);
                            if r.hovered() {
                                act_hover = Some(i);
                            }
                            if r.clicked() {
                                act_click = Some(i);
                            }
                        }
                    });
                    ui.add_space(t.gap / 2.0);
                    egui::ScrollArea::vertical().max_height(list_h).auto_shrink([false, false]).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = t.gap / 4.0;
                        for (i, item) in q.items().iter().enumerate() {
                            let (rect, resp) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), t.button_h),
                                egui::Sense::click(),
                            );
                            let current = i == q.current_index();
                            let focused = focus.group == G_LIST && focus.index == i;
                            let a = widgets::focus_amount(ui, t, resp.id, focused);
                            let rest = if current || resp.hovered() { t.glass } else { egui::Color32::TRANSPARENT };
                            let plate_ink = widgets::plate(ui, t, rect, t.field_radius, rest, a, 1.0);
                            let ink = if a > 0.5 {
                                plate_ink
                            } else if q.is_played(i) {
                                t.muted
                            } else {
                                t.text
                            };
                            let label =
                                if current { format!("▶ {}", queue_row_label(item)) } else { queue_row_label(item) };
                            ui.painter().with_clip_rect(rect).text(
                                rect.left_center() + egui::vec2(t.gap / 2.0, 0.0),
                                egui::Align2::LEFT_CENTER,
                                label,
                                t.font(t.body_size),
                                ink,
                            );
                            if focused && scroll {
                                ui.scroll_to_rect(rect, Some(egui::Align::Center));
                            }
                            row_xs.push(rect.center().x);
                            if resp.hovered() {
                                row_hover = Some(i);
                            }
                            if resp.clicked() {
                                row_click = Some(i);
                            }
                        }
                    });
                });
            });
        self.queue_scroll = false;
        groups[G_BTN] = Group { len: act_xs.len(), cols: act_xs.len(), xs: act_xs };
        groups[G_LIST] = Group { len: row_xs.len(), cols: 1, xs: row_xs };
        if let Some(i) = act_hover {
            self.focus = Focus { group: G_BTN, index: i };
        }
        if let Some(i) = row_hover {
            self.focus = Focus { group: G_LIST, index: i };
            self.queue_sel = i;
        }
        if let Some(i) = act_click {
            if let Some((_, a)) = acts.get(i).copied() {
                self.queue_act(cx, a);
            }
        }
        if let Some(i) = row_click {
            self.queue_jump(cx, i);
        }
    }

    /// FIX-1 #5: the Audio & Subtitles sheet (P-PLAY-6/7/8/9), tvOS style: a card along the bottom with an
    /// **Audio | Subtitles** segmented control and one scrolling list per tab (✔ on the current track). The
    /// Subtitles list ends with "Subtitle options…", which opens the second subtitles, offset, position and style.
    /// Fills the sheet's focus groups (G_TABS … G_SUB_BACK) and applies hover/click.
    fn draw_sub_panel(&mut self, ui: &mut egui::Ui, cx: &mut Cx, groups: &mut [Group]) {
        let t = cx.theme;
        let screen = ui.ctx().screen_rect();
        ui.painter().rect_filled(screen, 0.0, t.dim_scrim);

        let entries = self.subtitle_entries();
        let audio = self.audio_entries();
        let audio_now = self.current_audio();
        let size_idx = match self.sub_style.size {
            SubSize::Small => 0,
            SubSize::Medium => 1,
            SubSize::Large => 2,
            SubSize::ExtraLarge => 3,
        };
        let style_ok = cx.player.renders_subtitles();
        let focus = self.focus;
        let off_label = format!("{:.1} s", self.sub_delay);
        let pos_label = format!("{:.0}", self.sub_style.position);
        let sec_idx = entries.iter().position(|(_, v)| *v == self.sec_subtitle).unwrap_or(0);
        let pri_idx = match self.subtitle {
            Some(v) => entries.iter().position(|(_, e)| *e == v),
            None => None,
        };
        let colour_names: Vec<String> = SUB_COLOURS.iter().map(|(n, _)| n.to_string()).collect();
        let mut back_names = vec!["None".to_string()];
        back_names.extend(colour_names.iter().cloned());
        let text_sel: Vec<bool> =
            (0..SUB_COLOURS.len()).map(|i| sub_colour_index(&self.sub_style.text_colour) == Some(i)).collect();
        let back_sel: Vec<bool> = std::iter::once(self.sub_style.back_colour.is_none())
            .chain(
                (0..SUB_COLOURS.len())
                    .map(|i| self.sub_style.back_colour.as_deref().and_then(sub_colour_index) == Some(i)),
            )
            .collect();
        let font_names: Vec<String> = SubFont::ALL.iter().map(|f| f.label().to_string()).collect();
        let font_sel: Vec<bool> = SubFont::ALL.iter().map(|f| *f == self.sub_style.font).collect();
        let weight_names: Vec<String> = SubWeight::ALL.iter().map(|f| f.label().to_string()).collect();
        let weight_sel: Vec<bool> = SubWeight::ALL.iter().map(|f| *f == self.sub_style.weight).collect();
        let shadow_names: Vec<String> = SubShadow::ALL.iter().map(|f| f.label().to_string()).collect();
        let shadow_sel: Vec<bool> = SubShadow::ALL.iter().map(|f| *f == self.sub_style.shadow).collect();

        // (group, xs, hovered, clicked) per focus group, filled while drawing.
        let mut found: Vec<PanelGroup> = vec![];
        let tab = self.sub_tab;
        let scroll_now = self.panel_seen != Some(focus);
        self.panel_seen = Some(focus);
        let reveal = |ui: &mut egui::Ui, r: egui::Rect, focused: bool| {
            if focused && scroll_now {
                ui.scroll_to_rect(r, Some(egui::Align::Center));
            }
        };
        let card_w = t.player_panel_w.min(screen.width() - 2.0 * t.margin);
        let inner_w = card_w - 2.0 * t.dialog_pad;
        // The sheet sits on the bottom margin and never reaches above the top margin.
        let list_h = if tab == SUB_TAB_OPTIONS { screen.height() - 4.0 * t.margin } else { screen.height() / 2.0 };
        // The list's height is set from its rows (not remembered from the other tab), up to `list_h`.
        let rows = match tab {
            SUB_TAB_AUDIO => audio.len(),
            SUB_TAB_SUBS => entries.len() + 1,
            _ => usize::MAX,
        };
        let list_h = (rows as f32 * (t.button_h + t.gap / 4.0)).min(list_h);
        egui::Area::new(egui::Id::new("player-sub-panel"))
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -t.margin))
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                card_frame(t).show(ui, |ui| {
                    ui.set_width(inner_w);
                    let labels = ["Audio".to_string(), "Subtitles".to_string()];
                    let shown = if tab == SUB_TAB_AUDIO { 0 } else { 1 };
                    let out = widgets::tabs(ui, t, &labels, shown, (focus.group == G_TABS).then_some(focus.index));
                    found.push((G_TABS, out.xs, out.hovered, out.clicked));
                    ui.add_space(t.gap / 2.0);
                    egui::ScrollArea::vertical()
                        .id_salt(("player-sheet", tab))
                        .max_height(list_h)
                        .min_scrolled_height(list_h)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = t.gap / 4.0;
                            let mut list =
                                |ui: &mut egui::Ui, group: usize, rows: Vec<String>, current: Option<usize>| {
                                    let (mut xs, mut hover, mut click) = (vec![], None, None);
                                    for (i, label) in rows.iter().enumerate() {
                                        let focused = focus.group == group && focus.index == i;
                                        let label =
                                            if current == Some(i) { format!("✔ {label}") } else { label.clone() };
                                        let r = widgets::pill_button_width(ui, t, &label, focused, false, inner_w);
                                        reveal(ui, r.rect, focused);
                                        xs.push(r.rect.center().x);
                                        if r.hovered() {
                                            hover = Some(i);
                                        }
                                        if r.clicked() {
                                            click = Some(i);
                                        }
                                    }
                                    found.push((group, xs, hover, click));
                                };
                            match tab {
                                SUB_TAB_AUDIO => {
                                    let rows = audio.iter().map(|(l, _)| l.clone()).collect();
                                    list(ui, G_AUDIO, rows, audio.iter().position(|(_, i)| Some(*i) == audio_now));
                                }
                                SUB_TAB_SUBS => {
                                    let mut rows: Vec<String> = entries.iter().map(|(l, _)| l.clone()).collect();
                                    rows.push("Subtitle options…".to_string());
                                    list(ui, G_LIST, rows, pri_idx);
                                }
                                _ => {
                                    section(ui, t, "Second subtitles");
                                    let rows = entries.iter().map(|(l, _)| l.clone()).collect();
                                    list(ui, G_SEC, rows, Some(sec_idx));
                                    section(ui, t, "Timing");
                                    for (group, label, value) in [
                                        (G_SUB_OFFSET, "Offset", off_label.clone()),
                                        (G_SUB_POS, "Position", pos_label.clone()),
                                    ]
                                    .into_iter()
                                    .filter(|(g, ..)| style_ok || *g != G_SUB_POS)
                                    {
                                        let mut xs = vec![];
                                        let mut hover = None;
                                        let mut click = None;
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = t.gap / 2.0;
                                            row_label(ui, t, label, t.button_h);
                                            for (k, sign) in ["−", "+"].into_iter().enumerate() {
                                                let focused = focus.group == group && focus.index == k;
                                                let r = widgets::pill_button_width(
                                                    ui,
                                                    t,
                                                    sign,
                                                    focused,
                                                    false,
                                                    t.button_h * 1.4,
                                                );
                                                reveal(ui, r.rect, focused);
                                                xs.push(r.rect.center().x);
                                                if r.hovered() {
                                                    hover = Some(k);
                                                }
                                                if r.clicked() {
                                                    click = Some(k);
                                                }
                                                if k == 0 {
                                                    let g = ui.fonts(|f| {
                                                        f.layout_no_wrap(
                                                            value.clone(),
                                                            t.font_w(t.body_size, Weight::SemiBold),
                                                            t.text,
                                                        )
                                                    });
                                                    let (vr, _) = ui.allocate_exact_size(
                                                        egui::vec2(t.button_h * 1.8, t.button_h),
                                                        egui::Sense::hover(),
                                                    );
                                                    ui.painter().galley(vr.center() - g.size() / 2.0, g, t.text);
                                                }
                                            }
                                        });
                                        found.push((group, xs, hover, click));
                                    }
                                    section(ui, t, "Subtitle style");
                                    if !style_ok {
                                        ui.label(
                                            egui::RichText::new("Subtitle size isn't available on this TV")
                                                .font(t.font(t.body_size))
                                                .color(t.muted),
                                        );
                                    }
                                    let size_labels =
                                        ["S".to_string(), "M".to_string(), "L".to_string(), "XL".to_string()];
                                    let size_sel: Vec<bool> = (0..4).map(|i| size_idx == i).collect();
                                    let rows: [(usize, &str, &[String], &[bool]); 6] = [
                                        (G_SUB_SIZE, "Size", &size_labels, &size_sel),
                                        (G_SUB_FONT, "Font", &font_names, &font_sel),
                                        (G_SUB_WEIGHT, "Weight", &weight_names, &weight_sel),
                                        (G_SUB_SHADOW, "Shadow", &shadow_names, &shadow_sel),
                                        (G_SUB_TEXT, "Text", &colour_names, &text_sel),
                                        (G_SUB_BACK, "Background", &back_names, &back_sel),
                                    ];
                                    for (group, label, labels, selected) in rows.into_iter().filter(|_| style_ok) {
                                        ui.horizontal(|ui| {
                                            row_label(ui, t, label, t.chip_h);
                                            let top = ui.cursor().top();
                                            let out = widgets::chips(
                                                ui,
                                                t,
                                                labels,
                                                selected,
                                                (focus.group == group).then_some(focus.index),
                                            );
                                            let r = egui::Rect::from_min_size(
                                                egui::pos2(ui.min_rect().left(), top),
                                                egui::vec2(10.0, t.chip_h),
                                            );
                                            reveal(ui, r, focus.group == group);
                                            found.push((group, out.xs, out.hovered, out.clicked));
                                        });
                                    }
                                }
                            }
                        });
                });
            });

        let mut click = None;
        for (group, xs, hover, clicked) in found {
            if let Some(i) = hover {
                self.focus = Focus { group, index: i };
                if group == G_TABS {
                    self.sub_tab = if i == 0 { SUB_TAB_AUDIO } else { SUB_TAB_SUBS };
                }
            }
            if let Some(i) = clicked {
                click = Some((group, i));
            }
            let cols = if matches!(group, G_AUDIO | G_LIST | G_SEC) { 1 } else { xs.len() };
            groups[group] = Group { len: xs.len(), cols, xs };
        }
        if let Some((group, i)) = click {
            self.sub_choice(cx, group, i);
        }
    }

    /// UXP2: the Info panel — what is playing (title, time, tracks), not the keyboard help.
    fn draw_keys_help(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        let screen = ui.ctx().screen_rect();
        ui.painter().rect_filled(screen, 0.0, t.scrim);
        let title = self.req.as_ref().map(|r| r.title.clone()).unwrap_or_else(|| self.item.name.clone());
        let rows = now_playing_rows(
            &title,
            cx.player.position(),
            self.duration(cx),
            &self.audio_label(),
            &self.subtitle_label(),
        );
        let area = egui::Area::new(egui::Id::new("player-keys"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .order(egui::Order::Foreground);
        area.show(ui.ctx(), |ui| {
            ui.vertical_centered(|ui| {
                parts::heading(ui, t, "Now playing");
                ui.add_space(t.gap / 2.0);
                for (label, value) in rows {
                    parts::muted(ui, t, &format!("{label}: {value}"));
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
        let mut rows =
            stats_rows(method, container.as_deref(), codec.as_deref(), self.source_bitrate, &cx.player.stats());
        // FIX-1 #6: which audio track is playing, with its layout and codec ("English · 5.1 Surround (AC3)").
        if let Some(a) = self
            .current_audio()
            .and_then(|i| ms.and_then(|m| m.media_streams.iter().find(|s| s.kind == StreamKind::Audio && s.index == i)))
        {
            let lang = track_labels(std::slice::from_ref(a)).pop().unwrap_or_default();
            let mut v = lang;
            if let Some(ch) = channel_words(a) {
                v.push_str(&format!(" · {ch}"));
            }
            if let Some(c) = a.codec.as_deref() {
                v.push_str(&format!(" ({})", c.to_uppercase()));
            }
            let at = rows.iter().position(|(k, _)| *k == "Video").map(|i| i + 1).unwrap_or(rows.len());
            rows.insert(at, ("Audio", v));
        }
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
                // FIX-1 #5: Back from Subtitle options returns to the Subtitles list, on the options row.
                Key::Back if self.subtitle_open && self.sub_tab == SUB_TAB_OPTIONS => {
                    self.sub_tab = SUB_TAB_SUBS;
                    self.focus = Focus { group: G_LIST, index: self.subtitle_entries().len() };
                    true
                }
                Key::Back => {
                    self.close_menus();
                    self.focus = Focus { group: G_BTN, index: B_PLAY };
                    true
                }
                // FIX-1 #5: Left/Right on the segmented control switch the tab as focus moves.
                Key::Left | Key::Right if self.subtitle_open && self.focus.group == G_TABS => {
                    self.focus = move_focus(&self.groups, self.focus, key);
                    self.sub_tab = if self.focus.index == 0 { SUB_TAB_AUDIO } else { SUB_TAB_SUBS };
                    true
                }
                Key::Enter => {
                    self.pick_current(cx);
                    true
                }
                Key::Queue if self.queue_open => {
                    self.close_menus();
                    self.focus = Focus { group: G_BTN, index: B_PLAY };
                    true
                }
                Key::Delete if self.queue_open => {
                    if self.focus.group == G_LIST {
                        self.queue_act(cx, QueueAct::Remove);
                    }
                    true
                }
                Key::Up | Key::Down | Key::Left | Key::Right => {
                    self.focus = move_focus(&self.groups, self.focus, key);
                    self.queue_scroll = self.queue_open;
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
        let was_visible = self.osd_visible && !self.osd_dismissed && !self.osd_auto;
        self.osd_auto = false;
        let focus_kind = match (self.focus.group, self.focus.index) {
            (G_BAR, _) => OsdFocus::Timeline,
            (G_VOL, B_VOL_BAR) => OsdFocus::VolumeBar,
            _ => OsdFocus::Other,
        };
        let act = osd_key(was_visible, focus_kind, key, self.skip_back, self.skip_forward);
        if key == Key::Back {
            // UXP2: Back hides the OSD first, then leaves fullscreen, then leaves the player.
            if act == OsdAct::HideOsd {
                self.osd_visible = false;
                self.osd_dismissed = true;
                self.focus = Focus { group: G_BAR, index: 0 };
                return true;
            }
            if self.fullscreen {
                self.leave_fullscreen(cx);
                return true;
            }
            return false;
        }

        let show = |p: &mut PlayerScreen, cx: &Cx| {
            p.osd_visible = true;
            p.osd_dismissed = false;
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
            Key::Queue => {
                self.do_action(cx, Action::OpenQueue);
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

        show(self, cx);
        match act {
            OsdAct::Seek(d) => {
                self.seek_relative(cx, d);
                if !was_visible {
                    self.focus = Focus { group: G_BAR, index: 0 };
                }
                true
            }
            OsdAct::WakeButtons => {
                self.focus = Focus { group: G_BTN, index: B_PLAY };
                true
            }
            OsdAct::Wake => {
                self.focus = Focus { group: G_BAR, index: 0 };
                if key == Key::Enter {
                    if let Some(i) = active_segment(&self.segments, cx.player.position()) {
                        self.skip_segment(cx, i);
                    }
                }
                true
            }
            OsdAct::Move => {
                self.focus = if self.focus.group == G_BAR && key == Key::Down {
                    // Down from the timeline lands on Play, not on whatever button is nearest in x.
                    Focus { group: G_BTN, index: B_PLAY }
                } else {
                    move_focus(&self.groups, self.focus, key)
                };
                true
            }
            OsdAct::Volume(d) => {
                self.adjust_volume(cx, d);
                true
            }
            OsdAct::Activate => {
                self.enter_osd(cx);
                true
            }
            OsdAct::HideOsd | OsdAct::Leave | OsdAct::None => false,
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
            "subretry" => {
                let Ok(r) = payload.downcast::<Result<PlaybackPlan>>() else { return };
                let Some((item, opts)) = self.sub_retry.take() else { return };
                match *r {
                    Ok(plan) => self.start_playback_inner(cx, item, plan, opts, true),
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
            "season" => {
                let Ok(r) = payload.downcast::<Result<(String, Vec<Item>)>>() else { return };
                match *r {
                    Ok((id, items)) if id == self.item.id && self.queue.is_none() => {
                        if let Some(at) = items.iter().position(|x| x.id == id) {
                            self.queue = Some(Queue::new(items, at));
                        }
                    }
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    _ => {}
                }
            }
            "trickplay" => {
                let Ok(r) = payload.downcast::<Result<(String, Option<TrickplayInfo>)>>() else { return };
                match *r {
                    Ok((id, info)) if id == self.item.id => self.trickplay = info,
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    _ => {}
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

    fn on_pause(&mut self, cx: &mut Cx) {
        if matches!(cx.player.state(), PlayerState::Playing) {
            cx.player.set_paused(true);
            self.report_now(cx);
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
            self.osd_dismissed = false;
            self.osd_auto = false;
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
            self.first_frame_at = None;
            self.draw_loading(ui, cx);
            return;
        }
        // FIX-1 #1: no black gap after Play — the cover stays until mpv shows a frame, then fades.
        if self.first_frame_at.is_none() && cx.player.stats().width.is_some() && cx.player.position() > 0.0 {
            self.first_frame_at = Some(cx.now);
        }
        let cover = match self.first_frame_at {
            None => 1.0,
            Some(at) => 1.0 - ((cx.now - at) / COVER_FADE).clamp(0.0, 1.0) as f32,
        };
        if cover > 0.0 {
            self.draw_cover(ui, cx, cover);
            ui.ctx().request_repaint();
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
        let was_up = self.osd_visible;
        if (paused && !self.osd_dismissed) || self.any_menu() {
            self.osd_visible = true;
        } else if cx.now - self.osd_last_input > self.osd_hide_secs(t) {
            self.osd_visible = false;
            self.osd_auto = false;
        }
        // UXP2: the OSD always comes back with the timeline focused.
        if was_up && !self.osd_visible {
            self.focus = Focus { group: G_BAR, index: 0 };
        }
        if self.sub_lifted != (self.osd_visible && !self.subtitle_open) {
            self.apply_sub_style(cx);
        }

        // FIX-2 (FRICTION-3 player loading): the start-of-play OSD waits for the first frame, so the cover stays
        // clean; its OSD_START_SECS count from the picture, not from Play.
        let hold_for_cover = self.first_frame_at.is_none() && self.osd_auto;
        if hold_for_cover {
            self.osd_last_input = cx.now + OSD_START_SECS - t.osd_hide_after as f64;
        }
        if self.osd_visible && !hold_for_cover {
            self.draw_osd(ui, cx);
        }
        self.draw_skip_button(ui, cx);
        if self.stats_open {
            self.draw_stats(ui, cx);
        }
    }
}

// ---------- D4: Apple TV look helpers (docs/DESIGN-appletv.md) ----------

/// A vertical shade over `r`: `c` at the screen edge (top or bottom), clear towards the middle.
fn shade_rect(ui: &egui::Ui, r: egui::Rect, c: egui::Color32, from_top: bool) {
    let clear = egui::Color32::TRANSPARENT;
    let (top, bottom) = if from_top { (c, clear) } else { (clear, c) };
    let mut m = egui::Mesh::default();
    for (p, col) in [(r.left_top(), top), (r.right_top(), top), (r.right_bottom(), bottom), (r.left_bottom(), bottom)] {
        m.colored_vertex(p, col);
    }
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    ui.painter().add(m);
}

/// The translucent card the player's panels live in (menu material, big radius, soft shadow).
fn card_frame(t: &Theme) -> egui::Frame {
    egui::Frame::NONE.fill(t.menu_fill).corner_radius(t.dialog_radius).inner_margin(t.dialog_pad).shadow(t.shadow(1.0))
}

/// A centred card `w` wide and at most `max_h` tall with a heading; `add` draws the body.
fn card(ctx: &egui::Context, t: &Theme, id: &str, heading: &str, w: f32, max_h: f32, add: impl FnOnce(&mut egui::Ui)) {
    egui::Area::new(egui::Id::new(id))
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            card_frame(t).show(ui, |ui| {
                ui.set_width(w - 2.0 * t.dialog_pad);
                ui.set_max_height(max_h - 2.0 * t.dialog_pad);
                ui.label(egui::RichText::new(heading).font(t.font_w(t.heading_size, Weight::Bold)).color(t.text));
                ui.add_space(t.gap / 2.0);
                add(ui);
            });
        });
}

/// A small section heading inside a card.
fn section(ui: &mut egui::Ui, t: &Theme, text: &str) {
    ui.add_space(t.gap / 4.0);
    ui.label(egui::RichText::new(text).font(t.font_w(t.small_size, Weight::SemiBold)).color(t.muted));
}

/// The fixed-width label column of a setting row, vertically centred on the row's controls.
fn row_label(ui: &mut egui::Ui, t: &Theme, text: &str, h: f32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(t.form_label_w * 0.6, h), egui::Sense::hover());
    ui.painter().text(r.left_center(), egui::Align2::LEFT_CENTER, text, t.font(t.body_size), t.text);
}

/// FIX-4 R5: the player's own audio pick re-applies the default subtitle (drives the real screen).
#[cfg(test)]
mod fix4_tests {
    use super::*;
    use crate::app::test_support::World;

    // fixture: 2 eng audio (default), 3 fre audio, 0 ger external sub, 4 eng embedded sub; the server's default
    // sub 0 is ignored
    fn plan() -> PlaybackPlan {
        let info: PlaybackInfo = serde_json::from_str(
            &std::fs::read_to_string(format!("{}/tests/fixtures/playbackinfo.json", env!("CARGO_MANIFEST_DIR")))
                .unwrap(),
        )
        .unwrap();
        PlaybackPlan { info, transcoding_url: None, source_bitrate: None }
    }

    fn film(id: &str) -> Item {
        serde_json::from_str(&format!(r#"{{"Id":"{id}","Name":"Film","Type":"Movie"}}"#)).unwrap()
    }

    /// A playback started as Home Resume / next-prev start it: nothing chosen on a page.
    fn play(w: &mut World, item: Item, opts: PlayOptions) -> PlayerScreen {
        let mut p = new_player(item.clone(), opts.clone());
        w.cx(|cx| p.start_playback(cx, item, plan(), opts));
        p
    }

    /// The viewer picks subtitle `v` in the player's subtitle panel (the real list row).
    fn panel_pick(w: &mut World, p: &mut PlayerScreen, v: Option<i32>) {
        let row = p.subtitle_entries().iter().position(|(_, x)| *x == v).unwrap();
        w.cx(|cx| p.sub_choice(cx, G_LIST, row));
    }

    #[test]
    fn player_audio_pick_reapplies_the_default_subtitle() {
        let mut w = World::signed_in(true);
        let mut p = play(&mut w, film("i"), PlayOptions::default());
        assert_eq!(p.subtitle, Some(None), "eng audio → Off");
        w.cx(|cx| p.pick_audio(cx, 3));
        assert_eq!(p.subtitle, Some(Some(4)), "fre audio → the English sub");
        w.cx(|cx| p.pick_audio(cx, 2));
        assert_eq!(p.subtitle, Some(None), "back to eng audio → Off");
    }

    #[test]
    fn player_audio_pick_keeps_a_subtitle_the_viewer_picked() {
        let mut w = World::signed_in(true);
        let opts = PlayOptions { subtitle: Some(None), subtitle_picked: true, ..Default::default() };
        let mut p = play(&mut w, film("i"), opts);
        w.cx(|cx| p.pick_audio(cx, 3));
        assert_eq!(p.subtitle, Some(None));
    }

    #[test]
    fn a_player_pick_is_remembered_for_the_next_playback_after_a_restart() {
        let mut w = World::signed_in(true);
        let mut p = play(&mut w, film("i"), PlayOptions::default());
        panel_pick(&mut w, &mut p, Some(0));
        w.restart();
        let next = play(&mut w, film("i"), PlayOptions::default());
        assert_eq!(next.subtitle, Some(Some(0)), "Resume after a restart: the German sub");
        assert_eq!(next.req.as_ref().unwrap().subtitle, Some(0), "and the player was asked for it");
        let mut fre = play(&mut w, film("i"), PlayOptions::default());
        w.cx(|cx| fre.pick_audio(cx, 3));
        assert_eq!(fre.subtitle, Some(Some(0)), "fre audio would say the English sub 4; the remembered pick wins");
    }

    #[test]
    fn a_player_pick_of_off_is_remembered_too() {
        let mut w = World::signed_in(true);
        let mut p = play(&mut w, film("i"), PlayOptions { audio: Some(3), ..Default::default() });
        assert_eq!(p.subtitle, Some(Some(4)));
        panel_pick(&mut w, &mut p, None);
        w.restart();
        let next = play(&mut w, film("i"), PlayOptions { audio: Some(3), ..Default::default() });
        assert_eq!(next.subtitle, Some(None));
    }

    #[test]
    fn a_player_pick_is_not_remembered_when_the_user_does_not_remember_selections() {
        let mut w = World::signed_in(false);
        let mut p = play(&mut w, film("i"), PlayOptions::default());
        panel_pick(&mut w, &mut p, Some(0));
        assert_eq!(p.subtitle, Some(Some(0)), "the pick still wins for this playback");
        w.restart();
        assert!(w.settings().sub_picks.is_empty(), "nothing stored");
        assert_eq!(
            play(&mut w, film("i"), PlayOptions::default()).subtitle,
            Some(None),
            "the default-subtitle rule, Off"
        );
    }
}

#[cfg(test)]
mod fix4_substyle_tests {
    use super::*;
    use crate::api::{Client, DeviceInfo, PlaybackInfo, PlaybackPlan, Session};
    use crate::app::{AppOptions, Cx};
    use crate::platform::{NullPlayer, Player, PlayerState, ShellRequests, Storage, TrackRef};
    use crate::settings::MemStorage;
    use crate::tasks::Pool;
    use crate::ui::imageload::Images;
    use std::sync::Arc;

    /// A player that records the styles it is given; `renders` false = the TV's AVPlay.
    struct Rec {
        inner: NullPlayer,
        renders: bool,
        styles: Vec<SubStyle>,
    }
    impl Player for Rec {
        fn load(&mut self, req: &PlayRequest) {
            self.inner.load(req)
        }
        fn set_paused(&mut self, p: bool) {
            self.inner.set_paused(p)
        }
        fn seek(&mut self, secs: f64) {
            self.inner.seek(secs)
        }
        fn set_audio(&mut self, _: Option<TrackRef>) {}
        fn set_subtitle(&mut self, _: Option<TrackRef>) {}
        fn renders_subtitles(&self) -> bool {
            self.renders
        }
        fn set_sub_style(&mut self, style: &SubStyle) {
            self.styles.push(style.clone());
        }
        fn stop(&mut self) {
            self.inner.stop()
        }
        fn state(&self) -> PlayerState {
            self.inner.state()
        }
        fn position(&self) -> f64 {
            self.inner.position()
        }
        fn duration(&self) -> Option<f64> {
            None
        }
        fn draw_video(&mut self, _: u32, _: u32) {}
    }

    struct World {
        theme: Theme,
        client: Option<Client>,
        settings: Settings,
        store: Arc<dyn Storage>,
        mem: Arc<MemStorage>,
        tasks: Pool,
        images: Images,
        player: Rec,
        opts: AppOptions,
        overrides: std::collections::HashMap<String, String>,
        changed: Vec<String>,
        undo: Option<crate::app::PosterUndo>,
        keys: std::collections::HashSet<Key>,
        shell: ShellRequests,
        nav: Vec<crate::app::Nav>,
    }

    fn world(renders: bool) -> World {
        let mem = Arc::new(MemStorage::default());
        let d = DeviceInfo { name: "t".into(), id: "d".into(), version: "0".into() };
        let s = Session { user_id: "u".into(), user_name: "t".into(), token: "tok".into() };
        World {
            theme: Theme::placeholder(),
            client: Some(Client::with_session("http://h:8096", d, s)),
            settings: Settings::load(mem.as_ref()),
            store: mem.clone(),
            mem,
            tasks: Pool::new(1),
            images: Images::new(None),
            player: Rec { inner: NullPlayer::default(), renders, styles: vec![] },
            opts: AppOptions::default(),
            overrides: Default::default(),
            changed: vec![],
            undo: None,
            keys: Default::default(),
            shell: ShellRequests::default(),
            nav: vec![],
        }
    }

    impl World {
        fn cx(&mut self) -> Cx<'_> {
            Cx {
                theme: &self.theme,
                client: &mut self.client,
                settings: &mut self.settings,
                store: &self.store,
                tasks: &self.tasks,
                images: &mut self.images,
                player: &mut self.player,
                opts: &self.opts,
                admin: false,
                poster_overrides: &mut self.overrides,
                poster_changed: &mut self.changed,
                poster_gen: 0,
                undo: &mut self.undo,
                keys_down: &self.keys,
                shell: &mut self.shell,
                now: 0.0,
                sidebar: false,
                screen_id: 1,
                nav: &mut self.nav,
            }
        }
        /// The settings as a later run of the app would read them back from storage.
        fn reloaded(&self) -> Settings {
            Settings::load(self.mem.as_ref())
        }
    }

    fn plan() -> PlaybackPlan {
        let info: PlaybackInfo = serde_json::from_str(
            &std::fs::read_to_string(format!("{}/tests/fixtures/playbackinfo.json", env!("CARGO_MANIFEST_DIR")))
                .unwrap(),
        )
        .unwrap();
        PlaybackPlan { info, transcoding_url: None, source_bitrate: None }
    }

    #[test]
    fn player_size_pick_saves_setting() {
        let mut w = world(true);
        let mut p = new_player(Item::default(), PlayOptions::default());
        p.sub_choice(&mut w.cx(), G_SUB_SIZE, 3);
        assert_eq!(w.settings.subtitles.style.size, SubSize::ExtraLarge);
        assert_eq!(w.reloaded().subtitles.style.size, SubSize::ExtraLarge, "written to storage in the same frame");
    }

    #[test]
    fn player_style_rows_all_save() {
        let mut w = world(true);
        let mut p = new_player(Item::default(), PlayOptions::default());
        for (g, i) in [(G_SUB_FONT, 1), (G_SUB_WEIGHT, 1), (G_SUB_SHADOW, 1), (G_SUB_TEXT, 2), (G_SUB_BACK, 2)] {
            p.sub_choice(&mut w.cx(), g, i);
        }
        let s = w.reloaded().subtitles.style;
        assert_eq!(s.font, SubFont::ALL[1]);
        assert_eq!(s.weight, SubWeight::ALL[1]);
        assert_eq!(s.shadow, SubShadow::ALL[1]);
        assert_eq!(s.text_colour, SUB_COLOURS[2].1);
        assert_eq!(s.back_colour.as_deref(), Some(SUB_COLOURS[1].1));
    }

    #[test]
    fn player_position_saves_unlifted_value() {
        let mut w = world(true);
        let mut p = new_player(Item::default(), PlayOptions::default());
        p.osd_visible = true; // the OSD is up, so mpv gets a lifted position
        let before = p.sub_style.position;
        p.sub_choice(&mut w.cx(), G_SUB_POS, 1);
        let want = before + 5.0;
        assert_eq!(w.reloaded().subtitles.style.position, want);
        let sent = w.player.styles.last().unwrap().position;
        assert!(sent <= want, "mpv got the OSD-lifted position {sent}, the setting kept {want}");
    }

    #[test]
    fn offset_and_secondary_not_saved() {
        let mut w = world(true);
        let mut p = new_player(Item::default(), PlayOptions::default());
        p.sub_choice(&mut w.cx(), G_SUB_OFFSET, 1);
        p.sub_choice(&mut w.cx(), G_SEC, 0);
        assert!(w.mem.files.lock().unwrap().is_empty(), "nothing was written to storage");
        assert_eq!(w.settings.subtitles.style, SubStyle::default());
        assert!((p.sub_delay - 0.1).abs() < 1e-9, "the offset still applies for this playback");
    }

    #[test]
    fn next_playback_starts_with_saved_style() {
        let mut w = world(true);
        let mut first = new_player(Item::default(), PlayOptions::default());
        first.sub_choice(&mut w.cx(), G_SUB_SIZE, 3);
        // a later film, from a fresh screen, reading what was saved
        w.settings = w.reloaded();
        let mut next = new_player(Item::default(), PlayOptions::default());
        let item = Item { id: "i1".into(), ..Default::default() };
        next.start_playback(&mut w.cx(), item, plan(), PlayOptions::default());
        assert_eq!(next.sub_style.size, SubSize::ExtraLarge);
        assert_eq!(w.player.styles.last().unwrap().size, SubSize::ExtraLarge, "and the player was given it");
    }

    #[test]
    fn tv_style_rows_do_nothing_and_are_not_saved() {
        let mut w = world(false);
        let mut p = new_player(Item::default(), PlayOptions::default());
        p.sub_choice(&mut w.cx(), G_SUB_SIZE, 3);
        p.sub_choice(&mut w.cx(), G_SUB_POS, 1);
        assert_eq!(p.sub_style, SubStyle::default());
        assert!(w.mem.files.lock().unwrap().is_empty());
    }
}
