//! R-UI-9 Settings, split into tabs (G12 / P-USER-* / P-HOME-5 / P-AUTH-9): Account (profile picture,
//! change password), Home (reorder + hide sections), Playback (audio language, max bitrate, auto-play next,
//! skip lengths), Subtitles (G5's mode, language and appearance), Display (F3's year switches, date format, backdrops), Quick Connect (authorise a code),
//! Server (sign out / change server), TMDB (the key from F5b) and About (version). Built on the W0 widget kit.
use super::parts;
use super::widgets;
use crate::api::{image_width_bucket, ApiError, Client, Result, SegmentKind};
use crate::app::{Cx, Screen};
use crate::nav::{move_focus, Focus, Group};
use crate::platform::{parse_hex_colour, sub_colour_index, Key, SubFont, SubShadow, SubSize, SubWeight, SUB_COLOURS};
use crate::settings::{DateFormat, HomeSection, Screensaver, SubMode, SubPrefs};
use egui::{vec2, Sense};

const G_TABS: usize = 0;
const G_CONTENT: usize = 1;

const LANGUAGES: [&str; 11] = [
    "Any",
    "English",
    "French",
    "German",
    "Spanish",
    "Italian",
    "Japanese",
    "Korean",
    "Chinese",
    "Russian",
    "Portuguese",
];
/// FIX-1 #11: subtitle languages as a list (label, the 3-letter code the server's streams carry); "Any" = no preference.
const SUB_LANGS: [(&str, &str); 11] = [
    ("Any", ""),
    ("English", "eng"),
    ("French", "fre"),
    ("German", "ger"),
    ("Spanish", "spa"),
    ("Italian", "ita"),
    ("Japanese", "jpn"),
    ("Korean", "kor"),
    ("Chinese", "chi"),
    ("Russian", "rus"),
    ("Portuguese", "por"),
];

/// The list index for a stored subtitle language code (case-insensitive; also the 639-2/T forms fra/deu/zho).
fn sub_lang_index(code: &str) -> Option<usize> {
    let c = code.trim().to_ascii_lowercase();
    let c = match c.as_str() {
        "fra" => "fre",
        "deu" => "ger",
        "zho" => "chi",
        other => other,
    };
    SUB_LANGS.iter().position(|(_, k)| *k == c)
}

/// How long "Saved" stays after an automatic save.
const SAVED_SHOW_SECS: f64 = 2.0;
const BITRATE_LABELS: [&str; 7] = ["Auto", "1.5 Mbps", "3 Mbps", "5 Mbps", "10 Mbps", "20 Mbps", "40 Mbps"];
const BITRATE_KBPS: [u32; 7] = [0, 1500, 3000, 5000, 10000, 20000, 40000];
const SKIP_BACK: [(&str, u32); 4] = [("5 s", 5), ("10 s", 10), ("15 s", 15), ("30 s", 30)];
const SKIP_FORWARD: [(&str, u32); 4] = [("10 s", 10), ("15 s", 15), ("30 s", 30), ("60 s", 60)];
/// P-USER-4: days before an unwatched show drops out of Next up (0 = never).
const NEXT_UP_DAYS: [(&str, u32); 5] =
    [("No limit", 0), ("30 days", 30), ("90 days", 90), ("180 days", 180), ("365 days", 365)];
/// P-USER-2: idle minutes before the screensaver.
const SAVER_MINUTES: [(&str, u32); 6] =
    [("1 minute", 1), ("3 minutes", 3), ("5 minutes", 5), ("10 minutes", 10), ("15 minutes", 15), ("30 minutes", 30)];
/// Playback rows before the auto-skip toggles: audio, bitrate, auto-play, skip back, skip forward, fMP4, rewatching, days.
const PLAYBACK_FIXED: usize = 8;
/// A picture bigger than this is refused before it is read in full.
const MAX_PICTURE_BYTES: u64 = 10 << 20;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Account,
    Home,
    Playback,
    Subtitles,
    Display,
    QuickConnect,
    Server,
    Tmdb,
    About,
}

impl Tab {
    const ALL: [Tab; 9] = [
        Tab::Account,
        Tab::Home,
        Tab::Playback,
        Tab::Subtitles,
        Tab::Display,
        Tab::QuickConnect,
        Tab::Server,
        Tab::Tmdb,
        Tab::About,
    ];

    fn label(self) -> &'static str {
        match self {
            Tab::Account => "Account",
            // D11: "Home" read as "go Home"; this tab arranges the Home screen.
            Tab::Home => "Home screen",
            Tab::Playback => "Playback",
            Tab::Subtitles => "Subtitles",
            Tab::Display => "Display",
            Tab::QuickConnect => "Quick Connect",
            Tab::Server => "Server",
            Tab::Tmdb => "TMDB",
            Tab::About => "About",
        }
    }
}

/// Which dropdown is open (for key routing).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Dd {
    Audio,
    Bitrate,
    SkipBack,
    SkipForward,
    Date,
    SubMode,
    SubText,
    SubBack,
    SubLang,
    NextUpDays,
    SaverMode,
    SaverMinutes,
}

/// Why a picture change failed: our own check (plain words) or the server.
enum PicError {
    Local(String),
    Api(ApiError),
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum ServerAction {
    Admin,
    ChangeServer,
    SignOut,
}

pub struct SettingsScreen {
    tab: usize,
    focus: Focus,
    groups: Vec<Group>,

    // Account
    pw_editing: bool,
    pw_current: String,
    pw_new: String,
    pw_repeat: String,
    pw_busy: bool,
    pw_error: Option<String>,
    pw_ok: bool,
    // Account: profile picture (P-USER-1). `pic_rev` busts the cached avatar after a change.
    pic_editing: bool,
    pic_path: String,
    pic_busy: bool,
    pic_error: Option<String>,
    pic_ok: Option<&'static str>,
    pic_rev: u32,

    // Playback + Display dropdowns
    audio_lang: widgets::DropdownState,
    bitrate: widgets::DropdownState,
    skip_back: widgets::DropdownState,
    skip_forward: widgets::DropdownState,
    date_format: widgets::DropdownState,
    next_up_days: widgets::DropdownState,
    saver_mode: widgets::DropdownState,
    saver_minutes: widgets::DropdownState,

    // Subtitles (G5): saved as soon as anything changes (FIX-1 #11); `chip` is the focused chip within a chip row.
    subs: SubPrefs,
    sub_lang: widgets::DropdownState,
    /// When the Subtitles tab last saved (shows "Saved" for a moment).
    subs_saved_at: Option<f64>,
    subs_loaded: bool,
    sub_mode: widgets::DropdownState,
    sub_text: widgets::DropdownState,
    sub_back: widgets::DropdownState,
    text_draft: String,
    back_draft: String,
    chip: usize,

    // Quick Connect
    qc_code: String,
    qc_busy: bool,
    qc_error: Option<String>,
    qc_ok: bool,

    // Server
    busy: bool,
    /// UX R7: Sign out asks first — `Some(focused button)` while the confirm is open (0 = Cancel).
    confirm_signout: Option<usize>,
    /// D10: who is signing out, so their saved session goes too.
    signing_out_user: String,

    // TMDB
    tmdb_draft: String,
    tmdb_editing: bool,
    admin: bool,
}

pub fn screen() -> Box<dyn Screen> {
    Box::new(SettingsScreen {
        tab: 0,
        focus: Focus { group: G_TABS, index: 0 },
        groups: vec![],
        pw_editing: false,
        pw_current: String::new(),
        pw_new: String::new(),
        pw_repeat: String::new(),
        pw_busy: false,
        pw_error: None,
        pw_ok: false,
        pic_editing: false,
        pic_path: String::new(),
        pic_busy: false,
        pic_error: None,
        pic_ok: None,
        pic_rev: 0,
        audio_lang: widgets::DropdownState::default(),
        bitrate: widgets::DropdownState::default(),
        skip_back: widgets::DropdownState::default(),
        skip_forward: widgets::DropdownState::default(),
        date_format: widgets::DropdownState::default(),
        next_up_days: widgets::DropdownState::default(),
        saver_mode: widgets::DropdownState::default(),
        saver_minutes: widgets::DropdownState::default(),
        subs: SubPrefs::default(),
        sub_lang: widgets::DropdownState::default(),
        subs_saved_at: None,
        subs_loaded: false,
        sub_mode: widgets::DropdownState::default(),
        sub_text: widgets::DropdownState::default(),
        sub_back: widgets::DropdownState::default(),
        text_draft: String::new(),
        back_draft: String::new(),
        chip: 0,
        qc_code: String::new(),
        qc_busy: false,
        qc_error: None,
        qc_ok: false,
        busy: false,
        confirm_signout: None,
        signing_out_user: String::new(),
        tmdb_draft: String::new(),
        tmdb_editing: false,
        admin: false,
    })
}

/// G10: add or remove a segment kind from the auto-skip list and save.
fn toggle_auto_skip(cx: &mut Cx, kind: SegmentKind) {
    match cx.settings.auto_skip.iter().position(|&k| k == kind) {
        Some(pos) => {
            cx.settings.auto_skip.remove(pos);
        }
        None => cx.settings.auto_skip.push(kind),
    }
    cx.save_settings();
}

impl SettingsScreen {
    fn content_len(&self) -> usize {
        match Tab::ALL[self.tab] {
            Tab::Account if self.pw_editing => 5,
            Tab::Account if self.pic_editing => 4,
            Tab::Account => 3,
            Tab::Home => HomeSection::ALL.len(),
            Tab::Playback => PLAYBACK_FIXED + SegmentKind::SKIPPABLE.len(),
            Tab::Subtitles => 11,
            Tab::Display => 6,
            Tab::QuickConnect => 2,
            Tab::Server => 2 + usize::from(self.admin),
            Tab::Tmdb => usize::from(self.tmdb_editing) * 2 + usize::from(!self.tmdb_editing),
            Tab::About => 0,
        }
    }

    fn focused(&self, index: usize) -> bool {
        self.focus.group == G_CONTENT && self.focus.index == index
    }

    fn set_content(&mut self, index: usize) {
        self.focus = Focus { group: G_CONTENT, index };
    }

    fn sync_dropdowns(&mut self, cx: &Cx) {
        // Only re-anchor a dropdown to the stored value while it is closed; an open one is mid-navigation.
        if !self.audio_lang.open {
            let langs: Vec<String> = LANGUAGES.iter().map(|s| s.to_string()).collect();
            self.audio_lang.highlighted =
                langs.iter().position(|l| *l == cx.settings.preferred_audio_language).unwrap_or(0);
        }
        if !self.bitrate.open {
            self.bitrate.highlighted =
                BITRATE_KBPS.iter().position(|&k| k == cx.settings.max_streaming_bitrate).unwrap_or(0);
        }
        if !self.skip_back.open {
            self.skip_back.highlighted =
                SKIP_BACK.iter().position(|&(_, s)| s == cx.settings.skip_back_seconds).unwrap_or(1);
        }
        if !self.skip_forward.open {
            self.skip_forward.highlighted =
                SKIP_FORWARD.iter().position(|&(_, s)| s == cx.settings.skip_forward_seconds).unwrap_or(2);
        }
        if !self.sub_mode.open {
            self.sub_mode.highlighted = mode_index(self.subs.mode);
        }
        if !self.sub_lang.open {
            self.sub_lang.highlighted = sub_lang_index(&self.subs.language).unwrap_or(0);
        }
        if !self.next_up_days.open {
            self.next_up_days.highlighted =
                NEXT_UP_DAYS.iter().position(|&(_, d)| d == cx.settings.next_up_days).unwrap_or(0);
        }
        if !self.saver_mode.open {
            self.saver_mode.highlighted =
                Screensaver::ALL.iter().position(|&m| m == cx.settings.screensaver).unwrap_or(0);
        }
        if !self.saver_minutes.open {
            self.saver_minutes.highlighted =
                SAVER_MINUTES.iter().position(|&(_, m)| m == cx.settings.screensaver_minutes).unwrap_or(1);
        }
        if !self.sub_text.open {
            self.sub_text.highlighted = sub_colour_index(&self.subs.style.text_colour).unwrap_or(SUB_COLOURS.len());
        }
        if !self.sub_back.open {
            self.sub_back.highlighted = match &self.subs.style.back_colour {
                None => 0,
                Some(h) => sub_colour_index(h).map_or(SUB_COLOURS.len() + 1, |i| i + 1),
            };
        }
        if !self.date_format.open {
            self.date_format.highlighted =
                DateFormat::ALL.iter().position(|&f| f == cx.settings.date_format).unwrap_or(0);
        }
    }

    fn apply_pick(&mut self, cx: &mut Cx, dd: Dd, i: usize) {
        if dd == Dd::SubLang {
            self.subs.language = SUB_LANGS[i.min(SUB_LANGS.len() - 1)].1.to_string();
            return;
        }
        if dd == Dd::SubMode {
            self.subs.mode = MODES[i];
            return;
        }
        if dd == Dd::SubText {
            match SUB_COLOURS.get(i) {
                Some((_, hex)) => {
                    self.subs.style.text_colour = hex.to_string();
                    self.text_draft = hex.to_string();
                }
                // "Custom": use what is typed, or hand over to the field to type it.
                None => match parse_hex_colour(&self.text_draft) {
                    Some(h) => self.subs.style.text_colour = h,
                    None => self.set_content(8),
                },
            }
            return;
        }
        if dd == Dd::SubBack {
            match i.checked_sub(1) {
                None => self.subs.style.back_colour = None,
                Some(c) => match SUB_COLOURS.get(c) {
                    Some((_, hex)) => {
                        self.subs.style.back_colour = Some(hex.to_string());
                        self.back_draft = hex.to_string();
                    }
                    None => match parse_hex_colour(&self.back_draft) {
                        Some(h) => self.subs.style.back_colour = Some(h),
                        None => self.set_content(10),
                    },
                },
            }
            return;
        }
        match dd {
            Dd::Audio => {
                cx.settings.preferred_audio_language = if i == 0 { String::new() } else { LANGUAGES[i].into() }
            }
            Dd::Bitrate => cx.settings.max_streaming_bitrate = BITRATE_KBPS[i],
            Dd::SkipBack => cx.settings.skip_back_seconds = SKIP_BACK[i].1,
            Dd::SkipForward => cx.settings.skip_forward_seconds = SKIP_FORWARD[i].1,
            Dd::Date => cx.settings.date_format = DateFormat::ALL[i],
            Dd::NextUpDays => cx.settings.next_up_days = NEXT_UP_DAYS[i].1,
            Dd::SaverMode => cx.settings.screensaver = Screensaver::ALL[i],
            Dd::SaverMinutes => cx.settings.screensaver_minutes = SAVER_MINUTES[i].1,
            Dd::SubMode | Dd::SubText | Dd::SubBack | Dd::SubLang => {}
        }
        cx.save_settings();
    }

    fn dd_key(&mut self, cx: &mut Cx, key: Key, dd: Dd) -> bool {
        let ev = match dd {
            Dd::Audio => {
                let (s, ev) = widgets::dropdown_key(self.audio_lang, key, LANGUAGES.len());
                self.audio_lang = s;
                ev
            }
            Dd::Bitrate => {
                let (s, ev) = widgets::dropdown_key(self.bitrate, key, BITRATE_KBPS.len());
                self.bitrate = s;
                ev
            }
            Dd::SkipBack => {
                let (s, ev) = widgets::dropdown_key(self.skip_back, key, SKIP_BACK.len());
                self.skip_back = s;
                ev
            }
            Dd::SkipForward => {
                let (s, ev) = widgets::dropdown_key(self.skip_forward, key, SKIP_FORWARD.len());
                self.skip_forward = s;
                ev
            }
            Dd::Date => {
                let (s, ev) = widgets::dropdown_key(self.date_format, key, DateFormat::ALL.len());
                self.date_format = s;
                ev
            }
            Dd::SubMode => {
                let (s, ev) = widgets::dropdown_key(self.sub_mode, key, MODES.len());
                self.sub_mode = s;
                ev
            }
            Dd::SubLang => {
                let (s, ev) = widgets::dropdown_key(self.sub_lang, key, SUB_LANGS.len());
                self.sub_lang = s;
                ev
            }
            Dd::SubText => {
                let (s, ev) = widgets::dropdown_key(self.sub_text, key, SUB_COLOURS.len() + 1);
                self.sub_text = s;
                ev
            }
            Dd::SubBack => {
                let (s, ev) = widgets::dropdown_key(self.sub_back, key, SUB_COLOURS.len() + 2);
                self.sub_back = s;
                ev
            }
            Dd::NextUpDays => {
                let (s, ev) = widgets::dropdown_key(self.next_up_days, key, NEXT_UP_DAYS.len());
                self.next_up_days = s;
                ev
            }
            Dd::SaverMode => {
                let (s, ev) = widgets::dropdown_key(self.saver_mode, key, Screensaver::ALL.len());
                self.saver_mode = s;
                ev
            }
            Dd::SaverMinutes => {
                let (s, ev) = widgets::dropdown_key(self.saver_minutes, key, SAVER_MINUTES.len());
                self.saver_minutes = s;
                ev
            }
        };
        if let Some(widgets::DropdownEvent::Pick(i)) = ev {
            self.apply_pick(cx, dd, i);
        }
        matches!(key, Key::Up | Key::Down | Key::Enter | Key::Back)
    }

    fn open_dropdown(&self) -> Option<Dd> {
        match Tab::ALL[self.tab] {
            Tab::Playback => {
                if self.audio_lang.open {
                    Some(Dd::Audio)
                } else if self.bitrate.open {
                    Some(Dd::Bitrate)
                } else if self.skip_back.open {
                    Some(Dd::SkipBack)
                } else if self.skip_forward.open {
                    Some(Dd::SkipForward)
                } else if self.next_up_days.open {
                    Some(Dd::NextUpDays)
                } else {
                    None
                }
            }
            Tab::Subtitles => {
                if self.sub_mode.open {
                    Some(Dd::SubMode)
                } else if self.sub_lang.open {
                    Some(Dd::SubLang)
                } else if self.sub_text.open {
                    Some(Dd::SubText)
                } else if self.sub_back.open {
                    Some(Dd::SubBack)
                } else {
                    None
                }
            }
            Tab::Display => {
                if self.date_format.open {
                    Some(Dd::Date)
                } else if self.saver_mode.open {
                    Some(Dd::SaverMode)
                } else if self.saver_minutes.open {
                    Some(Dd::SaverMinutes)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn back(&mut self) -> bool {
        match Tab::ALL[self.tab] {
            Tab::Account if self.pw_editing => {
                self.cancel_password();
            }
            Tab::Account if self.pic_editing => {
                self.cancel_picture();
            }
            Tab::Tmdb if self.tmdb_editing => {
                self.tmdb_editing = false;
                self.tmdb_draft.clear();
            }
            _ => {
                self.focus = Focus { group: G_TABS, index: self.tab };
                return true;
            }
        }
        self.focus = Focus { group: G_CONTENT, index: 0 };
        true
    }

    /// UX R7: the Server tab's buttons in order — a safe first focus (Server admin for admins, else Change
    /// server); Sign out last, behind a confirm.
    fn server_action(&self, i: usize) -> ServerAction {
        let order: &[ServerAction] = if self.admin {
            &[ServerAction::Admin, ServerAction::ChangeServer, ServerAction::SignOut]
        } else {
            &[ServerAction::ChangeServer, ServerAction::SignOut]
        };
        order.get(i).copied().unwrap_or(ServerAction::ChangeServer)
    }

    fn content_left_right(&mut self, cx: &mut Cx, key: Key) -> bool {
        // UX R3/R7: the Server tab is a row of buttons — Left/Right move along it.
        if Tab::ALL[self.tab] == Tab::Server {
            self.focus = move_focus(&self.groups, self.focus, key);
            return true;
        }
        if Tab::ALL[self.tab] == Tab::Subtitles {
            let n = match self.focus.index {
                2 => SIZE_LABELS.len(),
                3 => 2,
                4 => SubFont::ALL.len(),
                5 => SubWeight::ALL.len(),
                6 => SubShadow::ALL.len(),
                _ => return true,
            };
            self.chip = widgets::chips_key(self.chip, n, key).0;
            return true;
        }
        if Tab::ALL[self.tab] == Tab::Home {
            let i = self.focus.index;
            let n = HomeSection::ALL.len();
            let (from, to) = match key {
                Key::Left if i > 0 => (i, i - 1),
                Key::Right if i + 1 < n => (i, i + 1),
                _ => return true,
            };
            cx.settings.home_order.swap(from, to);
            cx.save_settings();
            self.focus.index = to;
        }
        true
    }

    fn activate(&mut self, cx: &mut Cx) {
        let i = self.focus.index;
        match Tab::ALL[self.tab] {
            Tab::Account => {
                if self.pw_editing {
                    match i {
                        0 => self.set_content(1),
                        1 => self.set_content(2),
                        2 => self.set_content(3),
                        3 => self.save_password(cx),
                        4 => self.cancel_password(),
                        _ => {}
                    }
                } else if self.pic_editing {
                    match i {
                        0 => self.set_content(1),
                        1 => cx.shell.pick_file = true,
                        2 => self.upload_picture(cx),
                        3 => self.cancel_picture(),
                        _ => {}
                    }
                } else {
                    match i {
                        0 => self.start_picture(),
                        1 => self.remove_picture(cx),
                        _ => {
                            self.pw_editing = true;
                            self.pw_current.clear();
                            self.pw_new.clear();
                            self.pw_repeat.clear();
                            self.pw_error = None;
                            self.pw_ok = false;
                            self.set_content(0);
                        }
                    }
                }
            }
            Tab::Home => {
                if let Some(&section) = cx.settings.home_order.get(i) {
                    if let Some(pos) = cx.settings.home_hidden.iter().position(|&s| s == section) {
                        cx.settings.home_hidden.remove(pos);
                    } else {
                        cx.settings.home_hidden.push(section);
                    }
                    cx.save_settings();
                }
            }
            Tab::Playback => match i {
                0 => self.audio_lang.open = true,
                1 => self.bitrate.open = true,
                2 => {
                    cx.settings.auto_play_next = !cx.settings.auto_play_next;
                    cx.save_settings();
                }
                3 => self.skip_back.open = true,
                4 => self.skip_forward.open = true,
                5 => {
                    cx.settings.prefer_fmp4 = !cx.settings.prefer_fmp4;
                    cx.save_settings();
                }
                6 => {
                    cx.settings.next_up_rewatching = !cx.settings.next_up_rewatching;
                    cx.save_settings();
                }
                7 => self.next_up_days.open = true,
                n => {
                    if let Some(&kind) = SegmentKind::SKIPPABLE.get(n - PLAYBACK_FIXED) {
                        toggle_auto_skip(cx, kind);
                    }
                }
            },
            Tab::Subtitles => match i {
                0 => self.sub_mode.open = true,
                1 => self.sub_lang.open = true,
                2 => self.subs.style.size = size_from(self.chip.min(SIZE_LABELS.len() - 1)),
                3 => self.adj_pos(if self.chip == 0 { -5.0 } else { 5.0 }),
                4 => self.subs.style.font = SubFont::ALL[self.chip.min(SubFont::ALL.len() - 1)],
                5 => self.subs.style.weight = SubWeight::ALL[self.chip.min(SubWeight::ALL.len() - 1)],
                6 => self.subs.style.shadow = SubShadow::ALL[self.chip.min(SubShadow::ALL.len() - 1)],
                7 => self.sub_text.open = true,
                8 => self.set_content(9),
                9 => self.sub_back.open = true,
                _ => {}
            },
            Tab::Display => match i {
                0 => {
                    cx.settings.show_release_year = !cx.settings.show_release_year;
                    cx.save_settings();
                }
                1 => {
                    cx.settings.show_story_year = !cx.settings.show_story_year;
                    cx.save_settings();
                }
                2 => self.date_format.open = true,
                3 => {
                    cx.settings.backdrops = !cx.settings.backdrops;
                    cx.save_settings();
                }
                4 => self.saver_mode.open = true,
                5 => self.saver_minutes.open = true,
                _ => {}
            },
            Tab::QuickConnect => match i {
                0 => self.set_content(1),
                1 => self.authorize(cx),
                _ => {}
            },
            Tab::Server => match self.server_action(i) {
                ServerAction::Admin => cx.push(super::admin::screen()),
                ServerAction::ChangeServer => cx.reset(super::connect::screen(None)),
                ServerAction::SignOut => self.confirm_signout = Some(0),
            },
            Tab::Tmdb => {
                if self.tmdb_editing {
                    match i {
                        0 => self.set_content(1),
                        1 => self.save_tmdb(cx),
                        _ => {}
                    }
                } else {
                    self.tmdb_editing = true;
                    self.tmdb_draft.clear();
                    self.set_content(0);
                }
            }
            Tab::About => {}
        }
    }

    fn load_subs(&mut self, cx: &Cx) {
        if self.subs_loaded {
            return;
        }
        self.subs = cx.settings.subtitles.clone();
        self.sync_colour_drafts();
        self.subs_loaded = true;
    }

    /// FIX-1 #11: tvOS settings never ask to save — any change on the Subtitles tab is saved at once.
    fn autosave_subs(&mut self, cx: &mut Cx) {
        if self.subs_loaded && self.subs != cx.settings.subtitles {
            cx.settings.subtitles = self.subs.clone();
            cx.save_settings();
            self.subs_saved_at = Some(cx.now);
        }
    }

    fn sync_colour_drafts(&mut self) {
        self.text_draft = self.subs.style.text_colour.clone();
        self.back_draft = self.subs.style.back_colour.clone().unwrap_or_default();
    }

    fn adj_pos(&mut self, delta: f32) {
        self.subs.style.position = (self.subs.style.position + delta).clamp(0.0, 150.0);
    }

    fn start_picture(&mut self) {
        self.pic_editing = true;
        self.pic_path.clear();
        self.pic_error = None;
        self.pic_ok = None;
        self.set_content(0);
    }

    fn cancel_picture(&mut self) {
        self.pic_editing = false;
        self.pic_path.clear();
        self.pic_error = None;
        self.set_content(0);
    }

    /// P-USER-1: read the file named in the path field and send it as the profile picture.
    fn upload_picture(&mut self, cx: &mut Cx) {
        let path = self.pic_path.trim().trim_matches('"').to_string();
        if path.is_empty() {
            self.pic_error = Some("Type or browse to a picture file first.".to_string());
            return;
        }
        let (user_id, _) = Self::signed_in_user(cx);
        let client = cx.api();
        self.pic_busy = true;
        self.pic_error = None;
        self.pic_ok = None;
        cx.spawn("picture", move || -> std::result::Result<&'static str, PicError> {
            let len = std::fs::metadata(&path)
                .map_err(|e| PicError::Local(format!("Could not read that file ({e}).")))?
                .len();
            if len > MAX_PICTURE_BYTES {
                return Err(PicError::Local("That picture is bigger than 10 MB.".to_string()));
            }
            let bytes =
                std::fs::read(&path).map_err(|e| PicError::Local(format!("Could not read that file ({e}).")))?;
            match client.set_user_image(&user_id, &bytes) {
                Err(ApiError::Decode(m)) => Err(PicError::Local(m)),
                Err(e) => Err(PicError::Api(e)),
                Ok(()) => Ok("Picture changed."),
            }
        });
    }

    fn remove_picture(&mut self, cx: &mut Cx) {
        let (user_id, _) = Self::signed_in_user(cx);
        let client = cx.api();
        self.pic_busy = true;
        self.pic_error = None;
        self.pic_ok = None;
        cx.spawn("picture", move || -> std::result::Result<&'static str, PicError> {
            client.delete_user_image(&user_id).map(|()| "Picture removed.").map_err(PicError::Api)
        });
    }

    fn cancel_password(&mut self) {
        self.pw_editing = false;
        self.pw_current.clear();
        self.pw_new.clear();
        self.pw_repeat.clear();
        self.pw_error = None;
        self.pw_ok = false;
        self.set_content(0);
    }

    fn save_password(&mut self, cx: &mut Cx) {
        if self.pw_busy {
            return;
        }
        self.pw_ok = false;
        if self.pw_new != self.pw_repeat {
            self.pw_error = Some("The new passwords don't match.".to_string());
            return;
        }
        if self.pw_new.is_empty() {
            self.pw_error = Some("Enter a new password.".to_string());
            return;
        }
        let user_id =
            cx.settings.last().and_then(|e| e.session.as_ref()).map(|s| s.user_id.clone()).unwrap_or_default();
        let current = self.pw_current.clone();
        let new = self.pw_new.clone();
        let client = cx.api();
        self.pw_busy = true;
        self.pw_error = None;
        cx.spawn("password", move || client.change_password(&user_id, &current, &new));
    }

    fn authorize(&mut self, cx: &mut Cx) {
        if self.qc_busy {
            return;
        }
        self.qc_ok = false;
        let code = self.qc_code.trim().to_string();
        if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
            self.qc_error = Some("Enter the 6-digit code from the other device.".to_string());
            return;
        }
        let client = cx.api();
        self.qc_busy = true;
        self.qc_error = None;
        cx.spawn("authorize", move || client.quick_connect_authorize(&code));
    }

    fn sign_out(&mut self, cx: &mut Cx) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.signing_out_user = Self::signed_in_user(cx).0;
        let mut client = cx.api();
        cx.spawn("signout", move || {
            let _ = client.sign_out();
            client
        });
    }

    fn save_tmdb(&mut self, cx: &mut Cx) {
        cx.settings.tmdb_key = self.tmdb_draft.trim().to_string();
        self.tmdb_draft.clear();
        self.tmdb_editing = false;
        cx.save_settings();
        self.set_content(0);
    }

    fn signed_in_user(cx: &Cx) -> (String, String) {
        cx.settings
            .last()
            .and_then(|e| e.session.as_ref())
            .map(|s| (s.user_id.clone(), s.user_name.clone()))
            .unwrap_or_default()
    }

    // ---- per-tab drawing (each returns the content xs) ----

    fn draw_account(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> Vec<f32> {
        let t = cx.theme;
        let mut xs = vec![];
        let (user_id, user_name) = Self::signed_in_user(cx);
        ui.horizontal(|ui| {
            // A round avatar (Apple TV profiles); the initial on glass until a picture loads.
            let (avatar, _) = ui.allocate_exact_size(vec2(t.avatar_size, t.avatar_size), Sense::hover());
            let round = t.avatar_size / 2.0;
            let tex = (!user_id.is_empty()).then(|| {
                let ppp = ui.ctx().pixels_per_point();
                let url = cx.api().user_image_url(&user_id, image_width_bucket(t.avatar_size, ppp));
                format!("{url}&v={}", self.pic_rev)
            });
            match tex.and_then(|u| cx.images.get(&u)) {
                Some(tex) => parts::paint_image(ui, tex, avatar, round),
                None => {
                    ui.painter().rect_filled(avatar, round, t.glass_hi);
                    let initial = user_name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
                    ui.painter().text(
                        avatar.center(),
                        egui::Align2::CENTER_CENTER,
                        initial,
                        t.font(t.title_size),
                        t.text,
                    );
                }
            }
            ui.add_space(t.gap);
            ui.vertical(|ui| {
                ui.add_space(t.gap / 4.0);
                parts::heading(ui, t, &user_name);
                parts::muted(ui, t, "Signed in");
            });
        });
        ui.add_space(t.gap);
        if !self.pw_editing {
            parts::heading(ui, t, "Picture");
            ui.add_space(t.gap / 3.0);
            if self.pic_editing {
                let f0 = self.focused(0);
                let r = parts::text_field(ui, t, &mut self.pic_path, "Path to a picture file", f0, false);
                xs.push(r.rect.center().x);
                if r.clicked() {
                    self.set_content(0);
                }
                ui.add_space(t.gap / 2.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = t.gap / 2.0;
                    let f1 = self.focused(1);
                    let r = parts::button(ui, t, "Browse…", f1, false);
                    xs.push(r.rect.center().x);
                    if r.hovered() {
                        self.set_content(1);
                    }
                    if r.clicked() {
                        cx.shell.pick_file = true;
                    }
                    let label = if self.pic_busy { "Uploading…" } else { "Upload" };
                    let f2 = self.focused(2);
                    let r = parts::button(ui, t, label, f2, true);
                    xs.push(r.rect.center().x);
                    if r.hovered() {
                        self.set_content(2);
                    }
                    if r.clicked() {
                        self.upload_picture(cx);
                    }
                    let f3 = self.focused(3);
                    let r = parts::button(ui, t, "Cancel", f3, false);
                    xs.push(r.rect.center().x);
                    if r.hovered() {
                        self.set_content(3);
                    }
                    if r.clicked() {
                        self.cancel_picture();
                    }
                });
            } else {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = t.gap / 2.0;
                    let f0 = self.focused(0);
                    let r = parts::button(ui, t, "Change picture", f0, false);
                    xs.push(r.rect.center().x);
                    if r.hovered() {
                        self.set_content(0);
                    }
                    if r.clicked() {
                        self.start_picture();
                    }
                    let label = if self.pic_busy { "Removing…" } else { "Remove picture" };
                    let f1 = self.focused(1);
                    let r = parts::button(ui, t, label, f1, false);
                    xs.push(r.rect.center().x);
                    if r.hovered() {
                        self.set_content(1);
                    }
                    if r.clicked() {
                        self.remove_picture(cx);
                    }
                });
            }
            if let Some(e) = &self.pic_error {
                ui.add_space(t.gap / 2.0);
                parts::error_line(ui, t, e);
            }
            if let Some(m) = self.pic_ok {
                ui.add_space(t.gap / 2.0);
                parts::muted(ui, t, m);
            }
            ui.add_space(t.gap);
        }
        if !self.pic_editing {
            parts::heading(ui, t, "Password");
            ui.add_space(t.gap / 3.0);
        }
        if self.pw_editing {
            let f0 = self.focused(0);
            let r = parts::text_field(ui, t, &mut self.pw_current, "Current password", f0, true);
            xs.push(r.rect.center().x);
            if r.clicked() {
                self.set_content(0);
            }
            ui.add_space(t.gap / 3.0);
            let f1 = self.focused(1);
            let r = parts::text_field(ui, t, &mut self.pw_new, "New password", f1, true);
            xs.push(r.rect.center().x);
            if r.clicked() {
                self.set_content(1);
            }
            ui.add_space(t.gap / 3.0);
            let f2 = self.focused(2);
            let r = parts::text_field(ui, t, &mut self.pw_repeat, "Repeat new password", f2, true);
            xs.push(r.rect.center().x);
            if r.clicked() {
                self.set_content(2);
            }
            ui.add_space(t.gap / 2.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = t.gap / 2.0;
                let label = if self.pw_busy { "Changing…" } else { "Save" };
                let f3 = self.focused(3);
                let r = parts::button(ui, t, label, f3, true);
                xs.push(r.rect.center().x);
                if r.hovered() {
                    self.set_content(3);
                }
                if r.clicked() {
                    self.save_password(cx);
                }
                let f4 = self.focused(4);
                let r = parts::button(ui, t, "Cancel", f4, false);
                xs.push(r.rect.center().x);
                if r.hovered() {
                    self.set_content(4);
                }
                if r.clicked() {
                    self.cancel_password();
                }
            });
        } else if !self.pic_editing {
            let f2 = self.focused(2);
            let r = parts::button(ui, t, "Change password", f2, false);
            xs.push(r.rect.center().x);
            if r.hovered() {
                self.set_content(2);
            }
            if r.clicked() {
                self.pw_editing = true;
                self.pw_error = None;
                self.pw_ok = false;
                self.set_content(0);
            }
        }
        if let Some(e) = &self.pw_error {
            ui.add_space(t.gap / 2.0);
            parts::error_line(ui, t, e);
        }
        if self.pw_ok {
            ui.add_space(t.gap / 2.0);
            parts::muted(ui, t, "Password changed.");
        }
        xs
    }

    fn draw_home(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> Vec<f32> {
        let t = cx.theme;
        let mut xs = vec![];
        let order = cx.settings.home_order.clone();
        for (i, section) in order.iter().enumerate() {
            let on = cx.settings.home_visible(*section);
            let out = widgets::toggle_row(ui, t, section.label(), on, self.focused(i));
            xs.push(out.x);
            if out.hovered {
                self.set_content(i);
            }
            if out.clicked {
                if let Some(pos) = cx.settings.home_hidden.iter().position(|&s| s == *section) {
                    cx.settings.home_hidden.remove(pos);
                } else {
                    cx.settings.home_hidden.push(*section);
                }
                cx.save_settings();
            }
        }
        ui.add_space(t.gap / 2.0);
        parts::muted(ui, t, "Enter shows or hides a section; Left and Right move it.");
        xs
    }

    fn draw_playback(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> Vec<f32> {
        let t = cx.theme;
        let mut xs = vec![];
        let langs: Vec<String> = LANGUAGES.iter().map(|s| s.to_string()).collect();
        let f0 = self.focused(0);
        let out = widgets::dropdown_row(ui, t, &mut self.audio_lang, "Audio language", &langs, f0);
        xs.push(out.button_x);
        if out.button_hovered {
            self.set_content(0);
        }
        if let Some(i) = out.item_clicked {
            self.audio_lang.highlighted = i;
            self.audio_lang.open = false;
            self.apply_pick(cx, Dd::Audio, i);
        }

        let bitrates: Vec<String> = BITRATE_LABELS.iter().map(|s| s.to_string()).collect();
        let f1 = self.focused(1);
        let out = widgets::dropdown_row(ui, t, &mut self.bitrate, "Max streaming bitrate", &bitrates, f1);
        xs.push(out.button_x);
        if out.button_hovered {
            self.set_content(1);
        }
        if let Some(i) = out.item_clicked {
            self.bitrate.highlighted = i;
            self.bitrate.open = false;
            self.apply_pick(cx, Dd::Bitrate, i);
        }

        let out =
            widgets::toggle_row(ui, t, "Play next episode automatically", cx.settings.auto_play_next, self.focused(2));
        xs.push(out.x);
        if out.hovered {
            self.set_content(2);
        }
        if out.clicked {
            cx.settings.auto_play_next = !cx.settings.auto_play_next;
            cx.save_settings();
        }

        let skip_back: Vec<String> = SKIP_BACK.iter().map(|(l, _)| l.to_string()).collect();
        let f3 = self.focused(3);
        let out = widgets::dropdown_row(ui, t, &mut self.skip_back, "Skip back", &skip_back, f3);
        xs.push(out.button_x);
        if out.button_hovered {
            self.set_content(3);
        }
        if let Some(i) = out.item_clicked {
            self.skip_back.highlighted = i;
            self.skip_back.open = false;
            self.apply_pick(cx, Dd::SkipBack, i);
        }

        let skip_forward: Vec<String> = SKIP_FORWARD.iter().map(|(l, _)| l.to_string()).collect();
        let f4 = self.focused(4);
        let out = widgets::dropdown_row(ui, t, &mut self.skip_forward, "Skip forward", &skip_forward, f4);
        xs.push(out.button_x);
        if out.button_hovered {
            self.set_content(4);
        }
        if let Some(i) = out.item_clicked {
            self.skip_forward.highlighted = i;
            self.skip_forward.open = false;
            self.apply_pick(cx, Dd::SkipForward, i);
        }
        // RR8 / P-USER-4: fMP4 HLS and the Next up options.
        let out =
            widgets::toggle_row(ui, t, "Prefer fMP4-HLS media container", cx.settings.prefer_fmp4, self.focused(5));
        xs.push(out.x);
        if out.hovered {
            self.set_content(5);
        }
        if out.clicked {
            cx.settings.prefer_fmp4 = !cx.settings.prefer_fmp4;
            cx.save_settings();
        }
        let label = "Show episodes I've started rewatching in Next up";
        let out = widgets::toggle_row(ui, t, label, cx.settings.next_up_rewatching, self.focused(6));
        xs.push(out.x);
        if out.hovered {
            self.set_content(6);
        }
        if out.clicked {
            cx.settings.next_up_rewatching = !cx.settings.next_up_rewatching;
            cx.save_settings();
        }
        let days: Vec<String> = NEXT_UP_DAYS.iter().map(|(l, _)| l.to_string()).collect();
        let f7 = self.focused(7);
        let out = widgets::dropdown_row(ui, t, &mut self.next_up_days, "Next up window", &days, f7);
        xs.push(out.button_x);
        if out.button_hovered {
            self.set_content(7);
        }
        if let Some(i) = out.item_clicked {
            self.next_up_days.highlighted = i;
            self.next_up_days.open = false;
            self.apply_pick(cx, Dd::NextUpDays, i);
        }
        // G10 / P-PLAY-11: auto-skip per segment kind.
        for (n, kind) in SegmentKind::SKIPPABLE.into_iter().enumerate() {
            let on = cx.settings.auto_skip.contains(&kind);
            let label = format!("Skip {} automatically", kind.label());
            let out = widgets::toggle_row(ui, t, &label, on, self.focused(PLAYBACK_FIXED + n));
            xs.push(out.x);
            if out.hovered {
                self.set_content(PLAYBACK_FIXED + n);
            }
            if out.clicked {
                toggle_auto_skip(cx, kind);
            }
        }
        xs
    }

    fn draw_subtitles(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> Vec<f32> {
        let t = cx.theme;
        let mut xs = vec![];
        let modes: Vec<String> = MODE_LABELS.iter().map(|s| s.to_string()).collect();
        let f0 = self.focused(0);
        let out = widgets::dropdown_row(ui, t, &mut self.sub_mode, "Mode", &modes, f0);
        xs.push(out.button_x);
        if out.button_hovered {
            self.set_content(0);
        }
        if let Some(i) = out.item_clicked {
            self.sub_mode.highlighted = i;
            self.sub_mode.open = false;
            self.apply_pick(cx, Dd::SubMode, i);
        }
        ui.add_space(t.gap / 3.0);

        let langs: Vec<String> = SUB_LANGS.iter().map(|(l, _)| l.to_string()).collect();
        let f1 = self.focused(1);
        let out = widgets::dropdown_row(ui, t, &mut self.sub_lang, "Language", &langs, f1);
        xs.push(out.button_x);
        if out.button_hovered {
            self.set_content(1);
        }
        if let Some(i) = out.item_clicked {
            self.sub_lang.highlighted = i;
            self.sub_lang.open = false;
            self.apply_pick(cx, Dd::SubLang, i);
        }
        ui.add_space(t.gap / 3.0);

        parts::muted(ui, t, "Size");
        let selected = [0, 1, 2, 3].map(|i| size_index(self.subs.style.size) == i);
        let labels: Vec<String> = SIZE_LABELS.iter().map(|s| s.to_string()).collect();
        let chip = self.focused(2).then_some(self.chip);
        let out = widgets::chips(ui, t, &labels, &selected, chip);
        xs.push(out.xs.first().copied().unwrap_or_default());
        if let Some(i) = out.hovered {
            self.set_content(2);
            self.chip = i;
        }
        if let Some(i) = out.clicked {
            self.subs.style.size = size_from(i);
        }
        ui.add_space(t.gap / 3.0);

        let mut pos_click = None;
        let mut pos_hover = None;
        let pos_focus = self.focused(3);
        let chip = self.chip;
        let mut pos_x = 0.0;
        ui.horizontal(|ui| {
            parts::muted(ui, t, "Position");
            ui.add_space(t.gap);
            let r = parts::button(ui, t, "-", pos_focus && chip == 0, false);
            pos_x = r.rect.center().x;
            if r.hovered() {
                pos_hover = Some(0);
            }
            if r.clicked() {
                pos_click = Some(0);
            }
            ui.label(egui::RichText::new(format!("{:.0}", self.subs.style.position)).size(t.body_size).color(t.text));
            let r = parts::button(ui, t, "+", pos_focus && chip == 1, false);
            if r.hovered() {
                pos_hover = Some(1);
            }
            if r.clicked() {
                pos_click = Some(1);
            }
        });
        xs.push(pos_x);
        if let Some(i) = pos_hover {
            self.set_content(3);
            self.chip = i;
        }
        if let Some(i) = pos_click {
            self.adj_pos(if i == 0 { -5.0 } else { 5.0 });
        }
        ui.add_space(t.gap / 3.0);

        // Font, weight, shadow: chip rows (P-PLAY-9).
        let font_labels: Vec<String> = SubFont::ALL.iter().map(|f| f.label().to_string()).collect();
        let font_sel: Vec<bool> = SubFont::ALL.iter().map(|f| *f == self.subs.style.font).collect();
        let weight_labels: Vec<String> = SubWeight::ALL.iter().map(|f| f.label().to_string()).collect();
        let weight_sel: Vec<bool> = SubWeight::ALL.iter().map(|f| *f == self.subs.style.weight).collect();
        let shadow_labels: Vec<String> = SubShadow::ALL.iter().map(|f| f.label().to_string()).collect();
        let shadow_sel: Vec<bool> = SubShadow::ALL.iter().map(|f| *f == self.subs.style.shadow).collect();
        let rows: [(usize, &str, &[String], &[bool]); 3] = [
            (4, "Font", &font_labels, &font_sel),
            (5, "Weight", &weight_labels, &weight_sel),
            (6, "Shadow", &shadow_labels, &shadow_sel),
        ];
        for (idx, label, labels, selected) in rows {
            parts::muted(ui, t, label);
            let chip = self.focused(idx).then_some(self.chip);
            let out = widgets::chips(ui, t, labels, selected, chip);
            xs.push(out.xs.first().copied().unwrap_or_default());
            if let Some(i) = out.hovered {
                self.set_content(idx);
                self.chip = i;
            }
            if let Some(i) = out.clicked {
                match idx {
                    4 => self.subs.style.font = SubFont::ALL[i],
                    5 => self.subs.style.weight = SubWeight::ALL[i],
                    _ => self.subs.style.shadow = SubShadow::ALL[i],
                }
            }
            ui.add_space(t.gap / 3.0);
        }

        // Text and background colour: a dropdown of the named colours plus Custom, and a #RRGGBB field.
        let mut text_opts: Vec<String> = SUB_COLOURS.iter().map(|(n, _)| n.to_string()).collect();
        text_opts.push("Custom".to_string());
        let mut back_opts = vec!["None".to_string()];
        back_opts.extend(text_opts.iter().cloned());
        let f7 = self.focused(7);
        let out = widgets::dropdown_row(ui, t, &mut self.sub_text, "Text colour", &text_opts, f7);
        xs.push(out.button_x);
        if out.button_hovered {
            self.set_content(7);
        }
        if let Some(i) = out.item_clicked {
            self.sub_text.highlighted = i;
            self.sub_text.open = false;
            self.apply_pick(cx, Dd::SubText, i);
        }
        ui.add_space(t.gap / 3.0);
        let f8 = self.focused(8);
        let r = parts::text_field(ui, t, &mut self.text_draft, "Custom text colour (#RRGGBB)", f8, false);
        xs.push(r.rect.center().x);
        if r.clicked() {
            self.set_content(8);
        }
        if r.changed() {
            if let Some(h) = parse_hex_colour(&self.text_draft) {
                self.subs.style.text_colour = h;
            }
        }
        ui.add_space(t.gap / 3.0);
        let f9 = self.focused(9);
        let out = widgets::dropdown_row(ui, t, &mut self.sub_back, "Background colour", &back_opts, f9);
        xs.push(out.button_x);
        if out.button_hovered {
            self.set_content(9);
        }
        if let Some(i) = out.item_clicked {
            self.sub_back.highlighted = i;
            self.sub_back.open = false;
            self.apply_pick(cx, Dd::SubBack, i);
        }
        ui.add_space(t.gap / 3.0);
        let f10 = self.focused(10);
        let r = parts::text_field(ui, t, &mut self.back_draft, "Custom background colour (#RRGGBB)", f10, false);
        xs.push(r.rect.center().x);
        if r.clicked() {
            self.set_content(10);
        }
        if r.changed() {
            if let Some(h) = parse_hex_colour(&self.back_draft) {
                self.subs.style.back_colour = Some(h);
            }
        }
        ui.add_space(t.gap / 3.0);

        self.autosave_subs(cx);
        let shown = self.subs_saved_at.is_some_and(|at| cx.now - at < SAVED_SHOW_SECS);
        if shown {
            parts::muted(ui, t, "Saved");
            ui.ctx().request_repaint();
        }
        xs
    }

    fn draw_display(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> Vec<f32> {
        let t = cx.theme;
        let mut xs = vec![];
        let out = widgets::toggle_row(ui, t, "Show release year", cx.settings.show_release_year, self.focused(0));
        xs.push(out.x);
        if out.hovered {
            self.set_content(0);
        }
        if out.clicked {
            cx.settings.show_release_year = !cx.settings.show_release_year;
            cx.save_settings();
        }
        let out = widgets::toggle_row(ui, t, "Show story year", cx.settings.show_story_year, self.focused(1));
        xs.push(out.x);
        if out.hovered {
            self.set_content(1);
        }
        if out.clicked {
            cx.settings.show_story_year = !cx.settings.show_story_year;
            cx.save_settings();
        }
        let formats: Vec<String> = DateFormat::ALL.iter().map(|f| f.label().to_string()).collect();
        let f2 = self.focused(2);
        let out = widgets::dropdown_row(ui, t, &mut self.date_format, "Date format", &formats, f2);
        xs.push(out.button_x);
        if out.button_hovered {
            self.set_content(2);
        }
        if let Some(i) = out.item_clicked {
            self.date_format.highlighted = i;
            self.date_format.open = false;
            self.apply_pick(cx, Dd::Date, i);
        }
        let out = widgets::toggle_row(ui, t, "Show backdrops", cx.settings.backdrops, self.focused(3));
        xs.push(out.x);
        if out.hovered {
            self.set_content(3);
        }
        if out.clicked {
            cx.settings.backdrops = !cx.settings.backdrops;
            cx.save_settings();
        }
        // RR8 / P-USER-2: screensaver.
        let modes: Vec<String> = Screensaver::ALL.iter().map(|m| m.label().to_string()).collect();
        let f4 = self.focused(4);
        let out = widgets::dropdown_row(ui, t, &mut self.saver_mode, "Screensaver", &modes, f4);
        xs.push(out.button_x);
        if out.button_hovered {
            self.set_content(4);
        }
        if let Some(i) = out.item_clicked {
            self.saver_mode.highlighted = i;
            self.saver_mode.open = false;
            self.apply_pick(cx, Dd::SaverMode, i);
        }
        let mins: Vec<String> = SAVER_MINUTES.iter().map(|(l, _)| l.to_string()).collect();
        let f5 = self.focused(5);
        let out = widgets::dropdown_row(ui, t, &mut self.saver_minutes, "Start after", &mins, f5);
        xs.push(out.button_x);
        if out.button_hovered {
            self.set_content(5);
        }
        if let Some(i) = out.item_clicked {
            self.saver_minutes.highlighted = i;
            self.saver_minutes.open = false;
            self.apply_pick(cx, Dd::SaverMinutes, i);
        }
        xs
    }

    fn draw_quickconnect(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> Vec<f32> {
        let t = cx.theme;
        let mut xs = vec![];
        parts::muted(ui, t, "Authorise another device: type the 6-digit code it shows and confirm on this screen.");
        ui.add_space(t.gap / 2.0);
        let f0 = self.focused(0);
        let r = parts::text_field(ui, t, &mut self.qc_code, "6-digit code", f0, false);
        xs.push(r.rect.center().x);
        if r.clicked() {
            self.set_content(0);
        }
        ui.add_space(t.gap / 2.0);
        let label = if self.qc_busy { "Authorising…" } else { "Authorise" };
        let r = parts::button(ui, t, label, self.focused(1), true);
        xs.push(r.rect.center().x);
        if r.hovered() {
            self.set_content(1);
        }
        if r.clicked() {
            self.authorize(cx);
        }
        if let Some(e) = &self.qc_error {
            ui.add_space(t.gap / 2.0);
            parts::error_line(ui, t, e);
        }
        if self.qc_ok {
            ui.add_space(t.gap / 2.0);
            parts::muted(ui, t, "Code authorised.");
        }
        xs
    }

    fn draw_server(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> Vec<f32> {
        let t = cx.theme;
        let mut xs = vec![];
        let (server, user) = {
            let entry = cx.settings.last();
            let server = entry.map(|e| parts::server_label(&e.name, &e.address)).unwrap_or_default();
            let user = entry
                .and_then(|e| e.session.as_ref())
                .map(|s| s.user_name.clone())
                .unwrap_or_else(|| "Signed out".to_string());
            (server, user)
        };
        widgets::kv_row(ui, t, "Server", &server, false);
        widgets::kv_row(ui, t, "Signed in as", &user, true);
        ui.add_space(t.gap);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = t.gap / 2.0;
            let n = if self.admin { 3 } else { 2 };
            for k in 0..n {
                let action = self.server_action(k);
                let label = match action {
                    ServerAction::Admin => "Server admin",
                    ServerAction::ChangeServer => "Change server",
                    ServerAction::SignOut if self.busy => "Signing out…",
                    ServerAction::SignOut => "Sign out",
                };
                let r = parts::button(ui, t, label, self.focused(k), false);
                xs.push(r.rect.center().x);
                if r.hovered() {
                    self.set_content(k);
                }
                if r.clicked() {
                    match action {
                        ServerAction::Admin => cx.push(super::admin::screen()),
                        ServerAction::ChangeServer => cx.reset(super::connect::screen(None)),
                        ServerAction::SignOut => self.confirm_signout = Some(0),
                    }
                }
            }
        });
        xs
    }

    fn draw_tmdb(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> Vec<f32> {
        let t = cx.theme;
        let mut xs = vec![];
        if self.tmdb_editing {
            let f0 = self.focused(0);
            let r = parts::text_field(ui, t, &mut self.tmdb_draft, "Paste your key from themoviedb.org", f0, true);
            xs.push(r.rect.center().x);
            if r.clicked() {
                self.set_content(0);
            }
            ui.add_space(t.gap / 2.0);
            let r = parts::button(ui, t, "Save", self.focused(1), true);
            xs.push(r.rect.center().x);
            if r.hovered() {
                self.set_content(1);
            }
            if r.clicked() {
                self.save_tmdb(cx);
            }
        } else {
            let saved = cx.settings.tmdb_key.clone();
            let label = if saved.trim().is_empty() { "Add TMDB API key".to_string() } else { mask_tail(&saved) };
            let r = parts::button_width(ui, t, &label, self.focused(0), false, t.field_w);
            xs.push(r.rect.center().x);
            if r.hovered() {
                self.set_content(0);
            }
            if r.clicked() {
                self.tmdb_editing = true;
                self.tmdb_draft.clear();
                self.set_content(0);
            }
        }
        parts::muted(ui, t, "Used for the Acting and Crew filmography. Free at themoviedb.org.");
        xs
    }

    fn draw_about(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> Vec<f32> {
        let t = cx.theme;
        widgets::kv_row(ui, t, "Rustfin version", env!("CARGO_PKG_VERSION"), true);
        parts::muted(ui, t, "A fast native Jellyfin client for desktop.");
        let _ = cx;
        vec![]
    }
}

/// The key with all but its last four characters masked (R-F5-2).
fn mask_tail(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= 4 {
        return key.to_string();
    }
    let mut out = "•".repeat(chars.len() - 4);
    out.extend(&chars[chars.len() - 4..]);
    out
}

const MODES: [SubMode; 4] = [SubMode::Default, SubMode::Always, SubMode::OnlyForced, SubMode::None];
const MODE_LABELS: [&str; 4] = ["Default", "Always", "Only forced", "None"];
const SIZE_LABELS: [&str; 4] = ["S", "M", "L", "XL"];

fn mode_index(m: SubMode) -> usize {
    MODES.iter().position(|&x| x == m).unwrap_or(0)
}

fn size_index(s: SubSize) -> usize {
    match s {
        SubSize::Small => 0,
        SubSize::Medium => 1,
        SubSize::Large => 2,
        SubSize::ExtraLarge => 3,
    }
}

fn size_from(i: usize) -> SubSize {
    match i {
        0 => SubSize::Small,
        1 => SubSize::Medium,
        2 => SubSize::Large,
        _ => SubSize::ExtraLarge,
    }
}

impl Screen for SettingsScreen {
    fn name(&self) -> &'static str {
        "settings"
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        // The Sign out confirm swallows its keys: Left/Right between Cancel and Sign out, Back = Cancel.
        if let Some(f) = self.confirm_signout {
            match key {
                Key::Left | Key::Right => self.confirm_signout = Some(widgets::tabs_key(f, 2, key)),
                Key::Enter => {
                    self.confirm_signout = None;
                    if f == 1 {
                        self.sign_out(cx);
                    }
                }
                Key::Back => self.confirm_signout = None,
                _ => {}
            }
            return true;
        }
        // An open dropdown swallows its own keys.
        if let Some(dd) = self.open_dropdown() {
            return self.dd_key(cx, key, dd);
        }

        if self.focus.group == G_TABS {
            return match key {
                Key::Left | Key::Right => {
                    self.tab = widgets::tabs_key(self.tab, Tab::ALL.len(), key);
                    self.focus.index = self.tab;
                    true
                }
                Key::Down | Key::Enter => {
                    if self.content_len() > 0 {
                        self.set_content(0);
                    }
                    true
                }
                Key::Back => false,
                _ => false,
            };
        }

        match key {
            Key::Back => self.back(),
            Key::Up | Key::Down => {
                self.focus = move_focus(&self.groups, self.focus, key);
                true
            }
            Key::Left | Key::Right => self.content_left_right(cx, key),
            Key::Enter => {
                self.activate(cx);
                true
            }
            _ => false,
        }
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn std::any::Any + Send>) {
        match tag {
            "password" => {
                self.pw_busy = false;
                let Ok(r) = payload.downcast::<Result<()>>() else { return };
                match *r {
                    Ok(()) => {
                        self.pw_ok = true;
                        self.pw_error = None;
                        self.pw_editing = false;
                        self.pw_current.clear();
                        self.pw_new.clear();
                        self.pw_repeat.clear();
                        self.set_content(0);
                    }
                    Err(ApiError::Unauthorized) => {
                        self.pw_error = Some("The current password is wrong.".to_string());
                    }
                    Err(e) => self.pw_error = Some(e.to_string()),
                }
            }
            "picked-file" => {
                if let Ok(p) = payload.downcast::<String>() {
                    self.pic_path = *p;
                    if self.pic_editing {
                        self.set_content(2);
                    }
                }
            }
            "picture" => {
                self.pic_busy = false;
                let Ok(r) = payload.downcast::<std::result::Result<&'static str, PicError>>() else { return };
                match *r {
                    Ok(msg) => {
                        self.pic_rev += 1;
                        self.pic_ok = Some(msg);
                        self.pic_error = None;
                        self.pic_editing = false;
                        self.pic_path.clear();
                        self.set_content(0);
                    }
                    Err(PicError::Api(ApiError::Unauthorized)) => cx.signed_out(),
                    Err(PicError::Api(e)) => self.pic_error = Some(e.to_string()),
                    Err(PicError::Local(m)) => self.pic_error = Some(m),
                }
            }
            "authorize" => {
                self.qc_busy = false;
                let Ok(r) = payload.downcast::<Result<()>>() else { return };
                match *r {
                    Ok(()) => {
                        self.qc_ok = true;
                        self.qc_error = None;
                        self.qc_code.clear();
                    }
                    Err(ApiError::NotFound) => {
                        self.qc_error = Some("That code is not valid or has expired.".to_string());
                    }
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(e) => self.qc_error = Some(e.to_string()),
                }
            }
            "signout" => {
                self.busy = false;
                let Ok(c) = payload.downcast::<Client>() else { return };
                let client = *c;
                if let Some(id) = cx.settings.last_server.clone() {
                    cx.settings.forget_session(&id);
                    cx.settings.forget_saved_user(&id, &self.signing_out_user);
                }
                cx.save_settings();
                cx.reset(super::signin::screen(client, None));
            }
            _ => {}
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        self.load_subs(cx);
        self.sync_dropdowns(cx);
        self.admin = cx.admin;
        let tab = Tab::ALL[self.tab];

        // Tab bar.
        let labels: Vec<String> = Tab::ALL.iter().map(|t| t.label().to_string()).collect();
        let tab_focus = (self.focus.group == G_TABS).then_some(self.focus.index);
        let tabs = widgets::tabs(ui, t, &labels, self.tab, tab_focus);
        if let Some(i) = tabs.hovered {
            self.focus = Focus { group: G_TABS, index: i };
        }
        if let Some(i) = tabs.clicked {
            self.tab = i;
            self.focus = Focus { group: G_TABS, index: i };
        }

        ui.add_space(t.gap);

        // Content (scrollable).
        let mut xs = vec![];
        egui::ScrollArea::vertical().id_salt("settings-content").show(ui, |ui| {
            ui.add_space(t.gap / 2.0);
            parts::title(ui, t, tab.label());
            ui.add_space(t.gap / 2.0);
            // Each page sits in one grouped card (tvOS settings style); toggles and dropdowns are full-width rows.
            xs = widgets::group(ui, t, "", |ui| {
                ui.add_space(t.gap / 4.0);
                let xs = match tab {
                    Tab::Account => self.draw_account(ui, cx),
                    Tab::Home => self.draw_home(ui, cx),
                    Tab::Playback => self.draw_playback(ui, cx),
                    Tab::Subtitles => self.draw_subtitles(ui, cx),
                    Tab::Display => self.draw_display(ui, cx),
                    Tab::QuickConnect => self.draw_quickconnect(ui, cx),
                    Tab::Server => self.draw_server(ui, cx),
                    Tab::Tmdb => self.draw_tmdb(ui, cx),
                    Tab::About => self.draw_about(ui, cx),
                };
                ui.add_space(t.gap / 4.0);
                xs
            });
            ui.add_space(t.gap * 2.0);
        });

        // Clamp the content focus if the tab changed shape.
        let len = self.content_len();
        if self.focus.group == G_CONTENT {
            if len == 0 {
                self.focus = Focus { group: G_TABS, index: self.tab };
            } else {
                self.focus.index = self.focus.index.min(len - 1);
            }
        }

        // The Server tab's buttons are one row (Left/Right move along it); every other tab is a column.
        let cols = if tab == Tab::Server { len.max(1) } else { 1 };
        self.groups = vec![Group { len: Tab::ALL.len(), cols: Tab::ALL.len(), xs: tabs.xs }, Group { len, cols, xs }];

        if let Some(f) = self.confirm_signout {
            let buttons = vec!["Cancel".to_string(), "Sign out".to_string()];
            let body = "You will need to sign in again on this device.";
            let out = widgets::dialog(ui, t, "Sign out?", body, &buttons, Some(f));
            if let Some(i) = out.hovered {
                self.confirm_signout = Some(i);
            }
            if let Some(i) = out.clicked {
                self.confirm_signout = None;
                if i == 1 {
                    self.sign_out(cx);
                }
            }
        }
    }
}

#[cfg(test)]
mod fix1_sub_lang_tests {
    use super::*;

    #[test]
    fn subtitle_language_codes_map_to_the_list() {
        assert_eq!(sub_lang_index(""), Some(0));
        assert_eq!(sub_lang_index("ENG"), Some(1));
        assert_eq!(sub_lang_index("deu"), sub_lang_index("ger"));
        assert_eq!(sub_lang_index("xx"), None);
    }
}
